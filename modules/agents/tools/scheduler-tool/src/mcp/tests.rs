use super::*;

mod standing;
mod team;

fn tool(name: &str, caps: Caps) -> (link_test_kit::TempDir, Tool) {
    let dir = link_test_kit::TempDir::new(&format!("sched-mcp-{name}"));
    let store = Store::new(&dir).unwrap();
    (
        dir,
        Tool {
            store,
            caps,
            teams: Teams::default(),
            address: None,
            floor: link_connection_contract::access::Standing::Owner,
            connections: Vec::new(),
        },
    )
}

const CAPS: Caps = Caps {
    per_day: 100,
    subagents: 2,
    hops: 3,
    late_runs: true,
    timeout: None,
    interval: None,
};

fn ask(tool: &Tool, name: &str, arguments: Value, hops: u32) -> Value {
    ask_in(tool, "telegram:42", name, arguments, hops)
}

fn ask_in(tool: &Tool, thread: &str, name: &str, arguments: Value, hops: u32) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": name, "arguments": arguments,
        "_meta": {"link.local/standing": "owner", "link.local/thread": thread, "link.local/hops": hops}}});
    answer(tool, &line.to_string()).unwrap()["result"].clone()
}

/// A call's arguments are refused by their declaration before its
/// handler, naming what's wrong, and nothing is made.
#[test]
fn a_call_breaking_its_declaration_is_refused() {
    let (_dir, tool) = tool("declared-refusal", CAPS);
    for (arguments, says) in [
        (
            json!({"prompt": 5, "cron": "0 0 1 1 *", "recurring": false}),
            "invalid type",
        ),
        (
            json!({"prompt": "Later.", "cron": "0 0 1 1 *", "recurring": false, "soon": true}),
            "unknown field `soon`",
        ),
    ] {
        let answer = ask(&tool, "schedule", arguments, 0);
        assert_eq!(answer["isError"], true, "{answer}");
        let text = answer["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(says), "{text}");
    }
    let listed = &ask(&tool, "list", json!({}), 0)["structuredContent"];
    assert_eq!(listed["jobs"], json!([]), "{listed}");
}

/// `list` gives the soonest of many, and says how many more.
#[test]
fn a_long_list_is_cut_and_says_so() {
    let caps = Caps {
        per_day: 1000,
        ..CAPS
    };
    let (_dir, tool) = tool("cut", caps);
    for minute in (0..limits().list_max + 5).rev() {
        let at = json!({"prompt": "Later.", "cron": format!("{} {} 1 1 *", minute % 60, minute / 60), "recurring": false});
        assert_ne!(ask(&tool, "schedule", at, 0)["isError"], true);
    }
    let listed = &ask(&tool, "list", json!({}), 0)["structuredContent"];
    let jobs = listed["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), limits().list_max);
    assert_eq!(listed["cut"], 5);
    let next: Vec<&str> = jobs.iter().map(|j| j["next"].as_str().unwrap()).collect();
    assert!(next.is_sorted(), "{next:?}");
}

/// `schedules` and `unschedule`, the `cron` namespace's, see and delete
/// a conversation's schedules alone: its subagents are `list`'s and
/// `cancel`'s, never theirs.
#[test]
fn the_schedule_calls_see_schedules_alone() {
    let (_dir, tool) = tool("schedules-alone", CAPS);
    let schedule = ask(
        &tool,
        "schedule",
        json!({"prompt": "Later.", "cron": "0 0 1 1 *", "recurring": false}),
        0,
    );
    let schedule = schedule["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = ask(&tool, "task", json!({"prompt": "Look it up."}), 0);
    let task = task["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ids = |name: &str| -> Vec<String> {
        ask(&tool, name, json!({}), 0)["structuredContent"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids("schedules"), std::slice::from_ref(&schedule));
    assert_eq!(ids("list").len(), 2, "list keeps the whole view");
    let refused = ask(&tool, "unschedule", json!({"id": task}), 0);
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused.to_string().contains("cron__list"), "{refused}");
    assert!(tool.store.get(&task).is_some(), "the subagent is left");
    let deleted = ask(&tool, "unschedule", json!({"id": schedule}), 0);
    assert_eq!(
        deleted["structuredContent"]["deleted"], schedule,
        "{deleted}"
    );
    // Gone: no list shows it, ended or not, and deleted again it's refused.
    assert!(ids("schedules").is_empty());
    assert_eq!(ids("list"), std::slice::from_ref(&task));
    assert!(tool.store.ended(&schedule).is_none());
    let again = ask(&tool, "unschedule", json!({"id": schedule}), 0);
    assert_eq!(again["isError"], true, "{again}");
}

/// A delete of an id no schedule has is refused, and the schedules stay.
#[test]
fn deleting_an_unknown_schedule_leaves_the_others() {
    let (_dir, tool) = tool("unschedule-unknown", CAPS);
    let made = ask(
        &tool,
        "schedule",
        json!({"prompt": "Stays.", "cron": "0 9 1 1 *", "recurring": false}),
        0,
    );
    let kept = made["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let refused = ask(&tool, "unschedule", json!({"id": "qanosuchjob1"}), 0);
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused.to_string().contains("qanosuchjob1"), "{refused}");
    let listed = &ask(&tool, "schedules", json!({}), 0)["structuredContent"]["jobs"];
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["id"], kept.as_str());
    assert!(listed[0].get("ended").is_none(), "{listed}");
}

/// The harness keeps its conversation's compaction with `compact`,
/// declared with its arguments and listed for the namespaces, which offer
/// it to no model: kept again, it moves; the conversation's model neither
/// lists, reads nor cancels it, and an argument it doesn't take is refused.
#[test]
fn a_conversations_compaction_is_its_harnesses_alone() {
    let (_dir, tool) = tool("compact", CAPS);
    let offered = answer(
        &tool,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
    )
    .unwrap();
    let names: Vec<&str> = offered["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"compact"), "{names:?}");
    assert_eq!(
        ask(&tool, "compact", json!({"after": 60, "when": "now"}), 0)["isError"],
        true,
        "refused by its declaration"
    );
    assert!(tool.store.jobs().is_empty());
    let kept = ask(&tool, "compact", json!({"after": 3300}), 0);
    assert_eq!(kept["isError"], Value::Null, "{kept}");
    assert_ne!(
        ask(&tool, "compact", json!({"after": 60}), 0)["isError"],
        true
    );
    let id = link_scheduler_tool_contract::compaction::job("telegram:42");
    let waiting = tool.store.jobs();
    assert_eq!(waiting.len(), 1);
    assert!(
        waiting[0].due <= now() + 60,
        "moved to the later call's time"
    );
    let listed = &ask(&tool, "list", json!({}), 0)["structuredContent"]["jobs"];
    assert_eq!(listed.as_array().unwrap().len(), 0);
    assert_eq!(ask(&tool, "read", json!({"id": id}), 0)["isError"], true);
    assert_eq!(ask(&tool, "cancel", json!({"id": id}), 0)["isError"], true);
    assert!(tool.store.get(&id).is_some());
    // Kept with no time, it's gone.
    let gone = ask(&tool, "compact", json!({}), 0);
    assert_eq!(gone["structuredContent"]["cancelled"], true, "{gone}");
    assert!(tool.store.jobs().is_empty());
}

/// A conversation lists and cancels its own jobs, and its subagents',
/// never another's.
#[test]
fn a_conversation_sees_only_its_own_jobs() {
    let (_dir, tool) = tool("mine", CAPS);
    let id = |answer: &Value| {
        answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let task = id(&ask(&tool, "task", json!({"prompt": "Look it up."}), 0));
    // The subagent's own schedule is its parent's to see too.
    let its = id(&ask_in(
        &tool,
        &format!("task-{task}"),
        "schedule",
        json!({"prompt": "Again.", "cron": "0 0 1 1 *", "recurring": false}),
        1,
    ));
    let other = id(&ask_in(
        &tool,
        "slack:D42",
        "schedule",
        json!({"prompt": "Private.", "cron": "0 0 1 1 *", "recurring": false}),
        0,
    ));
    let listed = |thread: &str| -> Vec<String> {
        let mut ids: Vec<String> = ask_in(&tool, thread, "list", json!({}), 0)["structuredContent"]
            ["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    let mut ours = vec![task.clone(), its.clone()];
    ours.sort();
    assert_eq!(listed("telegram:42"), ours);
    assert_eq!(listed("slack:D42"), std::slice::from_ref(&other));
    let refused = ask(&tool, "cancel", json!({"id": other}), 0);
    assert_eq!(refused["isError"], true);
    assert!(tool.store.get(&other).is_some());
    assert_eq!(
        ask(&tool, "cancel", json!({"id": its}), 0)["isError"],
        Value::Null
    );
    // Without a conversation, nothing to list.
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "list", "arguments": {}}});
    assert_eq!(
        answer(&tool, &line.to_string()).unwrap()["result"]["isError"],
        true
    );
}

/// A subagent waits only on its own conversation's delegations, and
/// its coordinator is where it was started from, never what the
/// model says: another conversation's job, or a schedule, is refused.
#[test]
fn a_subagent_waits_on_its_own_conversations_work() {
    let caps = Caps {
        subagents: 10,
        ..CAPS
    };
    let (_dir, tool) = tool("after", caps);
    let id = |answer: &Value| {
        answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let first = id(&ask(&tool, "task", json!({"prompt": "Draft it."}), 0));
    let second = ask(
        &tool,
        "task",
        json!({"prompt": "Review it.", "after": [first]}),
        0,
    );
    assert_eq!(second["isError"], Value::Null, "{second}");
    let second = tool.store.get(&id(&second)).unwrap();
    assert_eq!(second.after, std::slice::from_ref(&first));
    assert_eq!(second.parent, None);
    // From the first's own conversation: it's the coordinator.
    let child = ask_in(
        &tool,
        &format!("task-{first}"),
        "task",
        json!({"prompt": "Check."}),
        0,
    );
    assert_eq!(
        tool.store.get(&id(&child)).unwrap().parent,
        Some(first.clone())
    );
    // Another conversation's, a schedule, or none at all: refused.
    let theirs = id(&ask_in(
        &tool,
        "slack:D42",
        "task",
        json!({"prompt": "Theirs."}),
        0,
    ));
    let later = id(&ask(
        &tool,
        "schedule",
        json!({"prompt": "Later.", "cron": "0 0 1 1 *", "recurring": false}),
        0,
    ));
    for after in [
        json!([theirs]),
        json!([later]),
        json!(["zzzzzzzzzzzz"]),
        json!("x"),
    ] {
        let refused = ask(&tool, "task", json!({"prompt": "No.", "after": after}), 0);
        assert_eq!(refused["isError"], true, "{refused}");
    }
    // Listed with what it waits on, and with how it ended once it has.
    let listed = ask(&tool, "list", json!({}), 0).to_string();
    assert!(
        listed.contains(&format!("\"after\":[\"{first}\"]")),
        "{listed}"
    );
    assert_eq!(
        ask(&tool, "cancel", json!({"id": first}), 0)["isError"],
        Value::Null
    );
    let listed = ask(&tool, "list", json!({}), 0).to_string();
    assert!(listed.contains("\"cancelled\""), "{listed}");
    let read = ask(&tool, "read", json!({"id": second.id}), 0);
    assert!(read.to_string().contains("didn't end done"), "{read}");
}

#[test]
fn subagents_and_schedules() {
    let (_dir, tool) = tool("calls", CAPS);
    let started = ask(&tool, "task", json!({"prompt": "Find three sources."}), 0);
    assert_eq!(started["isError"], Value::Null, "{started}");
    let id = started["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = tool.store.get(&id).unwrap();
    assert_eq!(job.thread, format!("task-{id}"));
    assert_eq!(job.answer, AnswerTo::Thread("telegram:42".into()));
    assert_eq!(job.hops, 1);
    ask(&tool, "task", json!({"prompt": "Another."}), 0);
    // At the cap: says so, and nothing more is started.
    let third = ask(&tool, "task", json!({"prompt": "A third."}), 0);
    assert_eq!(third["isError"], true);
    assert!(third.to_string().contains("subagents"), "{third}");
    assert_eq!(tool.store.jobs().len(), 2);

    let later = ask(
        &tool,
        "schedule",
        json!({"prompt": "Stretch.", "cron": "0 0 1 1 *", "recurring": false}),
        0,
    );
    let id = later["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = tool.store.get(&id).unwrap();
    // Once, at the cron's next match, keeping no cron.
    let once = crate::cron::Cron::parse("0 0 1 1 *").unwrap();
    assert_eq!(Some(job.due), once.next_after(now()));
    assert_eq!(job.cron, None);
    assert_eq!(job.answer, AnswerTo::Chat);
    let daily = ask(
        &tool,
        "schedule",
        json!({"prompt": "Brief me.", "cron": "0 6 * * *"}),
        0,
    );
    assert_eq!(daily["structuredContent"]["cron"], "0 6 * * *");
    let listed = ask(&tool, "list", json!({}), 0);
    assert_eq!(
        listed["structuredContent"]["jobs"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let cancelled = ask(&tool, "cancel", json!({"id": id}), 0);
    assert_eq!(cancelled["structuredContent"]["cancelled"], id.as_str());
    assert!(tool.store.get(&id).is_none());
}

#[test]
fn scheduled_answers_can_select_an_owners_connection() {
    let (_dir, mut tool) = tool("selected-service", CAPS);
    tool.connections = ["discord", "telegram", "slack"].map(String::from).into();
    // The model is offered where an answer may go, and nothing else.
    let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let offered = answer(&tool, &line.to_string()).unwrap()["result"]["tools"].clone();
    let schedule = offered
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "schedule")
        .unwrap();
    assert_eq!(
        schedule["inputSchema"]["properties"]["answer"]["enum"],
        json!(["here", "nowhere", "cli", "discord", "telegram", "slack"])
    );
    // What asks is the sandbox's to know, never the model's to read.
    assert!(!schedule.to_string().contains("x-link"), "{schedule}");
    for connection in ["discord", "telegram", "slack", "cli"] {
        let result = ask(
            &tool,
            "schedule",
            json!({"prompt":"Tell me a joke.","cron":"0 0 1 1 *","recurring":false,"answer":connection}),
            0,
        );
        assert_ne!(result["isError"], true, "{result}");
        let id = result["structuredContent"]["id"].as_str().unwrap();
        let job = tool.store.get(id).unwrap();
        assert_eq!(job.answer, AnswerTo::Connection(connection.into()));
        assert!(job.due > now() && job.cron.is_none());
        assert_eq!(job.firing().answer, job.answer);
    }
    // A service the agent isn't connected to is refused at once, never
    // left to fail when it fires.
    for bad in ["whatsapp", "../discord", "", "slack:someone-else", "cli\n"] {
        let result = ask(
            &tool,
            "schedule",
            json!({"prompt":"Never sent.","in":"3m","answer":bad}),
            0,
        );
        assert_eq!(result["isError"], true, "{result}");
    }
    let result = ask(
        &tool,
        "schedule",
        json!({"prompt":"Never sent.","in":"3m","answer":false}),
        0,
    );
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(tool.store.jobs().len(), 4);
}

fn task_with(tool: &Tool, arguments: Value, tools: Option<Value>) -> Value {
    let mut meta = json!({"link.local/standing": "owner", "link.local/thread": "telegram:42", "link.local/hops": 0});
    if let Some(tools) = tools {
        meta["link.local/tools"] = tools;
    }
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "task", "arguments": arguments, "_meta": meta}});
    answer(tool, &line.to_string()).unwrap()["result"].clone()
}

/// A subagent runs on the model it's started with, a model or an
/// alias, else on the one its parent's conversation runs on.
#[test]
fn a_subagent_runs_on_its_model_or_its_parents() {
    let caps = Caps {
        subagents: 10,
        ..CAPS
    };
    let (_dir, tool) = tool("models", caps);
    let call = |arguments: Value, model: Option<&str>| {
        let mut meta = json!({"link.local/standing": "owner", "link.local/thread": "telegram:42", "link.local/hops": 0});
        if let Some(model) = model {
            meta["link.local/model"] = json!(model);
        }
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "task", "arguments": arguments, "_meta": meta}});
        answer(&tool, &line.to_string()).unwrap()["result"].clone()
    };
    let started = |answer: Value| {
        assert_eq!(answer["isError"], Value::Null, "{answer}");
        let id = answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        tool.store.get(&id).unwrap()
    };
    let given = started(call(json!({"prompt": "Go.", "model": "high"}), Some("a")));
    assert_eq!(given.firing().model.as_deref(), Some("high"));
    let parents = started(call(json!({"prompt": "Go."}), Some("claude-opus-5-5")));
    assert_eq!(parents.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(started(call(json!({"prompt": "Go."}), None)).model, None);
    let refused = call(json!({"prompt": "Go.", "model": "two words"}), None);
    assert_eq!(refused["isError"], true, "{refused}");
}

/// A subagent gets its parent's tools, or fewer, never more; from a
/// harness that doesn't say its conversation's, all of them, and a
/// list is refused rather than ignored.
#[test]
fn a_subagent_gets_its_parents_tools_or_fewer() {
    let caps = Caps {
        subagents: 10,
        ..CAPS
    };
    let (_dir, tool) = tool("tools", caps);
    let mine = json!(["memory__search", "scheduler__task", "web__fetch"]);
    let started = |answer: &Value| {
        assert_eq!(answer["isError"], Value::Null, "{answer}");
        let id = answer["structuredContent"]["id"].as_str().unwrap();
        tool.store.get(id).unwrap()
    };
    // Fewer: those named, each once.
    let fewer = task_with(
        &tool,
        json!({"prompt": "Look.", "tools": ["memory__search", "memory__search"]}),
        Some(mine.clone()),
    );
    let job = started(&fewer);
    assert_eq!(job.tools, Some(vec!["memory__search".to_string()]));
    assert_eq!(job.firing().tools, job.tools);
    // None at all.
    let none = task_with(
        &tool,
        json!({"prompt": "Think.", "tools": []}),
        Some(mine.clone()),
    );
    assert_eq!(started(&none).tools, Some(Vec::new()));
    // Left out: all of its parent's, a narrowed parent's too.
    let all = task_with(&tool, json!({"prompt": "Go."}), Some(mine.clone()));
    assert_eq!(
        started(&all).tools,
        Some(vec![
            "memory__search".to_string(),
            "scheduler__task".to_string(),
            "web__fetch".to_string()
        ])
    );
    // More than its parent's: refused, naming what isn't.
    let jobs = tool.store.jobs().len();
    let more = task_with(
        &tool,
        json!({"prompt": "Run it.", "tools": ["memory__search", "shell__run"]}),
        Some(mine.clone()),
    );
    assert_eq!(more["isError"], true);
    assert!(more.to_string().contains("shell__run"), "{more}");
    for bad in [json!("memory__search"), json!([1])] {
        let refused = task_with(
            &tool,
            json!({"prompt": "x", "tools": bad}),
            Some(mine.clone()),
        );
        assert_eq!(refused["isError"], true, "{bad}");
    }
    // Said as something that isn't a list: no tools, never all.
    let garbled = task_with(&tool, json!({"prompt": "Go."}), Some(json!("all")));
    assert_eq!(started(&garbled).tools, Some(Vec::new()));
    // A harness that doesn't say: all, and a list refused.
    let today = task_with(&tool, json!({"prompt": "Go."}), None);
    assert_eq!(started(&today).tools, None);
    assert!(
        !tool
            .store
            .get(today["structuredContent"]["id"].as_str().unwrap())
            .map(|j| serde_json::to_string(&j).unwrap())
            .unwrap()
            .contains("tools")
    );
    let refused = task_with(
        &tool,
        json!({"prompt": "x", "tools": ["memory__search"]}),
        None,
    );
    assert_eq!(refused["isError"], true);
    assert!(refused.to_string().contains("fewer tools"), "{refused}");
    assert_eq!(tool.store.jobs().len(), jobs + 2);
}

/// The declared calls as serve offers them: `send` only on a team,
/// its choices each team and teammate, by the team's name where two
/// share a name, and each title naming what its call works on.
#[test]
fn the_declared_tools_are_filled_as_serve_says() {
    let names = |tools: &Value| -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    };
    let (_off, alone) = tool("declared-alone", CAPS);
    let offered = offered::offered(&alone).unwrap();
    assert_eq!(
        names(&offered),
        [
            "cancel",
            "compact",
            "list",
            "message",
            "read",
            "schedule",
            "schedules",
            "task",
            "unschedule"
        ]
    );
    let (_on, teamed) = on_a_team(CAPS);
    let offered = offered::offered(&teamed).unwrap();
    let send = offered
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "send")
        .cloned()
        .unwrap();
    assert_eq!(
        send["inputSchema"]["properties"]["to"]["enum"],
        json!([
            "press",
            "research",
            "research/writer",
            "checker",
            "press/writer",
            "editor"
        ])
    );
    assert_eq!(send["_meta"]["link.local/summary"], json!(["to"]));
}

