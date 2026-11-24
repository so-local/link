use link_test_kit::{env, programs, temp::TempDir};

#[test]
fn owner_inspection_without_a_model_is_usage_with_an_honest_way_on() {
    let home = TempDir::new("model-owner-missing-name");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-model"), &home)
        .args(["show", "--json"])
        .env_remove("LNK_MODEL_URL")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(said.contains("Name a model"), "{said}");
    assert!(said.contains("lnk model list"), "{said}");
    assert!(!said.contains("lists known model facts"), "{said}");
    assert!(!said.contains("Report it:"), "{said}");
}

#[test]
fn inspection_refuses_invalid_names_as_usage_before_asking_a_namespace() {
    let home = TempDir::new("model-invalid-name");
    for name in ["", "bad name", "bad?name", "a/../b", "a//b"] {
        let output = env::command(env!("CARGO_BIN_EXE_lnk-model"), &home)
            .args(["show", name, "--json"])
            .env("LNK_MODEL_URL", "http://127.0.0.1:1/unused")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{name:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(said.contains("Invalid model name"), "{said}");
        assert!(!said.contains("Report it:"), "{said}");
    }
}

#[test]
fn owner_inspection_returns_the_typed_model_in_json() {
    let home = TempDir::new("model-owner-json");
    for (model, known) in [("claude-haiku-5-5", true), ("qa-unknown-model", false)] {
        let output = env::command(programs::programs().join("lnk-model"), &home)
            .args(["show", model, "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let shown: link_model_contract::show::Shown =
            serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(shown.name, model);
        assert_eq!(shown.facts.is_some(), known);
        if let Some(facts) = shown.facts {
            assert!(facts.window.unwrap() > 0);
        }
    }
}

#[test]
fn recommendation_without_a_matching_machine_keeps_its_valid_reply() {
    let home = TempDir::new("model-recommend-json");
    let output = env::command(programs::programs().join("lnk-model"), &home)
        .args(["list", "--fit", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // The names that fit, none when nothing does.
    let names = reply.as_array().unwrap();
    // Link recommends a local model only on an Apple Silicon Mac.
    if cfg!(not(target_os = "macos")) {
        assert!(names.is_empty(), "{reply}");
    }
    assert!(names.len() <= 1, "{reply}");
    for name in names {
        assert!(!name.as_str().unwrap().is_empty(), "{reply}");
    }
}
