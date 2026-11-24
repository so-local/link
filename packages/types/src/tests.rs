use super::*;

#[test]
fn shared_shapes_read_checked() {
    let os: Os = serde_json::from_str("\"debian-13\"").unwrap();
    assert_eq!((os, os.word()), (Os::Debian13, "debian-13"));
    assert_eq!(Os::default(), Os::Ubuntu2404);
    assert!(serde_json::from_str::<Os>("\"windows\"").is_err());
    let user: User = serde_json::from_str("\"builder\"").unwrap();
    assert_eq!(user.named(), Some("builder"));
    assert!(User::root().is_root());
    let refused = serde_json::from_str::<User>("\"Bad User\"")
        .unwrap_err()
        .to_string();
    assert!(refused.contains("doesn't match"), "{refused}");
    let wait: Duration = serde_json::from_str("\"10m\"").unwrap();
    assert_eq!(wait.get(), std::time::Duration::from_secs(600));
    assert_eq!(Duration::minutes(1).to_string(), "1m");
    assert!(serde_json::from_str::<Duration>("\"soon\"").is_err());
}

#[test]
fn shared_shapes_describe_themselves() {
    let schema = serde_json::to_value(schemars::schema_for!(Os)).unwrap();
    assert_eq!(schema["enum"][0], "ubuntu-24.04");
    let schema = serde_json::to_value(schemars::schema_for!(User)).unwrap();
    assert!(schema["pattern"].is_string());
}
