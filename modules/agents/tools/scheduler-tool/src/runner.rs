//! The runner: each look at the store fires what's due ([`look`]), runs
//! for good ([`run`]) or wakes once ([`wake`]).

use super::*;

/// What came of one look at the store.
#[derive(Debug, Default, PartialEq)]
pub struct Looked {
    /// The jobs fired, by id.
    pub fired: Vec<String>,
    /// The jobs whose firing was skipped: missed while asleep, or past
    /// the day's cap.
    pub skipped: Vec<String>,
    /// When the soonest job not due yet is due: what to wait for, unless
    /// the jobs change.
    pub next: Option<u64>,
}

/// Fires every job due at `at` through `driver`, each on a thread of its
/// own (a subagent's answer can take a while); `wait` waits for them.
///
/// A job is taken first, moved aside, so a cancel meanwhile wins and no
/// other runner fires it too. It waits again only once its firing ended:
/// a recurring one at its next time, a failed one to be tried again
/// later ([`Limits::retries`]), one past the day's cap the next day; one due once
/// and fired, or missed asleep with `late = "skip"`, is gone.
pub fn look(
    store: &Store,
    driver: &std::sync::Arc<dyn Driver>,
    caps: &Caps,
    at: u64,
    wait: bool,
) -> Looked {
    let (looked, threads) = look_firing(store, driver, caps, at);
    if wait {
        for thread in threads {
            let _ = thread.join();
        }
    }
    looked
}

