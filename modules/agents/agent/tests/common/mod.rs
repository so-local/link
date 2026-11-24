//! What the harness's tests share: the real sandbox module behind a fake
//! core.

use std::path::Path;

/// A scenario adapter's current capability answer, with no optional features.
/// The rest of the script still owns its install, input, and launch behavior.
/// Fixtures testing another contract answer write their program directly.
/// Its folder is made if it isn't there, as a scenario's `bin` often isn't.
pub fn write_adapter(path: &Path, script: impl AsRef<str>) -> std::io::Result<()> {
    let contract = serde_json::to_string(&link_harness_contract::harness::Contract::new(false))
        .expect("the typed fixture contract serializes");
    let body = script
        .as_ref()
        .strip_prefix("#!/bin/sh\n")
        .expect("a scenario adapter is a shell program");
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    link_test_kit::write_program(
        path,
        format!(
            "#!/bin/sh\nif [ \"$#\" = 1 ] && [ \"$1\" = contract ]; then\n  printf '%s\\n' '{contract}'\n  exit 0\nfi\n{body}"
        ),
    )
}

#[test]
fn a_scenario_adapter_answers_the_current_contract_and_keeps_its_dispatch() {
    let root = link_test_kit::TempDir::new("scenario-contract");
    let program = root.join("lnk-scenario");
    write_adapter(
        &program,
        "#!/bin/sh\n[ \"$1\" = configure ] && { printf scenario-configured; exit 0; }\nexit 7\n",
    )
    .unwrap();
    let answer = link_test_kit::env::command(&program, &root)
        .arg("contract")
        .output()
        .unwrap();
    assert!(answer.status.success());
    let contract: link_harness_contract::harness::Contract =
        serde_json::from_slice(&answer.stdout).unwrap();
    assert_eq!(
        contract.version,
        link_harness_contract::harness::PROTOCOL.current
    );
    assert_eq!(
        contract,
        link_harness_contract::harness::Contract::new(false)
    );
    let configure = link_test_kit::env::command(&program, &root)
        .arg("configure")
        .output()
        .unwrap();
    assert!(configure.status.success());
    assert_eq!(configure.stdout, b"scenario-configured");
    let unknown = link_test_kit::env::command(&program, &root)
        .arg("unknown")
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(7));
}

/// A fixture's display name never comes from the real machine's hostname.
pub fn own_machine(home: &Path) {
    link_files::saved::write(
        &link_files::layout::Hidden::Installation.in_home(home),
        &link_machine::installation::Installation {
            id: link_id::candidate().unwrap(),
            name: "fixture-machine".into(),
        },
    )
    .unwrap();
}

/// The interaction store of the machine whose home is `home`.
pub fn interactions(home: &Path) -> link_interactions::Store {
    link_interactions::Store::at(link_files::layout::Hidden::Approvals.in_home(home))
}

/// Registers agent `agent`'s owner delivery as a reader of `home`'s
/// interaction store, past one question's event, as its running bridge
/// does: [`owner_reads`] then tells it's still registered.
pub fn owner_reader(home: &Path, agent: &str) {
    let store = interactions(home);
    store
        .create(
            link_interactions::sources::requester("agent", "service:m1a2i3n4s5v6", agent).unwrap(),
            link_interactions::sources::owner(),
            link_interactions::QuestionDraft {
                caller_idempotency_key: agent.into(),
                title: "A Question".into(),
                questions: vec![link_interactions::QuestionItem {
                    id: link_interactions::ItemId::new("ok").unwrap(),
                    prompt: "Go on?".into(),
                    answer_shape: link_interactions::AnswerShape::Confirmation,
                    default_answer: None,
                }],
                requested_timeout: None,
            },
            100,
        )
        .unwrap();
    let reader = format!("owner-delivery:{agent}");
    store.subscribe(&reader).unwrap();
    let newest = store.events(0).unwrap().last().unwrap().id;
    store.acknowledge(&reader, newest).unwrap();
}

