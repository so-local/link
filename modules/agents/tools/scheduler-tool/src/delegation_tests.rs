//! Durable delegation's conformance: a coordinator's subagents and
//! messages to teammates, their dependencies, outcomes and cancellation,
//! kept through the store across a runner's restart. Native coding
//! workers add their own cases to this same contract.

use super::*;
use crate::tests::{CAPS, failing, job, recording, store};
use link_connection_contract::Fired;
use std::sync::{Arc, Mutex};

/// A subagent `id` started from `parent_thread`, coordinated by task
/// `coordinator`, waiting on `after`.
fn task(id: &str, parent_thread: &str, coordinator: Option<&str>, after: &[&str]) -> Job {
    Job {
        kind: Kind::Task,
        thread: format!("task-{id}"),
        answer: AnswerTo::Thread(parent_thread.into()),
        parent: coordinator.map(str::to_string),
        after: after.iter().map(|a| a.to_string()).collect(),
        ..job(id, 0)
    }
}

fn outcome(store: &Store, id: &str) -> Option<How> {
    store.ended(id).and_then(|job| job.outcome).map(|o| o.how)
}

fn cancelled(store: &Store, id: &str) -> bool {
    matches!(outcome(store, id), Some(How::Cancelled { .. }))
}

/// A coordinator delegates two steps, the second after the first; each
/// reports, kept ended with its answer, and the second starts only once
/// the first answered: through a runner's restart between them.
#[test]
fn a_coordinator_delegates_reports_and_resumes() {
    let (dir, store) = store("delegates");
    store
        .put(&task("aaaaaaaaaad1", "telegram:42", None, &[]))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaad2",
            "telegram:42",
            None,
            &["aaaaaaaaaad1"],
        ))
        .unwrap();
    let (recorder, seen) = recording();
    let (allow_first, release_first) = std::sync::mpsc::channel();
    let release_first = Mutex::new(release_first);
    let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
        if firing.thread == "task-aaaaaaaaaad1" {
            release_first.lock().unwrap().recv()?;
        }
        recorder.fire(firing)
    }));
    // Hold the first answer until this look has checked its dependent.
    // A fast answer may otherwise make both jobs ready in the same look.
    let (looked, workers) = crate::runner::look_firing(&store, &driver, &CAPS, 10);
    assert_eq!(looked.fired, ["aaaaaaaaaad1"]);
    assert!(store.get("aaaaaaaaaad2").is_some());
    allow_first.send(()).unwrap();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(seen.lock().unwrap()[0].thread, "task-aaaaaaaaaad1");
    assert_eq!(
        outcome(&store, "aaaaaaaaaad1"),
        Some(How::Done {
            answer: "ok".into()
        })
    );
    assert!(store.get("aaaaaaaaaad2").is_some());
    // A runner restarts: what's kept is the store's.
    drop(store);
    let store = Store::new(&dir).unwrap();
    store.recover();
    look(&store, &driver, &CAPS, 20, true);
    let threads: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|f| f.thread.clone())
        .collect();
    assert_eq!(threads, ["task-aaaaaaaaaad1", "task-aaaaaaaaaad2"]);
    assert!(matches!(
        outcome(&store, "aaaaaaaaaad2"),
        Some(How::Done { .. })
    ));
    assert!(store.jobs().is_empty() && store.firing().is_empty());
}