/// [`look`], with a thread for each firing it started, still running.
pub(super) fn look_firing(
    store: &Store,
    driver: &std::sync::Arc<dyn Driver>,
    caps: &Caps,
    at: u64,
) -> (Looked, Vec<std::thread::JoinHandle<()>>) {
    if let Err(err) = crate::interactions::delete_tasks(store) {
        warn_why(
            sentence!("Selected task records could not be deleted."),
            &err,
        );
    }
    let mut looked = Looked::default();
    let _definitions = match crate::portable::lock(store) {
        Ok(lock) => lock,
        Err(err) => {
            warn_why(sentence!("Schedule definitions could not be locked."), &err);
            return (looked, Vec::new());
        }
    };
    let mut threads = Vec::new();
    let (due, waiting): (Vec<Job>, Vec<Job>) =
        store.jobs().into_iter().partition(|job| job.due <= at);
    looked.next = waiting.first().map(|job| job.due);
    for job in due {
        match crate::portable::ready(store, &job) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                warn_why(sentence!("A schedule activation could not be read."), &err);
                continue;
            }
        }
        match store.dependencies(&job) {
            Dependencies::Ready => {}
            // Fired once what it waits on ends done.
            Dependencies::Waiting => continue,
            Dependencies::Broken(why) => {
                if let Err(err) = store.break_off(&job.id, &why) {
                    warn_why(
                        sentence!(
                            "The job from {} couldn't be cancelled.",
                            Foreign::new(&job.from())
                        ),
                        &err,
                    );
                }
                looked.skipped.push(job.id.clone());
                continue;
            }
        }
        if !store.claim(&job.id) {
            // Cancelled, or another runner took it.
            continue;
        }
        let late = at.saturating_sub(job.due) > limits().late_after_secs;
        // `late = "skip"` is a schedule's: a step waiting on others, a
        // message to a teammate or its answer is late only because what it
        // waited on was, and still fires. A compaction late is one past
        // its prompt cache: never run.
        let skips = match job.kind {
            Kind::Schedule => !caps.late_runs,
            Kind::Compact => true,
            Kind::Task | Kind::ChildMessage | Kind::Send | Kind::Reply => false,
        };
        let skipped = if late && skips {
            Some(job.moved_on(at))
        } else if job.kind.counts() && store.sent_on(at) >= caps.per_day {
            // Kept for tomorrow: a recurring one at its first time then,
            // so a busy one isn't taken up and skipped again all day.
            let tomorrow = local_at(at, 1, 0, 0);
            Some(job.moved_on(tomorrow.saturating_sub(1)).or_else(|| {
                Some(Job {
                    due: tomorrow,
                    ..job.clone()
                })
            }))
        } else {
            None
        };
        if let Some(then) = skipped {
            if let Err(err) = store.settle(&job.id, then.as_ref()) {
                warn_why(
                    sentence!(
                        "The job from {} couldn't be updated.",
                        Foreign::new(&job.from())
                    ),
                    &err,
                );
            }
            looked.skipped.push(job.id.clone());
            continue;
        }
        match crate::interactions::ready(store, &job) {
            Ok(true) => {}
            blocked => {
                // Retain the exact job without consuming retry or per-day budget.
                let abandoned = store
                    .interactions
                    .as_ref()
                    .is_some_and(|s| s.delegation_abandoned(&job.id).unwrap_or(false));
                let _ = store.settle(&job.id, (!abandoned).then_some(&job));
                if let Err(err) = blocked {
                    warn_why(sentence!("Delegation could not be checked."), &err);
                }
                continue;
            }
        }
        // A message counts once, at its first try: trying it again after a
        // failure spends none of the day's, as `retries` bounds the tries.
        if job.tries == 0 && job.kind.counts() {
            let _ = store.count(at);
        }
        looked.fired.push(job.id.clone());
        let (store, driver, timeout) = (store.clone(), driver.clone(), caps.timeout);
        let hops = caps.hops;
        threads.push(std::thread::spawn(move || {
            let mut how = None;
            let then = match fire(&driver, &job, timeout) {
                Ended::TimedOut(timeout) => {
                    // What waits on it is cancelled, never run on nothing.
                    how = Some(How::Failed {
                        why: sentence!(
                            "It got no answer in {}, so it was given up on.",
                            Elapsed(timeout)
                        )
                        .to_string(),
                    });
                    job.moved_on(at)
                }
                Ended::Done(fired) => {
                    how = Some(How::done(&fired.answer));
                    // A teammate's answer, back to where its message was
                    // sent from.
                    if !fired.answer.trim().is_empty() {
                        send_back(&store, &job, hops, &fired.answer, "answer");
                    }
                    job.moved_on(at)
                }
                Ended::Refused(err) => {
                    // Refused by the way in's terms or a cap: trying it again
                    // changes nothing, so it ends, and a teammate's sender is
                    // told why, in the conversation it sent from.
                    warn_why(
                        sentence!(
                            "The job from {} was refused, so it isn't tried again.",
                            Foreign::new(&job.from())
                        ),
                        &err,
                    );
                    let why = format!("{err:#}");
                    send_back(
                        &store,
                        &job,
                        hops,
                        &format!("Your message wasn't taken, and isn't tried again: {why}."),
                        "refusal",
                    );
                    how = Some(How::Failed { why });
                    job.moved_on(at)
                }
                Ended::Failed(_)
                    if store
                        .interactions
                        .as_ref()
                        .is_some_and(|s| s.delegation_unstarted(&job.id).unwrap_or(false)) =>
                {
                    Some(job.clone())
                }
                Ended::Failed(err) if job.tries < limits().retries => {
                    let after = limits().retry_after_secs << job.tries;
                    warn_why(
                        sentence!(
                            "The job from {} failed; it is tried again in {}.",
                            Foreign::new(&job.from()),
                            Elapsed(Duration::from_secs(after))
                        ),
                        &err,
                    );
                    Some(Job {
                        due: at.max(now()) + after,
                        tries: job.tries + 1,
                        ..job.clone()
                    })
                }
                Ended::Failed(err) => {
                    warn_why(
                        sentence!(
                            "The job from {} failed {} and is given up on.",
                            Foreign::new(&job.from()),
                            Count::new(u64::from(job.tries) + 1, "time", "times")
                        ),
                        &err,
                    );
                    how = Some(How::Failed {
                        why: format!("{err:#}"),
                    });
                    job.moved_on(at)
                }
            };
            // A delegation's outcome is kept, for what waits on it and
            // whoever reads it; a schedule's next time, or nothing, is not.
            let settled = match (then.as_ref(), how) {
                (None, Some(how)) if job.keeps_outcome() => store.finish(&job.id, how),
                _ => store.settle(&job.id, then.as_ref()),
            };
            if let Err(err) = settled {
                warn_why(
                    sentence!(
                        "The job from {} couldn't be updated.",
                        Foreign::new(&job.from())
                    ),
                    &err,
                );
            }
        }));
    }
    (looked, threads)
}