/// Whether agent `agent`'s owner delivery is still a reader of `home`'s
/// interaction store, as [`owner_reader`] registered it: a forgotten
/// reader's cursor reads as a new one's, zero.
pub fn owner_reads(home: &Path, agent: &str) -> bool {
    interactions(home)
        .cursor(&format!("owner-delivery:{agent}"))
        .unwrap()
        > 0
}

/// Run beside a test's fakes, so real modules in the workspace's shared
/// build directory cannot take precedence over those on its PATH.
pub fn harness(bin: &Path) -> std::path::PathBuf {
    let program = bin.join("lnk-agent");
    if !program.exists() {
        std::fs::create_dir_all(bin).unwrap();
        link_test_kit::copy_program(Path::new(env!("CARGO_BIN_EXE_lnk-agent")), &program).unwrap();
    }
    let sandbox = bin.join("lnk-sandbox");
    if !sandbox.exists() {
        link_test_kit::copy_program(&sandbox_module(), &sandbox).unwrap();
    }
    mcp_tool(bin);
    program
}

/// Link MCP's programs, `lnk-mcp` and the `lnk-manual` it needs, beside
/// a test's fakes: a sandboxed start registers the tool with them.
pub fn mcp_tool(bin: &Path) {
    for dependency in ["lnk-mcp", "lnk-manual"] {
        let target = bin.join(dependency);
        if !target.exists() {
            link_test_kit::link_program(&link_test_kit::programs().join(dependency), &target)
                .unwrap();
        }
    }
}

/// `lnk-sandbox`, built next to `lnk-agent`.
pub fn sandbox_module() -> std::path::PathBuf {
    link_test_kit::programs().join("lnk-sandbox")
}

/// A core at `bin/lnk` that runs sandbox commands and forwards registry
/// discovery to the built core, refusing other calls, for `LNK_EXE`.
#[cfg(target_os = "linux")]
pub fn core_with_sandbox(bin: &Path) -> std::path::PathBuf {
    let lnk = bin.join("lnk");
    link_test_kit::write_core(
        &lnk,
        format!(
            "#!/bin/sh\n[ \"$1\" = sandbox ] && shift && exec '{}' \"$@\"\n[ \"$1\" = --link-registry ] && exec '{}' \"$@\"\necho \"fake lnk: $*\" >&2\nexit 1\n",
            sandbox_module().display(),
            link_test_kit::programs().join("lnk").display()
        ),
    )
    .unwrap();
    mcp_tool(bin);
    lnk
}

pub use link_agent_contract::testing::kept_key;
pub use link_agent_contract::testing::own_secrets;
pub use link_agent_contract::testing::service_of;
pub use link_agent_contract::testing::write_agent;
pub use link_agent_contract::testing::{record, set_record};

/// Whether every sandbox among `pids` is set up: each bubblewrap that
/// isn't another's child has started its own process list's first
/// process, a bubblewrap too, and that one has started what it runs.
/// Only then is that first process bound to its launcher's life
/// (`--die-with-parent` takes hold in it once it waits on its child), so
/// a start ended sooner can leave it running, under load above all.
pub fn sandboxes_set_up(pids: impl IntoIterator<Item = i32>) -> bool {
    let read = |pid: i32, file: &str| std::fs::read_to_string(format!("/proc/{pid}/{file}"));
    let is_bwrap = |pid: i32| read(pid, "comm").is_ok_and(|comm| comm.trim() == "bwrap");
    let children = |pid: i32| -> Vec<i32> {
        read(pid, &format!("task/{pid}/children"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|child| child.parse().ok())
            .collect()
    };
    let parent = |pid: i32| -> Option<i32> {
        let stat = read(pid, "stat").ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    pids.into_iter()
        .filter(|&pid| is_bwrap(pid) && !parent(pid).is_some_and(is_bwrap))
        .all(|launcher| {
            children(launcher)
                .into_iter()
                .any(|first| is_bwrap(first) && !children(first).is_empty())
        })
}

pub use link_agent_contract::testing::connection;
