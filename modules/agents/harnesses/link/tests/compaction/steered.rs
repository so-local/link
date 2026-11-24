//! A summary after an answer, with a message steered in while the model
//! writes it: the agent's work coordinated by an interactions store, as
//! an agent's is, where a steer moves the work's control revision.

use super::*;
use link_harness::turn::Asked;

/// [`world_paused`], its work coordinated by an interactions store of
/// its own.
async fn world_coordinated(name: &str, hold: common::Hold) -> World {
    let World {
        root,
        agent,
        harness,
        asked,
    } = world_paused(name, "", vec![], 600, hold).await;
    let interactions = link_interactions::Store::at(root.join("interactions"));
    let harness = harness.with_interactions(link_interactions::Host::shared(interactions));
    World {
        root,
        agent,
        harness,
        asked,
    }
}

/// A message steered in while the turn summarizes after its answer: the
/// summary the model answered is kept, asked once, and the steered
/// message is answered next, by the same turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_summary_answered_while_a_message_is_steered_in_is_kept() {
    let (let_go, hold) = common::LetGo::new();
    let world = world_coordinated("steered", hold).await;
    let thread = Thread::new("telegram:42").unwrap();
    let folder = world.folder("telegram-42");
    let say = |n: usize| event::Event::user(&format!("Message {n}: {}", "word ".repeat(80)));
    let_go.all();
    // Message 5 is the first whose turn compacts.
    for n in 1..=4 {
        let (pieces, _heard) = tokio::sync::mpsc::channel(64);
        world
            .harness
            .ask(&world.agent, &thread, say(n), pieces, None)
            .await
            .unwrap();
    }
    assert!(summaries(&folder).is_empty());
    // Its answer let go, its summary held.
    let before = world.asked().len();
    let_go.up_to(before + 1);
    let (pieces, _heard) = tokio::sync::mpsc::channel(64);
    let turn = world
        .harness
        .ask(&world.agent, &thread, say(5), pieces, None);
    let steer = async {
        link_test_kit::wait::eventually_async(
            "message 5's summary asked",
            std::time::Duration::from_secs(10),
            || world.asked().get(before + 1).is_some_and(summarizing),
        )
        .await;
        let (pieces, _heard) = tokio::sync::mpsc::channel(64);
        let steered = world
            .harness
            .ask(&world.agent, &thread, say(6), pieces, None)
            .await
            .unwrap();
        let_go.all();
        steered
    };
    let (answered, steered) = tokio::join!(turn, steer);
    // Steered in: the running turn takes it.
    let Asked::Answered(steered) = steered else {
        panic!("message 6 wasn't steered in: {steered:?}");
    };
    assert_eq!(steered.role, event::COMMAND, "{steered:?}");
    let Asked::Answered(answered) = answered.unwrap() else {
        panic!("message 5's turn joined another");
    };
    assert!(answered.text.starts_with("Answer "), "{}", answered.text);
    assert_eq!(summaries(&folder).len(), 1, "the summary wasn't kept");
    let since = &world.asked()[before..];
    assert_eq!(
        since.iter().filter(|r| summarizing(r)).count(),
        1,
        "the summary was asked again, of {} requests",
        since.len()
    );
    // Answered next, after the summary.
    let answering_six = since
        .iter()
        .position(|r| answering(r) && last_text(r).contains("Message 6:"))
        .expect("message 6 answered");
    assert!(answering_six > 1, "message 6 answered at {answering_six}");
}
