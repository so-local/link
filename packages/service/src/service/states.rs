//! Whether services run, and how they stand: each asked on its own or many
//! read from one look at launchd or systemd, with a process's uptime.

use super::*;

/// Whether the service is installed and its process running.
pub fn running(label: &str, unit_name: &str) -> bool {
    if !managed() {
        return false;
    }
    #[cfg(target_os = "linux")]
    match crate::guest::selected() {
        Ok(Some(driver)) => return driver.status(unit_name).running,
        Err(_) => return false,
        Ok(None) => {}
    }
    if cfg!(target_os = "macos") {
        link_secretive::process::command("launchctl")
            .args(["print", &target_of(label)])
            .output()
            .is_ok_and(|o| {
                o.status.success() && String::from_utf8_lossy(&o.stdout).contains("state = running")
            })
    } else {
        systemctl_command()
            .args(["is-active", "--quiet", unit_name])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// Whether each of `services` (a label and a unit name each) is
/// installed and its process running, as [`running`] says: asked at
/// once, in one `systemctl` or `launchctl`, rather than one per service.
pub fn running_each(services: &[(String, String)]) -> Vec<bool> {
    let one_by_one = || -> Vec<bool> {
        services
            .iter()
            .map(|(label, unit)| running(label, unit))
            .collect()
    };
    if !managed() || services.is_empty() {
        return vec![false; services.len()];
    }
    #[cfg(target_os = "linux")]
    if !matches!(crate::guest::selected(), Ok(None)) {
        return one_by_one();
    }
    if cfg!(target_os = "macos") {
        let Some(jobs) = launchd_domain() else {
            return one_by_one();
        };
        return services
            .iter()
            .map(|(label, _)| jobs.get(label.as_str()).is_some_and(|pid| *pid > 0))
            .collect();
    }
    let Ok(output) = systemctl_command()
        .arg("is-active")
        .args(services.iter().map(|(_, unit)| unit))
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return vec![false; services.len()];
    };
    match active_states(&String::from_utf8_lossy(&output.stdout), services.len()) {
        Some(active) => active,
        None => one_by_one(),
    }
}

/// Of `systemctl is-active`'s answer for `count` units, one state a line,
/// which are active, as its exit status says for one; none when it isn't
/// one line each. Nothing at all is systemd out of reach (no user
/// manager, as over SSH or in a container): none runs, as asking each
/// would say too, one `systemctl` apiece.
fn active_states(out: &str, count: usize) -> Option<Vec<bool>> {
    if out.trim().is_empty() {
        return Some(vec![false; count]);
    }
    let active: Vec<bool> = out
        .lines()
        .map(|state| matches!(state.trim(), "active" | "reloading" | "refreshing"))
        .collect();
    (active.len() == count).then_some(active)
}

/// Each job loaded in Link's launchd domain, by its label, with its
/// process's id (0 when it has none): the whole domain in one `launchctl
/// print`. None when that can't be read.
fn launchd_domain() -> Option<BTreeMap<String, u32>> {
    link_secretive::process::command("launchctl")
        .args(["print", &domain()])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| launchd_jobs(&String::from_utf8_lossy(&o.stdout)))
}

/// Each job `launchctl print <domain>` lists under `services`, by its
/// label, with its process's id (0 when it has none); none when it lists
/// no `services`, or the list is cut short.
fn launchd_jobs(print: &str) -> Option<BTreeMap<String, u32>> {
    let mut lines = print.lines();
    lines.find(|line| line.trim() == "services = {")?;
    let mut jobs = BTreeMap::new();
    for line in lines {
        if line.trim() == "}" {
            return Some(jobs);
        }
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(_), Some(label)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if let Ok(pid) = pid.parse() {
            jobs.insert(label.to_string(), pid);
        }
    }
    None
}

/// A service as it is now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    /// Whether its process runs ([`running`]).
    pub running: bool,
    /// Its running process's id.
    pub pid: Option<u32>,
    /// How many times it has started since it was loaded, counting
    /// restarts after crashes.
    pub runs: Option<u32>,
}

