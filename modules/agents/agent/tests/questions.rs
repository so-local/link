//! The real owner CLI saves drafts independently of explicit submission.
use link_interactions::*;
use link_test_kit::{TempDir, env};
use serde_json::{Value, json};

fn run(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    env::command(env!("CARGO_BIN_EXE_lnk-agent"), home)
        .args(["question", "list"])
        .args(args)
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap()
}

#[test]
fn drafts_wait_for_explicit_submission_and_stale_revisions_cannot_answer() {
    let home = TempDir::new("owner-question-cli");
    link_agent_contract::testing::write_agent(&home, "main", "harness = 'link'\n");
    let store = Store::at(home.join(".lnk/safe/approvals"));
    let mut requester = sources::requester("agent", "service:m1a2i3n4s5v6", "main").unwrap();
    requester.routing_agent = Some("main".into());
    let draft:QuestionDraft = serde_json::from_value(json!({"title":"Report", "caller_idempotency_key":"cli", "questions":[
        {"id":"focus", "prompt":"What should it cover?", "answer_shape":{"kind":"text","max_bytes":1000,"allow_empty":false}},
        {"id":"format", "prompt":"Include the appendix?", "answer_shape":{"kind":"confirmation"}}]})).unwrap();
    let id = store
        .create(requester, sources::owner(), draft, now())
        .unwrap();
    let listed = run(&home, &["--json"]);
    assert!(listed.status.success(), "{listed:?}");
    let envelope: delivery::DeliveryEnvelope = serde_json::from_slice(&listed.stdout).unwrap();
    let code = envelope.response_code;
    let path = home.join("draft.json");
    std::fs::write(&path, serde_json::to_vec(&json!({"answers":{"focus":{"kind":"text","text":"Measured costs"}}, "comment":"Keep my qualification"})).unwrap()).unwrap();
    let saved = run(&home, &[&code, "--draft", path.to_str().unwrap(), "--json"]);
    assert!(saved.status.success(), "{saved:?}");
    let form: Value = serde_json::from_slice(&saved.stdout).unwrap();
    assert_eq!(form["draft"]["revision"], 1);
    assert!(store.snapshot(&id, now()).unwrap().resolution.is_none());
    let stale = run(&home, &[&code, "--submit", "0", "--json"]);
    assert!(!stale.status.success(), "{stale:?}");
    assert!(store.snapshot(&id, now()).unwrap().resolution.is_none());
    let submitted = run(&home, &[&code, "--submit", "1", "--json"]);
    assert!(submitted.status.success(), "{submitted:?}");
    let resolution = store.snapshot(&id, now()).unwrap().resolution.unwrap();
    assert_eq!(resolution.trigger, Trigger::UserSubmission);
    assert_eq!(resolution.comment.unwrap().text, "Keep my qualification");
    assert!(matches!(
        resolution.items[&ItemId::new("format").unwrap()],
        ItemResult::Skipped { .. }
    ));
    let closed = run(&home, &[&code, "--draft", path.to_str().unwrap(), "--json"]);
    assert!(
        !closed.status.success(),
        "a closed request cannot gain another editable draft: {closed:?}"
    );
}
