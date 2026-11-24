use super::*;

fn tool() -> Tool {
    let actions = crate::actions::checked(
        r#"
["example show"]
help = "Show an example"
role = "secondary"
surfaces = ["cli", "tool"]
callers = ["agent"]
bridge = "command"
answer = "Text"
read_only = true
limits = { bytes = 4096, time_ms = 1000 }
["example show".args.name]
type = "string"
position = 1
required = true
runtime = true
["example show".args.agent]
type = "string"
long = "agent"
runtime = true
["example show".args.count]
type = "integer"
long = "count"
runtime = true
"#,
    )
    .unwrap();
    commands(&crate::actions::commands(&actions)).remove(0)
}

#[test]
fn schemas_and_calls_keep_the_declared_types_and_bound_identity() {
    let tool = tool();
    let offered = tool.offered(true);
    assert_eq!(offered["annotations"]["readOnlyHint"], true);
    assert!(offered["inputSchema"]["properties"].get("agent").is_none());
    assert_eq!(
        tool.arguments(&json!({"name":"--help", "count":2}), Some("main"))
            .unwrap(),
        [
            "example",
            "show",
            "--agent=main",
            "--count=2",
            "--",
            "--help"
        ]
    );
    for input in [
        json!({}),
        json!({"name":"x", "agent":"other"}),
        json!({"name":"x", "count":"2"}),
        json!({"name":"x", "request":"/secret"}),
    ] {
        assert!(tool.arguments(&input, Some("main")).is_err(), "{input}");
    }
}

#[test]
fn ordinary_and_hidden_commands_cannot_be_called_as_tools() {
    let tool = tool();
    let mut declarations = BTreeMap::from([(tool.path.clone(), tool.command)]);
    declarations.values_mut().next().unwrap().hidden = true;
    assert!(commands(&declarations).is_empty());
    declarations.values_mut().next().unwrap().hidden = false;
    declarations.values_mut().next().unwrap().tool = None;
    assert!(commands(&declarations).is_empty());
}

#[test]
fn manual_listing_and_one_command_have_distinct_typed_inputs() {
    let actions = crate::actions::parse(include_str!(
        "../../../../modules/tooling/manual/actions.toml"
    ))
    .unwrap();
    let tool = commands(&crate::actions::commands(&actions)).remove(0);
    assert_eq!(
        tool.arguments(&json!({"path":"services/boxes", "commands":true}), None)
            .unwrap(),
        ["manual", "show", "--commands", "--", "services/boxes"]
    );
    assert_eq!(
        tool.arguments(
            &json!({"path":"services/boxes", "command":"box create"}),
            None
        )
        .unwrap(),
        [
            "manual",
            "show",
            "--command=box create",
            "--",
            "services/boxes"
        ]
    );
    for commands in [json!(null), json!("box create")] {
        assert!(tool.arguments(&json!({"commands":commands}), None).is_err());
    }
}