/// Sends `text` (its `what`: an answer or a refusal) back from the
/// teammate `job` was sent to, to the conversation it was sent from: a
/// job of its own, so a failure to deliver it never sends the message
/// again; never past the `hops` a chain may have. Nothing for a job that
/// wasn't sent to a teammate.
fn send_back(store: &Store, job: &Job, hops: u32, text: &str, what: &str) {
    let Ok(id) = link_id::candidate() else {
        return;
    };
    let Some(reply) = job.reply(&id, text, now()) else {
        return;
    };
    if reply.hops > hops {
        warn(sentence!(
            "The {} to {} isn't sent back: it would be {} down a chain, past the {} there may be.",
            what,
            Foreign::new(&job.from()),
            Count::new(reply.hops.into(), "message", "messages"),
            Count::new(hops.into(), "message", "messages")
        ));
    } else if let Err(err) = store.put(&reply) {
        warn_why(
            sentence!("The {} to {} wasn't kept.", what, Foreign::new(&job.from())),
            &err,
        );
    }
}

/// How a firing ended.
enum Ended {
    /// Its answer went where it goes.
    Done(Fired),
    /// No answer came in its timeout, so it was given up on: failed, never
    /// sent again, since it may have reached the agent.
    TimedOut(Duration),
    /// The agent's way in read it and refused it, by its terms or a cap
    /// ([`link_sandbox_binding::Why::Refused`]): final, never tried again.
    Refused(anyhow::Error),
    /// It didn't reach the agent, or the agent didn't answer.
    Failed(anyhow::Error),
}

/// Fires `job`, waiting `timeout` at most for it before it's given up on;
/// returns only once it ended, so a subagent counts until then, even one
/// given up on, which ends failed whatever it answers later.
fn fire(driver: &std::sync::Arc<dyn Driver>, job: &Job, timeout: Option<Duration>) -> Ended {
    let (done, ended) = std::sync::mpsc::channel();
    let (driver, firing) = (driver.clone(), job.firing());
    let teammate = match (job.kind, &job.to) {
        (Kind::Send, Some(peers)) => Some(peers.to.clone()),
        _ => None,
    };
    std::thread::spawn(move || {
        let _ = done.send(match teammate {
            Some(agent) => driver.fire_to(&agent, &firing),
            None => driver.fire(&firing),
        });
    });
    let ended = match timeout {
        // A timeout of nothing gives up at once, whatever has answered.
        Some(timeout) => match (!timeout.is_zero())
            .then(|| ended.recv_timeout(timeout).ok())
            .flatten()
        {
            Some(ended) => ended,
            None => {
                warn(sentence!(
                    "The job from {} got no answer in {}, so it is given up on and never sent again.",
                    Foreign::new(&job.from()),
                    Elapsed(timeout)
                ));
                let _ = ended.recv();
                return Ended::TimedOut(timeout);
            }
        },
        None => ended
            .recv()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("its firing stopped"))),
    };
    match ended {
        Ok(fired) => Ended::Done(fired),
        Err(err) if is_refused(&err) => Ended::Refused(err),
        Err(err) => Ended::Failed(err),
    }
}

/// Whether `err` holds the way in's refusal, however it was wrapped.
fn is_refused(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<link_sandbox_binding::Failure>()
            .is_some_and(|failure| failure.why == link_sandbox_binding::Why::Refused)
    })
}

/// Says each job skipped: missed while asleep, or past the day's cap.
fn say_skipped(looked: &Looked) {
    for id in &looked.skipped {
        warn(sentence!(
            "Job {} was skipped: it was missed while asleep, or is past the day's cap.",
            Name::new("job", id)
        ));
    }
}

/// What waits for the agent elsewhere put in `store` as jobs
/// ([`Driver::arrivals`]): a failure said, and the jobs fired anyway.
fn arrive(store: &Store, driver: &std::sync::Arc<dyn Driver>) {
    if let Err(err) = driver.arrivals(store) {
        warn(sentence!(
            "Messages waiting elsewhere weren't taken: {}.",
            Foreign::new(&format!("{err:#}"))
        ));
    }
}

/// Runs for good on a resident driver: fires each job as it falls due
/// ([`look`]), sleeping until the soonest is due, or a job changes
/// ([`Store::wait`]). On one that isn't ([`Cron`], [`Addressed`]), wakes once
/// ([`wake`]) and returns.
pub fn run(store: &Store, driver: std::sync::Arc<dyn Driver>, caps: &Caps) -> anyhow::Result<()> {
    if !driver.resident() {
        say_skipped(&wake(store, &driver, caps, now())?);
        return Ok(());
    }
    run_while(store, driver, caps, &|| true)
}