/// A result lost to a runner cut off as it fired is fired again, and
/// kept once; a second settling of one result changes nothing.
#[test]
fn a_lost_or_duplicate_result_is_kept_once() {
    let (_dir, store) = store("lost");
    store
        .put(&task("aaaaaaaaaae1", "telegram:42", None, &[]))
        .unwrap();
    // Claimed, then the runner stopped before its answer came.
    assert!(store.claim("aaaaaaaaaae1"));
    store.recover();
    let (driver, seen) = recording();
    look(&store, &driver, &CAPS, 10, true);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let first = store.ended("aaaaaaaaaae1").unwrap();
    // The same result settled again: nothing moves, nothing is replaced.
    store
        .finish("aaaaaaaaaae1", How::done("a second answer"))
        .unwrap();
    assert_eq!(store.ended("aaaaaaaaaae1").unwrap(), first);
    look(&store, &driver, &CAPS, 20, true);
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// A step waiting on one that failed is cancelled, never fired, and so
/// is what waits on it.
#[test]
fn a_failed_dependency_cancels_what_waits_on_it() {
    let (_dir, store) = store("failed-dependency");
    // On its last try: its failure gives it up.
    store
        .put(&Job {
            tries: limits().retries,
            ..task("aaaaaaaaaaf1", "telegram:42", None, &[])
        })
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaaf2",
            "telegram:42",
            None,
            &["aaaaaaaaaaf1"],
        ))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaaf3",
            "telegram:42",
            None,
            &["aaaaaaaaaaf2"],
        ))
        .unwrap();
    let (driver, asked) = failing(usize::MAX);
    look(&store, &driver, &CAPS, 10, true);
    assert!(matches!(
        outcome(&store, "aaaaaaaaaaf1"),
        Some(How::Failed { .. })
    ));
    look(&store, &driver, &CAPS, 20, true);
    assert_eq!(*asked.lock().unwrap(), 1, "only the first was ever fired");
    assert!(cancelled(&store, "aaaaaaaaaaf2"));
    assert!(cancelled(&store, "aaaaaaaaaaf3"));
    assert!(store.jobs().is_empty());
}

/// Cancelling a coordinator cancels its children and what waits on it,
/// waiting or at work, each kept ended with why; a child at work ends
/// its turn, and its answer is never kept.
#[test]
fn cancelling_a_coordinator_cascades() {
    let (_dir, store) = store("cascade");
    store
        .put(&task("aaaaaaaaaag1", "telegram:42", None, &[]))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaag2",
            "task-aaaaaaaaaag1",
            Some("aaaaaaaaaag1"),
            &[],
        ))
        .unwrap();
    // Each firing says it's at work, then answers once let go.
    let (at_work, working) = std::sync::mpsc::channel::<()>();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let (at_work, wait) = (Mutex::new(at_work), Mutex::new(wait));
    let driver: Arc<dyn Driver> = Arc::new(InProcess(move |_: &Firing| {
        at_work.lock().unwrap().send(()).unwrap();
        let _ = wait.lock().unwrap().recv();
        Ok(Fired {
            answer: "late".into(),
        })
    }));
    // The coordinator and its first child fire, and neither answers yet.
    let looking = {
        let (store, driver) = (store.clone(), driver.clone());
        std::thread::spawn(move || look(&store, &driver, &CAPS, 10, true))
    };
    working.recv().unwrap();
    working.recv().unwrap();
    // Its child's child waiting, and a step after it.
    store
        .put(&task(
            "aaaaaaaaaag3",
            "task-aaaaaaaaaag2",
            Some("aaaaaaaaaag2"),
            &[],
        ))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaag4",
            "telegram:42",
            None,
            &["aaaaaaaaaag1"],
        ))
        .unwrap();
    assert!(store.cancel("aaaaaaaaaag1").unwrap());
    for id in [
        "aaaaaaaaaag1",
        "aaaaaaaaaag2",
        "aaaaaaaaaag3",
        "aaaaaaaaaag4",
    ] {
        assert!(cancelled(&store, id), "{id}");
    }
    assert_eq!(store.subagents(), 0);
    // Their turns end; what they answer isn't kept.
    go.send(()).unwrap();
    go.send(()).unwrap();
    looking.join().unwrap();
    assert!(cancelled(&store, "aaaaaaaaaag1"));
    assert!(cancelled(&store, "aaaaaaaaaag2"));
    assert!(store.firing().is_empty() && store.jobs().is_empty());
    assert!(!store.cancel("aaaaaaaaaag1").unwrap(), "cancelled once");
}

/// A driver reaching teammate `cccccccccccc` after `away` firings fail,
/// as one moving is unreached until it's where it's going: how many it
/// was asked.
struct Teammate {
    away: usize,
    asked: Mutex<usize>,
}

impl Driver for Teammate {
    fn fire(&self, _: &Firing) -> anyhow::Result<Fired> {
        Ok(Fired::default())
    }

    fn fire_to(&self, agent: &str, _: &Firing) -> anyhow::Result<Fired> {
        let mut asked = self.asked.lock().unwrap();
        *asked += 1;
        anyhow::ensure!(agent == "cccccccccccc", "{agent} isn't a teammate");
        anyhow::ensure!(*asked > self.away, "the teammate is moving");
        Ok(Fired {
            answer: String::new(),
        })
    }
}

