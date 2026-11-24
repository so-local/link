//! Runtime observations cross the actual local MCP and Store boundaries.

use super::*;
use link_harness::event::Event;
use link_harness_wire::evidence::{
    Outcome, Phase, Record,
    index::{self, Located},
};
use std::sync::atomic::Ordering;
mod fixture;
use fixture::Mcp;

fn events(world: &World, folder: &str) -> Vec<Event> {
    world
        .raw_events(folder)
        .into_iter()
        .map(|value| serde_json::from_value(value).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_reply_is_committed_while_listing_and_sibling_are_blocked() {
    let tool = Mcp::new().await;
    let world = world_calling("evidence-parallel", false, 2, memory(&tool.url)).await;
    ask(
        &world.endpoint,
        "telegram:prime",
        "Prime the persisted catalogue",
    )
    .await;
    link_test_kit::eventually_async(
        "priming turn saved its catalogue",
        std::time::Duration::from_secs(5),
        || world.root.join("home/tool-lists.json").is_file(),
    )
    .await;
    // The catalogue stays offered after an idle session ends. Its next
    // call starts a fresh session and refreshes beside the actual request.
    world
        .server
        .harness
        .end_idle_tools_within(std::time::Duration::ZERO, AGE)
        .await;
    let endpoint = world.endpoint.clone();
    let server = world.server.clone();
    tool.control.holding.store(true, Ordering::SeqCst);
    let asking = ask(&endpoint, "telegram:evidence", "Read both facts");
    let proof = async {
        link_test_kit::eventually_async("listing blocked and first result appended", std::time::Duration::from_secs(5), || {
            let stored = events(&world, "telegram-evidence");
            let first = stored.iter().any(|event|
                matches!(event.evidence.as_ref().map(|e| &e.record), Some(Record::Finished { call, outcome: Outcome::Completed { .. }, .. }) if call.ordinal == 0))
            ;
            let listing = tool.control.listing.load(Ordering::SeqCst);
            if listing && first { Ok(()) } else { Err(format!("listing={listing}, first={first}, effects={}, events={}", tool.control.effects.load(Ordering::SeqCst), stored.len())) }
        }).await;
        let stored = events(&world, "telegram-evidence");
        let trace = index::index(stored.iter().filter_map(|event| {
            event.evidence.as_ref().map(|evidence| Located {
                seq: event.seq,
                evidence,
            })
        }))
        .unwrap();
        assert_eq!(trace.executions.len(), 2);
        let first = trace.executions.values().next().unwrap();
        let second = trace.executions.values().nth(1).unwrap();
        assert!(first.finished.is_some());
        assert!(second.finished.is_none());
        assert!(
            second.dispatched_ns.unwrap() <= first.finished.as_ref().unwrap().offset_ns.unwrap()
        );
        assert_eq!(tool.control.release_list.available_permits(), 0);
        assert_eq!(tool.control.release_second.available_permits(), 0);
        tool.control.release_list.add_permits(1);
        tool.control.release_second.add_permits(1);
    };
    let (answer, ()) = tokio::join!(asking, proof);
    assert!(answer.contains("fixture answer"));
    let state = server
        .harness
        .exported_state(
            &Agent::new("main").unwrap(),
            &[link_agent_state_protocol::Kind::Sessions],
        )
        .unwrap();
    let session = state
        .sessions
        .unwrap()
        .into_iter()
        .find(|session| {
            session
                .evidence
                .as_ref()
                .is_some_and(|e| e.conversation.thread == "telegram:evidence")
        })
        .unwrap();
    let native = session.evidence.unwrap();
    native.validate(session.updates.len()).unwrap();
    let trace = index::index_with_responses(
        native.events.iter().filter_map(|event| {
            event.evidence.as_ref().map(|evidence| Located {
                seq: event.seq,
                evidence,
            })
        }),
        native
            .events
            .iter()
            .filter_map(|event| event.request.map(|request| (event.seq, request))),
    )
    .unwrap();
    assert_eq!(trace.executions.len(), 2);
    for (reference, execution) in &trace.executions {
        let (_, request) = trace.producing_request(*reference).unwrap();
        assert_eq!(request.phase, Phase::Turn);
        assert!(
            request
                .tools
                .iter()
                .any(|tool| tool.name == "memory__search")
        );
        let Outcome::Completed { reply } = execution.finished.as_ref().unwrap().outcome else {
            panic!("missing reply")
        };
        assert!(!reply.is_error);
        assert_eq!(
            reply
                .field("structuredContent")
                .unwrap()
                .retained()
                .unwrap()["exit"],
            7
        );
    }
}

#[tokio::test]
async fn failed_dispatch_recording_blocks_send_and_mismatched_replies_remain_unknown() {
    let tool = Mcp::new().await;
    let listed = tools::Listed::default();
    let kept = tools::Kept::default();
    let agent = Agent::new("main").unwrap();
    let servers = tools::from_env(memory(&tool.url));
    let toolbox = tools::Toolbox::open(
        &reqwest::Client::new(),
        (&listed, &kept),
        &agent,
        &servers,
        (tools::OPEN_TIMEOUT, tools::start_limit()),
        link_agent_spec::link(),
    )
    .await;
    let call = link_harness::event::Call {
        id: "provider-id".into(),
        name: "memory__search".into(),
        arguments: json!({"query":"fixture"}),
    };
    let (_, cancelled) = tokio::sync::watch::channel(false);
    let failed =
        |_: tools::Dispatch<'_>| -> anyhow::Result<()> { anyhow::bail!("fixture append failure") };
    let answer = toolbox
        .call_observed(&call, cancelled.clone(), &json!({}), &failed)
        .await
        .unwrap();
    assert!(matches!(answer.observation, tools::Observation::Refused));
    assert_eq!(tool.control.effects.load(Ordering::SeqCst), 0);
    for mode in [1, 2, 4] {
        tool.control.mode.store(mode, Ordering::SeqCst);
        let answer = toolbox.call(&call, cancelled.clone(), &json!({})).await;
        assert!(
            matches!(answer.observation, tools::Observation::Unknown),
            "mode {mode}: {answer:?}"
        );
    }
    assert_eq!(tool.control.effects.load(Ordering::SeqCst), 3);
    tool.control.mode.store(3, Ordering::SeqCst);
    let answer = toolbox.call(&call, cancelled, &json!({})).await;
    assert!(matches!(
        answer.observation,
        tools::Observation::RemoteError { code: -32001, .. }
    ));
}
