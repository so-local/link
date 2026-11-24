//! Hermes and OpenClaw agents move as Link Harness's do: their native
//! state, as each adapter's `state` names it, from the stopped harness
//! at the switch, so a database and its write-ahead log arrive together;
//! never their install or `.env`; the paths of the home they left
//! rewritten to where they land.

use super::*;

#[test]
fn hermes_and_openclaw_agents_move_with_their_native_state() {
    for (harness, folder, install) in [
        ("hermes", ".hermes", "hermes-agent/run.py"),
        ("openclaw", ".openclaw", "node_modules/openclaw/index.js"),
    ] {
        let world = World::new(&format!("native-{harness}"));
        let (here, there) = (&world.here, &world.there);
        crate::common::write_agent(
            here,
            "main",
            &format!(
                "id = \"agent:{ID}\"\nharness = \"{harness}\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
                world.files(here).display()
            ),
        );
        let box_here = here.join(format!(".lnk/data/agents/main/harnesses/{harness}"));
        let state = box_here.join(folder);
        // Its memory, its sessions' database with its write-ahead log,
        // and its config naming where its home is.
        write(&state.join("memories/MEMORY.md"), "the owner likes tea");
        write(&state.join("state.db"), "SQLite format 3\0pages");
        write(&state.join("state.db-wal"), "frames not yet checkpointed");
        write(
            &state.join("config.yaml"),
            &format!("cwd: {}/work\n", box_here.display()),
        );
        write(&state.join(install), "its install");
        write(&state.join(".env"), "ANTHROPIC_API_KEY=sk-ant-test");
        let sent: Vec<(&str, Vec<u8>)> = ["memories/MEMORY.md", "state.db", "state.db-wal"]
            .into_iter()
            .map(|file| (file, std::fs::read(state.join(file)).unwrap()))
            .collect();
        // What each asks for beyond Link's defaults, allowed.
        ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start", "--yes"]));
        let box_there = there.join(format!(".lnk/data/agents/main/harnesses/{harness}"));
        let landed = box_there.join(folder);
        for (file, text) in sent {
            assert_eq!(
                std::fs::read(landed.join(file)).ok(),
                Some(text),
                "{harness}: {file}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(landed.join("config.yaml")).unwrap(),
            format!("cwd: {}/work\n", box_there.display()),
            "{harness}: its home's paths, where it landed"
        );
        assert!(!landed.join(install).exists(), "{harness}: its install");
        assert!(!landed.join(".env").exists(), "{harness}: its .env");
    }
}

/// A harness that answers at once: what an agent switches to here.
const QUIET: &str = "#!/bin/sh\ncase \"$1\" in\n  command) printf '{\"argv\":[\"/bin/true\"],\"ports\":[]}' ;;\n  *) exit 0 ;;\nesac\n";

/// A switch away from an adapter whose contract says `identity`
/// (OpenClaw's, as built) runs its `state export` in the agent's sandbox,
/// which reads the adapter's own program, by its real path: the export
/// runs there, and the agent switches.
#[test]
fn a_switch_exports_the_leaving_adapters_state_in_its_sandbox() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let world = World::new("confined-export");
    let here = &world.here;
    crate::common::write_adapter(&world.bin.join("lnk-quiet"), QUIET).unwrap();
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"{ID}\"\nharness = \"openclaw\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(here).display()
        ),
    );
    let home = here.join(".lnk/data/agents/main/harnesses/openclaw/.openclaw");
    write(&home.join("memories/MEMORY.md"), "the owner likes tea");
    let said = ok(&world.lnk_agent(&["harness", "set", "quiet", "--foreground", "--yes"]));
    assert!(said.contains("Switching from openclaw to quiet"), "{said}");
    let settings = std::fs::read_to_string(here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(settings.contains("harness = \"quiet\""), "{settings}");
}