fn on_a_team(caps: Caps) -> (link_test_kit::TempDir, Tool) {
    let (dir, mut tool) = tool("team", caps);
    tool.teams = Teams {
        agent: Some("a1b2c3d4e5f6".into()),
        teams: [
            ("research".to_string(), "t4m9q2x7k1ab".to_string()),
            ("press".to_string(), "t5n0r3y8l2cd".to_string()),
        ]
        .into(),
        teammates: vec![
            mate("research", "writer", "g7h8j9k0m1n2"),
            mate("research", "checker", "p3q4r5s6t7u8"),
            mate("press", "writer", "v9w8x7y6z5a4"),
            mate("press", "editor", "p3q4r5s6t7u8"),
        ],
    };
    (dir, tool)
}

fn mate(team: &str, name: &str, agent: &str) -> crate::Teammate {
    crate::Teammate {
        team: team.into(),
        name: name.into(),
        agent: agent.into(),
    }
}

#[test]
fn a_chain_stops_at_its_cap() {
    let (_dir, tool) = tool("hops", CAPS);
    let past = ask(
        &tool,
        "schedule",
        json!({"prompt": "Again.", "cron": "0 0 1 1 *", "recurring": false}),
        3,
    );
    assert_eq!(past["isError"], true);
    assert!(past.to_string().contains("hops"), "{past}");
    assert!(tool.store.jobs().is_empty());
}

