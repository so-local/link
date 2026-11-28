//! `lnk agent stop` of an agent running in a terminal (`lnk agent start
//! --foreground`) ends everything started for it: its harness, and its
//! scheduler's runner (`lnk-scheduler-tool run`), which its bridge
//! started beside it; and one killed outright leaves its harness to end
//! with it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use link_test_kit::TempDir;

use crate::common;

/// A harness that takes messages from others (`from`) at its endpoint,
/// so it has a scheduler and a bridge, and runs until it's stopped. Its
/// one port, the first of its agent's block, it never serves.
const LASTING: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"proxy":false,"folders":true,"from":true,"ports":1}' ;;
  command) printf '{"argv":["/bin/sleep","300"],"ports":[%s]}' "$LNK_PORT" ;;
  endpoint) printf '"http://127.0.0.1:%s"' "$LNK_PORT" ;;
  *) exit 0 ;;
esac
"#;

/// Link scheduler standing in: its runner keeps its process's ID in
/// `runner.pid` beside it and runs until it's ended; the calendar's
/// entry put in place or taken away (`cron install`, `cron uninstall`)
/// is a line of `calendar.log` beside it; anything else (its tool's
/// definition) does nothing.
const SCHEDULER: &str = r#"#!/bin/sh
if [ "$1" = run ]; then
  echo $$ > "$(dirname "$0")/runner.pid"
  exec /bin/sleep 300
fi
[ "$1" != cron ] || echo "cron $2" >> "$(dirname "$0")/calendar.log"
exit 0
"#;

/// Link run and Link file, standing in: only defined, never called.
const TOOL: &str = "#!/bin/sh\nexit 0\n";

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    core: PathBuf,
}

