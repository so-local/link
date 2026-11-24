//! What runs a driver through its promises: of conversations, narrowing,
//! files and jobs ([`check`], [`check_jobs`]).

use super::*;

/// Runs `driver` (one run here, or one behind the store bridge) through
/// the contract, on agent `agent`'s conversations
/// (none kept yet): numbers one after another and only once, of eight
/// writers at once one winning; reading from a number; the lease, held,
/// renewed, ended and taken over once lapsed, by one of four takers at
/// once; a conversation listed and removed. Panics where it falls short:
/// for tests, of every driver.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn check<S: Store + ?Sized>(driver: &S, agent: &Agent) {
    let said = |seq: u64, text: &str| serde_json::json!({"seq": seq, "role": "user", "text": text});
    let thread = "telegram:42";
    assert_eq!(driver.last(agent, thread).unwrap(), 0);
    assert!(driver.read(agent, thread, 1).unwrap().is_empty());
    assert_eq!(
        driver.append(agent, thread, &said(2, "past")).unwrap(),
        Appended::Taken(0)
    );
    for seq in 1..=3 {
        let event = said(seq, &format!("m{seq}"));
        assert_eq!(
            driver.append(agent, thread, &event).unwrap(),
            Appended::Written
        );
    }
    assert_eq!(
        driver.append(agent, thread, &said(2, "again")).unwrap(),
        Appended::Taken(3)
    );
    assert_eq!(
        driver.append(agent, thread, &said(5, "past")).unwrap(),
        Appended::Taken(3)
    );
    let texts = |first| -> Vec<String> {
        driver
            .read(agent, thread, first)
            .unwrap()
            .iter()
            .map(|e| e["text"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    assert_eq!(texts(1), ["m1", "m2", "m3"]);
    assert_eq!(texts(2), ["m2", "m3"]);
    assert!(texts(4).is_empty());
    assert_eq!(driver.last(agent, thread).unwrap(), 3);
    // Eight writers of one number at once: one writes it.
    let barrier = std::sync::Barrier::new(8);
    let won: Vec<String> = std::thread::scope(|s| {
        let tries: Vec<_> = (0..8)
            .map(|n| {
                let barrier = &barrier;
                s.spawn(move || {
                    let text = format!("4-{n}");
                    barrier.wait();
                    let wrote = driver.append(agent, thread, &said(4, &text)).unwrap();
                    (wrote == Appended::Written).then_some(text)
                })
            })
            .collect();
        tries
            .into_iter()
            .filter_map(|t| t.join().unwrap())
            .collect()
    });
    assert_eq!(won.len(), 1, "{won:?}");
    assert_eq!(texts(4), won);
    // Another agent's conversations are its own: none of this one's, or,
    // behind the store bridge, refused.
    let other = Agent {
        id: format!("{}-other", agent.id),
        owner: agent.owner.clone(),
    };
    assert!(
        driver
            .read(&other, thread, 1)
            .map_or(true, |events| events.is_empty())
    );
    // The lease.
    let day = Duration::from_secs(86_400);
    assert_eq!(driver.take(agent, thread, "a", day).unwrap(), Took::Took);
    assert_eq!(driver.take(agent, thread, "b", day).unwrap(), Took::Held);
    assert!(driver.renew(agent, thread, "a").unwrap());
    assert!(!driver.renew(agent, thread, "b").unwrap());
    let leases = driver.leases(agent, day).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(
        (leases[0].thread.as_str(), leases[0].holder.as_str()),
        (thread, "a")
    );
    assert!(!leases[0].lapsed);
    assert!(driver.leases(agent, Duration::ZERO).unwrap()[0].lapsed);
    driver.end(agent, thread, "b").unwrap();
    assert_eq!(driver.take(agent, thread, "b", day).unwrap(), Took::Held);
    driver.end(agent, thread, "a").unwrap();
    assert!(driver.leases(agent, day).unwrap().is_empty());
    // Not renewed for longer than it lasts: taken over by one of four
    // takers at once, and held from the others. (One taking it a second
    // after the one who took it over finds that lease lapsed in turn, as
    // it is: only takers all done within the second hold it to one.)
    let second = Duration::from_secs(1);
    assert_eq!(driver.take(agent, thread, "a", day).unwrap(), Took::Took);
    std::thread::sleep(Duration::from_millis(1200));
    let barrier = std::sync::Barrier::new(4);
    let racing = std::time::Instant::now();
    let took: Vec<Took> = std::thread::scope(|s| {
        let takers: Vec<_> = (0..4)
            .map(|n| {
                let barrier = &barrier;
                s.spawn(move || {
                    barrier.wait();
                    driver
                        .take(agent, thread, &format!("taker-{n}"), second)
                        .unwrap()
                })
            })
            .collect();
        takers.into_iter().map(|t| t.join().unwrap()).collect()
    });
    let raced = racing.elapsed();
    let lapsed = took.iter().filter(|t| **t == Took::Lapsed).count();
    let held = took.iter().filter(|t| **t == Took::Held).count();
    assert!(lapsed >= 1 && lapsed + held == 4, "{took:?}");
    if raced < second {
        assert_eq!((lapsed, held), (1, 3), "{took:?} in {raced:?}");
    }
    let holder = driver.leases(agent, day).unwrap()[0].holder.clone();
    assert!(holder.starts_with("taker-"), "{holder}");
    assert!(!driver.renew(agent, thread, "a").unwrap());
    // Listed, and removed with its lease.
    let threads = driver.threads(agent).unwrap();
    assert_eq!(
        threads,
        [Kept {
            thread: thread.into(),
            last: 4
        }]
    );
    driver.remove(agent, thread).unwrap();
    assert!(driver.threads(agent).unwrap().is_empty());
    assert!(driver.leases(agent, day).unwrap().is_empty());
    assert_eq!(driver.last(agent, thread).unwrap(), 0);
}

/// Runs `driver` through what the contract promises of a conversation's
/// narrowed tools, on agent `agent`'s conversation (none kept yet): none
/// until a list narrows it; then each list narrows what's left, never
/// widening it, eight lists at once all applied; a conversation's its
/// own; and a conversation removed keeps its narrowing, so it's never
/// made again with more. Panics where it falls short: for tests, of every
/// driver that keeps them.
pub fn check_narrowing<S: Store + ?Sized>(driver: &S, agent: &Agent) {
    let names = |n: &[&str]| n.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    let thread = "task:narrowed";
    assert_eq!(driver.tools_read(agent, thread).unwrap(), None);
    let all = names(&[
        "a__1", "a__2", "a__3", "a__4", "a__5", "a__6", "a__7", "a__8",
    ]);
    assert_eq!(driver.tools_narrow(agent, thread, &all).unwrap(), all);
    assert_eq!(
        driver
            .tools_narrow(agent, thread, &names(&["a__1", "x__9"]))
            .unwrap(),
        ["a__1"]
    );
    assert_eq!(
        driver.tools_read(agent, thread).unwrap(),
        Some(names(&["a__1"]))
    );
    // Eight at once, each leaving out one: all applied, so nothing is left.
    let other = "task:raced";
    driver.tools_narrow(agent, other, &all).unwrap();
    std::thread::scope(|s| {
        for n in 0..8 {
            let list: Vec<String> = all
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != n)
                .map(|(_, t)| t.clone())
                .collect();
            s.spawn(move || driver.tools_narrow(agent, other, &list).unwrap());
        }
    });
    assert_eq!(driver.tools_read(agent, other).unwrap(), Some(Vec::new()));
    assert_eq!(driver.tools_read(agent, "task:untouched").unwrap(), None);
    // Removed, its events and lease go; its narrowing stays, so it's
    // never made again with more.
    let event = serde_json::json!({"seq": 1, "role": "user", "text": "hi"});
    driver.append(agent, thread, &event).unwrap();
    driver.remove(agent, thread).unwrap();
    assert_eq!(driver.last(agent, thread).unwrap(), 0);
    assert!(
        !driver
            .threads(agent)
            .unwrap()
            .iter()
            .any(|kept| kept.thread == thread)
    );
    assert_eq!(
        driver.tools_read(agent, thread).unwrap(),
        Some(names(&["a__1"]))
    );
    assert_eq!(driver.tools_narrow(agent, thread, &all).unwrap(), ["a__1"]);
    // Every one narrowed is listed, removed or never written to.
    assert_eq!(
        driver.tools_list(agent).unwrap(),
        BTreeMap::from([
            (thread.to_string(), names(&["a__1"])),
            (other.to_string(), Vec::new()),
        ])
    );
    driver.append(agent, thread, &event).unwrap();
    assert_eq!(driver.last(agent, thread).unwrap(), 1);
}

/// Runs `driver` through what the contract promises of an agent's files,
/// on agent `agent`'s (none kept yet): written only at the version named,
/// of eight writers of one version one winning; read back with its
/// version; listed; added to at its end in one step, eight appends at
/// once all kept whole; one that would grow it past [`FILE_MAX`] refused,
/// the file as it was; removed only at its version, and made again never
/// at a version it had before; another agent's files its own, never this
/// one's read, written or removed as its. Panics where it falls short: for
/// tests, of every driver whose contract says `files`.
pub fn check_files<S: Store + ?Sized>(driver: &S, agent: &Agent) {
    // Equal bytes are not an earlier revision: catch both A-B-A and
    // deletion followed by recreation with the same contents.
    let revisions = "instructions/revision-check.md";
    let mut seen = std::collections::BTreeSet::new();
    let mut expected = 0;
    for bytes in [b"A".as_slice(), b"B", b"A"] {
        let Written::Version(next) = driver
            .file_write(agent, revisions, bytes, expected)
            .unwrap()
        else {
            panic!("a write at the current revision was refused");
        };
        assert!(
            next != 0 && seen.insert(next),
            "a file revision repeated for equal bytes"
        );
        if expected != 0 {
            assert!(matches!(
                driver
                    .file_write(agent, revisions, b"stale", expected)
                    .unwrap(),
                Written::Stale(_)
            ));
            assert!(!driver.file_remove(agent, revisions, expected).unwrap());
        }
        expected = next;
    }
    assert!(driver.file_remove(agent, revisions, expected).unwrap());
    let Written::Version(recreated) = driver.file_write(agent, revisions, b"A", 0).unwrap() else {
        panic!("a removed file was not recreated");
    };
    assert!(
        recreated != 0 && seen.insert(recreated),
        "a file revision repeated after recreating equal bytes"
    );
    for old in seen.iter().copied().filter(|old| *old != recreated) {
        assert!(matches!(
            driver.file_write(agent, revisions, b"stale", old).unwrap(),
            Written::Stale(_)
        ));
        assert!(!driver.file_remove(agent, revisions, old).unwrap());
    }
    assert!(driver.file_remove(agent, revisions, recreated).unwrap());
    let path = "instructions/instruction.md";
    assert_eq!(driver.file_read(agent, path).unwrap(), None);
    let Written::Version(first) = driver.file_write(agent, path, b"Be brief.", 0).unwrap() else {
        panic!("a new file wasn't written");
    };
    assert_eq!(
        driver.file_write(agent, path, b"stale", 0).unwrap(),
        Written::Stale(first)
    );
    assert_eq!(
        driver.file_read(agent, path).unwrap(),
        Some((b"Be brief.".to_vec(), first))
    );
    // Eight writers of one version: one writes.
    let won: Vec<u64> = std::thread::scope(|s| {
        let writers: Vec<_> = (0..8)
            .map(|n| {
                s.spawn(move || {
                    driver
                        .file_write(agent, path, format!("writer {n}").as_bytes(), first)
                        .unwrap()
                })
            })
            .collect();
        writers
            .into_iter()
            .filter_map(|w| match w.join().unwrap() {
                Written::Version(v) => Some(v),
                Written::Stale(_) => None,
            })
            .collect()
    });
    assert_eq!(won.len(), 1, "{won:?}");
    // Versions are the driver's: compared whole, never 0.
    let now = won[0];
    assert!(now != first && now != 0);
    let (body, version) = driver.file_read(agent, path).unwrap().unwrap();
    assert_eq!(version, now);
    assert!(String::from_utf8(body).unwrap().starts_with("writer "));
    // Added to at its end: eight at once, each kept whole.
    let notes = "instructions/notes.md";
    std::thread::scope(|s| {
        for n in 0..8 {
            s.spawn(move || {
                driver
                    .file_append(agent, notes, format!("[line {n}]").as_bytes())
                    .unwrap()
            });
        }
    });
    let (body, appended) = driver.file_read(agent, notes).unwrap().unwrap();
    let body = String::from_utf8(body).unwrap();
    for n in 0..8 {
        assert_eq!(body.matches(&format!("[line {n}]")).count(), 1, "{body}");
    }
    assert_eq!(body.len(), 8 * "[line 0]".len(), "{body}");
    // Past the most a file holds: refused, left as it was.
    let big = vec![b'x'; FILE_MAX as usize - 8];
    assert!(driver.file_append(agent, notes, &big).is_err());
    assert_eq!(
        driver.file_read(agent, notes).unwrap(),
        Some((body.clone().into_bytes(), appended))
    );
    assert!(
        driver
            .file_write(agent, notes, &vec![b'x'; FILE_MAX as usize + 1], appended)
            .is_err()
    );
    // Listed, in order.
    driver
        .file_write(
            agent,
            "skills/greet/SKILL.md",
            b"---\nname: greet\n---\n",
            0,
        )
        .unwrap();
    assert_eq!(
        driver.file_list(agent, "instructions/").unwrap(),
        [path, notes]
    );
    assert_eq!(
        driver.file_list(agent, "skills/").unwrap(),
        ["skills/greet/SKILL.md"]
    );
    // Removed only at its version.
    assert!(!driver.file_remove(agent, path, first).unwrap());
    assert!(driver.file_remove(agent, path, now).unwrap());
    assert_eq!(driver.file_read(agent, path).unwrap(), None);
    assert_eq!(driver.file_list(agent, "instructions/").unwrap(), [notes]);
    // Made again: never at a version it had before, so a writer still
    // holding one from when it was removed is refused.
    let Written::Version(again) = driver.file_write(agent, path, b"Again.", 0).unwrap() else {
        panic!("a file made again wasn't written");
    };
    for old in [first, now] {
        assert_ne!(again, old, "a version reused after a removal");
        assert!(matches!(
            driver.file_write(agent, path, b"stale", old).unwrap(),
            Written::Stale(_)
        ));
        assert!(!driver.file_remove(agent, path, old).unwrap());
    }
    assert_eq!(
        driver.file_read(agent, path).unwrap(),
        Some((b"Again.".to_vec(), again))
    );
    assert!(driver.file_remove(agent, path, again).unwrap());
    // A path outside the files a driver keeps by path: refused, never
    // sent.
    for bad in [
        "../etc/passwd",
        "chats/x",
        "instructions/../x",
        "/instructions/x",
    ] {
        assert!(driver.file_write(agent, bad, b"x", 0).is_err(), "{bad}");
    }
    // Another agent's files are its own: none of this one's, or, behind
    // the store bridge, refused.
    let other = Agent {
        id: format!("{}-other", agent.id),
        owner: agent.owner.clone(),
    };
    assert!(
        driver
            .file_read(&other, notes)
            .map_or(true, |file| file.is_none())
    );
    // Nor written or removed as another's: refused, or another agent's
    // own, never this one's.
    let _ = driver.file_write(&other, notes, b"theirs", appended);
    let _ = driver.file_append(&other, notes, b"theirs");
    assert!(!driver.file_remove(&other, notes, appended).unwrap_or(false));
    assert_eq!(
        driver.file_read(agent, notes).unwrap(),
        Some((body.into_bytes(), appended))
    );
}

/// Runs `driver` (one run here, or one behind the store bridge) through
/// the contract's jobs, on agent `agent`'s (none kept yet): a job put,
/// read back, replaced, counted and listed in its place alone; shifted
/// whole, of eight shifters of one job at once one moving it, one in its
/// way replaced; removed; what's due, of the waiting alone; the day's
/// count; the mark, moving with the waiting jobs; another agent's jobs
/// its own. Panics where it falls short: for tests, of every driver
/// whose contract says `jobs`.
pub fn check_jobs<S: Store + ?Sized>(driver: &S, agent: &Agent) {
    use Place::{Ended, Ending, Firing, Waiting};
    let job = |id: &str, due: u64, text: &str| Job {
        id: id.into(),
        spec: serde_json::json!({"id": id, "kind": "schedule", "text": text}),
        due,
    };
    let ids = |place| -> Vec<String> {
        let mut ids: Vec<String> = driver
            .job_list(agent, place)
            .unwrap()
            .into_iter()
            .map(|job| job.id)
            .collect();
        ids.sort();
        ids
    };
    let mark = || driver.job_mark(agent).unwrap();
    // None kept.
    for place in [Waiting, Firing, Ending, Ended] {
        assert!(ids(place).is_empty());
        assert_eq!(driver.job_count(agent, place).unwrap(), 0);
    }
    assert_eq!(driver.job_get(agent, Waiting, "a").unwrap(), None);
    assert_eq!(driver.job_next(agent).unwrap(), None);
    assert_eq!(driver.job_day(agent).unwrap(), (0, 0));
    // Put, read back as put, replaced; the mark moves with each.
    let before = mark();
    driver
        .job_put(agent, Waiting, &job("a", 300, "one"))
        .unwrap();
    assert_ne!(mark(), before);
    driver
        .job_put(agent, Waiting, &job("b", 200, "two"))
        .unwrap();
    let before = mark();
    driver
        .job_put(agent, Waiting, &job("a", 100, "one again"))
        .unwrap();
    assert_ne!(mark(), before);
    assert_eq!(
        driver.job_get(agent, Waiting, "a").unwrap(),
        Some(job("a", 100, "one again"))
    );
    assert_eq!(ids(Waiting), ["a", "b"]);
    assert_eq!(driver.job_count(agent, Waiting).unwrap(), 2);
    assert_eq!(driver.job_next(agent).unwrap(), Some(100));
    // In its place alone.
    assert_eq!(driver.job_get(agent, Firing, "a").unwrap(), None);
    assert!(ids(Firing).is_empty());
    // Shifted whole, once: what's due is of the waiting alone.
    let before = mark();
    assert!(driver.job_shift(agent, "a", Waiting, Firing).unwrap());
    assert_ne!(mark(), before);
    assert!(!driver.job_shift(agent, "a", Waiting, Firing).unwrap());
    assert_eq!(driver.job_get(agent, Waiting, "a").unwrap(), None);
    assert_eq!(
        driver.job_get(agent, Firing, "a").unwrap(),
        Some(job("a", 100, "one again"))
    );
    assert_eq!(driver.job_next(agent).unwrap(), Some(200));
    // Waiting again while its firing ends: one of its ID in two places,
    // and a shift into a place replacing the one there.
    driver
        .job_put(agent, Waiting, &job("a", 400, "next time"))
        .unwrap();
    driver
        .job_put(agent, Ending, &job("a", 1, "stale"))
        .unwrap();
    assert!(driver.job_shift(agent, "a", Firing, Ending).unwrap());
    assert_eq!(
        driver.job_get(agent, Ending, "a").unwrap(),
        Some(job("a", 100, "one again"))
    );
    assert_eq!(ids(Waiting), ["a", "b"]);
    assert_eq!(ids(Ending), ["a"]);
    assert!(ids(Firing).is_empty());
    // Eight shifters of one job at once: one moves it.
    let barrier = std::sync::Barrier::new(8);
    let moved = std::thread::scope(|s| {
        let tries: Vec<_> = (0..8)
            .map(|_| {
                let barrier = &barrier;
                s.spawn(move || {
                    barrier.wait();
                    driver.job_shift(agent, "b", Waiting, Firing).unwrap()
                })
            })
            .collect();
        tries
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|moved| *moved)
            .count()
    });
    assert_eq!(moved, 1);
    assert_eq!(ids(Waiting), ["a"]);
    assert_eq!(ids(Firing), ["b"]);
    assert_eq!(driver.job_next(agent).unwrap(), Some(400));
    // Ended, kept whole with its outcome, never due: what waits is
    // still what's next.
    assert!(driver.job_shift(agent, "a", Ending, Ended).unwrap());
    assert!(!driver.job_shift(agent, "a", Ending, Ended).unwrap());
    assert_eq!(
        driver.job_get(agent, Ended, "a").unwrap(),
        Some(job("a", 100, "one again"))
    );
    assert_eq!(ids(Ended), ["a"]);
    assert_eq!(driver.job_count(agent, Ended).unwrap(), 1);
    assert!(ids(Ending).is_empty());
    assert_eq!(driver.job_next(agent).unwrap(), Some(400));
    assert!(driver.job_shift(agent, "a", Ended, Ending).unwrap());
    // Removed from its place alone, once.
    assert!(driver.job_remove(agent, Ending, "a").unwrap());
    assert!(!driver.job_remove(agent, Ending, "a").unwrap());
    assert_eq!(ids(Waiting), ["a"]);
    let before = mark();
    assert!(driver.job_remove(agent, Waiting, "a").unwrap());
    assert_ne!(mark(), before);
    assert_eq!(driver.job_next(agent).unwrap(), None);
    // The day's count.
    driver.job_set_day(agent, 20_000, 3).unwrap();
    assert_eq!(driver.job_day(agent).unwrap(), (20_000, 3));
    driver.job_set_day(agent, 20_001, 1).unwrap();
    assert_eq!(driver.job_day(agent).unwrap(), (20_001, 1));
    // A spec too large is never kept.
    let large = Job {
        spec: serde_json::json!({"text": "x".repeat(JOB_MAX as usize)}),
        ..job("c", 1, "")
    };
    assert!(driver.job_put(agent, Waiting, &large).is_err());
    assert!(ids(Waiting).is_empty());
    // Another agent's jobs are its own: none of this one's, or, behind
    // the store bridge, refused.
    let other = Agent {
        id: format!("{}-other", agent.id),
        owner: agent.owner.clone(),
    };
    assert!(
        driver
            .job_list(&other, Firing)
            .map_or(true, |jobs| jobs.is_empty())
    );
    assert!(
        driver
            .job_get(&other, Firing, "b")
            .map_or(true, |job| job.is_none())
    );
    assert!(!driver.job_remove(&other, Firing, "b").unwrap_or(false));
    assert!(driver.job_day(&other).map_or(true, |day| day == (0, 0)));
    // What's left, gone.
    assert!(driver.job_remove(agent, Firing, "b").unwrap());
    for place in [Waiting, Firing, Ending, Ended] {
        assert!(ids(place).is_empty());
    }
}