/// [`run`] on a resident driver while `keep` says so, looked at once a
/// second at least: once it says no, it returns when the firings it
/// started have ended. Its mark on the store ([`Store::runner`]) is let
/// go only then, so a runner after it never fires one of them again.
pub fn run_while(
    store: &Store,
    driver: std::sync::Arc<dyn Driver>,
    caps: &Caps,
    keep: &dyn Fn() -> bool,
) -> anyhow::Result<()> {
    let _runner = store.runner()?;
    let mut firing = Vec::new();
    while keep() {
        // The folder's time before the jobs are read: a job put while
        // they're looked at wakes the wait at once.
        let changed = store.changed();
        arrive(store, &driver);
        let (looked, threads) = look_firing(store, &driver, caps, now());
        say_skipped(&looked);
        firing.retain(|thread: &std::thread::JoinHandle<()>| !thread.is_finished());
        firing.extend(threads);
        store.wait_while(looked.next, changed, keep);
    }
    for thread in firing {
        let _ = thread.join();
    }
    Ok(())
}

/// One wake of a runner something else starts ([`Cron`], [`Addressed`]):
/// every job due at `at` fired, and its firing ended, before it returns,
/// and the driver told how the jobs are left ([`Driver::woken`]); what a
/// stopped runner was firing is moved back first if no other runner is
/// at work ([`Store::runner`]).
pub fn wake(
    store: &Store,
    driver: &std::sync::Arc<dyn Driver>,
    caps: &Caps,
    at: u64,
) -> anyhow::Result<Looked> {
    let _runner = store.runner()?;
    arrive(store, driver);
    let looked = look(store, driver, caps, at, true);
    driver.woken(store)?;
    Ok(looked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_job_fires_once_when_due() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("once-{wakes:?}"));
            let (driver, seen) = recording();
            store.put(&job("aaaaaaaaaaa1", 1000)).unwrap();
            let early = fire_at(wakes, &store, &driver, &CAPS, 999);
            assert!(early.fired.is_empty());
            // What the runner waits for next.
            assert_eq!(early.next, Some(1000));
            assert_eq!(
                fire_at(wakes, &store, &driver, &CAPS, 1000).fired,
                ["aaaaaaaaaaa1"]
            );
            assert!(
                fire_at(wakes, &store, &driver, &CAPS, 1001)
                    .fired
                    .is_empty()
            );
            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].from, "schedule:aaaaaaaaaaa1");
            assert!(store.jobs().is_empty());
            assert!(store.firing().is_empty());
        }
    }

    #[test]
    fn a_recurring_job_moves_on() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("cron-{wakes:?}"));
            let (driver, seen) = recording();
            let every = Job {
                cron: Some("* * * * *".into()),
                ..job("aaaaaaaaaaa2", 600)
            };
            store.put(&every).unwrap();
            for at in [600, 660, 720] {
                assert_eq!(fire_at(wakes, &store, &driver, &CAPS, at).fired.len(), 1);
            }
            assert_eq!(seen.lock().unwrap().len(), 3);
            assert_eq!(store.jobs()[0].due, 780);
        }
    }

    #[test]
    fn a_missed_firing_runs_once_or_is_skipped() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("late-{wakes:?}"));
            let (driver, seen) = recording();
            let every = Job {
                cron: Some("* * * * *".into()),
                ..job("aaaaaaaaaaa3", 600)
            };
            store.put(&every).unwrap();
            // Asleep for an hour: once, on waking, not sixty times.
            assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 4200).fired.len(), 1);
            assert_eq!(seen.lock().unwrap().len(), 1);
            assert_eq!(store.jobs()[0].due, 4260);
            let skip = Caps {
                late_runs: false,
                ..CAPS
            };
            let looked = fire_at(wakes, &store, &driver, &skip, 9000);
            assert_eq!(looked.skipped, ["aaaaaaaaaaa3"]);
            assert_eq!(seen.lock().unwrap().len(), 1);
            // Skipped, it moved on to its next time.
            assert_eq!(store.jobs()[0].due, 9060);
        }
    }

    /// `late = "skip"` skips only schedules: a subagent, a step waiting on
    /// others, a message to a teammate and its answer, due while the
    /// runner was off, still fire.
    #[test]
    fn late_skip_leaves_what_isnt_a_schedule() {
        let skip = Caps {
            late_runs: false,
            ..CAPS
        };
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("late-kinds-{wakes:?}"));
            let (driver, _seen) = recording();
            let kinds = [Kind::Task, Kind::Send, Kind::Reply, Kind::Schedule];
            for (n, kind) in kinds.into_iter().enumerate() {
                store
                    .put(&Job {
                        kind,
                        to: matches!(kind, Kind::Send | Kind::Reply).then(|| Peers {
                            from: "aaaaaaaaaaf1".into(),
                            to: "aaaaaaaaaaf2".into(),
                        }),
                        ..job(&format!("aaaaaaaaaae{n}"), 600)
                    })
                    .unwrap();
            }
            let looked = fire_at(wakes, &store, &driver, &skip, 9000);
            assert_eq!(looked.skipped, ["aaaaaaaaaae3"], "only the schedule");
            assert_eq!(
                looked.fired,
                ["aaaaaaaaaae0", "aaaaaaaaaae1", "aaaaaaaaaae2"]
            );
        }
    }

    /// The day's cap is schedules' and teammates' messages: a subagent
    /// still starts past it, and a recurring schedule past it waits for
    /// its first time tomorrow, taken up once, not every minute.
    #[test]
    fn subagents_are_outside_the_days_cap() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("cap-kinds-{wakes:?}"));
            let (driver, seen) = recording();
            let caps = Caps { per_day: 1, ..CAPS };
            let at = local_at(now(), 0, 12, 0);
            store.put(&job("aaaaaaaaaac1", at)).unwrap();
            store
                .put(&Job {
                    cron: Some("* * * * *".into()),
                    ..job("aaaaaaaaaac2", at)
                })
                .unwrap();
            store
                .put(&Job {
                    kind: Kind::Task,
                    thread: "task-aaaaaaaaaac3".into(),
                    answer: AnswerTo::Thread("telegram:42".into()),
                    ..job("aaaaaaaaaac3", at)
                })
                .unwrap();
            let looked = fire_at(wakes, &store, &driver, &caps, at);
            assert_eq!(looked.fired, ["aaaaaaaaaac1", "aaaaaaaaaac3"]);
            assert_eq!(looked.skipped, ["aaaaaaaaaac2"]);
            assert_eq!(seen.lock().unwrap().len(), 2);
            assert_eq!(store.sent_on(at), 1, "the subagent spent none");
            let every = store.get("aaaaaaaaaac2").unwrap();
            assert_eq!(every.due, local_at(at, 1, 0, 0), "tomorrow's first minute");
        }
    }

    /// Past the day's cap nothing fires, and a job due once waits for the
    /// next day rather than being lost.
    #[test]
    fn past_the_days_cap_nothing_fires_till_the_next_day() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("cap-{wakes:?}"));
            let (driver, seen) = recording();
            let caps = Caps { per_day: 2, ..CAPS };
            // Noon: the day after is a while off.
            let at = local_at(now(), 0, 12, 0);
            for n in 1..=3 {
                store.put(&job(&format!("aaaaaaaaaab{n}"), at)).unwrap();
            }
            let looked = fire_at(wakes, &store, &driver, &caps, at);
            assert_eq!(looked.fired.len(), 2);
            assert_eq!(looked.skipped, ["aaaaaaaaaab3"]);
            assert_eq!(seen.lock().unwrap().len(), 2);
            assert_eq!(store.sent_on(at), 2);
            let kept = store.get("aaaaaaaaaab3").expect("kept for tomorrow");
            assert_eq!(kept.due, local_at(at, 1, 0, 0));
            assert_eq!(local_day(kept.due), local_day(at) + 1);
            // Still today: still past the cap.
            assert!(
                fire_at(wakes, &store, &driver, &caps, at + 1)
                    .fired
                    .is_empty()
            );
            let looked = fire_at(wakes, &store, &driver, &caps, kept.due);
            assert_eq!(looked.fired, ["aaaaaaaaaab3"]);
            assert!(store.jobs().is_empty());
        }
    }

    /// A conversation's idle compaction fires when it's due, to its
    /// conversation from its schedule, its answer nowhere, even past the
    /// day's cap, and counts none of the day's messages; kept again, it
    /// moves rather than adding a second.
    #[test]
    fn a_compaction_fires_when_due_past_the_cap_counting_nothing() {
        use link_scheduler_tool_contract::compaction;
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("compact-{wakes:?}"));
            let (driver, seen) = recording();
            let caps = Caps { per_day: 1, ..CAPS };
            let at = local_at(now(), 0, 12, 0);
            store.put(&job("aaaaaaaaaac1", at)).unwrap();
            let compact = |due| Job {
                id: compaction::job("telegram:42"),
                kind: Kind::Compact,
                thread: "telegram:42".into(),
                answer: AnswerTo::Nowhere,
                ..job("unused", due)
            };
            store.put(&compact(at + 100)).unwrap();
            store.put(&compact(at + 200)).unwrap();
            assert_eq!(store.jobs().len(), 2, "one compaction a conversation");
            assert_eq!(fire_at(wakes, &store, &driver, &caps, at).fired.len(), 1);
            assert!(
                fire_at(wakes, &store, &driver, &caps, at + 199)
                    .fired
                    .is_empty()
            );
            let looked = fire_at(wakes, &store, &driver, &caps, at + 200);
            assert_eq!(looked.fired, [compaction::job("telegram:42")]);
            assert_eq!(store.sent_on(at), 1);
            let seen = seen.lock().unwrap();
            let fired = seen.last().unwrap();
            assert_eq!(fired.from, compaction::from("telegram:42"));
            assert_eq!(fired.thread, "telegram:42");
            assert_eq!(fired.answer, AnswerTo::Nowhere);
            assert!(store.jobs().is_empty());
            drop(seen);

            // Missed while asleep, it's dropped, even where missed
            // firings run: its prompt cache is gone.
            store.put(&compact(at + 300)).unwrap();
            let looked = fire_at(
                wakes,
                &store,
                &driver,
                &caps,
                at + 300 + limits().late_after_secs + 1,
            );
            assert_eq!(looked.skipped, [compaction::job("telegram:42")]);
            assert!(looked.fired.is_empty());
            assert!(store.jobs().is_empty());
        }
    }

    /// A firing that fails (the bridge not up yet, the model provider
    /// unreachable) keeps its job, tried again later, and given up only
    /// after [`Limits::retries`].
    #[test]
    fn a_failed_firing_is_kept_and_tried_again() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("retry-{wakes:?}"));
            let (driver, asked) = failing(2);
            store.put(&job("aaaaaaaaaae1", 1000)).unwrap();
            assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 1000).fired.len(), 1);
            let kept = store.get("aaaaaaaaaae1").expect("kept after a failure");
            assert_eq!(kept.tries, 1);
            assert!(kept.due >= 1000 + limits().retry_after_secs);
            assert!(
                fire_at(wakes, &store, &driver, &CAPS, kept.due - 1)
                    .fired
                    .is_empty()
            );
            fire_at(wakes, &store, &driver, &CAPS, kept.due);
            let kept = store.get("aaaaaaaaaae1").expect("kept after two");
            assert_eq!(kept.tries, 2);
            assert_eq!(
                fire_at(wakes, &store, &driver, &CAPS, kept.due).fired.len(),
                1
            );
            assert_eq!(*asked.lock().unwrap(), 3);
            // Tried three times, it counts once toward the day's cap.
            assert_eq!(store.sent_on(1000), 1);
            assert!(store.jobs().is_empty() && store.firing().is_empty());

            // One that never goes is given up, not tried for ever.
            let (driver, asked) = failing(usize::MAX);
            store.put(&job("aaaaaaaaaae2", 1000)).unwrap();
            while let Some(kept) = store.get("aaaaaaaaaae2") {
                fire_at(wakes, &store, &driver, &CAPS, kept.due);
            }
            assert_eq!(*asked.lock().unwrap(), limits().retries as usize + 1);
        }
    }

    /// A cancel while a job is being fired holds: its firing ending,
    /// failed or not, never brings it back.
    #[test]
    fn a_cancel_during_a_firing_is_not_undone() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("cancel-{wakes:?}"));
            for (id, cron, fails) in [
                ("aaaaaaaaaaf1", Some("* * * * *"), false),
                ("aaaaaaaaaaf2", None, true),
            ] {
                let cancelling = store.clone();
                let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
                    let id = firing.from.trim_start_matches("schedule:");
                    assert!(cancelling.cancel(id).unwrap());
                    match fails {
                        true => bail!("the model provider is unreachable"),
                        false => Ok(link_connection_contract::Fired::default()),
                    }
                }));
                let every = Job {
                    cron: cron.map(str::to_string),
                    ..job(id, 600)
                };
                store.put(&every).unwrap();
                assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 600).fired, [id]);
                assert!(store.get(id).is_none(), "{id} came back");
            }
            assert!(store.firing().is_empty());
        }
    }

    /// A recurring job whose next time can't be written is kept aside,
    /// not fired again and again until the runner restarts.
    #[test]
    fn a_job_that_cant_move_on_doesnt_fire_again() {
        let (dir, store) = store("stuck");
        let (driver, seen) = recording();
        let every = Job {
            cron: Some("* * * * *".into()),
            ..job("aaaaaaaaaai1", 600)
        };
        store.put(&every).unwrap();
        // The next waiting job cannot replace a directory. This blocks the
        // write independently of the driver's temporary-file naming.
        let aside = dir.join("schedule/jobs/aaaaaaaaaai1.json");
        let blocking = aside.clone();
        let wrapped = driver.clone();
        let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
            std::fs::create_dir(&blocking).unwrap();
            wrapped.fire(firing)
        }));
        look(&store, &driver, &CAPS, 600, true);
        for at in [601, 602, 660] {
            assert!(look(&store, &driver, &CAPS, at, true).fired.is_empty());
        }
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(store.firing().len(), 1);
        std::fs::remove_dir(&aside).unwrap();
        store.recover();
        assert_eq!(store.get("aaaaaaaaaai1").unwrap().tries, 1);
    }

    /// What a runner stopped mid-firing was firing waits again when it
    /// restarts, counted as a try.
    #[test]
    fn a_restart_loses_nothing() {
        let (_dir, store) = store("restart");
        store.put(&job("aaaaaaaaaag1", 5)).unwrap();
        assert!(store.claim("aaaaaaaaaag1"));
        assert!(store.get("aaaaaaaaaag1").is_none());
        store.recover();
        assert_eq!(store.get("aaaaaaaaaag1").unwrap().tries, 1);
        assert!(store.firing().is_empty());
    }

    #[test]
    fn a_subagent_counts_until_its_answer_lands() {
        let (_dir, store) = store("subagents");
        let task = Job {
            kind: Kind::Task,
            thread: "task-aaaaaaaaaac1".into(),
            answer: AnswerTo::Thread("telegram:42".into()),
            ..job("aaaaaaaaaac1", 0)
        };
        store.put(&task).unwrap();
        assert_eq!(store.subagents(), 1);
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let driver: Arc<dyn Driver> = Arc::new(InProcess(move |_: &Firing| {
            let _ = wait.lock().unwrap().recv();
            Ok(link_connection_contract::Fired::default())
        }));
        // Given up on at once: its driver can't answer before `go`, so a
        // timeout of nothing has passed whatever the machine's load. It
        // still counts while it works.
        let caps = Caps {
            timeout: Some(Duration::ZERO),
            ..CAPS
        };
        let looked = look(&store, &driver, &caps, 10, false);
        assert_eq!(looked.fired.len(), 1);
        // Fired, its answer not back: still at work.
        assert_eq!(store.subagents(), 1);
        go.send(()).unwrap();
        link_test_kit::eventually(
            "the subagent's answer to land",
            Duration::from_secs(10),
            || store.subagents() == 0,
        );
        assert!(store.jobs().is_empty());
        // Its outcome, kept: given up waiting on, never sent again, and
        // failed, so what waits on it is cancelled rather than run on an
        // answer it never had.
        let ended = store.ended("aaaaaaaaaac1").expect("kept ended");
        match ended.outcome.map(|o| o.how) {
            Some(How::Failed { why }) => assert!(why.contains("no answer"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// The runner sleeps till the soonest job, but wakes as soon as one
    /// is put.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_runner_wakes_when_a_job_is_put() {
        let (_dir, store) = store("wake");
        let putting = store.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            putting.put(&job("aaaaaaaaaah1", 5)).unwrap();
        });
        let started = std::time::Instant::now();
        store.wait(None, store.changed());
        assert!(started.elapsed() < Duration::from_secs(5));
        // One put after the runner read the jobs, before it waits: it
        // wakes at once, not a minute later.
        let changed = store.changed();
        std::thread::sleep(Duration::from_millis(20));
        store.put(&job("aaaaaaaaaah2", 5)).unwrap();
        let started = std::time::Instant::now();
        store.wait(None, changed);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// The calendar wakes a runner each minute, and one may still wait on
    /// a firing when the next starts: that one fires what fell due since,
    /// and never moves back, nor fires again, what the first is firing.
    /// What a runner cut off left is moved back by the next one alone,
    /// counted as a try.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_wake_leaves_another_runners_firing_alone() {
        const ISOLATED: &str = "LNK_TEST_SCHEDULER_WAKE";
        if std::env::var_os(ISOLATED).is_none() {
            // Other unit tests fork commands, which can briefly inherit
            // a live flock before exec closes it. Keep this runner-owner
            // test in a process whose threads don't fork unrelated work.
            let output = link_secretive::process::command(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "runner::tests::a_wake_leaves_another_runners_firing_alone",
                ])
                .env(ISOLATED, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let (_dir, store) = store("wakes");
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = seen.clone();
        let driver: Arc<dyn Driver> =
            Arc::new(Cron(Arc::new(InProcess(move |firing: &Firing| {
                kept.lock().unwrap().push(firing.from.clone());
                if firing.from == "schedule:aaaaaaaaaam1" {
                    let _ = wait.lock().unwrap().recv();
                }
                Ok(link_connection_contract::Fired::default())
            }))));
        assert!(!driver.resident());
        store.put(&job("aaaaaaaaaam1", 600)).unwrap();
        let (first_store, first_driver) = (store.clone(), driver.clone());
        let first = std::thread::spawn(move || wake(&first_store, &first_driver, &CAPS, 600));
        // The first in its driver, its firing claimed and `m1` seen.
        while seen.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(5));
        }
        store.put(&job("aaaaaaaaaam2", 660)).unwrap();
        let looked = wake(&store, &driver, &CAPS, 660).unwrap();
        assert_eq!(looked.fired, ["aaaaaaaaaam2"]);
        assert_eq!(store.firing().len(), 1, "the first's firing was moved");
        go.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap().fired, ["aaaaaaaaaam1"]);
        assert_eq!(
            *seen.lock().unwrap(),
            ["schedule:aaaaaaaaaam1", "schedule:aaaaaaaaaam2"]
        );
        assert!(store.jobs().is_empty() && store.firing().is_empty());
        // A runner cut off as it fired: the next wake, alone, fires it
        // again.
        store.put(&job("aaaaaaaaaam3", 720)).unwrap();
        assert!(store.claim("aaaaaaaaaam3"));
        let looked = wake(&store, &driver, &CAPS, 780).unwrap();
        assert_eq!(looked.fired, ["aaaaaaaaaam3"]);
        assert!(store.jobs().is_empty() && store.firing().is_empty());
    }

    /// Jobs kept by another store than the folder ([`store::Memory`])
    /// fire, move on, are cancelled and come back after a restart as in
    /// the folder.
    #[test]
    fn jobs_kept_elsewhere_fire_as_in_the_folder() {
        let (dir, _) = store("memory");
        let store = Store::with(&dir, Arc::new(store::Memory::default())).unwrap();
        let (driver, seen) = recording();
        let every = Job {
            cron: Some("* * * * *".into()),
            ..job("aaaaaaaaaar1", 600)
        };
        store.put(&every).unwrap();
        store.put(&job("aaaaaaaaaar2", 600)).unwrap();
        let task = Job {
            kind: Kind::Task,
            ..job("aaaaaaaaaar3", 5000)
        };
        store.put(&task).unwrap();
        assert_eq!((store.waiting(), store.subagents()), (3, 1));
        assert_eq!(store.next().unwrap(), Some(600));
        let changed = store.changed();
        let looked = look(&store, &driver, &CAPS, 600, true);
        assert_eq!(looked.fired, ["aaaaaaaaaar1", "aaaaaaaaaar2"]);
        assert_ne!(store.changed(), changed);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(store.get("aaaaaaaaaar1").unwrap().due, 660);
        assert!(store.get("aaaaaaaaaar2").is_none());
        assert_eq!(store.sent_on(600), 2);
        assert!(store.cancel("aaaaaaaaaar1").unwrap());
        assert!(!store.cancel("aaaaaaaaaar1").unwrap());
        // Cut off as it fired: back, counted as a try.
        assert!(store.claim("aaaaaaaaaar3"));
        assert_eq!(store.subagents(), 1);
        store.recover();
        assert_eq!(store.get("aaaaaaaaaar3").unwrap().tries, 1);
        assert!(store.firing().is_empty());
        // Nothing of the jobs in the folder.
        assert!(!dir.join("schedule/jobs").join("aaaaaaaaaar3.json").exists());
    }
}
