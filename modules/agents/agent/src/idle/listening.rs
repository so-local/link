//! The door's state at work: it listens on the agent's port, passes each
//! connection inside, wakes the agent, and puts it to sleep once idle.

use super::*;

impl State {
    /// Listens on the agent's port, on `bound` if it's bound already,
    /// until the door closes, taking each connection on a thread of its
    /// own. A port still held (by the proxy that stood in for the door) is
    /// tried again until it's let go.
    #[expect(
        clippy::disallowed_methods,
        reason = "retries a port another process is letting go"
    )]
    pub(super) fn listen(self: Arc<Self>, bound: Option<TcpListener>) {
        if self.sleep.handoff {
            return self.listen_while_asleep();
        }
        let listener = if let Some(bound) = bound {
            bound
        } else {
            loop {
                if self.stop.load(Ordering::Relaxed) {
                    return;
                }
                match TcpListener::bind(("127.0.0.1", self.sleep.port)) {
                    Ok(listener) => break listener,
                    Err(_) => std::thread::sleep(Duration::from_millis(250)),
                }
            }
        };
        for client in listener.incoming() {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            let Ok(client) = client else {
                continue;
            };
            let _ = client.set_nodelay(true);
            self.touch();
            let number = self.opened.fetch_add(1, Ordering::SeqCst);
            let Ok(held) = client.try_clone() else {
                continue;
            };
            let exchange = Arc::new(Mutex::new(Exchange::default()));
            self.lock_open().insert(
                number,
                Open {
                    client: held,
                    exchange: exchange.clone(),
                },
            );
            let state = self.clone();
            std::thread::spawn(move || {
                if let Err(err) = state.pass(client, &exchange) {
                    link_format::daemon::warn!("{}: {err:#}", state.key);
                }
                state.touch();
                state.lock_open().remove(&number);
            });
        }
    }

    /// For an agent handed its port: listens on it only while it's asleep,
    /// and at the first connection lets go of it, wakes the agent, and
    /// passes the connection on to it once it listens there.
    #[expect(
        clippy::disallowed_methods,
        reason = "retries a port another process is letting go"
    )]
    fn listen_while_asleep(self: Arc<Self>) {
        while !self.stop.load(Ordering::Relaxed) {
            let asleep = *self.asleep.lock().unwrap_or_else(|e| e.into_inner());
            let listener = match asleep {
                true => TcpListener::bind(("127.0.0.1", self.sleep.port)).ok(),
                false => None,
            };
            let Some(listener) = listener else {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            };
            let client = listener.incoming().next();
            // Let go of at once: its harness takes it as it starts.
            drop(listener);
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            let Some(Ok(client)) = client else {
                continue;
            };
            self.touch();
            if let Err(err) = self.wake() {
                link_format::daemon::warn!("{}: {err:#}", self.key);
                continue;
            }
            let state = self.clone();
            std::thread::spawn(
                move || match TcpStream::connect(("127.0.0.1", state.sleep.port)) {
                    Ok(inside) => pipe_tcp(client, inside),
                    Err(err) => link_format::daemon::warn!("{}: {err}", state.key),
                },
            );
        }
    }

    /// Passes `client` into the agent's sandbox, woken first if it's
    /// asleep: through `in.sock`, its port first, as its proxy would.
    fn pass(&self, client: TcpStream, exchange: &Mutex<Exchange>) -> anyhow::Result<()> {
        self.wake()?;
        let mut inside = reach_inside(&self.sleep)?;
        inside.write_all(&self.sleep.port.to_be_bytes())?;
        self.pipe(client, inside, exchange);
        Ok(())
    }

    /// A byte went through, or it woke: not idle from now.
    fn touch(&self) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    fn lock_open(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<usize, Open>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether an answer is still coming on a connection through the
    /// door, however long it pauses: an agent answering is never idle.
    fn answering(&self) -> bool {
        answering(&self.lock_open())
    }

    /// Copies both ways between `client` and `inside` until each side
    /// ends, each byte that goes counting as the agent busy, and each
    /// read into `exchange`, its asks and their answers.
    fn pipe(&self, client: TcpStream, inside: UnixStream, exchange: &Mutex<Exchange>) {
        let (Ok(client_in), Ok(inside_in)) = (client.try_clone(), inside.try_clone()) else {
            return;
        };
        let lock = || exchange.lock().unwrap_or_else(|e| e.into_inner());
        std::thread::scope(|scope| {
            scope.spawn(|| {
                self.copy(client_in, &inside, |bytes| lock().asked(bytes));
                let _ = inside.shutdown(std::net::Shutdown::Write);
            });
            self.copy(inside_in, &client, |bytes| lock().answered(bytes));
            let _ = client.shutdown(std::net::Shutdown::Write);
        });
    }

    /// Copies `from` to `to` until `from` ends, `read` told each piece.
    fn copy(&self, mut from: impl Read, mut to: impl Write, mut read: impl FnMut(&[u8])) {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match from.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    self.touch();
                    read(&buffer[..n]);
                    if to.write_all(&buffer[..n]).is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Wakes the agent if it's asleep, and returns once it answers.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a woken agent until it answers"
    )]
    fn wake(&self) -> anyhow::Result<()> {
        let mut asleep = self.asleep.lock().unwrap_or_else(|e| e.into_inner());
        if !*asleep {
            return Ok(());
        }
        let woken = Instant::now();
        let deadline = woken + WAKING;
        start_job(&self.sleep.job)?;
        // Its owner's harness, if it serves it, started once its proxy
        // runs and it's no longer marked asleep, so it's served.
        if let Some(shared) = &self.sleep.shared {
            while !proxy_runs(&self.sleep.net) {
                anyhow::ensure!(Instant::now() < deadline, "its proxy didn't start");
                std::thread::sleep(Duration::from_millis(5));
            }
            set_mark(&self.key, false);
            start_shared(shared);
        }
        while !answers(&self.sleep) {
            if Instant::now() >= deadline {
                set_mark(&self.key, true);
                anyhow::bail!(
                    "it didn't answer within {} s of being woken",
                    WAKING.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        *asleep = false;
        set_mark(&self.key, false);
        self.touch();
        link_format::daemon::info!(
            "{}: woken, answering in {} ms",
            self.key,
            woken.elapsed().as_millis()
        );
        drop(asleep);
        // Its background commands' ends not told yet, and what it finished
        // that nobody took: once, as it wakes, beside the message.
        let (sweeper, carrying) = (self.sweeper.clone(), self.carrying.clone());
        std::thread::spawn(move || {
            if let Some(argv) = sweeper {
                let _ = link_secretive::process::command(&argv[0])
                    .args(&argv[1..])
                    .stdin(std::process::Stdio::null())
                    .status();
            }
            if let Err(err) = carrying.recover_now() {
                link_format::daemon::warn!("asking what was finished that nobody took: {err:#}");
            }
        });
        Ok(())
    }

    /// Looks each second whether the agent's been idle long enough, and
    /// puts it to sleep, until the door closes.
    #[expect(clippy::disallowed_methods, reason = "the idle watch's own period")]
    pub(super) fn watch(self: Arc<Self>) {
        let after = Duration::from_millis(self.sleep.after_ms);
        let mut was_busy = false;
        while !self.stop.load(Ordering::Relaxed) {
            std::thread::sleep(LOOK);
            // Started without a message (its start, or a restart of its
            // service): awake, so it's stopped once idle. One handed its
            // port is never asleep but by its door.
            if !self.sleep.handoff {
                let mut asleep = self.asleep.lock().unwrap_or_else(|e| e.into_inner());
                if *asleep && answers(&self.sleep) {
                    *asleep = false;
                    set_mark(&self.key, false);
                    *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                }
            }
            // A turn ended with no connection: its caller may have gone (a
            // bridge stopped as it answered), so what it finished is asked
            // for now, not left to its next message.
            let busy = busy(&self.sleep);
            if was_busy && !busy {
                let carrying = self.carrying.clone();
                std::thread::spawn(move || {
                    if let Err(err) = carrying.recover_now() {
                        link_format::daemon::warn!(
                            "asking what was finished that nobody took: {err:#}"
                        );
                    }
                });
            }
            was_busy = busy;
            if self.idle_for() >= after
                && let Err(err) = self.put_to_sleep()
            {
                link_format::daemon::warn!("{}: not stopped for being idle: {err:#}", self.key);
            }
        }
    }

    /// How long it's had nothing to do: no byte through the door, no
    /// answer coming through it, no turn running, no background command;
    /// zero while it has, or it's asleep. A connection its client keeps
    /// open, idle, isn't something to do.
    fn idle_for(&self) -> Duration {
        let connected = self.sleep.handoff && connected(self.sleep.port);
        if *self.asleep.lock().unwrap_or_else(|e| e.into_inner())
            || connected
            || self.answering()
            || busy(&self.sleep)
        {
            *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            return Duration::ZERO;
        }
        self.last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
    }

    /// Stops its job, unless something came to do meanwhile.
    fn put_to_sleep(&self) -> anyhow::Result<()> {
        let mut asleep = self.asleep.lock().unwrap_or_else(|e| e.into_inner());
        let after = Duration::from_millis(self.sleep.after_ms);
        let quiet = self
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
            >= after;
        let connected = self.sleep.handoff && connected(self.sleep.port);
        if *asleep || !quiet || connected || busy(&self.sleep) {
            return Ok(());
        }
        // Its idle connections end: their clients connect anew, which
        // wakes it. One an answer is still coming on, never: it isn't idle.
        let mut open = self.lock_open();
        if answering(&open) {
            return Ok(());
        }
        for (number, held) in std::mem::take(&mut *open) {
            // Said, so a connection ended that shouldn't have been says
            // why it was.
            let exchange = held.exchange.lock().unwrap_or_else(|e| e.into_inner());
            link_format::daemon::info!(
                "{}: idle: ending connection {number}, answering {}: {}",
                self.key,
                exchange.answering(),
                exchange.framed()
            );
            drop(exchange);
            let _ = held.client.shutdown(std::net::Shutdown::Both);
        }
        drop(open);
        // Marked first, so its owner's harness, if it serves it, keeps it
        // as it stops.
        set_mark(&self.key, true);
        if let Err(err) = stop_job(&self.sleep.job) {
            set_mark(&self.key, false);
            return Err(err);
        }
        *asleep = true;
        link_format::daemon::info!("{}: idle, stopped until its next message", self.key);
        Ok(())
    }

    /// Asks what it finished that nobody took, once it answers: the agent
    /// starting as it's carried.
    #[expect(
        clippy::disallowed_methods,
        reason = "backs off before recovering again"
    )]
    pub(super) fn recover_soon(self: Arc<Self>) {
        let deadline = Instant::now() + WAKING;
        while Instant::now() < deadline && !self.stop.load(Ordering::Relaxed) {
            if *self.asleep.lock().unwrap_or_else(|e| e.into_inner()) {
                return;
            }
            if answers(&self.sleep) && self.carrying.recover_now().is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An agent whose answer, in chunks, pauses past `idle_after` is
    /// never stopped before the answer is all through, nor its connection
    /// cut; then, its client's connection open and idle, it's stopped.
    #[test]
    fn an_agent_answering_slowly_is_never_stopped_mid_answer() {
        answering_slowly(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nI see \r\n",
            b"4\r\nyou.\r\n0\r\n\r\n",
        );
    }

    /// The same of an answer of a length.
    #[test]
    fn an_agent_answering_slowly_by_length_is_never_stopped_mid_answer() {
        answering_slowly(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nI see ",
            b"you.",
        );
    }

    /// A harness whose answer is `first`, then, 2.5 s later (two looks
    /// past a 300 ms `idle_after`), `rest`, its connection then kept open:
    /// the answer comes whole, the agent stopped only once it has.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn answering_slowly(first: &'static [u8], rest: &'static [u8]) {
        let dir = link_test_kit::TempDir::short(&format!("idle-answering-{}", first.len()));
        let net = dir.join("net");
        std::fs::create_dir_all(&net).unwrap();
        // Its proxy runs: it's awake.
        let _proxy = std::os::unix::net::UnixListener::bind(net.join("out.sock")).unwrap();
        let harness = std::os::unix::net::UnixListener::bind(net.join("in.sock")).unwrap();
        // Its start in a terminal, which hears each `sleep` and `wake`.
        let control = dir.join("control");
        let listener = std::os::unix::net::UnixListener::bind(&control).unwrap();
        let told = Arc::new(Mutex::new(Vec::<String>::new()));
        let hearing = told.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = std::io::BufReader::new(&stream).read_line(&mut line);
                hearing.lock().unwrap().push(line.trim().to_string());
                let _ = (&stream).write_all(b"ok\n");
            }
        });
        // The harness: a probe's HEAD answered at once; the ask's answer
        // in two pieces, 2.5 s apart, its connection then kept open.
        std::thread::spawn(move || {
            for mut inside in harness.incoming().flatten() {
                let mut port = [0u8; 2];
                inside.read_exact(&mut port).unwrap();
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && inside.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                if head.starts_with(b"HEAD") {
                    let _ = inside.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                inside.write_all(first).unwrap();
                std::thread::sleep(Duration::from_millis(2500));
                inside.write_all(rest).unwrap();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(30));
                    drop(inside);
                });
            }
        });
        // Bound here and handed over, never let go in between: a port
        // found free and dropped is another test's to take meanwhile, its
        // client then talking to whatever listens there.
        let bound = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = bound.local_addr().unwrap().port();
        let key = format!("idle-answering-{}-{}", std::process::id(), first.len());
        let sleep = Sleep {
            port,
            net,
            after_ms: 300,
            run: dir.join("run"),
            conversations: dir.join("chats"),
            job: Job::Terminal(control),
            handoff: false,
            shared: None,
        };
        let door = Door::open_on(&key, sleep, Some(bound), Arc::new(Carrying::none()), None);
        let started = Instant::now();
        let mut client = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(client) => break client,
                Err(_) if started.elapsed() < Duration::from_secs(10) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("the door didn't listen: {err}"),
            }
        };
        client
            .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi")
            .unwrap();
        let whole = [first, rest].concat();
        let mut answer = Vec::new();
        let mut buffer = [0u8; 1024];
        while answer.len() < whole.len() {
            match client.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => answer.extend_from_slice(&buffer[..n]),
            }
        }
        assert_eq!(
            String::from_utf8_lossy(&answer),
            String::from_utf8_lossy(&whole),
            "cut; its start told {:?}",
            told.lock().unwrap()
        );
        assert!(
            told.lock().unwrap().is_empty(),
            "stopped mid-answer: {told:?}"
        );
        // All through, its connection idle: stopped, the connection ended.
        let _ = client.set_read_timeout(Some(Duration::from_secs(5)));
        assert_eq!(client.read(&mut buffer).ok(), Some(0));
        // Its connections end first, then its job is told.
        let ended = Instant::now();
        while told.lock().unwrap().is_empty() && ended.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*told.lock().unwrap(), ["sleep"]);
        door.close();
        let _ = std::fs::remove_file(mark_of(&key));
    }
}