/// A message to a teammate that's moving fails while it can't be
/// reached, is tried again and lands once: queued, never doubled, and
/// what waits on it then fires. A teammate no longer a member fails it
/// for good, and what waits on it is cancelled.
#[test]
fn a_teammate_moving_or_removed() {
    let (_dir, store) = store("teammate");
    let send = |id: &str, to: &str, tries: u32| Job {
        kind: Kind::Send,
        thread: "agent-aaaaaaaaaaaa".into(),
        answer: AnswerTo::Thread("telegram:42".into()),
        to: Some(Peers {
            from: "aaaaaaaaaaaa".into(),
            to: to.into(),
        }),
        tries,
        ..job(id, 0)
    };
    store.put(&send("aaaaaaaaaah1", "cccccccccccc", 0)).unwrap();
    store
        .put(&task(
            "aaaaaaaaaah2",
            "telegram:42",
            None,
            &["aaaaaaaaaah1"],
        ))
        .unwrap();
    // Away twice while it moves, then there.
    let teammate = Arc::new(Teammate {
        away: 2,
        asked: Mutex::new(0),
    });
    let driver: Arc<dyn Driver> = teammate.clone();
    let mut at = now();
    for _ in 0..4 {
        look(&store, &driver, &CAPS, at, true);
        at += limits().retry_after_secs << limits().retries;
    }
    assert_eq!(
        *teammate.asked.lock().unwrap(),
        3,
        "two failures, then it lands once"
    );
    assert!(matches!(
        outcome(&store, "aaaaaaaaaah1"),
        Some(How::Done { .. })
    ));
    assert!(matches!(
        outcome(&store, "aaaaaaaaaah2"),
        Some(How::Done { .. })
    ));
    // Removed from the team: refused on its last try, for good.
    store
        .put(&send("aaaaaaaaaah3", "dddddddddddd", limits().retries))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaah4",
            "telegram:42",
            None,
            &["aaaaaaaaaah3"],
        ))
        .unwrap();
    look(&store, &driver, &CAPS, at, true);
    look(&store, &driver, &CAPS, at + 1, true);
    assert!(matches!(
        outcome(&store, "aaaaaaaaaah3"),
        Some(How::Failed { .. })
    ));
    assert!(cancelled(&store, "aaaaaaaaaah4"));
}

/// Subagent work is outside the day's budget: past it, a
/// step still fires, and what waits on it still waits for it.
#[test]
fn an_exhausted_budget_leaves_the_steps() {
    let (_dir, store) = store("budget");
    store
        .put(&task("aaaaaaaaaai1", "telegram:42", None, &[]))
        .unwrap();
    store
        .put(&task(
            "aaaaaaaaaai2",
            "telegram:42",
            None,
            &["aaaaaaaaaai1"],
        ))
        .unwrap();
    let caps = Caps { per_day: 0, ..CAPS };
    let (driver, seen) = recording();
    let looked = look(&store, &driver, &caps, 10, true);
    assert_eq!(looked.fired, ["aaaaaaaaaai1"]);
    assert!(looked.skipped.is_empty());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(store.sent_on(10), 0);
    assert!(store.get("aaaaaaaaaai2").is_some());
}

/// Ended jobs are kept a while, and at most so many.
#[test]
fn ended_jobs_are_kept_a_while() {
    let (_dir, store) = store("pruned");
    store
        .put(&task("aaaaaaaaaaj1", "telegram:42", None, &[]))
        .unwrap();
    let (driver, _) = recording();
    look(&store, &driver, &CAPS, 10, true);
    let ended = store.ended("aaaaaaaaaaj1").unwrap().outcome.unwrap().at;
    store.prune(ended + limits().ended_keep_secs);
    assert!(store.ended("aaaaaaaaaaj1").is_some());
    store.prune(ended + limits().ended_keep_secs + 1);
    assert!(store.ended("aaaaaaaaaaj1").is_none());
    assert!(How::done(&"é".repeat(limits().result_max)) != How::done(""));
    let How::Done { answer } = How::done(&"é".repeat(limits().result_max)) else {
        unreachable!()
    };
    assert!(answer.len() <= limits().result_max && answer.chars().all(|c| c == 'é'));
}
