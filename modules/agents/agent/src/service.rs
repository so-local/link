//! Running an agent's harness in the background (`link_service`:
//! a launchd LaunchAgent on macOS, a systemd user service on Linux). An
//! agent runs one harness at a time, so it has one service, whatever the
//! harness, named by the agent's ID (`settings::key_of`), which no other
//! owner's agent shares. Each function takes the agent's name.

use link_format::WithinExt;
use std::path::Path;

use link_service as service;

pub use link_service::{Job, open_log, start_path, unload_other, uptime};

/// How many of [`spawn_logged`]'s streams are still being copied.
static COPYING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// An agent's log, open for a start in a terminal: its file, and the
/// agent's ID, which a line its harness wrote names when it names none.
pub struct Log {
    pub file: std::fs::File,
    pub agent: Option<String>,
}

/// `command`, a harness started in a terminal, spawned with its output
/// shown there and logged to `log` as it comes, a record a line
/// ([`link_format::log::relay`]): `lnk agent log show` reads it as a
/// service's. Each stream is copied until it ends ([`copied`]).
pub fn spawn_logged(
    command: &mut std::process::Command,
    log: &Log,
) -> std::io::Result<std::process::Child> {
    use link_format::streams::{Stream, passed};
    use std::process::Stdio;
    fn copy(
        from: impl std::io::Read,
        stream: Stream,
        mut log: std::fs::File,
        agent: Option<String>,
    ) {
        link_format::log::relay(
            from,
            &mut passed(stream),
            &link_format::log::styled(stream),
            &mut log,
            agent.as_deref(),
        );
        COPYING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    let (out_log, err_log) = (log.file.try_clone()?, log.file.try_clone()?);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(out) = child.stdout.take() {
        COPYING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let agent = log.agent.clone();
        std::thread::spawn(move || copy(out, Stream::Out, out_log, agent));
    }
    if let Some(err) = child.stderr.take() {
        COPYING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let agent = log.agent.clone();
        std::thread::spawn(move || copy(err, Stream::Err, err_log, agent));
    }
    Ok(child)
}

/// Waits, at most `within`, for what [`spawn_logged`]'s harnesses wrote
/// to be shown and logged: a harness's last lines, once it ended. A
/// program it left running that holds its output isn't waited for.
#[expect(
    clippy::disallowed_methods,
    reason = "polls copies in flight until done"
)]
pub fn copied(within: std::time::Duration) {
    let until = std::time::Instant::now() + within;
    while COPYING.load(std::sync::atomic::Ordering::SeqCst) > 0 && std::time::Instant::now() < until
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Agent `agent`'s launchd label, by its ID.
pub fn label(agent: &str) -> String {
    format!("link.agent.{}", crate::settings::key_of(agent))
}

/// Agent `agent`'s systemd unit, by its ID.
pub fn unit_name(agent: &str) -> String {
    format!("lnk-agent-{}.service", crate::settings::key_of(agent))
}

/// The launchd label of the service carrying agent `agent`'s connections
/// for a harness that speaks none (`lnk agent bridge`), by its ID.
pub fn bridge_label(agent: &str) -> String {
    format!("link.bridge.{}", crate::settings::key_of(agent))
}

/// Its systemd unit.
pub fn bridge_unit(agent: &str) -> String {
    format!("lnk-bridge-{}.service", crate::settings::key_of(agent))
}

/// Whether `service` (a launchd label or systemd unit) could be one of
/// Link's own: they all start `link.` or `lnk-`.
pub fn is_links(service: &str) -> bool {
    service.starts_with("link.") || service.starts_with("lnk-")
}

/// Installs agent `agent`'s service for `job` and (re)starts it, with
/// the service carrying its connections for `bridge`, if it has one.
pub fn start(agent: &str, job: &Job, bridge: Option<&Job>) -> anyhow::Result<()> {
    let (label, unit) = (label(agent), unit_name(agent));
    let (bridge_label, bridge_unit) = (bridge_label(agent), bridge_unit(agent));
    let mut services = vec![(label.as_str(), unit.as_str(), job)];
    if let Some(bridge) = bridge {
        services.push((bridge_label.as_str(), bridge_unit.as_str(), bridge));
    }
    service::start_all(&services)?;
    service::keep_after_logout("Your agent");
    Ok(())
}

/// Stops agent `agent`'s service and removes it, so it doesn't start at
/// login; and the service carrying its connections, if it has one.
pub fn stop(agent: &str) -> anyhow::Result<()> {
    // Both, then systemd reloaded once for them.
    stop_before_start(agent)?;
    service::reload();
    remove_calendar(agent);
    Ok(())
}

/// Has the service manager forget agent `agent`'s services, once
/// [`stop`]ped for good by its delete or move: none stays on its list of
/// failed services ([`service::forget`]).
pub fn forget(agent: &str) {
    service::forget(&[&unit_name(agent), &bridge_unit(agent)]);
}

/// Where a start that had the machine's calendar wake agent `agent`'s
/// scheduler (`[tool.scheduler] driver = "cron"`) keeps how to take it
/// away ([`Calendar`]).
pub fn calendar_record(agent: &str) -> std::path::PathBuf {
    crate::settings::config_of(agent).join("scheduler-cron.json")
}

/// How to take away the calendar entry waking an agent's scheduler:
/// `{"remove": ["<program>", "cron", "uninstall", "--label", "<label>"]}`,
/// naming its schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    pub remove: Vec<String>,
}

