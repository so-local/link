//! A stop sent to `lnk sandbox run` reaches the program inside the
//! sandbox, with or without its proxy, and nothing outside it.

use std::time::Duration;

use super::*;

/// Notes each stop it's sent in `got`, says it's ready in `ready`, and
/// ends once stopped.
const TRAP: &str = r#"
import signal, sys, time
def stopped(number, frame):
    open("got", "a").write(f"{number}\n")
    sys.exit(0)
signal.signal(signal.SIGTERM, stopped)
open("ready", "w").write("ready")
while True:
    time.sleep(1)
"#;

#[test]
fn a_stop_reaches_the_program_inside_and_nothing_outside() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let world = World::new("stops");
    std::fs::write(world.work.join("trap.py"), TRAP).unwrap();
    // A process outside the sandbox, which no stop of the sandbox's reaches.
    let outside = link_test_kit::ChildGroup::spawn(
        link_test_kit::env::command("sleep", &world.home).arg("300"),
    )
    .unwrap();
    for proxy in [false, true] {
        for file in ["got", "ready"] {
            let _ = std::fs::remove_file(world.work.join(file));
        }
        let mut args = vec![
            "run",
            "--commands.toolchains",
            "python",
            "--files.write",
            ".",
        ];
        if proxy {
            args.extend(["--internet.hosts", "*"]);
        }
        args.extend(["--", "python3", "trap.py"]);
        let mut command = world.lnk(&args);
        command.stdin(std::process::Stdio::null());
        let run = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
        link_test_kit::eventually(
            "the program inside is ready",
            Duration::from_secs(30),
            || world.work.join("ready").exists() || run.try_status().unwrap().is_some(),
        );
        assert!(
            run.try_status().unwrap().is_none(),
            "proxy={proxy}: the run ended before it was stopped"
        );
        run.terminate();
        let status =
            link_test_kit::eventually("the stopped run's end", Duration::from_secs(30), || {
                run.try_status().unwrap()
            });
        assert_eq!(
            read(&world.work.join("got")),
            "15\n",
            "proxy={proxy}: the program inside heard the stop once ({status:?})"
        );
        assert_eq!(
            status.code(),
            Some(0),
            "proxy={proxy}: it ended as it chose"
        );
    }
    assert!(
        outside.try_status().unwrap().is_none(),
        "a process outside the sandbox was stopped"
    );
}
