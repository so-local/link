//! A turn's lease: never swapped, and taken once when it lapses.

use super::*;

/// Two writers of one process at once, of one event's number or of
/// the lease, each write their own bytes: the event's winner is the
/// one the file holds, and a renewal never puts a waiter's lease in
/// place of its own.
#[test]
fn two_writers_of_one_process_never_swap() {
    let (root, agents) = world("swap");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    for seq in 1..=200 {
        let barrier = std::sync::Barrier::new(8);
        let won: Vec<String> = std::thread::scope(|s| {
            let tries: Vec<_> = (0..8)
                .map(|n| {
                    let (store, main, thread, barrier) = (&store, &main, &thread, &barrier);
                    s.spawn(move || {
                        let text = format!("{seq}-{n}");
                        barrier.wait();
                        let wrote = store.append(main, thread, &said(seq, &text), &[]);
                        wrote.is_ok().then_some(text)
                    })
                })
                .collect();
            tries
                .into_iter()
                .filter_map(|t| t.join().unwrap())
                .collect()
        });
        assert_eq!(won.len(), 1, "{seq}: {won:?}");
        let kept = store.read_from(&main, &thread, seq).unwrap();
        assert_eq!(kept[0].text, won[0], "{seq}");
    }
    let thread = Thread::new("telegram:43").unwrap();
    assert_eq!(
        store.take_turn(&main, &thread, "running").unwrap(),
        Took::Took
    );
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        for n in 0..3 {
            let (store, main, thread, stop) = (&store, &main, &thread, &stop);
            s.spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let took = store.take_turn(main, thread, &format!("waiting-{n}"));
                    assert_eq!(took.unwrap(), Took::Held);
                }
            });
        }
        for _ in 0..2000 {
            store.renew_turn(&main, &thread, "running").unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let folder = root.join("main/own/chats/telegram-43");
    assert_eq!(
        store
            .lease(&Held::root(&folder, false).unwrap().unwrap())
            .unwrap()
            .holder,
        "running"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A lapsed lease is taken by one taker only, however many try at
/// once: one finishes the crashed turn, the rest wait for it.
#[test]
fn a_lapsed_lease_is_taken_once() {
    let (root, agents) = world("lapsed");
    let store = Files::new(agents);
    let main = Agent::new("main").unwrap();
    let thread = Thread::new("telegram:42").unwrap();
    let folder = root.join("main/own/chats/telegram-42");
    for round in 0..100 {
        std::fs::create_dir_all(&folder).unwrap();
        let crashed = Lease {
            holder: "crashed".into(),
            thread: thread.name().into(),
            started: 1,
            renewed: 1,
        };
        std::fs::write(folder.join(TURN), serde_json::to_vec(&crashed).unwrap()).unwrap();
        let barrier = std::sync::Barrier::new(4);
        let took: Vec<Took> = std::thread::scope(|s| {
            let takers: Vec<_> = (0..4)
                .map(|n| {
                    let (store, main, thread, barrier) = (&store, &main, &thread, &barrier);
                    s.spawn(move || {
                        barrier.wait();
                        store
                            .take_turn(main, thread, &format!("taker-{n}"))
                            .unwrap()
                    })
                })
                .collect();
            takers.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let lapsed = took.iter().filter(|t| **t == Took::Lapsed).count();
        let fresh = took.iter().filter(|t| **t == Took::Took).count();
        assert_eq!((lapsed, fresh), (1, 0), "{round}: {took:?}");
        let holder = store
            .lease(&Held::root(&folder, false).unwrap().unwrap())
            .unwrap()
            .holder;
        assert!(holder.starts_with("taker-"), "{round}: {holder}");
        std::fs::remove_file(folder.join(TURN)).unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}