link_files::specification!(Calendar, include_str!("service/calendar.schema.toml"));

/// Each agent's scheduler calendar, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Calendar>()
}

/// Keeps how to take away the calendar entry waking agent `agent`'s
/// scheduler: `remove`, its command line.
pub fn keep_calendar(agent: &str, remove: Vec<String>) -> anyhow::Result<()> {
    let record = calendar_record(agent);
    link_files::write_private(
        &record,
        &link_files::saved::print_json(&Calendar { remove })?,
    )
    .within(format!("writing {}", record.display()))
}

/// Takes away the calendar entry waking agent `agent`'s scheduler, if a
/// start made one ([`calendar_record`]); what fails is said, and the
/// record kept for the next stop.
pub fn remove_calendar(agent: &str) {
    if let Err(err) = take_calendar(agent) {
        scheduler_still_woken(&format!("{err:#}"));
    }
}

/// [`remove_calendar`], its failure an error and the record kept: a
/// delete stops on it before anything of the agent goes, never leaving
/// the calendar starting a runner for an agent that's gone.
pub fn take_calendar(agent: &str) -> anyhow::Result<()> {
    let record = calendar_record(agent);
    let Some(argv) = std::fs::read(&record)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Calendar>(&bytes).ok())
        .map(|calendar| calendar.remove)
        .filter(|argv| argv.len() > 1)
    else {
        return Ok(());
    };
    let output = link_secretive::process::command(&argv[0])
        .args(&argv[1..])
        .output()
        .within(format!("running {}", argv[0]))?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let _ = std::fs::remove_file(&record);
    Ok(())
}

/// Said when the calendar still wakes the scheduler.
fn scheduler_still_woken(why: &str) {
    link_format::aside(
        link_format::Report::warning(link_format::sentence!(
            "Your agent's scheduler is still woken by the calendar."
        ))
        .why(link_format::Sentence::adopt(why)),
    );
}

/// Said when a service of a harness's own is turned off for Link's.
fn turned_off(service: &str) {
    link_format::aside(link_format::Note(link_format::sentence!(
        "Turned off its own background service ({}); Link runs it now.",
        link_format::Foreign::new(service)
    )));
}

/// Agent `agent`'s service's file: its plist or unit.
pub fn file(agent: &str) -> std::path::PathBuf {
    match cfg!(target_os = "macos") {
        true => service::plist_path_of(&label(agent)),
        false => service::unit_path_of(&unit_name(agent)),
    }
}

/// [`stop`], before a [`start`], which reloads systemd once for both.
/// A stop of one that ran is timed ([`crate::times`]). An agent its
/// owner's shared harness serves leaves it: it serves the rest without
/// it; and this machine's one bridge stops carrying it.
pub fn stop_before_start(agent: &str) -> anyhow::Result<()> {
    let stopping = crate::times::Stopping::begin(main_pids(agent));
    if let Ok(owner) = crate::settings::owner_of(agent) {
        let key = crate::settings::key_of(agent);
        let _ = std::fs::remove_file(crate::settings::shared_record_of(&owner, &key));
    }
    // This machine's one bridge, where it carries it, lets it go.
    crate::bridges::leave(agent);
    if bridge_installed(agent) {
        service::stop_before_start(&bridge_label(agent), &bridge_unit(agent))?;
    }
    service::stop_before_start(&label(agent), &unit_name(agent))?;
    stopping.stopped(agent);
    Ok(())
}

