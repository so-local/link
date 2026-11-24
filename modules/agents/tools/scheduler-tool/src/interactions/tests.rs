use super::*;
use link_interactions::{
    TransportEventId, WorkKind, owner, runtime, runtime::WorkAdmission, sources,
};
use std::sync::{Arc, Mutex};

struct World {
    _root: link_test_kit::TempDir,
    host: link_interactions::Store,
    tool: crate::mcp::Tool,
    binding: link_interactions::runtime::Binding,
}
fn world() -> World {
    let root = link_test_kit::TempDir::new("scheduler-interactions");
    let host = link_interactions::Store::at(root.join("interactions"));
    let binding = host
        .register_work(WorkAdmission {
            owner_route: Some("link:main".into()),
            kind: WorkKind::Conversation,
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: "link:main".into(),
            attempt: "parent".into(),
            parent_attempt: None,
            checkpoint: Some("original-input".into()),
        })
        .unwrap();
    let store = Store::new(&root.join("scheduler"))
        .unwrap()
        .with_interactions(host.clone())
        .with_interaction_agent(Some("main".into()));
    let tool = crate::mcp::Tool {
        store,
        caps: Caps {
            per_day: 100,
            subagents: 10,
            hops: 10,
            late_runs: true,
            timeout: None,
            interval: None,
        },
        teams: Teams::default(),
        address: None,
        floor: link_connection_contract::access::Standing::Owner,
        connections: Vec::new(),
    };
    World {
        _root: root,
        host,
        tool,
        binding,
    }
}
fn command(w: &World, text: &str, event: &str) -> String {
    owner::command(
        &w.host,
        &sources::owner(),
        "main",
        "link:main",
        text,
        TransportEventId::new(event).unwrap(),
    )
    .unwrap()
}
fn call(w: &World, name: &str, args: Value) -> Value {
    let current = w
        .host
        .work_step(w.binding.token.expose(), runtime::Step::Tool)
        .unwrap();
    let message = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args,
        "_meta":{"link.local/work":w.binding.token.expose(),"link.local/attempt":"parent","link.local/standing": "owner", "link.local/thread":"link:main", "link.local/control_revision":current.control_revision}}});
    crate::mcp::answer(&w.tool, &message.to_string()).unwrap()["result"].clone()
}

#[test]
fn a_held_job_survives_runner_restart_and_requires_fresh_plan_revalidation() {
    let w = world();
    let held = command(&w, "/work hold", "hold");
    let response = call(
        &w,
        "task",
        serde_json::json!({"prompt":"Find the measured cost"}),
    );
    assert_ne!(response["isError"], true, "{response}");
    let id = response["structuredContent"]["id"].as_str().unwrap();
    let before = w.tool.store.get(id).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
        recorded.lock().unwrap().push(firing.clone());
        Ok(Fired::default())
    }));
    assert!(
        look(&w.tool.store, &driver, &w.tool.caps, now(), true)
            .fired
            .is_empty()
    );
    w.tool.store.recover();
    assert_eq!(w.tool.store.get(id).unwrap(), before);
    let hold = held
        .split("; hold ")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    command(&w, &format!("/work release {hold}"), "release");
    assert!(
        look(&w.tool.store, &driver, &w.tool.caps, now(), true)
            .fired
            .is_empty()
    );
    let current = w
        .host
        .work_step(w.binding.token.expose(), runtime::Step::Model)
        .unwrap();
    w.host
        .revalidate_delegation(w.binding.token.expose(), current.control_revision, id, true)
        .unwrap();
    assert_eq!(
        look(&w.tool.store, &driver, &w.tool.caps, now(), true).fired,
        [id]
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(seen.lock().unwrap()[0].text, "Find the measured cost");
}

#[test]
fn abandoned_proposals_never_dispatch_and_do_not_consume_the_scheduler_cap() {
    let w = world();
    command(&w, "/work hold", "hold");
    let response = call(&w, "task", serde_json::json!({"prompt":"Obsolete plan"}));
    let id = response["structuredContent"]["id"].as_str().unwrap();
    let current = w
        .host
        .work_step(w.binding.token.expose(), runtime::Step::Model)
        .unwrap();
    w.host
        .revalidate_delegation(
            w.binding.token.expose(),
            current.control_revision,
            id,
            false,
        )
        .unwrap();
    let driver: Arc<dyn Driver> = Arc::new(InProcess(|_: &Firing| {
        panic!("abandoned task was dispatched")
    }));
    assert!(
        look(&w.tool.store, &driver, &w.tool.caps, now(), true)
            .fired
            .is_empty()
    );
    assert_eq!(w.tool.store.subagents(), 0);
    assert!(w.tool.store.get(id).is_none());
}