#[test]
fn nonsense_is_refused() {
    let (_dir, tool) = tool("bad", CAPS);
    for (name, arguments) in [
        ("schedule", json!({"prompt": "x"})),
        (
            "schedule",
            json!({"prompt": "x", "cron": "0 0 1 1 *", "recurring": "no"}),
        ),
        (
            "schedule",
            json!({"prompt": "x", "cron": "0 0 31 2 *", "recurring": false}),
        ),
        ("schedule", json!({"prompt": "x", "cron": "61 * * * *"})),
        (
            "schedule",
            json!({"prompt": "", "cron": "0 0 1 1 *", "recurring": false}),
        ),
        ("task", json!({})),
        ("cancel", json!({"id": "../../etc"})),
    ] {
        assert_eq!(
            ask(&tool, name, arguments.clone(), 0)["isError"],
            true,
            "{arguments}"
        );
    }
    // No conversation to come back to.
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "task", "arguments": {"prompt": "x"}}});
    assert_eq!(
        answer(&tool, &line.to_string()).unwrap()["result"]["isError"],
        true
    );
}

/// A schedule call that names no conversation, from a harness that
/// can't, is made in the agent's main chat: it fires there, answers
/// there, and is listed and deleted there.
#[test]
fn a_schedule_naming_no_conversation_is_the_main_chats() {
    let (_dir, tool) = tool("main-chat", CAPS);
    let unnamed = |name: &str, arguments: Value| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments}});
        answer(&tool, &line.to_string()).unwrap()["result"].clone()
    };
    let made = unnamed(
        "schedule",
        json!({"prompt": "Stretch.", "cron": "0 9 * * *"}),
    );
    assert_ne!(made["isError"], true, "{made}");
    assert_eq!(made["structuredContent"]["thread"], "link:main");
    let id = made["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = tool.store.get(&id).unwrap();
    assert_eq!(job.thread, "link:main");
    assert_eq!(job.answer, AnswerTo::Chat);
    // The main chat's own calls see it, as do calls naming none.
    let listed = ask_in(&tool, "link:main", "schedules", json!({}), 0);
    assert_eq!(listed["structuredContent"]["jobs"][0]["id"], id.as_str());
    let listed = unnamed("schedules", json!({}));
    assert_eq!(listed["structuredContent"]["jobs"][0]["id"], id.as_str());
    let deleted = unnamed("unschedule", json!({"id": id}));
    assert_eq!(
        deleted["structuredContent"]["deleted"],
        id.as_str(),
        "{deleted}"
    );
    assert!(tool.store.get(&id).is_none());
}