fn bridge_installed(agent: &str) -> bool {
    match cfg!(target_os = "macos") {
        true => service::plist_path_of(&bridge_label(agent)).exists(),
        false => service::unit_path_of(&bridge_unit(agent)).exists(),
    }
}

/// Stops the service carrying agent `agent`'s connections, if it has one.
pub fn stop_bridge(agent: &str) -> anyhow::Result<()> {
    if bridge_installed(agent) {
        service::stop(&bridge_label(agent), &bridge_unit(agent))?;
    }
    Ok(())
}

/// Held while agent `agent` runs in the foreground (`lnk agent start
/// --foreground`), so other commands can tell it's running.
pub fn foreground_lock(agent: &str) -> anyhow::Result<std::fs::File> {
    link_files::try_installation_dir()?;
    let dir = crate::settings::config_of(agent);
    link_files::create_private_dir(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("foreground.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("your agent is already running in another terminal"))?;
    // Its process, for `lnk agent list --json` (`processes`).
    let _ = lock.set_len(0);
    let _ = std::io::Write::write_all(&mut &lock, std::process::id().to_string().as_bytes());
    Ok(lock)
}

/// Whether agent `agent` runs in the foreground somewhere
/// ([`foreground_lock`]).
pub fn foreground_running(agent: &str) -> bool {
    link_files::try_installation_dir().is_ok()
        && locked(&crate::settings::config_of(agent).join("foreground.lock"))
}

/// Its service's and its connections' service's main processes, those
/// running.
fn main_pids(agent: &str) -> Vec<u32> {
    [
        status(agent).pid,
        service::status(&bridge_label(agent), &bridge_unit(agent)).pid,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The processes agent `agent` runs as: its service's and its connections'
/// service's main processes, or its start in a terminal; and, on Linux,
/// its service's own control group, which holds every process it
/// starts. As `lnk agent list --json` prints it: `{"pids", "group",
/// "groups"}`, its service's control group and each of its services'.
pub fn processes(agent: &str) -> serde_json::Value {
    processes_each(&[agent.to_string()]).remove(0)
}

/// [`processes`] of each of `agents`, their services and their connections'
/// asked at once ([`service::status_each`]).
pub fn processes_each(agents: &[String]) -> Vec<serde_json::Value> {
    let services: Vec<(String, String)> = agents
        .iter()
        .flat_map(|agent| {
            [
                (label(agent), unit_name(agent)),
                (bridge_label(agent), bridge_unit(agent)),
            ]
        })
        .collect();
    let statuses = service::status_each(&services);
    agents
        .iter()
        .zip(statuses.chunks(2))
        .map(|(agent, of)| processes_of(agent, of[0].pid, of.get(1).and_then(|s| s.pid)))
        .collect()
}

/// [`processes`] of `agent`, its service's main process `main` and its
/// connections' service's `bridge`: `groups` holds each one's own control
/// group, so what the bridge runs is measured with the agent.
fn processes_of(agent: &str, main: Option<u32>, bridge: Option<u32>) -> serde_json::Value {
    let mut pids: Vec<u32> = main.into_iter().chain(bridge).collect();
    let lock = crate::settings::config_of(agent).join("foreground.lock");
    if link_files::try_installation_dir().is_ok() && locked(&lock) {
        let pid = std::fs::read_to_string(&lock).ok();
        pids.extend(pid.and_then(|p| p.trim().parse::<u32>().ok()));
    }
    let group = main.and_then(|pid| own_group(pid, &unit_name(agent)));
    let groups: Vec<String> = group
        .iter()
        .cloned()
        .chain(bridge.and_then(|pid| own_group(pid, &bridge_unit(agent))))
        .collect();
    serde_json::json!({"pids": pids, "group": group, "groups": groups})
}

/// The control group process `pid` is in, when it's `unit`'s own
/// (cgroup v2, Linux): `/sys/fs/cgroup/.../<unit>`.
fn own_group(pid: u32, unit: &str) -> Option<String> {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?;
    let group = format!("/sys/fs/cgroup{path}");
    (path.ends_with(&format!("/{unit}")) && Path::new(&group).is_dir()).then_some(group)
}

/// Stops agent `agent` running in a terminal (`lnk agent start
/// --foreground`), as a SIGTERM to the process holding its lock, which
/// passes it on to its harness: false when none runs there. Waits for it
/// to let go of the lock, as long as a harness has to stop
/// ([`crate::waits::HarnessWaits::stop`]).
#[expect(
    clippy::disallowed_methods,
    reason = "polls a stopping agent until it ends"
)]
pub fn stop_foreground(agent: &str) -> anyhow::Result<bool> {
    let lock = crate::settings::config_of(agent).join("foreground.lock");
    if !foreground_running(agent) {
        return Ok(false);
    }
    let pid = std::fs::read_to_string(&lock)
        .ok()
        .and_then(|p| p.trim().parse::<i32>().ok())
        .filter(|&pid| pid > 1)
        .within("its terminal's process isn't said: Ctrl-C there stops it")?;
    // Timed as a service's stop is, until its terminal's lock lets go.
    let stopping = crate::times::Stopping::begin(Vec::new());
    #[cfg(unix)]
    // SAFETY: a signal to the one process, never a group.
    unsafe {
        libc::kill(pid, libc::SIGTERM)
    };
    // As long as its harness may take to finish the turns it's answering,
    // and exit.
    let start = std::time::Instant::now();
    let wait = crate::waits::HarnessWaits::get().stop();
    while locked(&lock) {
        anyhow::ensure!(
            start.elapsed() < wait,
            "{agent} in its terminal didn't stop: Ctrl-C there stops it"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    stopping.ended(agent);
    Ok(true)
}

fn locked(path: &Path) -> bool {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .is_ok_and(|lock| lock.try_lock().is_err())
}

/// Whether agent `agent` runs, as its background service or in a
/// terminal.
pub fn running_anywhere(agent: &str) -> bool {
    running(agent) || foreground_running(agent)
}

/// Whether each of `agents` runs, as [`running_anywhere`] says: their
/// services asked at once.
pub fn running_anywhere_each(agents: &[String]) -> Vec<bool> {
    let services: Vec<(String, String)> = agents
        .iter()
        .map(|agent| (label(agent), unit_name(agent)))
        .collect();
    service::running_each(&services)
        .into_iter()
        .zip(agents)
        .map(|(running, agent)| running || foreground_running(agent))
        .collect()
}

/// Whether each of `agents`' services runs, as [`running`] says: asked
/// at once.
pub fn running_each(agents: &[String]) -> Vec<bool> {
    let services: Vec<(String, String)> = agents
        .iter()
        .map(|agent| (label(agent), unit_name(agent)))
        .collect();
    service::running_each(&services)
}

/// Agent `agent`'s service: whether it runs, its process, its restarts.
pub fn status(agent: &str) -> service::Status {
    service::status(&label(agent), &unit_name(agent))
}

/// Whether agent `agent`'s service is installed and its process running.
pub fn running(agent: &str) -> bool {
    service::running(&label(agent), &unit_name(agent))
}

/// Stops and disables services a harness installed for itself (launchd
/// labels or systemd units), a launchd one's file kept in
/// `~/.lnk/safe/agents/stopped-services` at its path in the home folder
/// ([`crate::migrating::stopped_service`]), so it can be put back.
pub fn disable_foreign(services: &[String]) {
    for service in services {
        if cfg!(target_os = "macos") {
            let path = link_files::home_dir()
                .join("Library/LaunchAgents")
                .join(format!("{service}.plist"));
            if path.exists() {
                let _ =
                    service::launchctl(&["bootout", &format!("{}/{service}", service::domain())]);
                let home = link_files::home_dir();
                if let Some(kept) = link_files::Relative::within(&home, &path)
                    .and_then(|at| crate::migrating::stopped_service(&at).ok())
                {
                    if let Some(folder) = kept.parent() {
                        let _ = std::fs::create_dir_all(folder);
                    }
                    let _ = std::fs::rename(&path, kept);
                }
                turned_off(service);
            }
        } else if service::managed()
            && link_secretive::process::command("systemctl")
                .args(["--user", "cat", service])
                .output()
                .is_ok_and(|o| o.status.success())
        {
            let _ = service::systemctl(&["disable", "--now", service]);
            turned_off(service);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stop takes away the calendar entry a start made, by the command
    /// the start kept, and forgets it; one that fails is kept for the
    /// next stop; with none kept, nothing runs.
    #[test]
    fn a_stop_takes_the_calendar_away() {
        let _config = crate::settings::tests::own_config();
        let agent = "cron-agent";
        let record = calendar_record(agent);
        remove_calendar(agent);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let removed = record.with_file_name("removed");
        let keep = |argv: &[&str]| {
            let remove = argv.iter().map(|a| a.to_string()).collect();
            keep_calendar(agent, remove).unwrap();
        };
        keep(&["/bin/sh", "-c", "exit 1"]);
        remove_calendar(agent);
        assert!(record.exists());
        let script = format!("echo cron uninstall >> {}", removed.display());
        keep(&["/bin/sh", "-c", &script]);
        remove_calendar(agent);
        assert!(!record.exists());
        remove_calendar(agent);
        assert_eq!(
            std::fs::read_to_string(&removed).unwrap(),
            "cron uninstall\n"
        );
    }

    /// A delete takes the calendar entry away or fails: a removal that
    /// fails is an error naming why, its record kept, so no runner keeps
    /// starting for an agent that's gone; one that works forgets it.
    #[test]
    fn a_failed_calendar_removal_is_an_error() {
        if !crate::settings::tests::own_config() {
            return;
        }
        let agent = "cron-deleted";
        let record = calendar_record(agent);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        take_calendar(agent).unwrap();
        let keep = |argv: &[&str]| {
            let remove = argv.iter().map(|a| a.to_string()).collect();
            keep_calendar(agent, remove).unwrap();
        };
        keep(&["/bin/sh", "-c", "echo crontab refused >&2; exit 1"]);
        let err = take_calendar(agent).unwrap_err().to_string();
        assert!(err.contains("crontab refused"), "{err}");
        assert!(record.exists());
        keep(&["/bin/sh", "-c", "exit 0"]);
        take_calendar(agent).unwrap();
        assert!(!record.exists());
    }

    /// A harness in a terminal writes its log as a service's does: what
    /// it printed, from this start, where `lnk agent log show` reads it.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_terminals_harness_writes_its_log() {
        let root = link_test_kit::TempDir::new("logged");
        let path = root.path().join("logs/link.log");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "an earlier start\n").unwrap();
        let log = Log {
            file: open_log(&path).unwrap(),
            agent: Some("a1".into()),
        };
        let mut child = spawn_logged(
            std::process::Command::new("/bin/sh").args(["-c", "echo qa-out; echo qa-err >&2"]),
            &log,
        )
        .unwrap();
        assert!(child.wait().unwrap().success());
        copied(std::time::Duration::from_secs(10));
        let text = std::fs::read_to_string(&path).unwrap();
        let start: usize = std::fs::read_to_string(start_path(&path))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let this = &text[start..];
        assert!(
            this.contains("qa-out") && this.contains("qa-err"),
            "{text:?}"
        );
        assert!(!this.contains("an earlier start"), "{text:?}");
        // A record a line, each with its time and the agent's ID.
        for line in this.lines() {
            let line = link_format::log::Line::parse(line).unwrap();
            assert_eq!(line.agent.as_deref(), Some("a1"));
        }
    }

    #[test]
    fn links_services_are_known_by_their_prefix() {
        assert_eq!(unit_name("moltbot-2"), "lnk-agent-moltbot-2.service");
        // Never another agent's: no agent's unit starts `lnk-bridge-`.
        assert_eq!(bridge_unit("moltbot-2"), "lnk-bridge-moltbot-2.service");
        assert!(is_links(&bridge_label("main")));
        for links in [label("main"), unit_name("main"), "link.exit.gcp-1".into()] {
            assert!(is_links(&links), "{links}");
        }
        assert!(!is_links("ai.otter.gateway"));
        assert!(!is_links("heron-gateway.service"));
    }

    #[test]
    fn an_agents_plist_is_labelled_after_it() {
        let job = Job {
            argv: vec!["/bin/true".into()],
            env: Default::default(),
            workdir: "/".into(),
            log: "/tmp/h.log".into(),
        };
        let plist = link_service::plist(&label("main"), &job);
        assert!(plist.contains("<key>Label</key>\n\t<string>link.agent.main</string>"));
    }
}
