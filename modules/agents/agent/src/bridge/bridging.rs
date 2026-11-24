//! Each connection's `lnk-<connection> bridge`: which connections Link carries,
//! their folders and links, what each bridge reads, and keeping it running.

use super::*;
use link_format::WithinExt;

/// The connections Link carries for a harness speaking `speaks` itself, of
/// those `agent` has connected.
pub fn carried(agent: &Settings, speaks: &[String]) -> Vec<String> {
    agent
        .connections()
        .into_iter()
        .filter(|c| agent.connections[c].about.bridged)
        .filter(|c| !speaks.iter().any(|s| s == c))
        .collect()
}

/// Leaves out of `settings` (what the harness is handed) the connections
/// Link carries for it: the harness never holds their tokens.
pub fn keep_out(
    settings: &mut link_harness_contract::harness::Settings,
    carried: &[String],
) -> anyhow::Result<()> {
    let mut handed = serde_json::to_value(&*settings)?;
    if let Some(handed) = handed.as_object_mut() {
        for connection in carried {
            handed.remove(connection);
        }
    }
    *settings = serde_json::from_value(handed)?;
    Ok(())
}

/// Connection `connection`'s section of what a harness is handed: its token
/// and who may talk to it.
pub(super) fn section(
    agent: &Settings,
    connection: &str,
) -> Option<link_secretive::Secret<link_secretive::Json>> {
    Some(agent.connections.get(connection)?.settings.duplicate())
}

/// Connection `connection`'s own folder for agent `agent`
/// (`~/.lnk/safe/agents/<agent>/connections/<connection>`): what links a
/// linked connection, which the harness never reaches.
pub fn state(agent: &str, connection: &str) -> PathBuf {
    settings::config_of(agent)
        .join("connections")
        .join(connection)
}

/// What of agent `agent`'s folder for each linked connection Link carries
/// (`carried`) is the link, as `lnk-<connection> state` says: what `lnk agent
/// move` sends, by connection. A connection whose folder isn't there, or whose
/// module isn't installed or doesn't say, is left out: it's linked again
/// on the other machine.
pub fn links(
    agent: &Settings,
    carried: &[String],
) -> BTreeMap<String, link_harness_contract::harness::State> {
    carried
        .iter()
        .filter(|c| agent.connections.get(*c).is_some_and(|c| c.about.linked))
        .filter(|c| state(&agent.name, c).is_dir())
        .filter_map(|connection| Some((connection.clone(), module_state(connection)?)))
        .collect()
}

/// What of its folder links `connection`, as this machine's `lnk-<connection>
/// state` says; none without the module, or when it can't say.
pub fn module_state(connection: &str) -> Option<link_harness_contract::harness::State> {
    let program = link_core::find_module(connection)?;
    let output = link_secretive::process::ours(program)
        .arg("state")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    serde_json::from_slice(&output.stdout).ok()
}

