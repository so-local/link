use super::*;

/// A running proxy's traffic is listed; a gone one's file deleted.
#[test]
fn traffic_lists_running_proxies_only() {
    let dir = link_test_kit::TempDir::new("wire-traffic");
    let me = std::process::id();
    let mine = Traffic {
        pid: me,
        sandbox: "main-otter".into(),
        up: 10,
        down: 20,
        started: link_measure_contract::client::started(me),
    };
    let gone = Traffic {
        pid: 999_999_999,
        ..mine.clone()
    };
    // A process of this number before this one: its file isn't this
    // one's.
    let before = Traffic {
        pid: std::os::unix::process::parent_id(),
        started: Some(1),
        ..mine.clone()
    };
    let linux = cfg!(target_os = "linux");
    for t in [Some(&mine), Some(&gone), linux.then_some(&before)]
        .into_iter()
        .flatten()
    {
        link_files::write_private(
            &traffic_dir(&dir).join(format!("{}.json", t.pid)),
            &serde_json::to_vec(t).unwrap(),
        )
        .unwrap();
    }
    let parent = std::os::unix::process::parent_id();
    let before_path = traffic_dir(&dir).join(format!("{parent}.json"));
    assert_eq!(traffic(&dir), std::slice::from_ref(&mine));
    assert!(!traffic_dir(&dir).join("999999999.json").exists());
    assert!(!before_path.exists());
    // As the measure module reads it, from `lnk sandbox show --json`.
    let printed = serde_json::to_value(traffic(&dir)).unwrap();
    let read: link_measure_contract::client::Carried =
        serde_json::from_value(printed[0].clone()).unwrap();
    assert_eq!(
        read,
        link_measure_contract::client::Carried { up: 10, down: 20 }
    );
}

/// A proxy's file is credited to a process only when it names that
/// process's start: one naming none (planted, or left by a proxy of
/// the same number) is refused where the start can be read.
#[test]
fn a_traffic_file_without_its_start_is_no_ones() {
    let dir = link_test_kit::TempDir::new("lnk-traffic-started");
    let me = std::process::id();
    let write = |json: serde_json::Value| {
        link_files::write_private(
            &traffic_dir(&dir).join(format!("{me}.json")),
            json.to_string().as_bytes(),
        )
        .unwrap();
    };
    write(serde_json::json!({"pid": me, "sandbox": "main-x", "up": 1, "down": 2}));
    let carried = traffic(&dir);
    match link_measure_contract::client::started(me) {
        Some(now) => {
            assert_eq!(carried, []);
            write(
                serde_json::json!({"pid": me, "sandbox": "main-x", "up": 1, "down": 2, "started": now}),
            );
            assert_eq!(traffic(&dir)[0].down, 2);
        }
        None => assert_eq!(carried[0].up, 1),
    }
}

/// Refused while paused or over its limit, a wire's calls are logged
/// once a minute with a count: they can't push the log's history out.
#[test]
fn refusals_while_paused_are_counted_not_each_logged() {
    let dir = link_test_kit::TempDir::new("wire-quiet");
    let wire = Wire::new("run-3", dir.to_path_buf());
    pause(&dir, "run-3", "by a test").unwrap();
    let call = Call {
        verb: Some("connect"),
        to: "example.com:443".into(),
        ..Call::default()
    };
    for _ in 0..1000 {
        wire.record(&call, Outcome::Refused);
    }
    let limited = Call {
        limited: true,
        ..call
    };
    assert!(resume(&dir, "run-3").unwrap());
    for _ in 0..1000 {
        wire.record(&limited, Outcome::Refused);
    }
    let logged = read(&dir);
    assert_eq!(logged.len(), 1, "{logged:?}");
}

