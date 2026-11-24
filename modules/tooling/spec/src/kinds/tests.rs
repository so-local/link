use super::*;

fn description(id: &str, kinds: &[&str], bound: &[(&str, &str)]) -> Resolved {
    let mut text = serde_json::json!({
        "protocol": 1,
        "component": {
            "id": id, "cargo_name": format!("link-{id}"), "program": format!("lnk-{id}"),
            "kind": "module", "version": "0.1.0", "license": "MIT", "description": "x"
        },
        "commands": {},
    });
    for (path, kind) in bound {
        text["commands"][path] = serde_json::json!({"help": "x", "spec": kind});
    }
    let mut resolved: Resolved = serde_json::from_value(text).unwrap();
    resolved.kinds = kinds
        .iter()
        .map(|kind| Exported {
            kind: kind.to_string(),
            schema: 1,
            oldest: 1,
            command: Some(format!("lnk {kind} spec show --spec")),
            about: None,
            steps: Vec::new(),
            before: Default::default(),
            shape: serde_json::json!({"type": "object"}),
        })
        .collect();
    resolved
}

#[test]
fn each_kind_has_one_owner_and_its_consumers_from_every_module() {
    let found = owned(&[
        description("box", &["box"], &[("box create", "box")]),
        description("task", &[], &[("task run", "box")]),
        description("agent", &["team", "agent"], &[]),
    ])
    .unwrap();
    let names: Vec<&str> = found.iter().map(|o| o.kind.kind.as_str()).collect();
    assert_eq!(names, ["agent", "box", "team"]);
    assert_eq!(found[1].module, "box");
    assert_eq!(found[1].consumers, ["lnk box create", "lnk task run"]);
    let twice = owned(&[
        description("box", &["box"], &[]),
        description("other", &["box"], &[]),
    ])
    .unwrap_err();
    assert!(twice.to_string().contains("both box and other"), "{twice}");
}

#[test]
fn a_file_names_its_kind() {
    assert_eq!(kind_of("kind = \"box\"\n").unwrap().as_deref(), Some("box"));
    assert_eq!(kind_of("schema = 1\n").unwrap(), None);
    assert!(kind_of("kind = 1\n").is_err());
}

/// A file that names no kind is never owned by the kind its fields
/// happen to fit: it's told to name one.
#[test]
fn a_file_naming_no_kind_is_told_to_name_one() {
    let found = owned(&[
        description("box", &["box"], &[]),
        description("agent", &["agent"], &[]),
    ])
    .unwrap();
    assert_eq!(kind_of("cpus = 2\n").unwrap(), None);
    let said = no_kind(&found);
    assert!(said.contains("kind = \"agent\""), "{said}");
    assert!(said.contains("one of agent, box"), "{said}");
}