/// Makes `request`'s folder, readable only by the user.
pub(super) fn make_state(request: &link_connection_contract::Bridge) -> anyhow::Result<()> {
    std::fs::create_dir_all(&request.state)
        .within(format!("making {}", request.state.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&request.state, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Forgets what linked connection `connection` for agent `agent`, when it's
/// disconnected.
pub fn forget(agent: &str, connection: &str) {
    let _ = std::fs::remove_dir_all(state(agent, connection));
}

/// Links connection `connection`, which Link carries for agent `agent`'s
/// harness, as `lnk-<connection> link` does: from a QR code on this
/// terminal, anew with `again`. Whether it's linked: false when it isn't
/// and there's no terminal to link it on.
pub fn link(agent: &Settings, connection: &str, again: bool) -> anyhow::Result<bool> {
    let plan = Plan::default();
    let request =
        request(agent, &plan, connection).within(format!("{connection} isn't connected"))?;
    make_state(&request)?;
    let program = link_core::find_module(connection).within(format!(
        "the {connection} module (lnk-{connection}) isn't installed"
    ))?;
    let mut command = link_secretive::process::ours(&program);
    command.arg("link");
    if again {
        command.arg("--again");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .within(format!("running {}", program.display()))?;
    let mut stdin = child.stdin.take().within("no stdin")?;
    stdin.write_all(format!("{}\n", serde_json::to_string(&request)?).as_bytes())?;
    drop(stdin);
    let status = child.wait()?;
    match status.code() {
        Some(0) => Ok(true),
        Some(link_harness_contract::harness::NOT_LINKED) => Ok(false),
        _ => anyhow::bail!("linking {connection} failed ({status})"),
    }
}

/// What `lnk-<connection> bridge` reads on stdin.
pub fn request(
    agent: &Settings,
    plan: &Plan,
    connection: &str,
) -> Option<link_connection_contract::Bridge> {
    let spec = agent.spec().ok();
    Some(link_connection_contract::Bridge {
        agent: agent.name.clone(),
        identity: agent.identity(),
        endpoint: plan.endpoint.clone(),
        key: plan.key.duplicate(),
        proof: plan.proof,
        parts: plan.parts,
        settings: section(agent, connection)?,
        state: state(&agent.name, connection),
        spec: spec.as_ref().map(|s| s.connection(connection)),
        // Shared only with a harness that reads who a message is from: to
        // one that doesn't, a member's would pass for the owner's.
        shared: spec
            .iter()
            .filter(|_| plan.from)
            .filter_map(|spec| spec.spec.access.as_ref())
            .filter_map(|access| link_agent_spec::message_access(access).ok())
            .flat_map(|access| {
                access
                    .chats(connection)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect(),
    })
}

/// Keeps a bridge running for each of `plan`'s connections, each started
/// again after it stops (sooner the longer it ran); returns only if
/// there's none to run. A bridge stops when this process does: its stdin
/// closes.
pub fn run(
    agent: &Settings,
    plan: &Plan,
    runner: Option<link_scheduler_tool_contract::cli::Runner>,
) -> anyhow::Result<()> {
    carry(agent, plan, runner, true).join();
    Ok(())
}

/// Runs `program bridge` with `request`, again each time it stops, until
/// `stop` says; the one running is in `running`, for [`Carrying::stop`].
pub(super) fn keep(
    connection: &str,
    program: &std::path::Path,
    request: &link_connection_contract::Bridge,
    running: &Mutex<Option<Running>>,
    stop: &AtomicBool,
) {
    let mut wait = Duration::from_secs(2);
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        match once(program, request, running, stop) {
            Ok(status) => {
                link_format::daemon::info!("lnk-{connection} bridge stopped ({status})")
            }
            Err(err) => link_format::daemon::warn!("lnk-{connection} bridge: {err:#}"),
        }
        if started.elapsed() > Duration::from_secs(60) {
            wait = Duration::from_secs(2);
        }
        if sleep_unless(wait, stop) {
            return;
        }
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

fn once(
    program: &std::path::Path,
    request: &link_connection_contract::Bridge,
    running: &Mutex<Option<Running>>,
    stop: &AtomicBool,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut child = link_secretive::process::ours(program)
        .arg("bridge")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .within(format!("running {}", program.display()))?;
    // Held open while it runs: it stops when this closes.
    let mut stdin = child.stdin.take().within("no stdin")?;
    stdin.write_all(format!("{}\n", serde_json::to_string(request)?).as_bytes())?;
    stdin.flush()?;
    {
        let mut slot = running.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        // Stopped as it started: it ends at once.
        if stop.load(Ordering::Relaxed) {
            drop(stdin);
            let _ = child.kill();
        } else {
            *slot = Some(Running { stdin });
        }
    }
    let status = child.wait()?;
    if let Ok(mut slot) = running.lock() {
        slot.take();
    }
    Ok(status)
}
