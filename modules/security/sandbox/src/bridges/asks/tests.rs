use super::*;
use link_test_kit::{TempDir, wait};
use std::sync::atomic::AtomicUsize;

fn proposal() -> Proposal {
    let mut proposal = Proposal::sandbox(
        "main",
        "run tool example's write",
        vec![format!("With {{\"text\":\"{}\"}}", "x".repeat(2000))],
    );
    proposal.payload = b"exact call".to_vec();
    proposal
}
struct Fake {
    answer: Option<Answer>,
    shown: AtomicUsize,
}
impl Dialog for Fake {
    fn show(
        &self,
        text: &str,
        _: &[(String, String)],
        _: Duration,
        _: &AtomicBool,
    ) -> Option<Answer> {
        assert!(text.contains("x".repeat(2000).as_str()));
        self.shown.fetch_add(1, Ordering::Relaxed);
        self.answer
    }
}
#[test]
fn dialog_only_decisions_have_a_record_and_require_a_fresh_single_use_claim() {
    let root = TempDir::new("approval-dialog");
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    let dialog = Fake {
        answer: Some(Answer::Allowed),
        shown: AtomicUsize::new(0),
    };
    let reviewed = ask_with(
        &dialog,
        &dir,
        &proposal(),
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(reviewed.answer, Answer::Allowed);
    let id = reviewed.request.clone();
    let application = reviewed.claim(b"exact call", true).unwrap();
    drop(application);
    let snapshot = store(&dir).unwrap().snapshot(&id, now()).unwrap();
    assert!(matches!(snapshot.effect, EffectState::Unknown { .. }));
    assert!(snapshot.resolution.is_some());
    assert_eq!(dialog.shown.load(Ordering::Relaxed), 1);
}
#[test]
fn a_chat_and_dialog_share_one_decision_and_losing_controls_cannot_replace_it() {
    let root = TempDir::new("approval-surfaces");
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    let dialog = Fake {
        answer: None,
        shown: AtomicUsize::new(0),
    };
    std::thread::scope(|scope| {
        let ask = scope.spawn(|| {
            ask_with(
                &dialog,
                &dir,
                &proposal(),
                &[],
                Waits::Always,
                Duration::from_secs(30),
            )
            .unwrap()
        });
        let envelope = wait::eventually("full question available", Duration::from_secs(10), || {
            waiting(&dir).unwrap().into_iter().next()
        });
        assert!(answer(&dir, &envelope.response_code, false).unwrap());
        assert!(!answer(&dir, &envelope.response_code, true).unwrap());
        assert_eq!(ask.join().unwrap().answer, Answer::Refused);
    });
    assert!(waiting(&dir).unwrap().is_empty());
}
/// A sandbox's question is in the machine's one interaction store, where
/// every surface holds its presence and delivers the owner's questions
/// from: asked while a surface is present there, it waits there rather
/// than being refused for want of a dialog, and an answer submitted there,
/// as a chat submits one, reaches the ask.
#[test]
fn an_ask_waits_in_the_machine_store_every_surface_delivers_from() {
    use link_files::layout::Hidden;
    let root = TempDir::new("approval-machine-store");
    let dir = Hidden::Sandboxes.at(&root);
    // Where `Store::machine` is for this installation folder.
    let machine = Store::at(Hidden::Approvals.at(&root));
    let _present = machine.presence("sandbox:main", None).unwrap();
    let dialog = Fake {
        answer: None,
        shown: AtomicUsize::new(0),
    };
    std::thread::scope(|scope| {
        let ask = scope.spawn(|| {
            ask_with(
                &dialog,
                &dir,
                &proposal(),
                &[],
                Waits::IfAnswered,
                Duration::from_secs(30),
            )
            .unwrap()
        });
        let pending = wait::eventually("the question pending", Duration::from_secs(10), || {
            machine.pending(now()).unwrap().into_iter().next()
        });
        assert_eq!(pending.question.requester.source.as_str(), "sandbox");
        let envelope = machine
            .plan_delivery(
                &pending.question.id,
                "chat:owner",
                "chat",
                &render::text_capabilities(),
                None,
                now(),
            )
            .unwrap();
        assert!(submit(&machine, &envelope, true).unwrap());
        assert_eq!(ask.join().unwrap().answer, Answer::Allowed);
    });
    assert!(machine.pending(now()).unwrap().is_empty());
}
#[test]
fn absent_surfaces_cancel_the_record_instead_of_approving_outside_the_engine() {
    let root = TempDir::new("approval-unavailable");
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    let dialog = Fake {
        answer: None,
        shown: AtomicUsize::new(0),
    };
    let reviewed = ask_with(
        &dialog,
        &dir,
        &proposal(),
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(reviewed.answer, Answer::Unasked);
    assert!(reviewed.claim(b"exact call", true).is_err());
    assert!(waiting(&dir).unwrap().is_empty());
}

#[test]
fn a_qualified_yes_is_forwarded_as_clarification_without_authorizing_the_source() {
    struct Qualified;
    impl Dialog for Qualified {
        fn show(
            &self,
            _: &str,
            _: &[(String, String)],
            _: Duration,
            _: &AtomicBool,
        ) -> Option<Answer> {
            None
        }
        fn response(
            &self,
            _: &Store,
            envelope: &DeliveryEnvelope,
            _: &[(String, String)],
            _: Duration,
            _: &AtomicBool,
        ) -> Option<ResponseValue> {
            Some(ResponseValue::Submit(SetSubmission {
                answers: std::collections::BTreeMap::from([(
                    envelope.question.questions[0].id.clone(),
                    ItemAnswer::Confirmation { yes: true },
                )]),
                comment: Some("Only after I review the destination".into()),
            }))
        }
    }
    let root = TempDir::new("qualified-permission");
    let reviewed = ask_with(
        &Qualified,
        &link_files::layout::Hidden::Sandboxes.at(&root),
        &proposal(),
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(reviewed.answer, Answer::Refused);
    let result = reviewed.refusal();
    assert!(result.contains("Only after I review the destination"));
    assert!(result.contains("user_submission"));
    assert!(reviewed.claim(b"exact call", true).is_err());
}
#[test]
fn wrong_payload_and_revoked_source_authority_cannot_claim_a_yes() {
    let root = TempDir::new("approval-source-checks");
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    let dialog = Fake {
        answer: Some(Answer::Allowed),
        shown: AtomicUsize::new(0),
    };
    let reviewed = ask_with(
        &dialog,
        &dir,
        &proposal(),
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(reviewed.claim(b"changed call", true).is_err());
    let reviewed = ask_with(
        &dialog,
        &dir,
        &proposal(),
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(reviewed.claim(b"exact call", false).is_err());
}
#[test]
fn complete_reviews_escape_controls_and_keep_the_sources_wait_bounds() {
    let mut proposal = proposal();
    proposal.capability.push(char::from(27));
    assert!(!proposal.text().contains(char::from(27)));
    assert!(proposal.text().len() > 2000);
    for (seconds, expected) in [(0, 1), (600, 600), (u64::MAX, MOST.as_secs())] {
        assert_eq!(
            wait_of(Some(&link_sandbox_client::routes::Asking {
                agent: "main".into(),
                wait_secs: seconds
            }))
            .as_secs(),
            expected
        );
    }
    // Its questions are kept in its installation folder's store: the test's own.
    let root = TempDir::new("approval-unknown");
    assert!(
        !answer(
            &link_files::layout::Hidden::Sandboxes.at(&root),
            "abc234",
            true
        )
        .unwrap()
    );
}

#[test]
fn retrying_the_same_host_tool_call_cannot_create_a_new_approval_or_replay_an_unknown_effect() {
    let root = TempDir::new("approval-retry");
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    let store = store(&dir).unwrap();
    let binding = store
        .register_work(runtime::WorkAdmission {
            owner_route: Some("cli:main".into()),
            kind: WorkKind::Conversation,
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: "cli:main".into(),
            attempt: "one".into(),
            parent_attempt: None,
            checkpoint: Some("input".into()),
        })
        .unwrap();
    let mut proposal = proposal();
    proposal.work = Some(binding.state.work);
    proposal.call = Some("exact-model-call".into());
    let dialog = Fake {
        answer: Some(Answer::Allowed),
        shown: AtomicUsize::new(0),
    };
    let reviewed = ask_with(
        &dialog,
        &dir,
        &proposal,
        &[],
        Waits::IfAnswered,
        Duration::from_secs(30),
    )
    .unwrap();
    let id = reviewed.request.clone();
    let mut changed = proposal.clone();
    changed.payload = b"different call".to_vec();
    assert!(
        ask_with(
            &dialog,
            &dir,
            &changed,
            &[],
            Waits::IfAnswered,
            Duration::from_secs(30)
        )
        .is_err()
    );
    drop(reviewed.claim(&proposal.payload, true).unwrap());
    assert!(matches!(
        store.snapshot(&id, now()).unwrap().effect,
        EffectState::Unknown { .. }
    ));
    assert!(
        ask_with(
            &dialog,
            &dir,
            &proposal,
            &[],
            Waits::IfAnswered,
            Duration::from_secs(30)
        )
        .is_err()
    );
    assert_eq!(dialog.shown.load(Ordering::Relaxed), 1);
}
