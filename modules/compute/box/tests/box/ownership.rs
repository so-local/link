#[test]
fn resource_journal_capability_is_typed_and_not_human_help() {
    let dir = link_test_kit::temp::TempDir::new("box-journal-schema");
    let binary = env!("CARGO_BIN_EXE_lnk-box");
    let schema = link_test_kit::env::command(binary, dir.path())
        .arg(link_cloud_contract::ownership::SCHEMA_ARGUMENT)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: link_cloud_contract::ownership::Schema =
        serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(schema.version, 1);
    let help = link_test_kit::env::command(binary, dir.path())
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(
        !String::from_utf8_lossy(&help.stdout)
            .contains(link_cloud_contract::ownership::SCHEMA_ARGUMENT)
    );
    let extra = link_test_kit::env::command(binary, dir.path())
        .args([
            link_cloud_contract::ownership::SCHEMA_ARGUMENT,
            "unexpected",
        ])
        .output()
        .unwrap();
    assert!(
        !extra.status.success(),
        "the reserved response requires exact arguments"
    );
}
