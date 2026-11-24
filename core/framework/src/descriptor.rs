//! The reserved discovery request runs before a module's operational startup.

use link_format::WithinExt;

pub use link_component::{Component, Dependency, Kind, Resolved, Value};

/// This program's retired spellings, from the history its build embedded
/// beside its description: kept by [`answer`] for its parser, which
/// answers them. Apart from the description, whose pages make it most of
/// a megabyte, so a command's start reads only what it needs.
static HISTORY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// The spec kinds this program owns, as its `main` gave them to
/// [`answer`]: what its commands' bindings name. Each is exported, its
/// whole schema made, only when first asked for.
static KINDS: crate::named::Named<link_spec::Exported> = crate::named::Named::new();

/// The spec kinds this program's commands bind but another module owns,
/// as its `main` gave them to [`uses`]: never exported as its own.
static USED: crate::named::Named<link_spec::Exported> = crate::named::Named::new();

/// Name the spec kinds another module owns that this program's commands
/// bind, such as an agent's grants, which are the sandbox spec's: their
/// field flags are this program's commands', their owner the other.
pub fn uses(kinds: &[fn() -> link_spec::Exported]) {
    USED.name(kinds);
}

/// The binding of the spec kind named `kind`, where this program owns it
/// or [`uses`] it.
pub fn binding(kind: &str) -> Option<link_spec::Binding> {
    let exported = KINDS
        .get()
        .iter()
        .chain(USED.get())
        .find(|k| k.kind == kind)?;
    Some(link_spec::Binding {
        schema: exported.shape.clone(),
        format: exported.format(),
    })
}

/// The internal request the core answers with the release's history:
/// its modules renamed and removed, and each module's retired spellings.
pub const HISTORY_FLAG: &str = "--link-history";

/// The release's history, as the core this module runs under answers it:
/// what the names its specs and conversations hold follow
/// ([`link_spec::follow`]).
pub fn release() -> anyhow::Result<link_spec::history::Wire> {
    let mut lnk = crate::lnk();
    let answer = link_spec::history::asking(&mut lnk)
        .arg(HISTORY_FLAG)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| anyhow::anyhow!("asking lnk for the release's history: {e}"))?;
    anyhow::ensure!(
        answer.status.success(),
        "lnk didn't answer the release's history"
    );
    link_spec::history::Wire::parse("lnk", &answer.stdout)
}

/// This program's retired spellings, where its `main` gave them to
/// [`answer`]; none where it gave none.
pub fn history() -> link_spec::history::History {
    HISTORY
        .get()
        .and_then(|embedded| serde_json::from_str(embedded).ok())
        .unwrap_or_default()
}

/// Every Link program installed beside this `lnk`, each `lnk-<name>` in
/// its folder, with its description as it answers `--link-describe`,
/// which it answers before any operational startup.
pub fn installed() -> Vec<(String, anyhow::Result<Resolved>)> {
    let Some(dir) = crate::lnk_program()
        .parent()
        .map(std::path::Path::to_path_buf)
    else {
        return Vec::new();
    };
    let mut programs: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let program = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(std::env::consts::EXE_SUFFIX))
                .and_then(|n| n.strip_prefix("lnk-"));
            path.is_file()
                && program.is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
        })
        .collect();
    programs.sort();
    // Asked at once, so one slow program costs one wait, not one each.
    std::thread::scope(|scope| {
        let asked: Vec<_> = programs
            .iter()
            .map(|program| scope.spawn(move || describe(program)))
            .collect();
        programs
            .iter()
            .zip(asked)
            .map(|(program, asked)| {
                let name = program
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let described = asked
                    .join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("describing {name} failed")));
                (name, described)
            })
            .collect()
    })
}

/// What `program` says of itself, within the wait a description gets.
fn describe(program: &std::path::Path) -> anyhow::Result<Resolved> {
    let output = crate::source::described(program)
        .map_err(|unanswered| anyhow::anyhow!("{} {unanswered}", program.display()))?;
    Resolved::parse(std::str::from_utf8(&output)?)
}

/// Answer only the exact discovery request, with the spec kinds the
/// program owns: its description `embedded`, and `history`, its retired
/// spellings, as its build wrote them. No operational dependency is
/// probed.
pub fn answer(
    embedded: &'static str,
    history: &'static str,
    kinds: &[fn() -> link_spec::Exported],
) {
    let _ = HISTORY.set(history);
    KINDS.name(kinds);
    let Some(query) = link_spec::entry::discovery() else {
        return;
    };
    if let link_spec::entry::Discovery::Upgrade(stage) = query {
        crate::saved::answer(stage);
    }
    let mut out = link_format::Out::with(link_settings::output::machine());
    let result = link_component::asked()
        .and_then(|asked| Ok((asked, Resolved::parse(embedded)?)))
        .map(|(asked, mut description)| {
            description.kinds = KINDS.get().to_vec();
            description.stating(asked)
        })
        .and_then(|description| {
            let mut document = if query == link_spec::entry::Discovery::Documentation {
                serde_json::to_vec(&description.pages)?
            } else {
                serde_json::to_vec(&description)?
            };
            document.push(b'\n');
            out.data(&document);
            Ok(link_format::Done::shown())
        })
        .within("describing the component")
        .map_err(link_format::Report::from);
    std::process::exit(i32::from(out.finish(result)));
}

