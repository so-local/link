use crate::{
    control::*,
    runtime::{Binding, Step},
    sources, *,
};

/// Whose work the tests bind, as `crate::runtime::tests::admit` binds it.
const MAIN: &str = "main";
use std::time::{Duration, Instant};

fn admit(store: &Store, session: &str, attempt: &str) -> Binding {
    crate::runtime::tests::admit(store, session, attempt, None)
}

/// The calls one bound turn of the host makes with one model request and
/// one tool round: recovery and admission (`bind_work`), the
/// dispatch-fenced model and tool steps (`step_work`, `same_step`), its
/// inputs (`take_inputs`), inclusion, the lease's interruption check, and
/// the finish with its check for queued inputs.
fn turn(store: &Store, session: &str, attempt: &str) -> Binding {
    store.recover_work(MAIN, session).unwrap();
    let binding = admit(store, session, attempt);
    let token = binding.token.expose();
    for step in [Step::Model, Step::Tool, Step::Tool] {
        let _fence = store.dispatch_fence().unwrap();
        store.work_step(token, step).unwrap();
    }
    assert!(store.waiting_inputs(token, true).unwrap().is_empty());
    store.inputs_included(token, &[]).unwrap();
    assert!(!store.interrupted_work(token).unwrap());
    assert!(store.waiting_inputs(token, false).unwrap().is_empty());
    store.finish_work(token, None).unwrap();
    binding
}

/// A turn whose one tool call went through a sandbox's policy check.
fn turn_with_call(store: &Store, session: &str, attempt: &str) -> Binding {
    let binding = turn(store, session, attempt);
    store.continue_work(binding.token.expose()).unwrap();
    store
        .policy_call(
            &binding.state.work,
            &SourceId::new("sandbox").unwrap(),
            "run/command",
            "call-1",
            b"payload",
        )
        .unwrap()
        .report(approval::EffectOutcome::Applied {
            detail: "ran".into(),
        })
        .unwrap();
    store.finish_work(binding.token.expose(), None).unwrap();
    binding
}

fn bindings(store: &Store) -> Vec<String> {
    store
        .read(|s| Ok(s.controls.runtime.work.keys().cloned().collect()))
        .unwrap()
}

fn event_ids(store: &Store) -> Vec<u64> {
    store.events(0).unwrap().into_iter().map(|e| e.id).collect()
}

fn ask(store: &Store, key: &str) -> RequestId {
    store
        .create(
            crate::tests::source(),
            sources::owner(),
            crate::tests::draft(key),
            100,
        )
        .unwrap()
}

fn state_bytes(dir: &std::path::Path) -> u64 {
    std::fs::metadata(dir.join(constants::STATE_FILE))
        .unwrap()
        .len()
}

/// Ten thousand turns, every tenth with a policy-checked tool call,
/// leave one binding and the state the size the first left, but for the
/// digits of its counters.
#[test]
fn ten_thousand_turns_keep_the_state_bounded() {
    let dir = link_test_kit::TempDir::in_memory("bounded-turns");
    let store = Store::at(&*dir);
    turn_with_call(&store, "cli:main", "attempt-00000");
    let first = state_bytes(&dir);
    for n in 1..10_000 {
        let attempt = format!("attempt-{n:05}");
        if n % 10 == 0 {
            turn_with_call(&store, "cli:main", &attempt);
        } else {
            turn(&store, "cli:main", &attempt);
        }
    }
    assert_eq!(bindings(&store), ["attempt-09999"]);
    assert!(
        state_bytes(&dir) <= first + 16,
        "{} bytes after one turn, {} after ten thousand",
        first,
        state_bytes(&dir)
    );
    let effects = store
        .read(|s| Ok(s.controls.runtime.effects.len()))
        .unwrap();
    assert_eq!(effects, 0);
    let status = store.work_status(MAIN, "cli:main").unwrap().unwrap();
    assert_eq!(status.work.generation, 10_000);
    assert_eq!(event_ids(&store), Vec::<u64>::new());
}

