//! What the harness runs with: its sandbox or none, its proxy's
//! secrets, routes and tools, and how it sleeps.

use link_format::WithinExt;
use link_format::{Foreign, Name, Report, aside, cmd, prose, sentence};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure};
use link_agent::{
    adapter::{Adapter, Boxed},
    bridge, bridges,
    grants::Grants,
    sandbox_file, service,
    settings::{self, Settings},
};

use crate::connecting::connection_module;
use crate::shared;
mod tools;
pub(crate) use tools::{
    defined_tools, forget_defined_tools, hand_grants, memory_env, reach_now, team_tools, tools_for,
};

mod bound;
mod handed;
pub(crate) use handed::harness_tools;
use handed::{handed_names, rename_tool_env};

/// For each connection the harness speaks whose module installs its
/// program in the harness's home (`describe`'s `cli`, as Signal's
/// `signal-cli`): installs it there (`lnk-<connection> cli install`, in the
/// harness's sandbox), and hands the adapter its path in the connection's
/// section (`program`).
pub(crate) fn connection_programs(
    adapter: &Adapter,
    boxed: &Boxed,
    settings: &Settings,
    handed: &mut link_harness_contract::harness::Settings,
) -> anyhow::Result<()> {
    let speaks = adapter.connections();
    let mut json = serde_json::to_value(&*handed)?;
    for (name, connection) in &settings.connections {
        if !connection.about.cli || !speaks.contains(name) || json.get(name).is_none() {
            continue;
        }
        connection_module(name)?;
        let installed = adapter.connection_install(boxed, name, &json[name])?;
        if let Some(program) = installed.get("program") {
            json[name]["program"] = program.clone();
        }
    }
    *handed = serde_json::from_value(json)?;
    Ok(())
}

/// Links connection `connection` in the home of the harness `adapter` runs,
/// which speaks it: with its module's program there (`lnk-<connection> cli
/// link`) when its module installs one, else the adapter's own `link`.
pub(crate) fn link_on(
    adapter: &Adapter,
    boxed: &Boxed,
    settings: &Settings,
    connection: &str,
    again: bool,
) -> anyhow::Result<bool> {
    match settings
        .connections
        .get(connection)
        .is_some_and(|c| c.about.cli)
    {
        true => adapter.connection_link(boxed, connection, again),
        false => adapter.link(boxed, connection, again),
    }
}

/// The secrets harness `harness`'s proxy adds: its connections'
/// ([`Settings::wire_secrets`]), but for the connections Link carries for it
/// ([`bridge::carried`]): the harness is never handed those, so its
/// proxy never holds their secrets either; and the sealed ones granted
/// to the agent ([`Settings::sealed_variables`]). Two of one name are
/// refused: the route would add one for the other's calls.
pub(crate) fn proxy_secrets(
    settings: &Settings,
    harness: &str,
) -> anyhow::Result<Vec<settings::Wired>> {
    let mut secrets = match settings.keys_wired()? {
        true => {
            let carried = bridge::carried(settings, &Adapter::find(harness)?.connections());
            settings.wire_secrets(&connection_rules(settings)?, &carried)?
        }
        false => Vec::new(),
    };
    secrets.extend(settings.sealed_variables()?.into_iter().map(|(_, s)| s));
    let mut names = std::collections::BTreeSet::new();
    for secret in &secrets {
        ensure!(
            names.insert(secret.named.name.as_str()),
            "two of {}'s secrets are called {} on its proxy: name a connection or a secret \
             another way",
            settings.name,
            secret.named.name
        );
    }
    Ok(secrets)
}