impl World {
    fn new(test: &str) -> Self {
        let root = TempDir::new(&format!("stop-{test}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join("Link/Agents/main")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        for (name, script) in [
            ("lnk-lasting", LASTING),
            ("lnk-scheduler-tool", SCHEDULER),
            ("lnk-run-tool", TOOL),
            ("lnk-file-tool", TOOL),
        ] {
            link_test_kit::write_program(&bin.join(name), script).unwrap();
        }
        let core = common::core_with_sandbox(&bin);
        World {
            _root: root,
            home,
            bin,
            core,
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(crate::common::harness(&self.bin));
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(Stdio::null());
        command
    }

    fn lnk(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// `lnk agent create <name>` in a terminal, running until it's
    /// stopped.
    fn start(&self, name: &str) -> Child {
        self.start_with(name, "")
    }

    /// [`World::start`], `scheduler` more of its spec's
    /// `[tool.scheduler]`.
    fn start_with(&self, name: &str, scheduler: &str) -> Child {
        let spec = self.home.join("scheduler-on.toml");
        std::fs::write(
            &spec,
            format!("schema = 1\n[tool.scheduler]\nenabled = true\n{scheduler}"),
        )
        .unwrap();
        let spec = spec.to_str().unwrap();
        let files = self.home.join("Link/Agents").join(name).join("home");
        let files = files.display().to_string();
        let mut args = vec![
            "create",
            name,
            "--harness",
            "lasting",
            "--spec",
            spec,
            "--foreground",
            "--run.files",
            &files,
        ];
        if !link_test_kit::gate::sandbox_with_landlock() {
            args.push("--no-sandbox");
        }
        self.command(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
}

/// Whether process `pid` runs: one ended but not yet reaped (a zombie,
/// whose parent left it to whoever reaps here) doesn't.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn alive(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&output.stdout);
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

/// Waits until the start in a terminal `started` runs its harness, each
/// sandbox set up: stopped sooner, bubblewrap's own first process can
/// outlive the stop ([`common::sandboxes_set_up`]), and with no sandbox
/// begun yet every sandbox of none is set up, so the harness and its
/// proxy, inside it, are waited for too.
fn harness_running(started: &Child) {
    let start = i32::try_from(started.id()).unwrap();
    link_test_kit::eventually(
        "its harness to run, set up",
        Duration::from_secs(30),
        || {
            let found = descendants(start);
            let runs = |what: &str| found.iter().any(|(_, cmdline)| cmdline.contains(what));
            let inside = !link_test_kit::gate::sandbox_with_landlock()
                || (runs("proxy-inside") && runs("/bin/sleep\x00300"));
            (inside && common::sandboxes_set_up(found.into_iter().map(|(pid, _)| pid)))
                .then_some(())
        },
    );
}

fn runner(bin: &Path) -> Option<i32> {
    std::fs::read_to_string(bin.join("runner.pid"))
        .ok()
        .and_then(|pid| pid.trim().parse().ok())
}

#[test]
fn a_stop_ends_the_scheduler_runner_of_an_agent_in_a_terminal() {
    let world = World::new("terminal");
    let mut started = world.start("main");
    let found = link_test_kit::within("its runner to start", Duration::from_secs(60), || {
        runner(&world.bin)
    });
    if let Err(timed_out) = found {
        let _ = started.kill();
        let output = started.wait_with_output().unwrap();
        panic!("{timed_out}: {}", String::from_utf8_lossy(&output.stderr));
    }
    let pid = runner(&world.bin).unwrap();
    assert!(alive(pid));
    harness_running(&started);

    let stopped = world.lnk(&["-a", "main", "stop"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    // Its runner ends with it, not before the stop returns.
    let ended = link_test_kit::within(
        "its scheduler's runner to end",
        Duration::from_secs(10),
        || !alive(pid),
    )
    .is_err();
    // Its terminal's process has ended too.
    link_test_kit::eventually(
        "its terminal's process to end",
        Duration::from_secs(10),
        || matches!(started.try_wait(), Ok(Some(_))),
    );
    if ended {
        // SAFETY: the stand-in runner this test started, left behind.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(!ended, "its scheduler's runner ({pid}) outlived its stop");
    // Nor anything else of Link's started for it (K8).
    link_test_kit::eventually(
        "nothing of its left running",
        Duration::from_secs(10),
        || {
            let left = link_test_kit::left_under(&world._root);
            left.is_empty().then_some(()).ok_or(left)
        },
    );
}

/// An agent in a terminal whose spec says the calendar wakes its
/// scheduler (`driver = "cron"`) has the calendar's entry put in place
/// as its start in the background does, and runs no runner beside it;
/// its stop takes the entry away.
#[test]
fn an_agent_in_a_terminal_has_the_calendar_wake_its_scheduler() {
    let world = World::new("calendar");
    let calendar = world.bin.join("calendar.log");
    let mut started = world.start_with("main", "driver = \"cron\"\n");
    let installed = link_test_kit::within(
        "the calendar's entry to be put in place",
        Duration::from_secs(60),
        || std::fs::read_to_string(&calendar).is_ok_and(|log| log.contains("cron install")),
    );
    if let Err(timed_out) = installed {
        let _ = started.kill();
        let output = started.wait_with_output().unwrap();
        panic!("{timed_out}: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert_eq!(runner(&world.bin), None, "a runner ran beside it");
    harness_running(&started);
    let stopped = world.lnk(&["-a", "main", "stop"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    link_test_kit::eventually(
        "its terminal's process to end",
        Duration::from_secs(10),
        || matches!(started.try_wait(), Ok(Some(_))),
    );
    assert_eq!(
        std::fs::read_to_string(&calendar).unwrap(),
        "cron install\ncron uninstall\n"
    );
}

/// A start in a terminal killed outright (SIGKILL, which it can't pass
/// on) leaves nothing it started running to hold the agent's ports: its
/// sandbox, the sandbox's proxy (`lnk-sandbox proxy-inside`) and its
/// harness end with it, not only the start's own child.
#[test]
fn a_start_killed_outright_leaves_nothing_running() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let world = World::new("killed");
    let mut started = world.start("main");
    let start = i32::try_from(started.id()).unwrap();
    // Everything it started, once its harness (`sleep 300`) and its
    // sandbox's proxy run and each sandbox is set up: killed sooner,
    // bubblewrap's own first process can outlive it
    // ([`common::sandboxes_set_up`]).
    let found = link_test_kit::within("its harness to run", Duration::from_secs(60), || {
        let started = descendants(start);
        let runs = |what: &str| started.iter().any(|(_, cmdline)| cmdline.contains(what));
        (runs("/bin/sleep\x00300")
            && runs("proxy-inside")
            && common::sandboxes_set_up(started.iter().map(|(pid, _)| *pid)))
        .then_some(started)
    });
    let running = match found {
        Ok(running) => running,
        Err(timed_out) => {
            let _ = started.kill();
            let output = started.wait_with_output().unwrap();
            panic!("{timed_out}: {}", String::from_utf8_lossy(&output.stderr));
        }
    };
    started.kill().unwrap();
    started.wait().unwrap();
    let left = || -> Vec<&(i32, String)> {
        running
            .iter()
            .filter(|(pid, cmdline)| {
                alive(*pid)
                    && std::fs::read(format!("/proc/{pid}/cmdline"))
                        .is_ok_and(|now| String::from_utf8_lossy(&now) == *cmdline)
            })
            .collect()
    };
    let ended = link_test_kit::within("what it started to end", Duration::from_secs(10), || {
        left().is_empty().then_some(())
    });
    if ended.is_err() {
        let left: Vec<(i32, String)> = left().into_iter().cloned().collect();
        for (pid, _) in &left {
            // SAFETY: a plain kill, of what this test started, left behind.
            unsafe { libc::kill(*pid, libc::SIGKILL) };
        }
        panic!("these outlived the start: {left:?}");
    }
}

/// Every process descended from `pid`, each with its command line (its
/// arguments joined by NUL).
fn descendants(pid: i32) -> Vec<(i32, String)> {
    let parents: Vec<(i32, i32)> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let child: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let parent = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            Some((child, parent))
        })
        .collect();
    let mut found = vec![pid];
    let mut at = 0;
    while at < found.len() {
        let of = found[at];
        found.extend(parents.iter().filter(|(_, p)| *p == of).map(|(c, _)| *c));
        at += 1;
    }
    found
        .into_iter()
        .skip(1)
        .filter_map(|pid| {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            Some((pid, String::from_utf8_lossy(&cmdline).into_owned()))
        })
        .collect()
}
