use super::*;

#[test]
fn damaged_tool_restrictions_never_become_unrestricted() {
    let dir = link_test_kit::TempDir::new("mini-tool-restrictions");
    let path = dir.path().join("tools.json");
    assert_eq!(read_tools(&path).unwrap(), None);
    std::fs::write(&path, r#"{"tools":[]}"#).unwrap();
    assert!(read_tools(&path).is_err());
    std::fs::write(&path, r#"{"tools":[],"schema":1}"#).unwrap();
    assert_eq!(read_tools(&path).unwrap(), Some(Vec::new()));
    std::fs::write(&path, r#"{"tools":["web"],"schema":1}"#).unwrap();
    assert_eq!(read_tools(&path).unwrap(), Some(vec!["web".to_string()]));
    std::fs::write(&path, r#"{"tools":[],"schema":2}"#).unwrap();
    assert!(read_tools(&path).is_err());
    std::fs::write(&path, "broken").unwrap();
    assert!(read_tools(&path).is_err());
}

#[test]
fn session_names_cannot_select_paths_or_alias_encoded_names() {
    for bad in [
        "",
        ".",
        "..",
        "../other",
        "/absolute",
        "a/b",
        "a\\b",
        "a%3Ab",
        "a\n",
    ] {
        assert!(check(bad).is_err(), "{bad:?}");
    }
    for good in ["default", "connection-fixture:42", "main.work"] {
        check(good).unwrap();
    }
}

/// A recorded call reaches the model under the name its tool and command
/// have today; one since removed, or not Link's, keeps its name.
#[test]
fn a_replayed_call_is_named_as_it_is_today() {
    use link_spec::{follow::References, history::History};
    let core = History::parse(
        "[[renamed]]\nmodule = 'memory-tool'\nnow = 'notes-tool'\nsince = '1.1.0'\nuntil = '2.0.0'\n\
         [[removed]]\nmodule = 'pager-tool'\nwhy = 'Gone.'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let history = References::from(core.release_wire([]));
    let call = |name: &str| crate::core::Call {
        id: name.into(),
        name: name.into(),
        arguments: serde_json::json!({}),
    };
    let recorded = vec![crate::core::Message {
        role: "assistant".into(),
        calls: vec![
            call("memory__search"),
            call("pager__page"),
            call("gh__list"),
        ],
        ..crate::core::Message::default()
    }];
    let names: Vec<String> = replayed(recorded, &|name| history.call(name))[0]
        .calls
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["notes__search", "pager__page", "gh__list"]);
}

/// A thread's narrowed tools are read by the names they have today, so a
/// tool renamed since stays allowed by its new name and grants no other;
/// the file keeps what it said.
#[test]
fn a_narrowed_tool_is_named_as_it_is_today() {
    use link_spec::{follow::References, history::History};
    let core = History::parse(
        "[[renamed]]\nmodule = 'memory-tool'\nnow = 'notes-tool'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let history = References::from(core.release_wire([]));
    let dir = link_test_kit::TempDir::new("mini-tool-names");
    let path = dir.path().join("tools.json");
    let saved = r#"{"tools":["memory__search","gh__list"],"schema":1}"#;
    std::fs::write(&path, saved).unwrap();
    let tools = read_tools(&path).unwrap().unwrap();
    assert_eq!(
        named(tools, &|name| history.call(name)),
        ["notes__search", "gh__list"]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
}
