//! The component declarations' unit tests.

use super::*;

/// A module reads control unless its `module.toml` says it doesn't,
/// and only a module saying so carries `reads-control` to core.
#[test]
fn a_module_reads_control_unless_it_says_otherwise() {
    let source = |extra: &str| {
        Source::parse(&format!(
            "schema = 1\n[module]\nid = \"example\"\nkind = \"module\"\n{extra}"
        ))
        .unwrap()
    };
    assert!(source("").module.reads_control);
    let declared = source("\"reads-control\" = false\n").module;
    assert!(!declared.reads_control);
    let component = |reads_control| Component {
        id: "example".into(),
        cargo_name: "link-example".into(),
        version: "1.0.0".into(),
        license: "MIT".into(),
        description: "An example".into(),
        program: "lnk-example".into(),
        kind: Kind::Module,
        reads_control,
    };
    let said = |reads| serde_json::to_value(component(reads)).unwrap();
    assert_eq!(said(true).get("reads-control"), None);
    assert_eq!(said(false)["reads-control"], serde_json::json!(false));
    let read: Component = serde_json::from_value(said(false)).unwrap();
    assert!(!read.reads_control);
}

#[test]
fn installed_answers_reject_build_metadata_and_unknown_requirements() {
    let source = r#"schema = 1
[module]
id = "example"
kind = "module"
[commands.show]
help = "Show the value."
"#;
    let component = Component {
        id: "example".into(),
        cargo_name: "link-example".into(),
        version: "1.0.0".into(),
        license: "MIT".into(),
        description: "An example".into(),
        program: "lnk-example".into(),
        kind: Kind::Module,
        reads_control: true,
    };
    let resolved = Source::parse(source)
        .unwrap()
        .resolve(component, vec![])
        .unwrap();
    let mut answer = serde_json::to_value(resolved).unwrap();
    assert!(Resolved::parse(&answer.to_string()).is_ok());
    answer["tests"] = serde_json::json!({});
    assert!(Resolved::parse(&answer.to_string()).is_err());
    assert!(Source::parse(&source.replace("kind = \"module\"", "kind = \"unknown\"")).is_err());
    assert!(Source::parse(&source.replace("id = \"example\"", "id = \"link\"")).is_err());
}