/// A schedule's answer goes somewhere it can be delivered: here, in a
/// subagent's conversation, is the conversation that started it; in a
/// teammate's or a schedule's own, with no chat, it's refused when made,
/// never lost when it fires.
#[test]
fn a_schedules_here_is_where_its_answer_can_go() {
    let (_dir, tool) = tool("here", CAPS);
    let started = ask(&tool, "task", json!({"prompt": "Watch the build."}), 0);
    let task = started["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let subagent = format!("task-{task}");
    let made = ask_in(
        &tool,
        &subagent,
        "schedule",
        json!({"prompt": "Look again.", "cron": "0 0 1 1 *", "recurring": false}),
        1,
    );
    assert_ne!(made["isError"], true, "{made}");
    let id = made["structuredContent"]["id"].as_str().unwrap();
    assert_eq!(
        tool.store.get(id).unwrap().answer,
        AnswerTo::Thread("telegram:42".into())
    );
    for thread in ["agent-aaaaaaaaaaa1", "schedule-aaaaaaaaaaa2-600"] {
        for answer in [json!({}), json!({"answer": "here"})] {
            let mut arguments =
                json!({"prompt": "Never sent.", "cron": "0 0 1 1 *", "recurring": false});
            arguments
                .as_object_mut()
                .unwrap()
                .extend(answer.as_object().unwrap().clone());
            let refused = ask_in(&tool, thread, "schedule", arguments, 1);
            assert_eq!(refused["isError"], true, "{thread}: {refused}");
        }
        // Said where, it's kept.
        let kept = ask_in(
            &tool,
            thread,
            "schedule",
            json!({"prompt": "Kept.", "cron": "0 0 1 1 *", "recurring": false, "answer": "nowhere"}),
            1,
        );
        assert_ne!(kept["isError"], true, "{thread}: {kept}");
    }
}

/// Cancelling a subagent cancels the schedules it made, and once
/// cancelled it starts nothing more; one that ended done still may, its
/// children's answers coming in after its turn.
#[test]
fn a_cancelled_subagent_leaves_nothing_firing() {
    let (_dir, tool) = tool("cancelled", CAPS);
    let started = ask(&tool, "task", json!({"prompt": "Watch the build."}), 0);
    let task = started["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let subagent = format!("task-{task}");
    let later = json!({"prompt": "Look again.", "cron": "0 0 1 1 *", "recurring": false});
    let made = ask_in(&tool, &subagent, "schedule", later.clone(), 1);
    let schedule = made["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        tool.store.get(&schedule).unwrap().parent.as_deref(),
        Some(task.as_str())
    );
    assert!(tool.store.cancel(&task).unwrap());
    assert!(
        tool.store.get(&schedule).is_none(),
        "its schedule went with it"
    );
    for (name, arguments) in [
        ("schedule", later.clone()),
        ("task", json!({"prompt": "Look."})),
    ] {
        let refused = ask_in(&tool, &subagent, name, arguments, 1);
        assert_eq!(refused["isError"], true, "{name}: {refused}");
        assert!(refused.to_string().contains("cancelled"), "{refused}");
    }
    // A subagent that ended done still may: its children answer later.
    let done = ask(&tool, "task", json!({"prompt": "Plan it."}), 0);
    let done = done["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = tool.store.get(&done).unwrap();
    assert!(tool.store.claim(&done));
    tool.store
        .finish(&done, crate::How::done("Planned."))
        .unwrap();
    assert!(
        tool.store.get(&done).is_none() && tool.store.ended(&done).is_some(),
        "{job:?}"
    );
    let more = ask_in(&tool, &format!("task-{done}"), "schedule", later.clone(), 1);
    assert_ne!(more["isError"], true, "{more}");
    let left = more["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Cancelled after it answered: what it left goes, and it starts no more.
    let stopped = ask(&tool, "cancel", json!({"id": done}), 0);
    assert_ne!(stopped["isError"], true, "{stopped}");
    assert!(tool.store.get(&left).is_none(), "its schedule went with it");
    let refused = ask_in(&tool, &format!("task-{done}"), "schedule", later, 1);
    assert_eq!(refused["isError"], true, "{refused}");
}

/// A recurring schedule fires no more often than the spec's interval:
/// one more often is refused when it's made.
#[test]
fn a_schedule_repeats_no_more_often_than_the_interval() {
    let caps = Caps {
        interval: Some(std::time::Duration::from_secs(300)),
        ..CAPS
    };
    let (_dir, tool) = tool("interval", caps);
    for cron in ["* * * * *", "*/2 * * * *", "0,3 9 * * *"] {
        let refused = ask(
            &tool,
            "schedule",
            json!({"prompt": "Ping.", "cron": cron}),
            0,
        );
        assert_eq!(refused["isError"], true, "{cron}: {refused}");
        assert!(refused.to_string().contains("interval"), "{refused}");
    }
    for cron in ["*/5 * * * *", "0 6 * * 1-5"] {
        let kept = ask(
            &tool,
            "schedule",
            json!({"prompt": "Ping.", "cron": cron}),
            0,
        );
        assert_ne!(kept["isError"], true, "{cron}: {kept}");
    }
    assert_eq!(tool.store.jobs().len(), 2);
}

/// The owner's command line lists every conversation's jobs
/// (`link.local/all`), which a conversation's own list never shows
///.
#[test]
fn the_owners_list_shows_every_conversations_jobs() {
    let (_dir, tool) = tool("all", CAPS);
    let later = json!({"prompt": "Stretch.", "cron": "0 0 1 1 *", "recurring": false});
    ask_in(&tool, "telegram:42", "schedule", later.clone(), 0);
    ask_in(&tool, "slack:C0TEAM", "schedule", later, 0);
    let mine = ask_in(&tool, "telegram:42", "list", json!({}), 0);
    assert_eq!(
        mine["structuredContent"]["jobs"].as_array().unwrap().len(),
        1
    );
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "list", "arguments": {}, "_meta": {"link.local/all": true}}});
    let all = answer(&tool, &line.to_string()).unwrap()["result"].clone();
    let threads: Vec<&str> = all["structuredContent"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["thread"].as_str().unwrap())
        .collect();
    assert_eq!(threads.len(), 2, "{all}");
    assert!(threads.contains(&"telegram:42") && threads.contains(&"slack:C0TEAM"));
}

/// `schedule` is CronCreate's shape: recurring by default, keeping its
/// cron; `recurring: false` fires once at the next match, keeping none,
/// and isn't held to the interval, being no repeat.
#[test]
fn a_schedule_recurs_unless_it_says_once() {
    let caps = Caps {
        interval: Some(std::time::Duration::from_secs(300)),
        ..CAPS
    };
    let (_dir, tool) = tool("recurring", caps);
    let id = |answer: Value| {
        answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let every = id(ask(
        &tool,
        "schedule",
        json!({"prompt": "Water.", "cron": "7 9 * * *"}),
        0,
    ));
    assert_eq!(
        tool.store.get(&every).unwrap().cron.as_deref(),
        Some("7 9 * * *")
    );
    let once = json!({"prompt": "Now-ish.", "cron": "* * * * *", "recurring": false});
    let once = id(ask(&tool, "schedule", once, 0));
    let job = tool.store.get(&once).unwrap();
    assert_eq!(job.cron, None);
    assert!(job.due > now() && job.due <= now() + 60);
}

/// A subagent's description, as Agent's, is what `list` shows of it.
#[test]
fn a_subagents_description_is_listed() {
    let (_dir, tool) = tool("described", CAPS);
    let started = ask(
        &tool,
        "task",
        json!({"description": "Find sources", "prompt": "Find three sources on tides."}),
        0,
    );
    assert_ne!(started["isError"], true, "{started}");
    let listed = &ask(&tool, "list", json!({}), 0)["structuredContent"]["jobs"][0];
    assert_eq!(listed["description"], "Find sources", "{listed}");
    let long = "x".repeat(limits().description_max + 1);
    let refused = ask(
        &tool,
        "task",
        json!({"description": long, "prompt": "Go."}),
        0,
    );
    assert_eq!(refused["isError"], true, "{refused}");
}