/// What harness `harness`'s proxy adds outside, as routes of its one
/// bind: its hosted model's key ([`Settings::wire_key`]) and those at
/// addresses of their own ([`Settings::address_keys`]), on the models
/// module's bridge, and its secrets ([`proxy_secrets`]), on the secret
/// module's; and who calls them, the agent, by its binding in this
/// machine's keyring, which its start made, or a harness run's
/// ([`bound::RunBinding`]).
pub(crate) fn proxy_routes(
    settings: &Settings,
    harness: &str,
) -> anyhow::Result<(
    Vec<link_sandbox_route::Route>,
    Option<link_sandbox_binding::Binding>,
)> {
    let mut routes = crate::action_routes::routes(settings)?;
    // Its hosted model's key, and each at an address of its own, every
    // one on the model route's bridge, inference alone.
    for key in settings
        .wire_key()?
        .into_iter()
        .chain(settings.address_keys()?)
    {
        let bridge = link_model_contract::route::bridge()?;
        routes.push(link_model_contract::route::hosted(bridge, key)?);
    }
    for (model, fallback) in [
        (settings.model.as_ref(), false),
        (settings.fallback.as_ref(), true),
    ] {
        if let Some(api) = model
            .map(|model| settings.model_api(model, fallback))
            .transpose()?
            .flatten()
        {
            routes.extend(link_model_contract::route::actions::routes(
                link_model_contract::route::bridge()?,
                api,
            )?);
        }
    }
    // By name: its bridge reads each from your secrets, granted to the
    // agent, at each call.
    let secrets: Vec<_> = proxy_secrets(settings, harness)?
        .into_iter()
        .map(|wired| wired.named)
        .collect();
    if !secrets.is_empty() {
        let bridge = link_secret_contract::route::bridge()?;
        routes.push(link_secret_contract::route::route(
            bridge,
            Vec::new(),
            secrets,
        )?);
    }
    let caller = link_sandbox_binding::Keyring::here().bound(&settings.key());
    Ok((routes, caller))
}

/// What the harness is told of each sealed secret granted to the agent:
/// `$<NAME>`, its placeholder, and
/// `$LNK_SEALED_<NAME>_URL`, its service's root on the proxy at `port`.
/// Never the secret.
pub(crate) fn sealed_env(
    settings: &Settings,
    port: u16,
    env: &mut std::collections::BTreeMap<String, String>,
) -> anyhow::Result<()> {
    for (name, secret) in settings.sealed_variables()? {
        env.insert(
            link_secret_contract::sealed_url_var(&name),
            link_secret_contract::route::url(port, &secret.named.name),
        );
        env.insert(name, secret.named.placeholder.into_exposed());
    }
    Ok(())
}

/// Fails when the agent has sealed secrets granted and its harness runs
/// without its own proxy here (no sandbox, or one shared with other
/// agents), where none could be added for it alone.
pub(crate) fn sealed_need_proxy(settings: &Settings, why: &str) -> anyhow::Result<()> {
    let sealed = settings.sealed_variables()?;
    if let Some((name, _)) = sealed.first() {
        bail!(
            "{name} is granted to {} sealed, which its own proxy adds, and {why}: lnk secret \
             remove {name}, or run it in its own sandbox",
            settings.name
        );
    }
    Ok(())
}

