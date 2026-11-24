//! An agent on a team: `send`, offered there alone, reaching its
//! teammates.

use super::*;

/// `send` is offered only on a team, names whom it may reach, and
/// sends to a teammate, or each of a team's, from this agent, the
/// answer back to this conversation.
#[test]
fn a_teammate_is_sent_a_message() {
    let (_dir, alone) = tool("alone", CAPS);
    let listed = |tool: &Tool| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        answer(tool, &line.to_string()).unwrap()["result"]["tools"].clone()
    };
    let names = |tools: &Value| -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(!names(&listed(&alone)).contains(&"send".to_string()));
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "send", "arguments": {"to": "writer", "message": "Hi."}}});
    assert!(answer(&alone, &line.to_string()).unwrap()["error"].is_object());

    let (_dir, tool) = on_a_team(CAPS);
    let tools = listed(&tool);
    assert!(names(&tools).contains(&"send".to_string()));
    let send = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "send")
        .unwrap();
    let to: Vec<&str> = send["inputSchema"]["properties"]["to"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    // Two writers on two teams are named by their team.
    assert_eq!(
        to,
        [
            "press",
            "research",
            "research/writer",
            "checker",
            "press/writer",
            "editor"
        ]
    );

    let sent = ask(
        &tool,
        "send",
        json!({"to": "checker", "message": "Check it."}),
        2,
    );
    assert_eq!(sent["isError"], Value::Null, "{sent}");
    let id = sent["structuredContent"]["sent"][0]["id"].as_str().unwrap();
    let job = tool.store.get(id).unwrap();
    assert_eq!(job.kind, Kind::Send);
    assert_eq!(job.thread, "agent-a1b2c3d4e5f6");
    assert_eq!(job.answer, AnswerTo::Thread("telegram:42".into()));
    assert_eq!(job.hops, 3);
    assert_eq!(job.to.as_ref().unwrap().to, "p3q4r5s6t7u8");
    assert_eq!(job.from(), "agent:a1b2c3d4e5f6");
    // The conversation it was sent from lists it, and may cancel it.
    let listed = ask(&tool, "list", json!({}), 0);
    assert_eq!(listed["structuredContent"]["jobs"][0]["id"], id);
    assert_eq!(
        ask_in(&tool, "slack:D9", "list", json!({}), 0)["structuredContent"]["jobs"],
        json!([])
    );

    // The whole team: each of its others once, in the team's
    // conversation.
    let all = ask(
        &tool,
        "send",
        json!({"to": "research", "message": "Status?"}),
        0,
    );
    let sent = all["structuredContent"]["sent"].as_array().unwrap();
    assert_eq!(sent.len(), 2);
    for each in sent {
        let job = tool.store.get(each["id"].as_str().unwrap()).unwrap();
        assert_eq!(job.thread, "team-t4m9q2x7k1ab");
    }
    // A name two teams share asks which; a stranger is refused.
    let both = ask(&tool, "send", json!({"to": "writer", "message": "x"}), 0);
    assert_eq!(both["isError"], true);
    assert!(both.to_string().contains("press/writer"), "{both}");
    let one = ask(
        &tool,
        "send",
        json!({"to": "press/writer", "message": "x"}),
        0,
    );
    assert_eq!(one["isError"], Value::Null, "{one}");
    // The same agent under two names is one teammate.
    let same = ask(&tool, "send", json!({"to": "editor", "message": "x"}), 0);
    assert_eq!(same["isError"], Value::Null, "{same}");
    for bad in [
        json!({"to": "stranger", "message": "x"}),
        json!({"to": "checker"}),
        json!({"message": "x"}),
    ] {
        assert_eq!(ask(&tool, "send", bad.clone(), 0)["isError"], true, "{bad}");
    }
    // Past the hops: not sent, and said.
    let past = ask(&tool, "send", json!({"to": "checker", "message": "x"}), 3);
    assert_eq!(past["isError"], true);
    assert!(past.to_string().contains("hops"), "{past}");
}
