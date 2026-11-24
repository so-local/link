//! The sandbox's tests, one program: each file a module, so `cargo test`
//! links once, not once per file. One file: `cargo test --test sandbox
//! proxy::`.

link_test_kit::program!(system);

#[cfg(target_os = "linux")]
mod blank;
#[cfg(target_os = "linux")]
mod dotfiles;
#[cfg(target_os = "linux")]
mod enter;
#[cfg(target_os = "linux")]
mod loopback;
#[cfg(target_os = "linux")]
mod open;
#[cfg(target_os = "linux")]
mod pins;
#[cfg(target_os = "linux")]
mod proxy;
#[cfg(target_os = "linux")]
mod run;
#[cfg(target_os = "macos")]
mod seatbelt;
#[cfg(unix)]
mod stop;
#[cfg(target_os = "linux")]
mod tcp;
#[cfg(target_os = "linux")]
mod tools;
#[cfg(target_os = "linux")]
mod unwalled;

/// Whether executable allowlist kernel coverage is available. File and
/// network tests with unrestricted programs do not need this capability.
/// `LNK_REQUIRE_NATIVE` makes missing coverage a failure.
#[cfg(target_os = "linux")]
fn landlock() -> bool {
    use link_test_kit::gate;
    if !link_sandbox_confine::linux::FILTERED {
        return gate::lacks(
            "executable restrictions",
            "they need a seccomp filter built for this architecture",
            Some(gate::NATIVE),
        );
    }
    match link_sandbox_confine::linux::landlock::probe() {
        Ok(_) => true,
        Err(error) => gate::lacks("Landlock", error, Some(gate::NATIVE)),
    }
}

/// Secret `name` holding `value`, kept in `home`'s secrets as `lnk
/// secret` keeps them, granted to the agent [`caller_here`] binds, given
/// it as `given` (a grant's `as`, in TOML): what a bridge reads by the
/// agent's ID and its use, or a variable's name.
#[cfg(target_os = "linux")]
fn grant_here(home: &std::path::Path, name: &str, value: &str, given: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let file = home.join(".lnk/safe/secrets/secrets.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let mut kept = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&file)
        .unwrap();
    // Under an ID of its own, drawn from its name; an agent's own has no
    // name, a variable its own.
    let id: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .chain(std::iter::repeat('0'))
        .take(12)
        .collect();
    let named = match given {
        "{ kind = \"variable\" }" => format!("kind = \"named\"\nname = {name:?}\n"),
        _ => "kind = \"own\"\n".to_string(),
    };
    writeln!(
        kept,
        "[entry.{id}]\n{named}value = {value:?}\nto = [{{ principal = \"service:{SERVICE}\", \
         action = \"secret.secret.read\", resource = \"secret:{id}\", workload = \"agent:{AGENT}\", \
         name = \"agent\", as = {given} }}]\n"
    )
    .unwrap();
}

/// The service the agent [`caller_here`] binds acts as: who its secrets
/// are granted to.
#[cfg(target_os = "linux")]
const SERVICE: &str = "s1t2u3v4w5x6";

/// The agent [`caller_here`] binds, by its ID, as its grants show it.
#[cfg(target_os = "linux")]
const AGENT: &str = "a1g2e3n4t5i6";

/// Routes as a program of `home` is given them, each served by its
/// module's bridge, from this build's programs (started on demand): the
/// model routes' for this machine's runtimes on `models` and a hosted
/// model's, its key `key` granted to its caller; and who calls them,
/// bound in `home`'s keyring.
#[cfg(target_os = "linux")]
fn routes_here(
    home: &std::path::Path,
    models: &[u16],
    key: Option<(link_model_contract::route::Hosted, &str)>,
) -> (
    Vec<link_sandbox_route::Route>,
    link_sandbox_binding::Binding,
) {
    let (settings, keys) = (home.join(".lnk"), home.join(".lnk/safe/agents/credentials"));
    let bridge = link_sandbox_route::Bridge::at(
        &home.join("run"),
        &settings,
        &module("model"),
        "model",
        None,
        &keys,
    );
    let mut routes = Vec::new();
    if !models.is_empty() {
        let local = link_sandbox_route::local_model::local(
            bridge.clone(),
            models.iter().copied().collect(),
        );
        routes.push(local.unwrap());
    }
    if let Some((hosted, key)) = key {
        grant_here(
            home,
            "AGENT_MODEL_KEY",
            key,
            "{ kind = \"own\", purpose = \"model\" }",
        );
        routes.push(link_model_contract::route::hosted(bridge, hosted).unwrap());
    }
    (routes, caller_here(home))
}

/// Who calls a test's routes: an agent bound in `home`'s keyring, acting
/// as [`SERVICE`].
#[cfg(target_os = "linux")]
fn caller_here(home: &std::path::Path) -> link_sandbox_binding::Binding {
    let keys = link_sandbox_binding::Keyring::at(&home.join(".lnk/safe/agents/credentials"));
    let identity = link_sandbox_binding::Identity {
        agent: "agent-1".into(),
        service: SERVICE.into(),
        owner: "me@here".into(),
        teams: Vec::new(),
    };
    keys.bind(&identity, &"k".repeat(32).into()).unwrap()
}

/// Module `name`'s program (`lnk-<name>`), built from its own folder
/// beside this one: never the whole workspace, which would build this
/// module's program again while its tests run it.
#[cfg(target_os = "linux")]
fn module(name: &str) -> std::path::PathBuf {
    let name = match name {
        "models" => "model",
        "accounts" => "auth",
        "harness" => "agent",
        other => other,
    };
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name);
    link_test_kit::programs_in(&folder).join(format!("lnk-{name}"))
}

mod lifetime;
mod specs;