fn per(calls: u32, f: impl Fn()) -> Duration {
    let start = Instant::now();
    for _ in 0..calls {
        f();
    }
    start.elapsed() / calls
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// What a turn and each of the host's reads cost after 1,000 and
/// 10,000 turns: run by hand on one machine, before and after a change
/// (`cargo test --release -p link-interactions measure -- --ignored
/// --nocapture`).
#[test]
#[ignore = "measured by hand, release build"]
fn measure_bound_turns() {
    let dir = link_test_kit::TempDir::new("bound-turn-cost");
    let store = Store::at(&*dir);
    let mut turns = 0;
    for target in [1_000, 10_000] {
        while turns < target {
            turn(&store, "cli:main", &format!("attempt-{turns:05}"));
            turns += 1;
        }
        let bytes = state_bytes(&dir);
        let timed = std::cell::Cell::new(0);
        let one_turn = per(5, || {
            timed.set(timed.get() + 1);
            turn(
                &store,
                "cli:main",
                &format!("timed-{turns}-{}", timed.get()),
            );
        });
        let binding = admit(&store, "cli:main", &format!("measured-{turns}"));
        let token = binding.token.expose();
        let interrupted = per(50, || {
            store.interrupted_work(token).unwrap();
        });
        let step = per(50, || {
            store.work_step(token, Step::Model).unwrap();
        });
        let inputs = per(50, || {
            store.waiting_inputs(token, true).unwrap();
        });
        store.finish_work(token, None).unwrap();
        eprintln!(
            "after {target} turns: state {bytes} bytes; a turn {:.2} ms, \
             interrupted_work {:.3} ms, work_step {:.3} ms, waiting_inputs {:.3} ms",
            ms(one_turn),
            ms(interrupted),
            ms(step),
            ms(inputs),
        );
    }
}

#[test]
fn reads_parse_the_state_file_only_after_it_changes() {
    let dir = link_test_kit::TempDir::new("parse-count");
    let store = Store::at(&*dir);
    let binding = admit(&store, "cli:main", "one");
    let token = binding.token.expose();
    let parsed = store.parses();
    for _ in 0..100 {
        assert!(!store.interrupted_work(token).unwrap());
    }
    store.clone().work_step(token, Step::Model).unwrap();
    assert_eq!(store.parses(), parsed, "its own write left it parsed");
    // Another process's write, as a store of its own.
    Store::at(&*dir).finish_work(token, None).unwrap();
    assert!(store.interrupted_work(token).unwrap());
    assert!(store.interrupted_work(token).unwrap());
    assert_eq!(store.parses(), parsed + 1);
}

#[test]
fn an_edit_in_place_is_read_again() {
    let dir = link_test_kit::TempDir::new("edited-state");
    let store = Store::at(&*dir);
    ask(&store, "a");
    assert_eq!(store.pending(100).unwrap().len(), 1);
    let path = dir.join(constants::STATE_FILE);
    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Report preferences", "Report preference!");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, edited.as_bytes()))
        .unwrap();
    assert_eq!(
        store.pending(100).unwrap()[0].question.title,
        "Report preference!"
    );
}

#[test]
fn a_change_that_alters_nothing_leaves_the_file_unwritten() {
    use std::os::unix::fs::MetadataExt;
    let dir = link_test_kit::TempDir::new("unchanged-write");
    let store = Store::at(&*dir);
    ask(&store, "a");
    let inode = || {
        std::fs::metadata(dir.join(constants::STATE_FILE))
            .unwrap()
            .ino()
    };
    let before = inode();
    assert_eq!(store.pending(100).unwrap().len(), 1);
    assert_eq!(inode(), before);
}

#[test]
fn a_removed_attempt_stays_stale_for_questions_controls_and_policy_calls() {
    let dir = link_test_kit::TempDir::new("removed-attempt");
    let store = Store::at(&*dir);
    let one = turn(&store, "cli:main", "one");
    let two = admit(&store, "cli:main", "two");
    assert_eq!(bindings(&store), ["two"]);
    assert!(store.interrupted_work(one.token.expose()).unwrap_or(true));
    assert!(store.work_step(one.token.expose(), Step::Model).is_err());
    let mut requester = crate::tests::source();
    requester.work = Some(one.state.work.clone());
    let refused = store
        .create(
            requester.clone(),
            sources::owner(),
            crate::tests::draft("old"),
            100,
        )
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("work changed before question admission"),
        "{refused:#}"
    );
    let replay = store
        .policy_call(
            &one.state.work,
            &SourceId::new("sandbox").unwrap(),
            "run/command",
            "call-1",
            b"payload",
        )
        .err()
        .unwrap();
    assert!(
        format!("{replay:#}").contains("work changed before policy call admission"),
        "{replay:#}"
    );
    requester.work = Some(two.state.work.clone());
    store
        .create(requester, sources::owner(), crate::tests::draft("new"), 100)
        .unwrap();
}