/// Each connected connection's rule (`lnk-<connection> rule --json`), when the
/// proxy adds the harness's secrets ([`Settings::wire_secrets`]); none
/// asked otherwise. An error when a connection's module can't say: its
/// secret never goes to the harness unasked.
pub(crate) fn connection_rules(settings: &Settings) -> anyhow::Result<settings::Rules> {
    let mut rules = settings::Rules::new();
    if !settings.keys_wired()? {
        return Ok(rules);
    }
    // A connection's rule doesn't change while this runs: asked once each.
    type Asked = std::collections::HashMap<String, Option<link_connection_contract::Rule>>;
    static ASKED: std::sync::Mutex<Option<Asked>> = std::sync::Mutex::new(None);
    for connection in settings.connections() {
        if let Some(rule) = ASKED
            .lock()
            .ok()
            .and_then(|asked| asked.as_ref()?.get(&connection).cloned())
        {
            rules.insert(connection.to_string(), rule);
            continue;
        }
        // Added if it's missing (a box starts without it).
        let program = connection_module(&connection)?;
        let output = link_secretive::process::ours(&program)
            .args(["rule", "--json"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .output()
            .within(format!("running {}", program.display()))?;
        ensure!(
            output.status.success(),
            "lnk-{connection} doesn't say how its secret stays out of the sandbox \
             (rule --json): lnk upgrade"
        );
        let rule = serde_json::from_slice::<link_connection_contract::Ruled>(&output.stdout)
            .within(format!("reading lnk-{connection} rule --json"))?
            .rule;
        if let Some(rule) = &rule {
            rule.wire
                .check()
                .within(format!("lnk-{connection}'s rule"))?;
        }
        if let Ok(mut asked) = ASKED.lock() {
            asked
                .get_or_insert_default()
                .insert(connection.to_string(), rule.clone());
        }
        rules.insert(connection.to_string(), rule);
    }
    Ok(rules)
}

/// `lnk agent harness run`: `command` in the sandbox harness `name`
/// runs in, with its environment, in the files folder; its exit status. Its own rules
/// file and proxy, beside the running agent's: on a Mac a port of its own,
/// on Linux a sockets folder of its own. Behind its proxy it's the agent
/// calling, as its start bound it, or bound for this run alone
/// ([`bound::RunBinding`]) while it's stopped.
pub(crate) fn run_inside(
    settings: &Settings,
    name: &str,
    command: &[String],
) -> anyhow::Result<i32> {
    let mut boxed = Boxed::new(settings, name)?;
    boxed.ports.extend(settings.model_port());
    // The files folder as the harness has it.
    if match Adapter::find(name) {
        Ok(adapter) => adapter.contract()?.folders,
        Err(_) => false,
    } {
        boxed.folders = Some(sandbox_file::of(&settings.name).folders());
    }
    let profile = format!("{}-run", boxed.profile);
    let mut env = boxed.env();
    if !boxed.sandbox {
        aside(
            Report::warning(sentence!(
                "{} runs without its sandbox for {}: this runs unconfined.",
                Name::new("harness", name),
                Name::new("agent", &settings.name)
            ))
            .why(prose!(
                "It was started with {}, or it needs none.",
                Foreign::new("--no-sandbox")
            ))
            .next(
                cmd!(
                    "lnk agent harness-permission list --harness {}",
                    Name::new("harness", name)
                ),
                "see what it needs",
            ),
        );
    }
    // A port of this machine's loopback its proxy serves on is held from
    // the moment it's chosen, and handed to the proxy open: never free for
    // another program to take in between.
    let mut held = None;
    // Its proxy's routes, which may hold credentials, handed to it as it
    // starts, at once: never written to a file, as nothing starts it again.
    let mut handed = link_sandbox_client::Handed::default();
    // Who its proxy's calls are proved as, held until the command ends.
    let mut _bound = None;
    let argv = match settings.proxy_of(name) {
        None => {
            sealed_need_proxy(settings, "it runs without its proxy here")?;
            boxed.wrap(command, &profile, &[], &[], true, &[])?
        }
        // No sandbox: a proxy of its own beside it, on a free port of
        // this machine's loopback, the agent's own holding its port.
        Some(upstream) if !boxed.sandbox => {
            let port = held.insert(link_sandbox_client::HeldPort::new()?).port()?;
            _bound = Some(bound::RunBinding::of(
                &link_sandbox_binding::Keyring::here(),
                settings,
            )?);
            let (routes, caller) = proxy_routes(settings, name)?;
            let at = link_sandbox_client::ProxyAt {
                upstream,
                listen: Some(port),
                routes,
                caller,
                secret: Some(settings.url_secret()?.into()),
                ..Default::default()
            };
            let (wrapped, unwalled) = unwalled_wrap(&boxed, settings, &profile, at, command, true)?;
            handed = wrapped.handed;
            env.extend(wrapped.wrapped.env);
            behind_proxy_env(settings, Some(&unwalled.proxy), port, &mut env)?;
            wrapped.wrapped.argv
        }
        Some(upstream) => {
            // On Linux in the sandbox's own network, where no other
            // program is.
            let port = if cfg!(target_os = "macos") {
                held.insert(link_sandbox_client::HeldPort::new()?).port()?
            } else {
                settings.proxy_port()
            };
            _bound = Some(bound::RunBinding::of(
                &link_sandbox_binding::Keyring::here(),
                settings,
            )?);
            let (routes, caller) = proxy_routes(settings, name)?;
            let at = link_sandbox_client::ProxyAt {
                upstream,
                dir: settings.net_dir(&format!("{name}-run")),
                listen: Some(port),
                routes,
                caller,
                secret: Some(settings.url_secret()?.into()),
                ..Default::default()
            };
            let confinement = boxed.confinement(
                with_tools(boxed.spec(true), tools_for(settings, name)),
                std::path::Path::new(command.first().within("nothing to run")?),
                &profile,
                &[],
                &[],
                true,
                link_sandbox_client::Way::Proxy(at),
            )?;
            let wrapped = link_sandbox_client::wrap_handing(&confinement, command)?;
            handed = wrapped.handed;
            env.extend(wrapped.wrapped.env);
            behind_proxy_env(settings, proxy_in(&confinement), port, &mut env)?;
            wrapped.wrapped.argv
        }
    };
    let mut run = link_secretive::process::command(&argv[0]);
    run.args(&argv[1..])
        .env_clear()
        .envs(&env)
        .current_dir(settings.files());
    if let Some(held) = held {
        held.hand_to(&mut run);
    }
    handed.hand_to(&mut run);
    let status = run.status().within(format!("running {}", argv[0]));
    link_sandbox_client::forget(&profile);
    let status = status?;
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return Ok(128 + signal);
    }
    Ok(status.code().unwrap_or(1))
}

/// Removes what agent `settings`'s proxy is handed as its service or
/// terminal starts it ([`link_sandbox_client::secret_files`]), which may
/// hold credentials: needed only while it runs.
pub(crate) fn forget_routes(settings: &Settings) {
    link_sandbox_client::forget_secrets(&settings.profile(settings.harness()));
}

/// Which of `launch`'s ports are on this machine's loopback, where another
/// program could take them first. Behind its proxy on Linux, only those
/// the proxy serves: the others are in the sandbox's own network. On
/// macOS all of them, proxy or not: Seatbelt has no network of its own,
/// so the connection daemons (`signal-cli`, a harness's relay) listen on
/// the host's loopback.
pub(crate) fn on_loopback(
    launch: &link_harness_contract::harness::Launch,
    proxied: bool,
    macos: bool,
) -> Vec<u16> {
    match proxied && !macos {
        true => launch.served(),
        false => launch.ports.clone(),
    }
}

/// The ports that are the harness's own inside its sandbox, behind its
/// proxy, never this machine's for it to reach: a port the bridge serves
/// (`front`), and on Linux every port of `launch`, in the sandbox's own
/// network, where forwarded out the proxy would hold it first and the
/// harness couldn't listen on it. On macOS, Seatbelt has no network of
/// its own: the rest are on this machine's loopback.
pub(crate) fn own_inside(
    launch: &link_harness_contract::harness::Launch,
    front: Option<u16>,
    macos: bool,
) -> Vec<u16> {
    match macos {
        true => front.into_iter().collect(),
        false => launch.ports.iter().copied().chain(front).collect(),
    }
}

/// Fails when another program listens on one of `ports`, on IPv4's or
/// IPv6's loopback, after waiting a few seconds for a harness just stopped
/// to let go of them.
#[expect(
    clippy::disallowed_methods,
    reason = "waits for ports to be let go, which nothing announces"
)]
pub(crate) fn ports_free(name: &str, ports: &[u16]) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        let Some(port) = ports.iter().copied().find(|&p| settings::port_taken(p)) else {
            return Ok(());
        };
        if start.elapsed() > Duration::from_secs(5) {
            bail!(
                "port {port}, one of {name}'s, is taken by another program: it would be \
                 taken for part of your agent. Stop that program (lsof -i :{port} names \
                 it), then start again"
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Whether agent `name` runs here now, as a service, in a terminal, or
/// carried by this machine's one bridge (which holds an idle agent's
/// port): then a port of its block taken is likely its own.
pub(crate) fn runs_here(name: &str) -> bool {
    service::status(name).running
        || service::foreground_running(name)
        || bridges::record_of(&settings::key_of(name)).exists()
}

/// The harness in a network of its own, whose only way out is the
/// sandbox's filtering proxy, to `upstream`, and only to its hosts if it
/// has a list; the proxy's environment added to `env`.
pub(crate) fn isolated(
    boxed: &Boxed,
    settings: &Settings,
    launch: &link_harness_contract::harness::Launch,
    upstream: &str,
    env: &mut std::collections::BTreeMap<String, String>,
    hold: bool,
    front: Option<u16>,
) -> anyhow::Result<(Vec<String>, link_sandbox_client::Confinement)> {
    // A hosted model's key and its connections' secrets, added outside
    // (`lnk agent secret wire`), each a route of its bind.
    let (routes, caller) = proxy_routes(settings, &boxed.name)?;
    let at = link_sandbox_client::ProxyAt {
        // Alone, for its owner's one harness here ([`shared`]).
        hold,
        upstream: upstream.to_string(),
        dir: settings.net_dir(&boxed.name),
        // Its endpoint's port, unless this machine's bridge serves it,
        // through the proxy's folder, as it wakes it.
        serve: launch
            .served()
            .into_iter()
            .filter(|port| Some(*port) != front)
            .collect(),
        // macOS: the port the harness was set up with.
        listen: Some(settings.proxy_port()),
        routes,
        caller,
        // What its tool and local model URLs carry.
        secret: Some(settings.url_secret()?.into()),
        // Its questions asked on its owner's chats too, as long as its
        // spec's `[connections] ask_wait`.
        asking: asking(settings),
    };
    let mut spec = with_tools(boxed.spec(true), tools_for(settings, &boxed.name));
    let own = own_inside(launch, front, cfg!(target_os = "macos"));
    if let (false, Some(localhost)) = (own.is_empty(), &mut spec.localhost) {
        let ports = localhost.ports.get_or_insert_default();
        ports.retain(|p| {
            !own.iter()
                .any(|o| *p == link_sandbox_contract::spec::Port::Number(*o))
        });
        if ports.is_empty() {
            localhost.ports = None;
        }
        if localhost.models.is_none() && localhost.ports.is_none() {
            spec.localhost = None;
        }
    }
    let confinement = boxed.confinement(
        spec,
        std::path::Path::new(launch.argv.first().within("its launch runs nothing")?),
        &boxed.profile,
        &launch.write,
        &launch.read,
        true,
        link_sandbox_client::Way::Proxy(at),
    )?;
    let wrapped = link_sandbox_client::wrap(&confinement, &launch.argv)?;
    env.extend(wrapped.env);
    behind_proxy_env(settings, proxy_in(&confinement), settings.proxy_port(), env)?;
    Ok((wrapped.argv, confinement))
}

/// With no sandbox (`sandbox = false`): `launch`'s program as it is,
/// under its proxy on its port of this machine's loopback, which it's
/// pointed at and reaches its tools, model and secrets on, and its
/// environment beside it. Nothing holds it there: the proxy is advice
/// it may ignore. Never through an exit, which only a sandbox keeps a
/// harness to.
pub(crate) fn unwalled(
    boxed: &Boxed,
    settings: &Settings,
    launch: &link_harness_contract::harness::Launch,
    upstream: &str,
    env: &mut std::collections::BTreeMap<String, String>,
) -> anyhow::Result<Vec<String>> {
    let (routes, caller) = proxy_routes(settings, &boxed.name)?;
    let at = link_sandbox_client::ProxyAt {
        upstream: upstream.to_string(),
        listen: Some(settings.proxy_port()),
        routes,
        caller,
        secret: Some(settings.url_secret()?.into()),
        ..Default::default()
    };
    let wrapped = unwalled_wrap(boxed, settings, &boxed.profile, at, &launch.argv, false)?;
    let wrapped = (wrapped.0.wrapped, wrapped.1);
    env.extend(wrapped.0.env);
    behind_proxy_env(settings, Some(&wrapped.1.proxy), settings.proxy_port(), env)?;
    Ok(wrapped.0.argv)
}

/// `argv` of harness `boxed` with no sandbox, as `profile`, under its
/// proxy run as `at` says, made from its spec as it says with no wall
/// ([`link_sandbox_client::Unwalled`]): what runs it, and the proxy.
pub(crate) fn unwalled_wrap(
    boxed: &Boxed,
    settings: &Settings,
    profile: &str,
    at: link_sandbox_client::ProxyAt,
    argv: &[String],
    hand: bool,
) -> anyhow::Result<(link_sandbox_client::Handing, link_sandbox_client::Unwalled)> {
    let spec = with_tools(boxed.spec(false), tools_for(settings, &boxed.name));
    let unwalled = link_sandbox_client::Unwalled::of(&spec, profile, at)
        .within(format!("{}'s proxy for {}", boxed.agent, boxed.name))?;
    let wrapped = match hand {
        true => link_sandbox_client::wrap_unwalled_handing(&unwalled, argv)?,
        false => link_sandbox_client::Handing {
            wrapped: link_sandbox_client::wrap_unwalled(&unwalled, argv)?,
            ..Default::default()
        },
    };
    Ok((wrapped, unwalled))
}

/// What a program behind `proxy` on `port` is told beside what its wrap
/// gave: each sealed secret's placeholder and address, its memory's and
/// Link's tools' addresses, and, for the clients that read their
/// provider's address from the environment besides the harness's own
/// setting, its hosted model's.
pub(crate) fn behind_proxy_env(
    settings: &Settings,
    proxy: Option<&link_sandbox_client::Proxy>,
    port: u16,
    env: &mut std::collections::BTreeMap<String, String>,
) -> anyhow::Result<()> {
    sealed_env(settings, port, env)?;
    let tools = proxy.map(|p| p.tools()).unwrap_or_default();
    memory_env(settings, &tools, port, env);
    let hosted = proxy.and_then(|p| p.route("model")).and_then(|r| {
        link_sandbox_route::given::<link_model_contract::route::Hosted>(&r.given).ok()
    });
    if let Some(hosted) = &hosted {
        let url = link_model_contract::route::url(&hosted.provider, port).unwrap_or_default();
        let var = match hosted.provider.as_str() {
            "anthropic" => "ANTHROPIC_BASE_URL",
            "openai" => "OPENAI_BASE_URL",
            _ => "OPENROUTER_BASE_URL",
        };
        env.insert(var.to_string(), url);
    }
    Ok(())
}

/// `spec` with `tools` its proxy's: those it's granted that are defined,
/// and Link's own for it ([`tools_for`]).
pub(crate) fn with_tools(
    mut spec: link_sandbox_client::Spec,
    tools: Vec<String>,
) -> link_sandbox_client::Spec {
    spec.tools = (!tools.is_empty()).then(|| link_sandbox_contract::spec::Tools {
        names: Some(tools),
        ..Default::default()
    });
    spec
}

/// The proxy a confinement runs behind, if it has its own.
pub(crate) fn proxy_in(
    confinement: &link_sandbox_client::Confinement,
) -> Option<&link_sandbox_client::Proxy> {
    match &confinement.net {
        link_sandbox_client::Net::Proxied { proxy, .. } => Some(proxy),
        _ => None,
    }
}

/// How agent `settings` is stopped when idle and woken at its next
/// message, if it can be: this machine's one bridge carries it (`plan`),
/// its harness speaks none of its connections itself (`speaks`: it would
/// stop hearing them), and its spec's `[run] idle_after` isn't `never`;
/// its job a service, or its start in a terminal (`foreground`); served
/// by its owner's harness if it `shares`. On Linux behind its proxy
/// (`proxied`), the bridge serves its port through the proxy's folder;
/// else it hands the port over as it wakes.
pub(crate) fn sleep_of(
    settings: &Settings,
    plan: Option<&bridge::Plan>,
    speaks: &[String],
    proxied: bool,
    boxed: &Boxed,
    foreground: bool,
    shares: bool,
) -> anyhow::Result<Option<link_agent::idle::Sleep>> {
    use link_agent::idle;
    let Some(plan) = plan else {
        return Ok(None);
    };
    if plan.endpoint.is_empty()
        || !bridges::sharing()
        || settings.connections().iter().any(|c| speaks.contains(c))
    {
        return Ok(None);
    }
    let inside = cfg!(target_os = "linux") && boxed.sandbox && proxied;
    let Some(after) = settings.spec()?.spec.run.unwrap_or_default().idle_after() else {
        return Ok(None);
    };
    let job = match (foreground, link_service::managed()) {
        (true, _) => idle::Job::Terminal(settings.wires().join("sleep.sock")),
        (false, true) => idle::Job::Service {
            label: service::label(&settings.name),
            unit: service::unit_name(&settings.name),
        },
        // Started by nothing that could start it again.
        (false, false) => return Ok(None),
    };
    let shared = match shares {
        true => {
            let dir = settings::shared_harness_of(&settings.owner()?);
            let program = std::env::current_exe().within("finding lnk-agent")?;
            Some(idle::Shared {
                argv: vec![
                    program.display().to_string(),
                    "shared".into(),
                    dir.display().to_string(),
                ],
                unit: shared::unit_of(&settings.owner()?),
                dir,
            })
        }
        false => None,
    };
    Ok(Some(idle::Sleep {
        port: link_harness_contract::harness::endpoint_port(&plan.endpoint)?,
        net: settings.net_dir(&boxed.name),
        after_ms: after.as_millis() as u64,
        run: link_agent::background::state(settings),
        conversations: settings
            .folder()
            .of(link_store_contract::storage::Collection::Conversations),
        job,
        handoff: !inside,
        shared,
    }))
}

/// Makes sure the sandbox can run on this machine (`lnk sandbox check --ready`,
/// which offers, in a terminal, what it needs).
pub(crate) fn sandbox_can_run() -> Result<(), Report> {
    link_sandbox_client::ready().map_err(|_| sandbox_cant_run())
}

/// What failed, and running without it the way on.
fn sandbox_cant_run() -> Report {
    Report::error(sentence!("The sandbox can't run on this machine.")).next(
        cmd!("lnk agent start --no-sandbox"),
        "run without it, with a warning",
    )
}

/// Whom agent `settings`'s harness's questions are asked for, and how
/// long each waits: its spec's `[connections] ask_wait`; None when its spec
/// doesn't resolve, and then its sandbox asks you alone, on this machine.
fn asking(settings: &Settings) -> Option<link_sandbox_client::routes::Asking> {
    let spec = settings.spec().ok()?;
    let wait = spec.spec.connections.as_ref()?.ask_wait?;
    Some(link_sandbox_client::routes::Asking {
        agent: settings.name.clone(),
        wait_secs: u64::try_from(wait.get().as_millis()).ok()?.div_ceil(1000),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_sandbox_that_cant_run_says_so_and_offers_going_without() {
        let report = super::sandbox_cant_run();
        assert_eq!(
            report.headline.to_string(),
            "The sandbox can't run on this machine."
        );
        assert!(!report.next.is_empty());
    }

    use super::*;

    #[test]
    fn on_macos_every_port_is_checked_behind_the_proxy_too() {
        // A gateway served by the proxy, and a connection daemon's port.
        let launch = link_harness_contract::harness::Launch {
            ports: vec![18789, 18790],
            serve: Some(vec![18789]),
            ..Default::default()
        };
        assert_eq!(on_loopback(&launch, true, true), [18789, 18790]);
        assert_eq!(on_loopback(&launch, false, true), [18789, 18790]);
        assert_eq!(on_loopback(&launch, false, false), [18789, 18790]);
        // Linux behind the proxy: the daemon is in the sandbox's network.
        assert_eq!(on_loopback(&launch, true, false), [18789]);
    }

    /// Behind its proxy on Linux, a harness's own ports stay inside its
    /// sandbox's network, never forwarded to this machine's, where the
    /// proxy would take its gateway's port first; on macOS only the
    /// bridge's.
    #[test]
    fn a_harness_s_own_ports_are_never_reached_out_on_linux() {
        let launch = link_harness_contract::harness::Launch {
            ports: vec![18789, 18790, 18791],
            serve: Some(vec![18790]),
            ..Default::default()
        };
        assert_eq!(own_inside(&launch, None, false), [18789, 18790, 18791]);
        assert_eq!(
            own_inside(&launch, Some(20000), false),
            [18789, 18790, 18791, 20000]
        );
        assert_eq!(own_inside(&launch, Some(20000), true), [20000]);
        assert!(own_inside(&launch, None, true).is_empty());
    }
}