/// The service's [`Status`], from one `launchctl print` or `systemctl
/// show`.
pub fn status(label: &str, unit_name: &str) -> Status {
    if !managed() {
        return Status::default();
    }
    #[cfg(target_os = "linux")]
    match crate::guest::selected() {
        Ok(Some(driver)) => return driver.status(unit_name),
        Err(_) => return Status::default(),
        Ok(None) => {}
    }
    if cfg!(target_os = "macos") {
        let Ok(output) = link_secretive::process::command("launchctl")
            .args(["print", &target_of(label)])
            .output()
        else {
            return Status::default();
        };
        if !output.status.success() {
            return Status::default();
        }
        launchd_status(&String::from_utf8_lossy(&output.stdout))
    } else {
        let Ok(output) = systemctl_command()
            .args(["show", "-p", "ActiveState,MainPID,NRestarts"])
            .arg(unit_name)
            .stderr(std::process::Stdio::null())
            .output()
        else {
            return Status::default();
        };
        systemd_status(&String::from_utf8_lossy(&output.stdout))
    }
}

/// Each of `services`' (label, unit) [`Status`], as [`status`] says:
/// their units asked at once, one `systemctl show` for all on Linux, and
/// on macOS one `launchctl print` of the domain, which says each job's
/// process but not how many times it ran (`runs` is none there).
pub fn status_each(services: &[(String, String)]) -> Vec<Status> {
    let one_by_one = || -> Vec<Status> {
        services
            .iter()
            .map(|(label, unit)| status(label, unit))
            .collect()
    };
    if !managed() || services.is_empty() {
        return vec![Status::default(); services.len()];
    }
    #[cfg(target_os = "linux")]
    if !matches!(crate::guest::selected(), Ok(None)) {
        return one_by_one();
    }
    if cfg!(target_os = "macos") {
        let Some(jobs) = launchd_domain() else {
            return one_by_one();
        };
        return services
            .iter()
            .map(|(label, _)| {
                let pid = jobs.get(label.as_str()).copied().filter(|pid| *pid > 0);
                Status {
                    running: pid.is_some(),
                    pid,
                    runs: None,
                }
            })
            .collect();
    }
    let Ok(output) = systemctl_command()
        .args(["show", "-p", "Id,ActiveState,MainPID,NRestarts"])
        .args(services.iter().map(|(_, unit)| unit))
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return vec![Status::default(); services.len()];
    };
    match systemd_statuses(&String::from_utf8_lossy(&output.stdout), services) {
        Some(each) => each,
        None => one_by_one(),
    }
}

/// Each of `services`' [`Status`] from one `systemctl show` of all their
/// units: a block of `Key=value` lines each, apart by an empty line, in
/// the order asked, each naming its unit (`Id=`); none when they aren't.
fn systemd_statuses(show: &str, services: &[(String, String)]) -> Option<Vec<Status>> {
    let blocks: Vec<&str> = show
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .collect();
    if blocks.len() != services.len() {
        return None;
    }
    blocks
        .iter()
        .zip(services)
        .map(|(block, (_, unit))| {
            let id = block
                .lines()
                .find_map(|line| line.trim().strip_prefix("Id="))?;
            (id == unit).then(|| systemd_status(block))
        })
        .collect()
}

/// A job's [`Status`] from what `launchctl print` said of it.
fn launchd_status(print: &str) -> Status {
    Status {
        running: print.contains("state = running"),
        pid: field(print, "pid")
            .and_then(|p| p.parse().ok())
            .filter(|p| *p > 0),
        runs: field(print, "runs").and_then(|r| r.parse().ok()),
    }
}

/// A unit's [`Status`] from `systemctl show`'s `Key=value` lines.
fn systemd_status(show: &str) -> Status {
    let value = |key: &str| {
        show.lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
    };
    Status {
        running: matches!(value("ActiveState"), Some("active" | "reloading")),
        pid: value("MainPID")
            .and_then(|p| p.parse().ok())
            .filter(|p| *p > 0),
        runs: value("NRestarts")
            .and_then(|n| n.parse::<u32>().ok())
            .map(|n| n + 1),
    }
}

/// `name = value` from `launchctl print` (the job's own, indented once).
fn field(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(" = ")?;
        (key == name).then(|| value.trim().to_string())
    })
}

/// How long process `pid` has run: on Linux from `/proc`, without a
/// process; else as `ps` says.
pub fn uptime(pid: u32) -> Option<std::time::Duration> {
    if cfg!(target_os = "linux") {
        return proc_uptime(pid);
    }
    let output = link_secretive::process::command("ps")
        .args(["-o", "etime=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    parse_etime(String::from_utf8_lossy(&output.stdout).trim())
}

/// How long process `pid` has run, as `/proc` says: the machine's
/// uptime less when the process started after it.
fn proc_uptime(pid: u32) -> Option<std::time::Duration> {
    let started = crate::started(pid)?;
    // SAFETY: sysconf reads a constant.
    let ticks = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) })
        .ok()
        .filter(|t| *t > 0)?;
    let up: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let since_boot = std::time::Duration::from_secs_f64(up);
    Some(since_boot.saturating_sub(std::time::Duration::from_millis(
        started.saturating_mul(1000) / ticks,
    )))
}