#[test]
fn retained_child_messages_preserve_the_target_and_owner_hold_across_restart() {
    let w = world();
    let task = call(
        &w,
        "task",
        serde_json::json!({"prompt":"Find the measured cost"}),
    );
    let child = task["structuredContent"]["id"].as_str().unwrap();
    assert!(ready(&w.tool.store, &w.tool.store.get(child).unwrap()).unwrap());
    w.host
        .register_work(WorkAdmission {
            owner_route: Some("link:main".into()),
            kind: WorkKind::Child { id: child.into() },
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: format!("task-{child}"),
            attempt: "child-attempt".into(),
            parent_attempt: Some("parent".into()),
            checkpoint: Some("retained child".into()),
        })
        .unwrap();
    let held = command(&w, "/work hold", "hold-messages");
    let message = call(
        &w,
        "message",
        serde_json::json!({"id":child, "message":"Keep the qualification"}),
    );
    assert_ne!(message["isError"], true, "{message}");
    let id = message["structuredContent"]["id"].as_str().unwrap();
    let job = w.tool.store.get(id).unwrap();
    assert_eq!(job.thread, format!("task-{child}"));
    assert_eq!(job.text, "Keep the qualification");
    assert_eq!(job.answer, AnswerTo::Nowhere);
    let before = w.tool.store.jobs().len();
    let foreign = call(
        &w,
        "message",
        serde_json::json!({"id":"another-coordinators-child", "message":"Wrong target"}),
    );
    assert_eq!(foreign["isError"], true, "{foreign}");
    assert_eq!(w.tool.store.jobs().len(), before);
    assert!(!ready(&w.tool.store, &job).unwrap());
    w.tool.store.recover();
    assert_eq!(w.tool.store.get(id).unwrap(), job);
    let revision = w
        .host
        .work_step(w.binding.token.expose(), runtime::Step::Model)
        .unwrap()
        .control_revision;
    assert!(
        w.host
            .revalidate_delegation(w.binding.token.expose(), revision, id, true)
            .is_err()
    );
    let hold = held
        .split("; hold ")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    command(&w, &format!("/work release {hold}"), "release-messages");
    assert!(!ready(&w.tool.store, &job).unwrap());
    let revision = w
        .host
        .work_step(w.binding.token.expose(), runtime::Step::Model)
        .unwrap()
        .control_revision;
    w.host
        .revalidate_delegation(w.binding.token.expose(), revision, id, true)
        .unwrap();
    let mut changed = job.clone();
    changed.text = "Changed after review".into();
    assert!(ready(&w.tool.store, &changed).is_err());
    assert!(ready(&w.tool.store, &job).unwrap());
}

#[test]
fn tool_timer_occurrences_are_isolated_and_definition_pause_preserves_the_job() {
    let w = world();
    let response = call(
        &w,
        "schedule",
        serde_json::json!({"prompt":"Daily briefing", "cron":"0 0 1 1 *", "recurring":false}),
    );
    let id = response["structuredContent"]["id"].as_str().unwrap();
    let job = w.tool.store.get(id).unwrap();
    let occurrence = job.occurrence.as_ref().unwrap();
    let firing = job.firing();
    assert_eq!(firing.answer, AnswerTo::OwnerThread("link:main".into()));
    assert_ne!(firing.thread, "link:main");
    command(
        &w,
        &format!("/work pause --target={}", occurrence.schedule.0),
        "pause",
    );
    let driver: Arc<dyn Driver> = Arc::new(InProcess(|_: &Firing| Ok(Fired::default())));
    assert!(
        look(&w.tool.store, &driver, &w.tool.caps, job.due, true)
            .fired
            .is_empty()
    );
    assert_eq!(w.tool.store.get(id).unwrap(), job);
    command(
        &w,
        &format!("/work resume --target={}", occurrence.schedule.0),
        "resume",
    );
    assert_eq!(
        look(&w.tool.store, &driver, &w.tool.caps, job.due, true).fired,
        [id]
    );
}

#[test]
fn delivered_child_results_release_retained_jobs_without_dispatching_the_task_again() {
    let w = world();
    let response = call(&w, "task", serde_json::json!({"prompt":"Measure once"}));
    let id = response["structuredContent"]["id"].as_str().unwrap();
    let job = w.tool.store.get(id).unwrap();
    assert!(ready(&w.tool.store, &job).unwrap());
    assert!(w.tool.store.claim(id));
    let child = w
        .host
        .register_work(WorkAdmission {
            owner_route: Some("link:main".into()),
            kind: WorkKind::Child { id: id.into() },
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: job.thread.clone(),
            attempt: "child".into(),
            parent_attempt: Some("parent".into()),
            checkpoint: Some("input".into()),
        })
        .unwrap();
    w.host
        .finish_work(child.token.expose(), Some("result".into()))
        .unwrap();
    assert_eq!(w.tool.store.subagents(), 1);
    assert!(
        w.tool.store.cancel(id).is_err(),
        "a lost driver acknowledgement cannot remove admitted work"
    );
    w.host.result_delivered("main", &job.thread).unwrap();
    let driver: Arc<dyn Driver> = Arc::new(InProcess(|_: &Firing| {
        panic!("a delivered child was repeated")
    }));
    assert!(
        look(&w.tool.store, &driver, &w.tool.caps, now(), true)
            .fired
            .is_empty()
    );
    assert!(w.tool.store.firing().is_empty());
    assert!(w.tool.store.get(id).is_none());
    assert_eq!(w.tool.store.subagents(), 0);
}

/// A subagent's firing names the conversation that started it, its
/// lineage for a harness keeping its work in a store of its own; a
/// schedule's names none.
#[test]
fn a_subagents_firing_names_the_conversation_that_started_it() {
    let w = world();
    let task = call(
        &w,
        "task",
        serde_json::json!({"prompt":"Find the deepest lake"}),
    );
    let id = task["structuredContent"]["id"].as_str().unwrap();
    let firing = w.tool.store.get(id).unwrap().firing();
    assert_eq!(firing.thread, format!("task-{id}"));
    assert_eq!(firing.parent.as_deref(), Some("link:main"));
    let later = call(
        &w,
        "schedule",
        serde_json::json!({"prompt":"Later", "cron":"0 0 1 1 *", "recurring":false}),
    );
    let id = later["structuredContent"]["id"].as_str().unwrap();
    assert_eq!(w.tool.store.get(id).unwrap().firing().parent, None);
}
