//! The kill switch, `lnk sandbox stop`: after closing every wire, it runs
//! what asked to stop with it (`lnk sandbox on-stop`), naming no one.

use std::process::Command;

use link_test_kit::TempDir;

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn stop_runs_what_asked_to_stop_with_it() {
    let root = TempDir::new("stop");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // A fake lnk, noting what it's asked.
    let lnk = root.join("lnk");
    let asked = root.join("asked");
    link_test_kit::write_program(
        &lnk,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$4\" = broken ] && exit 1\nexit 0\n",
            asked.display()
        ),
    )
    .unwrap();
    let sandbox = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&home))
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_EXE", &lnk)
            .output()
            .unwrap()
    };
    for args in [
        &["on-stop", "agent-main", "--", "agent", "stop", "-a", "main"][..],
        &["on-stop", "agent-side", "--", "agent", "stop", "-a", "side"],
        &["on-stop", "agent-gone", "--", "agent", "stop", "-a", "gone"],
        &[
            "on-stop",
            "agent-worse",
            "--",
            "agent",
            "stop",
            "-a",
            "broken",
        ],
    ] {
        assert!(sandbox(args).status.success(), "{args:?}");
    }
    // Forgotten, and never the kill switch itself or a name that isn't one.
    assert!(sandbox(&["on-stop", "agent-gone"]).status.success());
    assert!(
        !sandbox(&["on-stop", "loop", "--", "sandbox", "stop"])
            .status
            .success()
    );
    assert!(
        !sandbox(&["on-stop", "../x", "--", "agent", "stop"])
            .status
            .success()
    );

    let stopped = sandbox(&["stop"]);
    assert!(stopped.status.success());
    // What it stopped is about the run: on stderr, beside the result.
    let said = String::from_utf8_lossy(&stopped.stderr);
    assert!(
        said.contains("Stopped agent-main and agent-side."),
        "{said}"
    );
    assert!(said.contains("Couldn't stop agent-worse"), "{said}");
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        "agent stop -a main\nagent stop -a side\nagent stop -a broken\n"
    );
    assert!(home.join(".lnk/safe/sandboxes/stopped").exists());
    assert!(sandbox(&["start"]).status.success());
}