/// A call failed for its upstream socket being another user's says so
/// in the log; any other failure, whose words a sandbox's request may
/// have chosen, says only that it failed.
#[test]
fn a_refused_upstream_socket_is_logged_with_why() {
    let dir = link_test_kit::TempDir::new("wire-why");
    let wire: &'static Wire = Box::leak(Box::new(Wire::new("run-5", dir.to_path_buf())));
    let why = "/run/upstream.sock isn't an upstream socket owned by this user or root";
    for err in [
        anyhow::Error::new(link_sandbox_route::dial::UntrustedUpstream(why.into()))
            .context("the SOCKS upstream isn't there"),
        anyhow::anyhow!("connection refused by asked-for.example"),
    ] {
        let mut call = Call::on(Some(wire));
        call.verb = Some("connect");
        call.to = "example.com:443".into();
        call.end(&Err(err));
    }
    let logged = read(&dir);
    assert_eq!(logged.len(), 2);
    assert_eq!(logged[0].why.as_deref(), Some(why));
    assert_eq!(logged[0].cells()[4], format!("failed: {why}"));
    assert_eq!(logged[1].why, None);
    assert_eq!(logged[1].cells()[4], "failed");
}

/// Whatever its calls end in, a wire logs at most so many lines of each
/// outcome a minute, then says how many it left out; a flood of
/// failures hides neither the first failures nor another outcome's
/// lines, and the watch still sees every refusal.
#[test]
fn a_flood_of_any_outcome_is_counted_not_each_logged() {
    let dir = link_test_kit::TempDir::new("wire-flood");
    let wire = Wire::new("run-4", dir.to_path_buf());
    let call = |to: &str| Call {
        verb: Some("connect"),
        to: to.into(),
        ..Call::default()
    };
    for n in 0..1000 {
        wire.record(&call(&format!("nowhere-{n}.invalid:443")), Outcome::Failed);
        wire.record(&call("example.com:443"), Outcome::Allowed);
    }
    wire.record(&call("example.com:443"), Outcome::Ended);
    let logged = read(&dir);
    let of = |outcome| logged.iter().filter(|e| e.outcome == outcome).count();
    assert_eq!(of(Outcome::Failed), LINES_A_MINUTE as usize);
    assert_eq!(of(Outcome::Allowed), LINES_A_MINUTE as usize);
    assert_eq!(of(Outcome::Ended), 1);
    assert_eq!(logged[0].to, "nowhere-0.invalid:443");
    // The next minute's first line says what was left out.
    {
        let mut lines = wire.lines.lock().unwrap();
        lines[Outcome::Failed as usize].since = Some(std::time::Instant::now() - MINUTE);
    }
    wire.record(&call("nowhere.invalid:443"), Outcome::Failed);
    let last = read(&dir).pop().unwrap();
    assert_eq!(last.to, "nowhere.invalid:443 (and 880 more failed since)");
    // Refusals past the cap still count toward a pause.
    {
        let mut lines = wire.lines.lock().unwrap();
        lines[Outcome::Refused as usize].since = Some(std::time::Instant::now());
        lines[Outcome::Refused as usize].written = LINES_A_MINUTE;
    }
    for n in 0..BURST {
        wire.record(&call(&format!("host-{}:443", n % 5)), Outcome::Refused);
    }
    assert!(wire.stopped());
    assert!(!read(&dir).iter().any(|e| e.to.starts_with("host-")));
}

/// A pause holds when its reason can't be read, or can't be written
/// yet (a proxy out of descriptors): the wire refuses all the same.
#[test]
fn a_pause_never_fails_open() {
    let dir = link_test_kit::TempDir::new("wire-open");
    std::fs::create_dir_all(dir.join("paused/run-1")).unwrap();
    // Its file there but unreadable (here, a folder): paused.
    let wire = Wire::new("run-1", dir.to_path_buf());
    assert!(wire.stopped());
    // A pause that can't be written (here, `paused` is a file): paused
    // meanwhile, and written once it can be.
    std::fs::remove_dir_all(dir.join("paused")).unwrap();
    std::fs::write(dir.join("paused"), "in the way").unwrap();
    let wire = Wire::new("run-2", dir.to_path_buf());
    wire.pause_for("its calls looked wrong");
    assert!(wire.stopped());
    std::fs::remove_file(dir.join("paused")).unwrap();
    assert!(
        wire.why_stopped()
            .unwrap()
            .contains("its calls looked wrong")
    );
    assert!(pause_file(&dir, "run-2").is_file());
    assert!(resume(&dir, "run-2").unwrap());
    assert!(!wire.stopped());
}

fn entry(time: u64, to: &str, outcome: Outcome) -> Entry {
    Entry {
        time,
        sandbox: "main-otter".into(),
        verb: "connect".into(),
        to: to.into(),
        outcome,
        up: 0,
        down: 0,
        why: None,
    }
}