/// `ps`'s elapsed time: `[[dd-]hh:]mm:ss`.
fn parse_etime(text: &str) -> Option<std::time::Duration> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let mut seconds = 0;
    for part in rest.split(':') {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(std::time::Duration::from_secs(days * 86_400 + seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn uptime_from_proc() {
        // This test's process started moments ago, and a process that
        // isn't has none.
        let up = super::uptime(std::process::id()).unwrap();
        assert!(up < std::time::Duration::from_secs(600), "{up:?}");
        assert_eq!(super::uptime(u32::MAX), None);
    }

    #[test]
    fn elapsed_times_and_fields() {
        let secs = |t| parse_etime(t).map(|d| d.as_secs());
        assert_eq!(secs("00:05"), Some(5));
        assert_eq!(secs("02:14:00"), Some(2 * 3600 + 14 * 60));
        assert_eq!(secs("3-00:00:01"), Some(3 * 86_400 + 1));
        assert_eq!(secs(""), None);
        let print = "gui/501/link.agent.main = {\n\tstate = running\n\truns = 3\n\tpid = 4242\n}";
        assert_eq!(field(print, "pid").as_deref(), Some("4242"));
        assert_eq!(field(print, "runs").as_deref(), Some("3"));
        assert_eq!(
            launchd_status(print),
            Status {
                running: true,
                pid: Some(4242),
                runs: Some(3)
            }
        );
        assert_eq!(
            systemd_status("ActiveState=active\nMainPID=77\nNRestarts=2\n"),
            Status {
                running: true,
                pid: Some(77),
                runs: Some(3)
            }
        );
        assert_eq!(
            systemd_status("MainPID=0\nNRestarts=0\nActiveState=inactive\n"),
            Status {
                running: false,
                pid: None,
                runs: Some(1)
            }
        );
    }

    /// One `systemctl show` of many units: a block each, in order, each
    /// taken as its unit's only when it names it.
    #[test]
    fn many_units_are_read_from_one_show() {
        let services: Vec<(String, String)> = ["a.service", "b.service"]
            .iter()
            .map(|unit| (String::new(), unit.to_string()))
            .collect();
        let show = "Id=a.service\nActiveState=active\nMainPID=7\nNRestarts=0\n\n\
                    MainPID=0\nId=b.service\nNRestarts=0\nActiveState=inactive\n";
        let each = systemd_statuses(show, &services).unwrap();
        assert_eq!(
            each,
            [
                Status {
                    running: true,
                    pid: Some(7),
                    runs: Some(1)
                },
                Status {
                    running: false,
                    pid: None,
                    runs: Some(1)
                }
            ]
        );
        // Out of order, or one short: not taken.
        let swapped = "Id=b.service\nActiveState=active\n\nId=a.service\nActiveState=active\n";
        assert!(systemd_statuses(swapped, &services).is_none());
        assert!(systemd_statuses("Id=a.service\nActiveState=active\n", &services).is_none());
    }

    #[test]
    fn launchds_jobs_from_one_print_of_the_domain() {
        let print = "gui/501 = {\n\ttype = gui\n\tservices = {\n\t\t   45847    127 \tlink.agent.main\n\
                     \t\t       0     78 \tlink.bridge.main\n\t\t    2280      - \tlink.agent.b6s4\n\t}\n\
                     \tendpoints = {\n\t\t  0x1 M A com.apple.other\n\t}\n}\n";
        let jobs = launchd_jobs(print).unwrap();
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs["link.agent.main"], 45847);
        assert_eq!(jobs["link.bridge.main"], 0);
        assert_eq!(jobs["link.agent.b6s4"], 2280);
        // No list of services, or one cut short: asked one by one instead.
        assert_eq!(launchd_jobs("gui/501 = {\n}\n"), None);
        assert_eq!(launchd_jobs("\tservices = {\n\t\t 1 - a\n"), None);
    }

    #[test]
    fn systemds_states_one_line_each() {
        assert_eq!(
            active_states("active\ninactive\nfailed\nreloading\n", 4),
            Some(vec![true, false, false, true])
        );
        // Not one line each: asked one by one instead.
        assert_eq!(active_states("active\n", 2), None);
        // Nothing: systemd out of reach, so none runs, unasked again.
        assert_eq!(active_states("", 2), Some(vec![false, false]));
    }
}
