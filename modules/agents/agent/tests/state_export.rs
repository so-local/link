//! Owner state capture reuses the stopped adapter without starting a model.

use link_test_kit::{TempDir, env};

#[cfg(unix)]
#[test]
fn a_stopped_agents_state_is_exported_through_its_declared_adapter() {
    if !link_test_kit::gate::program("sh", &["-c", "exit 0"]) {
        return;
    }
    let home = TempDir::under(
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")),
        "owner-state-export",
    );
    if !link_test_kit::gate::out_of_tmp(&home) {
        return;
    }
    link_agent_contract::testing::write_agent(
        &home,
        "main",
        "harness = \"evidence-fixture\"\nsandbox = false\n",
    );
    for control in [
        home.join("Link/Safe/Agents/main/agent.toml"),
        home.join(".lnk/safe/map.toml"),
    ] {
        if let Err(error) = link_files::settings::read::<toml::Table>(&control) {
            link_test_kit::gate::lacks("trusted state-export fixture control", error, None);
            return;
        }
    }
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let command = bin.join("lnk-agent");
    link_test_kit::copy_program(
        std::path::Path::new(env!("CARGO_BIN_EXE_lnk-agent")),
        &command,
    )
    .unwrap();
    // The core beside it answers only the release's history.
    link_test_kit::write_core(&bin.join("lnk"), "#!/bin/sh\nexit 1\n").unwrap();
    // A harness is installed before its offline export, as a start would.
    let adapter = bin.join("lnk-evidence-fixture");
    link_test_kit::write_program(&adapter, "#!/bin/sh\ncase \"$1\" in\ncontract) cat \"$0.contract\";;\ninstall) ;;\nstate) [ \"$2\" = export ] || exit 19; cat \"$0.export\";;\n*) exit 19;;\nesac\n").unwrap();
    let mut contract = link_harness_contract::harness::Contract::new(false);
    contract.offline_state = true;
    contract.state.export = vec![link_harness_contract::asp::Kind::Sessions];
    std::fs::write(
        bin.join("lnk-evidence-fixture.contract"),
        serde_json::to_vec(&contract).unwrap(),
    )
    .unwrap();
    let expected = serde_json::json!({"sessions":[{"sessionId":"link:main","updates":[{
        "sessionUpdate":"user_message_chunk", "content":{"type":"text","text":"fixture record"}
    }]}]});
    std::fs::write(
        bin.join("lnk-evidence-fixture.export"),
        expected.to_string(),
    )
    .unwrap();
    let output = env::command(&command, &home)
        .args(["state", "export", "--agent", "main", "--json"])
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        expected
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("API key"));

    let unknown = env::command(&command, &home)
        .args(["state", "export", "--agent", "absent", "--json"])
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(!unknown.status.success(), "{unknown:?}");
    for output in [&unknown.stdout, &unknown.stderr] {
        assert!(
            !String::from_utf8_lossy(output).contains("fixture record"),
            "an unknown agent must not export main's state"
        );
    }
}