#[test]
fn a_burst_of_refusals_pauses_but_a_busy_program_doesnt() {
    let mut watch = Watch::default();
    // Allowed calls, however many, and refusals spread out: normal.
    for i in 0..1_000 {
        assert_eq!(
            watch.saw(&entry(i, "api.anthropic.com:443", Outcome::Allowed)),
            None
        );
    }
    for i in 0..100 {
        let refused = entry(i * 61, "localhost:3000", Outcome::Refused);
        assert_eq!(watch.saw(&refused), None, "one a minute, to one place");
    }
    // Thirty within a minute: paused.
    let mut watch = Watch::default();
    for i in 0..BURST as u64 - 1 {
        assert_eq!(
            watch.saw(&entry(1_000 + i, "localhost:3000", Outcome::Refused)),
            None
        );
    }
    let why = watch
        .saw(&entry(1_050, "localhost:3000", Outcome::Refused))
        .unwrap();
    assert!(why.contains("within a minute"), "{why}");
}

#[test]
fn a_watch_keeps_only_its_window() {
    let mut watch = Watch::default();
    // A refusal every 3 seconds for an hour, to two targets in turn:
    // no pattern, and only the last ten minutes kept.
    for i in 0..1_200u64 {
        let to = ["a:443", "b:443"][i as usize % 2];
        assert_eq!(watch.saw(&entry(i * 3, to, Outcome::Refused)), None);
    }
    assert!(watch.refused.len() <= (PROBE_WINDOW / 3 + 1) as usize);
    assert_eq!(watch.targets.len(), 2);
    assert_eq!(watch.targets.values().sum::<usize>(), watch.refused.len());
}

#[test]
fn hosts_or_ports_tried_in_turn_pause() {
    let mut watch = Watch::default();
    for port in 0..PROBE as u64 - 1 {
        let to = format!("10.0.0.1:{}", 20 + port);
        assert_eq!(watch.saw(&entry(port * 20, &to, Outcome::Refused)), None);
    }
    let why = watch
        .saw(&entry(500, "10.0.0.2:22", Outcome::Refused))
        .unwrap();
    assert!(why.contains("different hosts or ports"), "{why}");
    // The same, spread over more than ten minutes: they age out.
    let mut watch = Watch::default();
    for i in 0..100u64 {
        let to = format!("10.0.0.1:{}", 20 + i);
        assert_eq!(
            watch.saw(&entry(i * 60, &to, Outcome::Refused)),
            None,
            "{i}"
        );
    }
}

#[test]
fn a_metadata_service_pauses_at_once() {
    for to in [
        "169.254.169.254:80",
        "[fd00:ec2::254]:80",
        "metadata.google.internal:80",
        "Metadata.Google.Internal.:80",
        // Refused before it was resolved (not a listed host): judged by the
        // address it's written as, in IPv6 or a URL's number.
        "[::ffff:169.254.169.254]:443",
        "[64:ff9b::a9fe:a9fe]:80",
        "2852039166:80",
        "0xa9fea9fe:80",
    ] {
        let why = Watch::default().saw(&entry(1, to, Outcome::Refused));
        assert!(why.is_some_and(|w| w.contains("metadata")), "{to}");
    }
    assert_eq!(
        Watch::default().saw(&entry(1, "metadata.example.com:443", Outcome::Refused)),
        None
    );
    // By its address, however IPv6 writes it.
    for ip in [
        "169.254.169.254",
        "::ffff:169.254.169.254",
        "64:ff9b::a9fe:a9fe",
        "2002:a9fe:a9fe::1",
        "fd00:ec2::254",
        "fd20:ce::254",
        "100.100.100.200",
        "::ffff:100.100.100.200",
    ] {
        assert!(is_metadata(ip.parse().unwrap()), "{ip}");
    }
    assert!(!is_metadata("169.254.1.1".parse().unwrap()));
}