/// A Link Harness agent starts sandboxed where a move takes it, answering
/// on its port as it did where it was: here first, then on the box, both
/// homes this machine's.
#[test]
fn a_moved_link_harness_agent_starts_sandboxed_where_it_lands() {
    if !link_test_kit::gate::sandbox_with_landlock() {
        return;
    }
    let world = World::new("link-sandboxed");
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"link\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
    let here = started(&world, &[]);
    assert!(here.contains("Link Harness answering on"), "{here}");
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start", "--yes"]));
    let there = started(&world, &["box", "run", "b1", "--"]);
    assert!(
        there.contains("Link Harness answering on"),
        "the moved agent doesn't start sandboxed where it lands (Q-move-4's far end, \
         the sandbox session's finding: its harness can't bind its port in its sandbox):\n{there}"
    );
    // Each start stopped with its sandbox: nothing of either home runs
    // but its home's sandbox bridge, which outlives an agent's stop as it
    // serves every agent there; ended here, as nothing else would.
    let others = || -> Vec<(String, String)> {
        running_under(&world.root)
            .into_iter()
            .filter(|(_, line)| !line.contains("sandbox bridge --socket"))
            .collect()
    };
    let left = link_test_kit::wait::within(
        "every process of the two starts to end",
        std::time::Duration::from_secs(15),
        || others().is_empty(),
    );
    for (pid, _) in running_under(&world.root) {
        let _ = link_test_kit::env::command("kill", &world.here)
            .arg(&pid)
            .status();
    }
    assert!(left.is_ok(), "{:#?}", others());
}

/// `lnk agent start --foreground` of `main`, through `via` (`lnk box run
/// b1 --`, for the box) or here: what it said until it answered on its
/// port, or stopped, a minute at most; then stopped as Ctrl-C stops it.
fn started(world: &World, via: &[&str]) -> String {
    let log = world.root.join(format!("started-{}.log", via.len()));
    let mut command = match via {
        [] => world.lnk_agent_command(&["start", "--foreground"]),
        via => {
            let mut lnk = link_test_kit::env::command(world.bin.join("lnk"), &world.here);
            lnk.args(via)
                .args(["agent", "start", "--foreground"])
                .env_clear()
                .env("LNK_SERVICES", "off")
                .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
                .env("LNK_EXE", world.bin.join("lnk"))
                .stdin(Stdio::null());
            lnk
        }
    };
    let file = std::fs::File::create(&log).unwrap();
    command.stdout(file.try_clone().unwrap()).stderr(file);
    let mut child = command.spawn().unwrap();
    let said = || std::fs::read_to_string(&log).unwrap_or_default();
    let _ = link_test_kit::wait::within(
        "the agent to answer on its port",
        std::time::Duration::from_secs(60),
        || said().contains("Link Harness answering on") || child.try_wait().unwrap().is_some(),
    );
    let said = said();
    // Stopped once each sandbox is set up: sooner, bubblewrap's own first
    // process can outlive the stop ([`crate::common::sandboxes_set_up`]).
    let _ = link_test_kit::wait::within(
        "each sandbox to be set up",
        std::time::Duration::from_secs(30),
        || {
            let pids = running_under(&world.root)
                .into_iter()
                .filter_map(|(pid, _)| pid.parse().ok());
            crate::common::sandboxes_set_up(pids)
        },
    );
    let _ = link_test_kit::env::command("kill", &world.here)
        .args(["-TERM", &child.id().to_string()])
        .status();
    link_test_kit::wait::exited(
        "the start to stop",
        &mut child,
        std::time::Duration::from_secs(30),
    );
    said
}

/// The processes still running from under `root`: each one's ID and
/// command line.
fn running_under(root: &Path) -> Vec<(String, String)> {
    let root = root.display().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().into_owned();
            pid.parse::<u32>().ok()?;
            let line = std::fs::read(entry.path().join("cmdline")).ok()?;
            Some((pid, String::from_utf8_lossy(&line).replace('\0', " ")))
        })
        .filter(|(_, line)| line.contains(&root))
        .collect()
}