#[test]
fn a_policy_call_of_unknown_outcome_keeps_its_attempt() {
    let dir = link_test_kit::TempDir::new("unknown-call");
    let store = Store::at(&*dir);
    let one = turn(&store, "cli:main", "one");
    store.continue_work(one.token.expose()).unwrap();
    let call = store
        .policy_call(
            &one.state.work,
            &SourceId::new("sandbox").unwrap(),
            "run/command",
            "call-1",
            b"payload",
        )
        .unwrap();
    // The source ends before it reports: its outcome is unknown.
    drop(call);
    store.finish_work(one.token.expose(), None).unwrap();
    store.recover_work(MAIN, "cli:main").unwrap();
    let refused = store.register_work(crate::runtime::tests::admission("cli:main", "two", None));
    // Unknown work can't be replaced, and nothing of it leaves.
    assert!(refused.is_err());
    assert_eq!(bindings(&store), ["one"]);
    assert_eq!(
        store
            .read(|s| Ok(s.controls.runtime.effects.len()))
            .unwrap(),
        1
    );
}

#[test]
fn an_event_naming_a_retired_attempt_keeps_it_until_every_subscriber_passes() {
    let dir = link_test_kit::TempDir::new("named-attempt");
    let store = Store::at(&*dir);
    assert_eq!(store.subscribe("owner-delivery").unwrap(), 0);
    let one = turn(&store, "cli:main", "one");
    let start = store
        .control_work(
            WorkControl {
                id: ControlId::new("start").unwrap(),
                actor: sources::owner(),
                selector: ControlSelector::Exact {
                    work: one.state.work.clone(),
                },
                operation: ControlOperation::StartPreparedWork,
                transport_event: TransportEventId::new("start").unwrap(),
            },
            &store.lineage().unwrap(),
            100,
        )
        .unwrap();
    turn(&store, "cli:main", "two");
    let member = start.members[0].id.as_str();
    assert_eq!(
        store.receipts(member).unwrap().last().unwrap().stage,
        ReceiptStage::Applied
    );
    // The subscriber hasn't read the start's receipts, which name "one":
    // its notice finds the owner's route there.
    assert_eq!(bindings(&store), ["one", "two"]);
    assert_eq!(
        store.owner_route(&one.state.work).unwrap().as_deref(),
        Some("cli:main")
    );
    let newest = *event_ids(&store).last().unwrap();
    store.acknowledge("owner-delivery", newest).unwrap();
    assert_eq!(event_ids(&store), [newest]);
    turn(&store, "cli:main", "three");
    assert_eq!(bindings(&store), ["three"]);
    // Correlation still finds the receipts: the batch is among the
    // newest settled.
    assert_eq!(
        store.receipts(member).unwrap().last().unwrap().stage,
        ReceiptStage::Applied
    );
}

#[test]
fn events_leave_once_every_subscriber_passed_them_the_newest_always_kept() {
    let dir = link_test_kit::TempDir::new("event-retention");
    let store = Store::at(&*dir);
    ask(&store, "a");
    // With no subscriber, no event waits: the newest numbers the next.
    assert_eq!(event_ids(&store), [1]);
    assert_eq!(store.subscribe("one").unwrap(), 0);
    assert_eq!(store.subscribe("two").unwrap(), 0);
    ask(&store, "b");
    ask(&store, "c");
    assert_eq!(event_ids(&store), [1, 2, 3]);
    store.acknowledge("two", 2).unwrap();
    assert_eq!(event_ids(&store), [1, 2, 3]);
    store.acknowledge("one", 3).unwrap();
    assert_eq!(event_ids(&store), [3]);
    assert_eq!(store.subscribe("one").unwrap(), 3);
    ask(&store, "d");
    assert_eq!(event_ids(&store), [3, 4]);
    assert_eq!(store.events(2).unwrap()[0].id, 3);
}

#[test]
fn a_subscriber_that_unsubscribes_keeps_no_event_and_another_keeps_its_own() {
    let dir = link_test_kit::TempDir::new("unsubscribed");
    let store = Store::at(&*dir);
    assert_eq!(store.subscribe("deleted").unwrap(), 0);
    assert_eq!(store.subscribe("stopped").unwrap(), 0);
    ask(&store, "a");
    ask(&store, "b");
    store.acknowledge("stopped", 1).unwrap();
    // The one behind holds what it hasn't read.
    assert_eq!(event_ids(&store), [1, 2]);
    assert!(store.unsubscribe("deleted").unwrap());
    // Gone in that write: only the stopped one's unread event waits.
    assert_eq!(event_ids(&store), [2]);
    assert!(!store.unsubscribe("deleted").unwrap());
    ask(&store, "c");
    assert_eq!(event_ids(&store), [2, 3]);
    assert_eq!(store.subscribe("stopped").unwrap(), 1);
}

/// The seconds a resolved question stays: `retention.toml`'s days.
fn window() -> u64 {
    super::retention().resolved_days * super::DAY_SECS
}

