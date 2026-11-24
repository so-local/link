//! The watch and the network: no capabilities run by root, a killed watch
//! fails closed, a watch under another listener runs, only named ports.

use super::*;

/// Run by root, the sandbox has none of root's capabilities: its program
/// can't open the descriptors of the exec watch (its parent, pid 2, not
/// dumpable), as no unprivileged user's can.
#[test]
fn run_by_root_it_has_no_capabilities() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    if !link_test_kit::gate::root() {
        return;
    }
    let world = World::new("rootcaps");
    let script = r#"while read -r key value; do
[ "$key" = CapEff: ] && echo "CapEff $value"
done </proc/self/status
grep -q confine /proc/2/cmdline && echo "2 is the watch"
for f in /proc/2/fd/*; do
readlink "$f" >/dev/null 2>&1 && echo "read $f"
(: <"$f") 2>/dev/null && echo "opened $f"
done
true"#;
    let output = world
        .lnk(&[
            "run",
            "--commands.allow",
            "sh",
            "--commands.allow",
            "grep",
            "--commands.allow",
            "readlink",
            "--",
            "sh",
            "-c",
            script,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(out, "CapEff 0000000000000000\n2 is the watch\n");
}

/// A program can't kill its exec watch (pid 2) and keep running unwatched.
/// Landlock's signal scope (ABI 6+) refuses the kill; older kernels let it
/// land but then fail closed on the next exec, ending with the watch.
#[test]
fn killing_the_watch_fails_closed() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    if !crate::landlock() {
        return;
    }
    let world = World::new("killwatch");
    let script = "kill -9 2 2>/dev/null && echo killed; /bin/true && echo ran";
    let args = [
        "run",
        "--commands.allow",
        "sh",
        "--commands.allow",
        "true",
        "--",
        "sh",
        "-c",
        script,
    ];
    let output = world.lnk(&args).output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        !out.contains("killed") || (!out.contains("ran") && !output.status.success()),
        "killed its watch and kept running unwatched: {out}"
    );
}

/// Under another seccomp listener (a container runtime's, or an outer
/// sandbox's watch), the kernel gives a sandbox's watch none. It still
/// runs, but its own base filter denies memory files without assuming
/// that the outer listener seals them.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_watch_under_another_listener_still_runs() {
    if !crate::landlock() {
        return;
    }
    let Some(loader) = loader() else {
        link_test_kit::gate::lacks("a dynamic loader", "it knows x86_64's and aarch64's", None);
        return;
    };
    if !Path::new("/usr/bin/true").exists() {
        link_test_kit::gate::lacks("/usr/bin/true", "it isn't installed", None);
        return;
    }
    let me = env!("CARGO_BIN_EXE_lnk-sandbox");
    let confine = [
        "confine", "--only", "--exec", me, "--exec", "/bin/sh", "--exec",
    ];
    let output = Command::new(me)
        .args(confine)
        .arg(&loader)
        .arg("--")
        .arg(me)
        .args(confine)
        .arg(&loader)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "printf 'inner-started\\n'; exec \"$@\"",
            "inner",
            me,
            "try-exec",
            "--how",
            "memfd-proc",
        ])
        .args(["--payload", "/usr/bin/true"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with("inner-started\n"),
        "inner target did not start: {err}"
    );
    // The memory executable is refused even on a kernel without seals.
    assert_eq!(output.status.code(), Some(1), "{err}");
    assert!(
        err.contains("memfd_create is disabled for restricted programs"),
        "{err}"
    );
}

/// A sandbox reaches none of this machine's loopback ports but those its
/// spec's `[localhost]` names (`--localhost.ports`): a program here that trusts the
/// loopback (an unkeyed forwarder, say) is never reached from one.
#[test]
fn only_the_localhost_ports_named_are_reached() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("localhost");
    let serving = |text: &'static [u8]| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                let _ = std::io::Write::write_all(&mut s, text);
            }
        });
        port.to_string()
    };
    let (named, other) = (serving(b"named"), serving(b"other"));
    let script = r#"for port in "$@"; do
  if exec 3<>"/dev/tcp/127.0.0.1/$port"; then head -c 5 <&3; echo; exec 3<&-; else echo blocked; fi
done 2>/dev/null"#;
    let output = world
        .lnk(&[
            "run",
            "--commands.toolchains",
            "shell",
            "--files.write",
            ".",
            "--localhost.ports",
            &named,
            "--",
            "/bin/bash",
            "-c",
            script,
            "probe",
            &named,
            &other,
        ])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(said.lines().collect::<Vec<_>>(), ["named", "blocked"]);
}

/// A run's wire paused while it ran (here, at once, for asking for a
/// cloud's metadata service) is no longer listed paused once the run has
/// ended: its name, `run-<pid>`, is never a sandbox's again.
#[test]
fn a_runs_pause_goes_with_it() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::for_specs("paused-run");
    let spec = world.root.join("run.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"run\"\nversion = 1\n\n[commands]\ntoolchains = [\"shell\"]\n\n[internet]\nhosts = [\"*\"]\n",
    )
    .unwrap();
    let ask = r#"curl -s -m 5 -p -x "$HTTPS_PROXY" http://169.254.169.254/ >/dev/null; true"#;
    let output = world
        .lnk(&[
            "run",
            "--spec",
            &spec.display().to_string(),
            "--yes",
            "--",
            "/bin/sh",
            "-c",
            ask,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = world.lnk(&["log", "show", "--json"]).output().unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(log.contains("\"pause\""), "it was paused: {log}");
    let paused = world.lnk(&["list", "--paused", "--json"]).output().unwrap();
    let paused: serde_json::Value = serde_json::from_slice(&paused.stdout).unwrap();
    assert_eq!(paused["sandboxes"], serde_json::json!([]), "{paused}");
    // Stopping a name no sandbox has is refused; one that ran is paused.
    let nothing = world.lnk(&["stop", "no-such-sandbox"]).output().unwrap();
    assert!(!nothing.status.success());
    assert!(String::from_utf8_lossy(&nothing.stderr).contains("No sandbox named"));
    let ran = log.split('"').find(|w| w.starts_with("run-")).unwrap();
    assert!(world.lnk(&["stop", ran]).output().unwrap().status.success());
    // The list names it, ended, paused by the command; --paused keeps it.
    for args in [&["list", "--json"][..], &["list", "--paused", "--json"]] {
        let listed = world.lnk(args).output().unwrap();
        let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
        let row = listed["sandboxes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["sandbox"] == ran)
            .unwrap_or_else(|| panic!("{args:?} names {ran}: {listed}"))
            .clone();
        assert_eq!(row["state"], "ended", "{listed}");
        assert!(
            row["paused"].as_str().unwrap().contains("lnk sandbox stop"),
            "{listed}"
        );
    }
    // `start` with its name opens it again, and --paused no longer names it.
    assert!(
        world
            .lnk(&["start", ran])
            .output()
            .unwrap()
            .status
            .success()
    );
    let paused = world.lnk(&["list", "--paused", "--json"]).output().unwrap();
    let paused: serde_json::Value = serde_json::from_slice(&paused.stdout).unwrap();
    assert_eq!(paused["sandboxes"], serde_json::json!([]), "{paused}");
}
