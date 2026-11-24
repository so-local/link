use super::*;

#[tokio::test]
async fn protocol_errors_are_distinct_from_command_errors_and_notifications_have_no_reply() {
    let catalog = Catalog::new([], Some("main".into())).unwrap();
    assert_eq!(catalog.answer("{").await.unwrap()["error"]["code"], -32700);
    assert_eq!(catalog.answer("[]").await.unwrap()["error"]["code"], -32600);
    assert!(
        catalog
            .answer(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await
            .is_none()
    );
    for request in [
        json!({"jsonrpc":"2.0","id":[],"method":"tools/call","params":{"name":"manual"}}),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":[]}),
    ] {
        assert_eq!(
            catalog.answer(&request.to_string()).await.unwrap()["error"]["code"],
            -32600
        );
    }
    for name in [
        "agent__allow",
        "agent__tool__add",
        "login",
        "secret__get",
        "unknown",
    ] {
        let message = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":{}}});
        assert_eq!(
            catalog.answer(&message.to_string()).await.unwrap()["error"]["code"],
            -32602
        );
    }
    let answer = catalog.answer(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).await.unwrap();
    assert!(
        answer["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("not necessarily enabled")
    );
}

#[test]
fn permission_and_credential_commands_are_absent_from_the_shipped_tools() {
    let actions =
        link_component::actions::parse(include_str!("../../../agent/actions.toml")).unwrap();
    let commands = link_component::actions::commands(&actions);
    for forbidden in [
        "agent permission add",
        "agent permission remove",
        "agent tool add",
        "agent tool remove",
        "agent harness-permission add",
    ] {
        assert!(commands[forbidden].tool.is_none(), "{forbidden}");
    }
    assert!(commands["agent tool list"].tool.as_ref().unwrap().read_only);
}

#[test]
fn transport_settings_have_positive_bounds_and_a_protocol() {
    assert!(SETTINGS.message_max > 0 && SETTINGS.stderr_max > 0 && SETTINGS.calls > 0);
    assert!(!SETTINGS.protocol_version.is_empty() && !SETTINGS.instructions.is_empty());
}

#[test]
fn list_outputs_keep_their_value_in_a_valid_mcp_structured_object() {
    let result = content("[1,2]\n".into(), true).unwrap();
    assert_eq!(result["structuredContent"], json!({"result":[1,2]}));
    assert_eq!(result["content"][0]["text"], "[1,2]\n");
    assert!(content("not JSON".into(), true).is_err());
}