/// A build-generated answer binds declarations and pages to this Cargo member.
#[macro_export]
macro_rules! describe_component {
    ($($kind:ty),* $(,)?) => {{
        $crate::__spec::entry::initialize();
        $crate::source!();
        $crate::descriptor::answer(
            include_str!(concat!(env!("OUT_DIR"), "/component.json")),
            include_str!(concat!(env!("OUT_DIR"), "/history.json")),
            &[$(|| $crate::__spec::Exported::of::<$kind>()),*],
        )
    }};
}

/// Check an owner's effective operational settings. Owners resolve authored
/// settings before calling this; discovery never invokes a capability probe.
pub fn check_requirements(
    embedded: &str,
    command: &str,
    values: &std::collections::BTreeMap<String, link_component::Value>,
) -> anyhow::Result<()> {
    use link_component::{
        Dependency,
        requirements::{self, Environment},
    };
    let descriptor = Resolved::parse(embedded)?;
    let operation = descriptor
        .commands
        .get(command)
        .ok_or_else(|| anyhow::anyhow!("undeclared operation {command}"))?;
    let effective = requirements::effective_options(Some(operation), values)?;
    for edge in descriptor.requires.iter().chain(&operation.requires) {
        if !requirements::applicable(edge, &Environment::here(), &effective) {
            continue;
        }
        let Some(target) = descriptor.dependencies.get(&edge.dependency) else {
            continue;
        };
        match target {
            Dependency::Module { id, .. } => anyhow::ensure!(
                crate::find_module(id).is_some(),
                "{command} requires the {id} module; run lnk module add {id}"
            ),
            Dependency::Capability { id, min_abi } => probe_capability(id, min_abi.unwrap_or(1))
                .map_err(|e| anyhow::anyhow!("{command}: {e:#}"))?,
            Dependency::External { id, .. } if id == "bubblewrap" => anyhow::ensure!(
                link_files::bubblewrap().is_some(),
                "{command} requires bubblewrap (bwrap)"
            ),
            Dependency::External { id, .. } => {
                anyhow::bail!("{command}: no runtime provider is bound for {id}")
            }
            // A package is compiled into the program that names it.
            Dependency::Package { .. } => {}
        }
    }
    Ok(())
}

/// Capability providers test the executing host, rather than its OS label.
pub fn probe_capability(id: &str, min_abi: u32) -> anyhow::Result<()> {
    match id {
        "linux.user-namespaces" => {
            let wall = link_files::bubblewrap()
                .ok_or_else(|| anyhow::anyhow!("bubblewrap is not installed"))?;
            let answer = link_secretive::process::command(wall)
                .args([
                    "--unshare-user",
                    "--unshare-pid",
                    "--ro-bind",
                    "/",
                    "/",
                    "--",
                    "/bin/true",
                ])
                .output()?;
            anyhow::ensure!(
                answer.status.success(),
                "user namespaces are unavailable: {}",
                String::from_utf8_lossy(&answer.stderr).trim()
            );
        }
        "linux.landlock" => {
            #[cfg(target_os = "linux")]
            {
                let abi = unsafe { libc::syscall(444, 0, 0, 1) };
                if abi < 0 {
                    let error = std::io::Error::last_os_error();
                    let reason = match error.raw_os_error() {
                        Some(libc::ENOSYS) => "syscall unavailable",
                        Some(libc::EOPNOTSUPP) => "support disabled",
                        Some(libc::EPERM | libc::EACCES) => "probe denied",
                        _ => "probe failed",
                    };
                    anyhow::bail!(
                        "Landlock version probe: {reason}: {error}; ABI {min_abi} is required; target not started"
                    );
                }
                anyhow::ensure!(
                    abi >= i64::from(min_abi),
                    "Landlock version probe: support disabled; ABI {min_abi} is required; host probe returned {abi}"
                );
            }
            #[cfg(not(target_os = "linux"))]
            anyhow::bail!("Landlock ABI {min_abi} is unavailable on this host");
        }
        "macos.seatbelt" => {
            let answer = link_secretive::process::command("/usr/bin/sandbox-exec")
                .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
                .output()?;
            anyhow::ensure!(answer.status.success(), "the macOS sandbox is unavailable");
        }
        _ => anyhow::bail!("unknown required capability {id}"),
    }
    Ok(())
}