#[test]
fn a_wire_is_paused_alone_and_resumed() {
    let dir = link_test_kit::TempDir::new("wire-pause");
    let (one, two) = (
        Wire::new("main-otter", dir.to_path_buf()),
        Wire::new("guest-alice", dir.to_path_buf()),
    );
    pause(&dir, "main-otter", "by a test").unwrap();
    let why = one.why_stopped().unwrap();
    assert!(
        why.contains("paused, by a test") && why.contains("lnk sandbox start main-otter"),
        "{why}"
    );
    assert!(!two.stopped());
    assert_eq!(
        paused(&dir),
        [("main-otter".to_string(), "by a test".to_string())]
    );
    assert!(resume(&dir, "main-otter").unwrap());
    assert!(!resume(&dir, "main-otter").unwrap());
    assert!(!one.stopped());
    // A name that isn't a sandbox's: no file written outside.
    for bad in ["../x", "a/b", "", "-x"] {
        assert!(pause(&dir, bad, "x").is_err(), "{bad:?}");
    }
}

#[test]
fn a_wire_pauses_itself_logs_it_and_refuses() {
    let dir = link_test_kit::TempDir::new("wire-self");
    let wire: &'static Wire = Box::leak(Box::new(Wire::new("run-1", dir.to_path_buf())));
    let mut call = Call::on(Some(wire));
    call.verb = Some("http");
    call.to = "169.254.169.254:80".into();
    call.end(&Err(anyhow::anyhow!("refused: this machine's network")));
    assert!(wire.why_stopped().is_some_and(|w| w.contains("metadata")));
    let log = read(&dir);
    assert_eq!(
        log.last().map(|e| e.verb.as_str()),
        Some("pause"),
        "{log:?}"
    );
}

#[test]
fn a_wire_takes_its_calls_a_minute_and_no_more() {
    let dir = link_test_kit::TempDir::new("wire-limit");
    assert_eq!(Wire::new("s", dir.to_path_buf()).over_limit(), None);
    let wire: &'static Wire = Box::leak(Box::new(
        Wire::new("run-1", dir.to_path_buf()).limited(Some(3)),
    ));
    for _ in 0..3 {
        assert_eq!(wire.over_limit(), None);
    }
    let (wait, why) = wire.over_limit().unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    assert!(why.contains("3 calls a minute"), "{why}");
    // Refused for it, however often: never a pause, and logged once
    // a minute, with a count.
    for _ in 0..BURST * 2 {
        let mut call = Call::on(Some(wire));
        call.verb = Some("connect");
        call.to = "example.com:443".into();
        call.limited = true;
        call.end(&Err(anyhow::anyhow!("refused: {why}")));
    }
    assert!(!wire.stopped());
    assert_eq!(read(&dir).len(), 1);
}

#[test]
fn dates_are_utc() {
    assert_eq!(utc(0), "1970-01-01 00:00:00");
    assert_eq!(utc(951_782_400), "2000-02-29 00:00:00");
    assert_eq!(utc(1_790_517_789), "2026-09-27 14:03:09");
}

#[test]
fn sizes_read_as_people_write_them() {
    assert_eq!(bytes(0), "0 B");
    assert_eq!(bytes(999), "999 B");
    assert_eq!(bytes(1_234), "1.2 kB");
    assert_eq!(bytes(3_400_000), "3.4 MB");
}

#[test]
fn the_log_keeps_each_call_and_shows_no_escapes() {
    let dir = link_test_kit::TempDir::new("wire");
    let wire: &'static Wire = Box::leak(Box::new(Wire::new("main-otter", dir.to_path_buf())));
    let mut call = Call::on(Some(wire));
    call.verb = Some("connect");
    // The terminal's escape and bell, as characters: what a call's
    // address could hold to act on a terminal.
    let (esc, bel) = (char::from(27u8), char::from(7u8));
    call.to = format!("{esc}]52;c;evil{bel}{}", "x".repeat(1000));
    call.up.store(1_234, Ordering::Relaxed);
    call.end(&Err(anyhow::anyhow!("refused: this machine")));
    // Nothing asked: nothing logged.
    Call::on(Some(wire)).end(&Ok(()));
    let entries = read(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].outcome, Outcome::Refused);
    assert_eq!(entries[0].up, 1_234);
    assert_eq!(entries[0].to.chars().count(), MAX_TO);
    let line = entries[0].cells().join(" ");
    assert!(!line.contains(esc) && !line.contains(bel), "{line}");
    assert!(line.contains("main-otter") && line.contains("refused"));

    assert!(!is_stopped(&dir) && !wire.stopped());
    stop(&dir, true).unwrap();
    assert!(wire.stopped());
    stop(&dir, false).unwrap();
    stop(&dir, false).unwrap();
    assert!(!wire.stopped());
}

