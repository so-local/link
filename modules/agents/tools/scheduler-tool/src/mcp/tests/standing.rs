//! What a job made from a call stands as.

use super::*;

/// A job keeps the standing of the conversation that made it, and a
/// subagent's schedule its tools too, so neither fires as more than
/// that conversation; a call that says none keeps the agent's floor, a
/// claim never goes above it, and one that isn't a standing makes
/// nothing.
#[test]
fn a_job_fires_as_no_more_than_its_conversation() {
    use link_connection_contract::access::Standing;
    let caps = Caps {
        subagents: 10,
        ..CAPS
    };
    let (_dir, tool) = tool("standing", caps);
    let made = |name: &str, arguments: Value, standing: Option<&str>| {
        let mut meta = json!({"link.local/thread": "telegram:42", "link.local/hops": 0,
            "link.local/tools": ["memory__search"]});
        if let Some(standing) = standing {
            meta["link.local/standing"] = json!(standing);
        }
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments, "_meta": meta}});
        answer(&tool, &line.to_string()).unwrap()["result"].clone()
    };
    let kept = |answer: &Value| {
        assert_eq!(answer["isError"], Value::Null, "{answer}");
        let id = answer["structuredContent"]["id"].as_str().unwrap();
        tool.store.get(id).unwrap()
    };
    let later = json!({"prompt": "Water the plants.", "cron": "0 0 1 1 *", "recurring": false});
    let schedule = kept(&made("schedule", later.clone(), Some("member")));
    assert_eq!(schedule.standing, Standing::Member);
    let firing = schedule.firing();
    assert_eq!(firing.standing, Standing::Member);
    // A chat's own tools aren't kept: a list would narrow it for good.
    assert_eq!(firing.tools, None);
    // A narrowed subagent's schedule fires with its tools, no more.
    let started = kept(&made(
        "task",
        json!({"prompt": "Look after them."}),
        Some("owner"),
    ));
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "schedule", "arguments": later.clone(), "_meta": {
            "link.local/thread": format!("task-{}", started.id), "link.local/hops": 1,
            "link.local/tools": ["memory__search"], "link.local/standing": "owner"}}});
    let subagents = kept(&answer(&tool, &line.to_string()).unwrap()["result"]);
    assert_eq!(
        subagents.firing().tools,
        Some(vec!["memory__search".to_string()])
    );
    let task = kept(&made("task", json!({"prompt": "Look."}), Some("stranger")));
    assert_eq!(task.firing().standing, Standing::Stranger);
    // Said by no harness: the agent's floor, here an owner-only agent's.
    let unsaid = kept(&made("schedule", later.clone(), None));
    assert_eq!(unsaid.firing().standing, Standing::Owner);
    let jobs = tool.store.jobs().len();
    for standing in ["admin", "Owner"] {
        let refused = made("schedule", later.clone(), Some(standing));
        assert_eq!(refused["isError"], true, "{standing}: {refused}");
        assert!(refused.to_string().contains("standing"), "{refused}");
    }
    assert_eq!(tool.store.jobs().len(), jobs);
}

/// On an agent that takes shared chats or is on a team, its floor is a
/// member's: a call that says none keeps it, and a harness claiming its
/// owner's standing is lowered to it, never raised.
#[test]
fn a_claim_never_raises_a_job_above_the_floor() {
    use link_connection_contract::access::Standing;
    let (_dir, mut tool) = tool("floor", CAPS);
    tool.floor = Standing::Member;
    let made = |standing: Option<&str>| {
        let mut meta = json!({"link.local/thread": "slack:C0TEAM", "link.local/hops": 0});
        if let Some(standing) = standing {
            meta["link.local/standing"] = json!(standing);
        }
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "schedule", "arguments": {"prompt": "Remind them.", "cron": "0 0 1 1 *", "recurring": false},
            "_meta": meta}});
        let answer = answer(&tool, &line.to_string()).unwrap()["result"].clone();
        assert_eq!(answer["isError"], Value::Null, "{answer}");
        let id = answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        tool.store.get(&id).unwrap().firing().standing
    };
    assert_eq!(made(None), Standing::Member);
    assert_eq!(made(Some("owner")), Standing::Member);
    assert_eq!(made(Some("member")), Standing::Member);
    assert_eq!(made(Some("stranger")), Standing::Stranger);
}