/// When `id` was resolved, by retention's clock.
fn resolved_at(store: &Store, id: &RequestId) -> u64 {
    store
        .read(|s| Ok(s.records[id].resolved_at.expect("resolved")))
        .unwrap()
}

/// Every rule of retention, as if a write came at `now`.
fn apply_at(store: &Store, now: u64) {
    store
        .change(|s| {
            super::apply(s, now);
            Ok(())
        })
        .unwrap();
}

fn gone(store: &Store, id: &RequestId) -> bool {
    match store.snapshot(id, 101) {
        Ok(_) => false,
        Err(err) => {
            assert_eq!(err.to_string(), "unknown request");
            true
        }
    }
}

/// An answered question stays `resolved_days` after its answer, so a
/// late reply is answered "already resolved", then goes with its code,
/// deliveries, their messages and drafts. One unanswered and one a kept
/// event names stay; one whose delivery never reached its owner goes
/// all the same, since nothing delivers a resolved question.
#[test]
fn an_answered_question_goes_its_days_after_its_answer() {
    use crate::tests::{candidate, delivery, ingress};
    let dir = link_test_kit::TempDir::new("answered-retention");
    let store = Store::at(&*dir);
    let answer = |id: &RequestId, route: &str, deliver: bool| {
        let envelope = delivery(&store, id, route);
        if deliver {
            store
                .delivered(&envelope.delivery.id, Some(format!("message-{route}")))
                .unwrap();
        }
        let reply = candidate(&envelope, "1", &format!("answer-{route}"));
        assert_eq!(
            store.submit(&ingress(route), reply, 101).unwrap(),
            SubmitDisposition::Accepted
        );
        envelope
    };
    assert_eq!(store.subscribe("reader").unwrap(), 0);
    let old = ask(&store, "old");
    let open = ask(&store, "open");
    let unreached = ask(&store, "unreached");
    let envelope = answer(&old, "cli:main", true);
    answer(&unreached, "cli:other", false);
    let at = resolved_at(&store, &old);
    // A late reply inside the window is answered as before.
    answer_again(&store, &envelope);
    apply_at(&store, at + window() - 1);
    assert!(!gone(&store, &old));
    // An unread event naming it holds it past the window.
    apply_at(&store, at + window());
    assert!(!gone(&store, &old));
    // Read to the end, the newest event names only the last question.
    ask(&store, "later");
    let newest = *event_ids(&store).last().unwrap();
    store.acknowledge("reader", newest).unwrap();
    apply_at(&store, resolved_at(&store, &unreached).max(at) + window());
    assert!(gone(&store, &old));
    assert!(gone(&store, &unreached));
    assert_eq!(store.find_code(&envelope.response_code).unwrap(), None);
    assert!(store.deliveries(&old).unwrap().is_empty());
    assert!(store.deliveries(&unreached).unwrap().is_empty());
    assert_eq!(
        store
            .reply_delivery("cli:main", "test", "message-cli:main")
            .unwrap(),
        None
    );
    assert!(store.snapshot(&open, 101).unwrap().resolution.is_none());
}

/// The newest event stays past every cursor, so a subscriber that starts
/// afterwards reads it: the question it names stays with it, and goes
/// once a newer event takes its place.
#[test]
fn the_question_the_newest_event_names_stays_for_a_new_subscriber() {
    use crate::tests::{candidate, delivery, ingress};
    let dir = link_test_kit::TempDir::new("newest-event-question");
    let store = Store::at(&*dir);
    assert_eq!(store.subscribe("reader").unwrap(), 0);
    let id = ask(&store, "a");
    let envelope = delivery(&store, &id, "cli:main");
    let reply = candidate(&envelope, "1", "answer");
    store.submit(&ingress("cli:main"), reply, 101).unwrap();
    let newest = *event_ids(&store).last().unwrap();
    store.acknowledge("reader", newest).unwrap();
    let past = resolved_at(&store, &id) + window();
    apply_at(&store, past);
    let late = store.subscribe("late").unwrap();
    let events = store.events(late).unwrap();
    assert_eq!(events.last().unwrap().request.as_ref(), Some(&id));
    for event in &events {
        if let Some(request) = &event.request {
            store.snapshot(request, 101).unwrap();
        }
    }
    store.unsubscribe("late").unwrap();
    ask(&store, "b");
    let newest = *event_ids(&store).last().unwrap();
    store.acknowledge("reader", newest).unwrap();
    apply_at(&store, past);
    assert!(gone(&store, &id));
}