/// A description or a registry entry of a protocol this release
/// doesn't read, below the oldest it accepts or newer than its own, is
/// refused by its version before any field of its shape is read; so is
/// one stating none.
#[test]
fn a_description_of_an_older_protocol_is_refused_naming_the_upgrade() {
    let older = PROTOCOL.oldest - 1;
    let described = format!(
        r#"{{"protocol": {older}, "component": {{"program": "lnk-example"}}, "unknown": true}}"#
    );
    let refused = Resolved::parse(&described).unwrap_err().to_string();
    assert!(
        refused.starts_with(&format!(
            "lnk-example speaks the description protocol {older}, below {}",
            PROTOCOL.oldest
        )),
        "{refused}"
    );
    assert!(
        refused.ends_with("upgrade it with lnk upgrade"),
        "{refused}"
    );
    let registry = format!(r#"{{"example": {described}}}"#);
    let refused = parse_registry(registry.as_bytes()).unwrap_err().to_string();
    assert!(refused.contains("upgrade it with lnk upgrade"), "{refused}");
    let newer = PROTOCOL.current + 1;
    let described = format!(
        r#"{{"protocol": {newer}, "component": {{"program": "lnk-example"}}, "unknown": true}}"#
    );
    for refused in [
        Resolved::parse(&described).unwrap_err().to_string(),
        parse_registry(format!(r#"{{"example": {described}}}"#).as_bytes())
            .unwrap_err()
            .to_string(),
    ] {
        assert!(
            refused.starts_with(&format!(
                "lnk-example speaks the description protocol {newer}, newer than {}",
                PROTOCOL.current
            )),
            "{refused}"
        );
    }
    let unstated = r#"{"schema": 1, "component": {"program": "lnk-example"}}"#;
    let refused = Resolved::parse(unstated).unwrap_err().to_string();
    assert_eq!(
        refused,
        "lnk-example's description states no description protocol"
    );
}

/// A description is answered in the version its asker stated, and in
/// this release's own to a person asking by hand or a newer asker.
#[test]
fn a_description_is_answered_in_the_askers_version() {
    let described = format!(
        r#"{{"protocol": {}, "component": {{"id": "example", "cargo_name": "link-example", "version": "1.0.0", "license": "MIT", "description": "An example", "program": "lnk-example", "kind": "module"}}, "commands": {{}}}}"#,
        PROTOCOL.current
    );
    let resolved = Resolved::parse(&described).unwrap();
    assert_eq!(resolved.clone().stating(None).protocol, PROTOCOL.current);
    assert_eq!(
        resolved
            .clone()
            .stating(PROTOCOL.accept("a newer asker", PROTOCOL.current + 1).ok())
            .protocol,
        PROTOCOL.current
    );
    let registry = BTreeMap::from([("example".to_string(), resolved)]);
    let answered = registry_stating(
        &registry,
        PROTOCOL.accept("the asker", PROTOCOL.oldest).ok(),
    );
    assert_eq!(answered["example"].protocol, PROTOCOL.oldest);
}

/// The asker states its version in the variable: a module refuses
/// one below the oldest it accepts, naming the upgrade, and answers one
/// that states none.
#[test]
fn a_module_refuses_an_older_asker_naming_the_upgrade() {
    let current = PROTOCOL.current.to_string();
    assert_eq!(asker(None).unwrap(), None);
    assert_eq!(
        asker(Some(&current))
            .unwrap()
            .map(link_version::Accepted::version),
        Some(PROTOCOL.current)
    );
    let older = (PROTOCOL.oldest - 1).to_string();
    let refused = asker(Some(&older)).unwrap_err().to_string();
    assert!(
        refused
            .starts_with("the program asking for this description speaks the description protocol"),
        "{refused}"
    );
    assert!(
        refused.ends_with("upgrade it with lnk upgrade"),
        "{refused}"
    );
    assert!(asker(Some("one")).is_err());
    let home = link_test_kit::TempDir::new("component-asking");
    let stated = asking(&mut link_test_kit::env::command("lnk-example", home.path()))
        .get_envs()
        .find(|(name, _)| *name == PROTOCOL_VARIABLE)
        .and_then(|(_, value)| value)
        .map(std::ffi::OsStr::to_owned);
    assert_eq!(stated, Some(std::ffi::OsString::from(&current)));
}

#[test]
fn questions_are_answered_through_the_hidden_answer_command() {
    let component = Component {
        id: "example".into(),
        cargo_name: "link-example".into(),
        version: "1.0.0".into(),
        license: "MIT".into(),
        description: "An example".into(),
        program: "lnk-example".into(),
        kind: Kind::Module,
        reads_control: true,
    };
    let source = |answers: &str, answer: &str| {
        format!(
            "schema = 1\n{answers}\n[module]\nid = \"example\"\nkind = \"module\"\n[commands.\"example list\"]\nhelp = \"List.\"\n{answer}"
        )
    };
    let answer = "";
    let resolve = |answers: &str, answer: &str| {
        Source::parse(&source(answers, answer))?.resolve(component.clone(), vec![])
    };
    let resolved = resolve("answers = [\"store.folders\"]", answer).unwrap();
    assert_eq!(resolved.answers, ["store.folders"]);
    assert!(resolve("answers = [\"store.folders\", \"store.folders\"]", answer).is_err());
    assert!(resolve("answers = [\"store\"]", answer).is_err());
    // The command map of before is no list of questions.
    assert!(
        Source::parse(&source(
            "answers = { \"store.folders\" = \"example list\" }",
            answer
        ))
        .is_err()
    );
    for bad in [
        "store",
        "store.",
        ".folders",
        "Store.folders",
        "store.folders.x",
    ] {
        assert!(check_question(bad).is_err(), "{bad}");
    }
}

#[test]
fn account_setup_matches_only_its_declared_origin_and_capabilities() {
    let mut resolved: Resolved = serde_json::from_value(serde_json::json!({
        "protocol": 1,
        "component": {"id": "example", "cargo_name": "link-example",
            "program": "lnk-example", "kind": "module", "version": "1.0.0",
            "license": "MIT", "description": "An example"},
        "commands": {"answer": {"help": "Answer.", "hidden": true}},
        "answers": ["cloud.accounts"],
        "account_services": ["https://accounts.example"]
    }))
    .unwrap();
    resolved.validate().unwrap();
    assert!(resolved.after_sign_in("https://accounts.example/"));
    for unrelated in [
        "https://accounts.example.evil",
        "http://accounts.example",
        "https://accounts.example:444",
        "https://evil@accounts.example",
        "https://accounts.example/path",
        "https://accounts.example?other",
    ] {
        assert!(!resolved.after_sign_in(unrelated), "{unrelated}");
    }
    for invalid in [
        "http://accounts.example",
        "https://evil@accounts.example",
        "https://accounts.example/path",
        "https://accounts.example?other",
        "https://accounts.example/",
        "https://",
        "accounts.example",
    ] {
        resolved.account_services = vec![invalid.into()];
        assert!(resolved.validate().is_err(), "{invalid}");
    }
    resolved.account_services = vec!["https://accounts.example".into()];
    resolved.answers.clear();
    assert!(resolved.validate().is_err());
}

#[test]
fn required_programs_are_ordered_and_recommendations_do_not_select_themselves() {
    fn component(id: &str, required: &[&str], recommended: &[&str]) -> Resolved {
        serde_json::from_value(serde_json::json!({
            "protocol": 1, "component": {"id": id, "cargo_name": format!("link-{id}"),
            "program": format!("lnk-{id}"), "kind": "module", "version": "1.0.0", "license": "MIT", "description": id},
            "commands": {}, "dependencies": required.iter().chain(recommended).map(|id| ((*id).to_string(), serde_json::json!({"kind": "module", "id": id}))).collect::<BTreeMap<_,_>>(),
            "requires": required.iter().map(|id| serde_json::json!({"dependency": id})).collect::<Vec<_>>(),
            "recommends": recommended.iter().map(|id| serde_json::json!({"dependency": id})).collect::<Vec<_>>()
        })).unwrap()
    }
    let mut registry = BTreeMap::from([
        ("first".into(), component("first", &["second"], &["extra"])),
        ("second".into(), component("second", &[], &[])),
        ("extra".into(), component("extra", &[], &[])),
    ]);
    assert_eq!(
        required_programs(&registry, &["first"]).unwrap(),
        ["second", "first"]
    );
    registry.insert("second".into(), component("second", &["first"], &[]));
    assert!(
        required_programs(&registry, &["first"])
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert!(required_programs(&registry, &["missing"]).is_err());
}