#[test]
fn an_allowed_call_is_logged_when_it_starts_and_when_it_ends() {
    let dir = link_test_kit::TempDir::new("wire-ends");
    let wire: &'static Wire = Box::leak(Box::new(Wire::new("s", dir.to_path_buf())));
    let call = |result: anyhow::Result<()>, allowed: bool| {
        let mut call = Call::on(Some(wire));
        call.verb = Some("connect");
        if allowed {
            call.allowed();
            call.down.store(5_000, Ordering::Relaxed);
        }
        call.end(&result);
    };
    call(Err(anyhow::anyhow!("connection reset")), true);
    call(Ok(()), false);
    call(
        Err(anyhow::anyhow!("Connection refused (os error 111)")),
        false,
    );
    call(Err(anyhow::anyhow!("refused: not listed")), false);
    let outcomes: Vec<_> = read(&dir).iter().map(|e| (e.outcome, e.down)).collect();
    use Outcome::*;
    assert_eq!(
        outcomes,
        [
            (Allowed, 0),
            (Ended, 5_000),
            (Allowed, 0),
            (Failed, 0),
            (Refused, 0)
        ]
    );
}

/// The traffic file is written at once, and again once bytes move;
/// between them its thread sleeps, not waking on a timer; and it ends
/// with the proxy, file gone.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn traffic_thread_sleeps_until_counts_change() {
    let dir = link_test_kit::TempDir::new("wire-keep");
    let wire = Wire::new("run-9", dir.to_path_buf());
    keep_traffic(&wire);
    let path = traffic_dir(&dir).join(format!("{}.json", std::process::id()));
    let read = || -> Option<Traffic> { serde_json::from_slice(&std::fs::read(&path).ok()?).ok() };
    let until = |what: &dyn Fn() -> bool| {
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while !what() && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        what()
    };
    assert!(until(&|| read().is_some()), "written at once");
    let before = read().unwrap().up;

    // How often the thread has slept and woken.
    let wakes = || -> Option<u64> {
        for task in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
            let comm = std::fs::read_to_string(task.path().join("comm")).ok()?;
            if comm.trim() == "traffic" {
                let status = std::fs::read_to_string(task.path().join("status")).ok()?;
                return status
                    .lines()
                    .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
                    .and_then(|n| n.trim().parse().ok());
            }
        }
        None
    };
    if let Some(first) = wakes() {
        std::thread::sleep(Duration::from_secs(2));
        // A timer of 250 ms would have woken it 8 times; other tests
        // carrying bytes or counting may wake it once or twice.
        let woke = wakes().unwrap() - first;
        assert!(woke <= 3, "woke {woke} times in 2 s");
    }

    // Bytes moved: written within their few seconds.
    carried(0, 7);
    let end = std::time::Instant::now() + TRAFFIC_EVERY + Duration::from_secs(2);
    while !read().is_some_and(|t| t.up > before) && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        read().is_some_and(|t| t.up > before),
        "bytes moved, written"
    );

    end_traffic();
    assert!(!path.exists());
    assert!(until(&|| wakes().is_none()), "its thread ends");
}

/// Two proxies finding the log too long at once rotate it once: the
/// second, holding the lock after the first, finds a fresh log and moves
/// nothing over the one just kept.
#[test]
fn a_log_found_long_twice_is_rotated_once() {
    let dir = link_test_kit::TempDir::new("wire-rotate");
    let log = dir.join("wire.log");
    std::fs::write(&log, "old lines\n").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                barrier.wait();
                rotate(&log, 5);
                // A line the next proxy writes, after its rotation.
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                    .unwrap();
                std::io::Write::write_all(&mut file, b"new\n").unwrap();
            });
        }
    });
    assert_eq!(
        std::fs::read_to_string(kept(&log, 1)).unwrap(),
        "old lines\n"
    );
    assert!(!kept(&log, 2).exists());
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "new\nnew\n");
}