/// A question that expired or was cancelled is stamped when it's
/// resolved and goes its days later, as an answered one does: none
/// stays toward the record limit.
#[test]
fn an_expired_or_cancelled_question_goes_its_days_after_it_closed() {
    let dir = link_test_kit::TempDir::new("closed-retention");
    let store = Store::at(&*dir);
    let expired = ask(&store, "expired");
    let cancelled = ask(&store, "cancelled");
    let late = store.snapshot(&expired, u64::MAX).unwrap();
    assert!(late.resolution.is_some());
    store
        .cancel(&cancelled, &crate::tests::source(), "not needed", 101)
        .unwrap();
    // A newer event than either, so none names them.
    ask(&store, "later");
    let (first, last) = (
        resolved_at(&store, &expired),
        resolved_at(&store, &cancelled),
    );
    apply_at(&store, first.min(last) + window() - 1);
    assert!(!gone(&store, &expired));
    assert!(!gone(&store, &cancelled));
    apply_at(&store, first.max(last) + window());
    assert!(gone(&store, &expired));
    assert!(gone(&store, &cancelled));
}

/// A second reply to `envelope`'s question.
fn answer_again(store: &Store, envelope: &crate::delivery::DeliveryEnvelope) {
    let reply = crate::tests::candidate(envelope, "2", "late");
    assert_eq!(
        store
            .submit(&crate::tests::ingress("cli:main"), reply, 102)
            .unwrap(),
        SubmitDisposition::AlreadyResolved
    );
}

fn user_input(work: &WorkRef, n: usize) -> UserInput {
    UserInput {
        id: InputId::new(format!("input-{n}")).unwrap(),
        actor: sources::owner(),
        target: work.clone(),
        content: TextEvidence {
            text: format!("input {n}"),
            supplied_by: sources::owner(),
            input_kind: InputKind::Typed,
        },
        admission: Admission::Steer,
        transport_event: TransportEventId::new(format!("event-{n}")).unwrap(),
        submitted_at: 100,
    }
}

fn host(work: &WorkRef) -> WorkState {
    WorkState {
        work: work.clone(),
        lifecycle: Lifecycle::Open,
        pause_reasons: Default::default(),
        observed: Execution::Running,
        checkpoint: None,
        in_flight_effects: Default::default(),
    }
}

/// An input admitted, delivered and, unless `unread`, included.
fn input(store: &Store, work: &WorkRef, n: usize, unread: bool) -> InputId {
    let input = user_input(work, n);
    store.admit_input(input.clone(), &host(work), 100).unwrap();
    let stages: &[ReceiptStage] = if unread {
        &[ReceiptStage::Delivered]
    } else {
        &[ReceiptStage::Delivered, ReceiptStage::Included]
    };
    for stage in stages {
        store
            .record_receipt(input.id.as_str(), work, stage.clone(), "seen", 100)
            .unwrap();
    }
    input.id
}

#[test]
fn only_the_newest_settled_inputs_stay_and_an_unread_one_always() {
    let dir = link_test_kit::TempDir::new("settled-inputs");
    let store = Store::at(&*dir);
    // Work this store never bound, as a source's own may be: bound in
    // another.
    let elsewhere = link_test_kit::TempDir::new("settled-elsewhere");
    let work = admit(&Store::at(&*elsewhere), "cli:main", "one").state.work;
    let unread = input(&store, &work, 0, true);
    let read: Vec<_> = (1..=4).map(|n| input(&store, &work, n, false)).collect();
    store
        .change(|s| {
            super::settled(s, 2);
            Ok(())
        })
        .unwrap();
    let kept = store
        .read(|s| Ok(s.controls.inputs.keys().cloned().collect::<Vec<_>>()))
        .unwrap();
    assert_eq!(kept, [unread.clone(), read[2].clone(), read[3].clone()]);
    assert!(store.receipts(read[0].as_str()).unwrap().is_empty());
    assert_eq!(store.receipts(unread.as_str()).unwrap().len(), 2);
    // A retried admission of a kept input is answered as before.
    let again = store
        .admit_input(user_input(&work, 4), &host(&work), 100)
        .unwrap();
    assert_eq!(again, store.receipts(read[3].as_str()).unwrap()[0]);
    // Numbers continue after those that left.
    let numbers = store
        .read(|s| {
            Ok(s.controls
                .receipts
                .iter()
                .map(|r| r.sequence)
                .collect::<Vec<_>>())
        })
        .unwrap();
    let newest = input(&store, &work, 5, false);
    let next = store.receipts(newest.as_str()).unwrap()[0].sequence;
    assert!(numbers.iter().all(|n| *n < next), "{numbers:?} then {next}");
}
