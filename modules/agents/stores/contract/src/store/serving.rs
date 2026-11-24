//! Drivers kept running by the store bridge, a few at a time, and a
//! driver's own `main` ([`drive`]).

use super::*;

/// How many idle drivers of one program and folder the store bridge
/// keeps running: each holds what it reaches its store with (a
/// connection), so an agent's bridge holds at most this many while idle.
pub const BRIDGE_KEPT: usize = 2;

/// How long the store bridge keeps an idle driver running before it stops
/// it, and whatever it held with it.
pub const BRIDGE_KEPT_IDLE: Duration = Duration::from_secs(60);

/// How often idle drivers are looked for, to stop each idle past
/// [`BRIDGE_KEPT_IDLE`], whether or not another call comes.
pub const SWEEP_EVERY: Duration = Duration::from_secs(5);

/// A driver as it's run: its program, folder and environment besides.
type Run = (PathBuf, PathBuf, Vec<(String, String)>);

/// Each idle driver, by how it's run, and since when.
type Idle = std::collections::HashMap<Run, Vec<(Server, std::time::Instant)>>;

/// The drivers the store bridge keeps running, those whose contract says
/// `serve`: each idle one by its program and folder, since when, and
/// each program's contract, asked once.
pub(super) struct Running {
    contracts: std::sync::Mutex<std::collections::HashMap<PathBuf, Option<Contract>>>,
    idle: std::sync::Mutex<Idle>,
    /// The longest a call may take before its driver is stopped.
    timeout: Duration,
    /// How many idle ones it keeps of a program and folder, and how long.
    kept: usize,
    kept_idle: Duration,
    /// How often its sweep looks for idle ones ([`Running::sweep`]).
    sweep_every: Duration,
}

impl Default for Running {
    fn default() -> Self {
        Self {
            contracts: Default::default(),
            idle: Default::default(),
            timeout: CALL_TIMEOUT,
            kept: BRIDGE_KEPT,
            kept_idle: BRIDGE_KEPT_IDLE,
            sweep_every: SWEEP_EVERY,
        }
    }
}

impl Running {
    /// `request`, answered by `driver`: one kept running when it serves,
    /// else one run for it. A request of jobs to a driver that keeps none
    /// is refused, the driver never run for it.
    pub(super) fn call(&self, driver: &Driver, request: &Request) -> anyhow::Result<Reply> {
        let contract = self.contract(driver);
        if request.of_jobs() && !contract.as_ref().is_some_and(|c| c.jobs) {
            bail!(
                "lnk-{} keeps no jobs: its contract doesn't say jobs",
                driver.name
            );
        }
        if request.of_files() && !contract.as_ref().is_some_and(|c| c.files) {
            bail!(
                "lnk-{} keeps no files: its contract doesn't say files",
                driver.name
            );
        }
        self.answer(driver, contract.as_ref(), request)
    }

    /// `request`, answered by `driver`, whose contract is `contract`: one
    /// kept running when it serves, started again if it stopped while
    /// idle, else one run for it ([`Driver::call_once`]).
    pub(super) fn answer(
        &self,
        driver: &Driver,
        contract: Option<&Contract>,
        request: &Request,
    ) -> anyhow::Result<Reply> {
        if !contract.is_some_and(|c| c.serve) {
            return driver.call_once(request);
        }
        self.stop_idle();
        let key = (
            driver.program.clone(),
            driver.dir.clone(),
            driver.env.clone(),
        );
        let idle = |key: &Run| {
            self.idle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(key)
                .and_then(Vec::pop)
                .map(|(server, _)| server)
        };
        let mut server = match idle(&key) {
            Some(server) => server,
            None => Server::start(driver)?,
        };
        let answered = match server.call(request, self.timeout) {
            // It stopped while idle, and never read the request: once
            // more, on a fresh one. One that stopped after it read it, or
            // was stopped for taking too long, is never kept nor asked
            // again: the call may have been done.
            Err(Sent::Not(_)) => {
                server = Server::start(driver)?;
                server.call(request, self.timeout)
            }
            other => other,
        };
        let reply = match answered {
            Ok(reply) => reply,
            Err(Sent::Not(err) | Sent::Unanswered(err)) => return Err(err),
        };
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        let kept = idle.entry(key).or_default();
        if kept.len() < self.kept {
            kept.push((server, std::time::Instant::now()));
        }
        drop(idle);
        match reply {
            Reply::Error { error } => bail!("lnk-{}: {}", driver.name, link_format::clean(&error)),
            reply => Ok(reply),
        }
    }

    /// A thread of its own stopping `running`'s idle drivers once they've
    /// been idle long enough, whether or not another call comes. It ends
    /// once `stop` says so, or `running` is gone.
    #[expect(
        clippy::disallowed_methods,
        reason = "the sweep's own period of looking"
    )]
    pub(super) fn sweep(
        running: &std::sync::Arc<Running>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let every = running.sweep_every;
        let running = std::sync::Arc::downgrade(running);
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(every);
                match running.upgrade() {
                    Some(running) => running.stop_idle(),
                    None => return,
                }
            }
        });
    }

    /// Every driver idle longer than it keeps one, stopped.
    pub(super) fn stop_idle(&self) {
        let mut stopped = Vec::new();
        {
            let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
            for kept in idle.values_mut() {
                let (old, fresh): (Vec<_>, Vec<_>) = std::mem::take(kept)
                    .into_iter()
                    .partition(|(_, since)| since.elapsed() >= self.kept_idle);
                *kept = fresh;
                stopped.extend(old);
            }
            idle.retain(|_, kept| !kept.is_empty());
        }
        // Stopped outside the lock: each is a kill and a wait.
        drop(stopped);
    }

    /// `driver`'s contract, none when it doesn't say one of this version:
    /// asked once a program.
    pub(super) fn contract(&self, driver: &Driver) -> Option<Contract> {
        if let Some(contract) = self
            .contracts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&driver.program)
        {
            return contract.clone();
        }
        let contract = driver.contract().ok();
        self.contracts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(driver.program.clone(), contract.clone());
        contract
    }
}

/// A driver kept running: `lnk-<name> serve`, a call a line. Stopped when
/// it's dropped.
struct Server {
    name: String,
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

/// Why a call to a driver kept running failed: before it was sent, or
/// after, its answer never read.
enum Sent {
    Not(anyhow::Error),
    Unanswered(anyhow::Error),
}

impl Server {
    fn start(driver: &Driver) -> anyhow::Result<Self> {
        // A process group of its own: stopping it stops whatever it started,
        // which could otherwise hold its answer's pipe open.
        let mut child = std::os::unix::process::CommandExt::process_group(
            &mut link_secretive::process::ours(&driver.program),
            0,
        )
        .arg("serve")
        .env(DIR_ENV, &driver.dir)
        .envs(driver.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("running lnk-{} serve", driver.name))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            bail!("running lnk-{} serve: no pipes", driver.name);
        };
        Ok(Self {
            name: driver.name.clone(),
            child,
            stdin,
            stdout: std::io::BufReader::new(stdout),
        })
    }

    /// `request` as a line, its reply read as one, stopped past
    /// `timeout`.
    fn call(&mut self, request: &Request, timeout: Duration) -> Result<Reply, Sent> {
        use std::io::{BufRead, Read, Write};
        let mut line = serde_json::to_vec(request).map_err(|err| Sent::Not(err.into()))?;
        line.push(b'\n');
        if let Err(err) = self
            .stdin
            .write_all(&line)
            .and_then(|()| self.stdin.flush())
        {
            return Err(Sent::Not(
                anyhow::Error::new(err).context(format!("calling lnk-{} serve", self.name)),
            ));
        }
        let pid = self.child.id();
        let (done, waiting) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = waiting.recv_timeout(timeout) {
                // SAFETY: a signal to the child's own group, the child not
                // yet waited for.
                unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
            }
        });
        let mut answer = String::new();
        let read = (&mut self.stdout).take(ANSWER_MAX).read_line(&mut answer);
        let _ = done.send(());
        let _ = watchdog.join();
        let unanswered =
            |why: String| Sent::Unanswered(anyhow::anyhow!("lnk-{} serve: {why}", self.name));
        match read {
            Err(err) => Err(unanswered(err.to_string())),
            Ok(0) => Err(unanswered("it stopped without answering".into())),
            Ok(_) => serde_json::from_str(&answer)
                .map_err(|_| unanswered("its answer isn't a reply".into())),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: a signal to the child's own group, the child not yet
        // waited for.
        unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Which of the store protocol's commands a driver was run with
/// (`actions.toml`): what each store's generated parser reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Contract,
    Call,
    Serve,
}

impl Operation {
    /// The command a store's generated parser read, by its words
    /// (`contract`, `call`, `serve`): none for another, as core's
    /// `uninstall`, which isn't the protocol's.
    pub fn of(words: &[String]) -> Option<Self> {
        match words {
            [word] if word == "contract" => Some(Self::Contract),
            [word] if word == "call" => Some(Self::Call),
            [word] if word == "serve" => Some(Self::Serve),
            _ => None,
        }
    }
}

/// A driver's `operation`: `contract` (saying `serve`, and what more
/// `contract` keeps: `jobs`, `files`), `call` with `answer` once, and
/// `serve` with `answer` for each request, one a line on stdin, until
/// stdin ends, each reply a line on stdout. An error `answer` returns is
/// a [`Reply::Error`]. Whatever `answer` holds between calls (a
/// connection) it keeps for the next. Replies are the protocol, so only
/// `out.data` writes them.
pub fn run(
    out: &mut Out,
    operation: Operation,
    contract: Contract,
    mut answer: impl FnMut(Request) -> anyhow::Result<Reply>,
) -> Result<Done, Report> {
    use std::io::{BufRead, Read};
    let contract = Contract {
        serve: true,
        ..contract
    };
    // Each reply, and the contract, as the declarations write them.
    let reply = |out: &mut Out, reply: &Reply| -> Result<(), Report> {
        out.data(&link_actions::answer::json_line::<crate::declared::Reply>(
            reply,
        )?);
        Ok(())
    };
    let mut answered = |request: Result<Request, anyhow::Error>| {
        request
            .and_then(&mut answer)
            .unwrap_or_else(|err| Reply::Error {
                error: format!("{err:#}"),
            })
    };
    match operation {
        Operation::Contract => {
            out.data(&link_actions::answer::json_line::<crate::declared::Contract>(&contract)?);
        }
        Operation::Call => {
            let mut input = Vec::new();
            let request = std::io::stdin()
                .take(EVENT_MAX * 2)
                .read_to_end(&mut input)
                .context("reading the request on stdin")
                .and_then(|_| serde_json::from_slice(&input).context("reading the request"));
            reply(out, &answered(request))?;
        }
        Operation::Serve => {
            let mut input = std::io::stdin().lock();
            loop {
                let mut line = Vec::new();
                let read = (&mut input)
                    .take(EVENT_MAX * 2)
                    .read_until(b'\n', &mut line)
                    .within("reading a request on stdin")?;
                if read == 0 {
                    break;
                }
                if !(line.ends_with(b"\n") || (read as u64) < EVENT_MAX * 2) {
                    return Err(Report::error(sentence!(
                        "A request is larger than {} bytes.",
                        EVENT_MAX * 2
                    ))
                    .exit(Exit::Usage));
                }
                let request = serde_json::from_slice(&line).context("reading the request");
                reply(out, &answered(request))?;
            }
        }
    }
    Ok(Done::shown())
}

#[cfg(test)]
mod tests {
    use super::super::tests::lines;
    use super::*;

    /// A driver whose contract says `serve` is kept running by the store
    /// bridge: started once for many calls, its errors still errors, and
    /// started again once it stops.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_store_bridge_keeps_a_serving_driver_running() {
        let dir = link_test_kit::TempDir::new("store-bridge-serve");
        let program = dir.join("lnk-kept");
        let started = dir.join("started");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 contract) echo '{{\"version\":1,\"serve\":true}}' ;;\n\
                 serve) echo \"$$\" >> '{}'\n\
                   n=0\n\
                   while read -r line; do\n\
                     n=$((n+1))\n\
                     case \"$line\" in\n\
                     *'\"thread\":\"bad\"'*) echo '{{\"reply\":\"error\",\"error\":\"no such\"}}' ;;\n\
                     *) echo \"{{\\\"reply\\\":\\\"last\\\",\\\"last\\\":$n}}\" ;;\n\
                     esac\n\
                   done ;;\n\
                 *) exit 2 ;;\n\
                 esac\n",
                started.display()
            ),
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("kept", program, &dir));
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        let starts = || {
            std::fs::read_to_string(&started)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        // One process, its count going up: the same one each call.
        for n in 1..=5 {
            assert_eq!(bridged.last(&agent, "t").unwrap(), n);
        }
        assert_eq!(starts().len(), 1, "{:?}", starts());
        let err = bridged.last(&agent, "bad").unwrap_err();
        assert!(format!("{err:#}").contains("no such"), "{err:#}");
        assert_eq!(bridged.last(&agent, "t").unwrap(), 7);
        // Stopped while idle: the next call starts another.
        let pid: i32 = starts()[0].parse().unwrap();
        // SAFETY: a signal to the driver this test started.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(bridged.last(&agent, "t").unwrap(), 1);
        assert_eq!(starts().len(), 2, "{:?}", starts());
    }

    /// A fake serving driver in `dir`: each request it reads is a line of
    /// `dir/asked`, and it answers `last` with how many it has answered;
    /// one for thread `slow` it never answers, and at one for thread
    /// `die` it stops. Each start is a line of `dir/started`.
    fn serving(dir: &Path) -> Driver {
        let program = dir.join("lnk-kept");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 contract) echo '{{\"version\":1,\"serve\":true}}' ;;\n\
                 serve) echo \"$$\" >> '{dir}/started'\n\
                   n=0\n\
                   while read -r line; do\n\
                     echo \"$line\" >> '{dir}/asked'\n\
                     case \"$line\" in\n\
                     *'\"thread\":\"slow\"'*) sleep 30 ;;\n\
                     *'\"thread\":\"die\"'*) exit 1 ;;\n\
                     esac\n\
                     n=$((n+1))\n\
                     echo \"{{\\\"reply\\\":\\\"last\\\",\\\"last\\\":$n}}\"\n\
                   done ;;\n\
                 esac\n",
                dir = dir.display()
            ),
        )
        .unwrap();
        Driver::at("kept", program, dir)
    }

    fn last_of(thread: &str) -> Request {
        Request::Last {
            agent: Agent::default(),
            thread: thread.into(),
        }
    }

    /// A kept driver that takes past the timeout is stopped, never kept or
    /// asked again; one that stops in the middle of a request isn't asked
    /// it again either: either may have done it.
    #[test]
    fn a_driver_stopped_or_dying_in_a_call_is_never_used_again() {
        let dir = link_test_kit::TempDir::new("store-kept-dies");
        let driver = serving(&dir);
        let running = Running {
            timeout: Duration::from_secs(1),
            ..Running::default()
        };
        assert_eq!(
            running.call(&driver, &last_of("t")).unwrap(),
            Reply::Last { last: 1 }
        );
        let started = std::time::Instant::now();
        assert!(running.call(&driver, &last_of("slow")).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        // A fresh one: its count starts again.
        assert_eq!(
            running.call(&driver, &last_of("t")).unwrap(),
            Reply::Last { last: 1 }
        );
        assert_eq!(lines(&dir.join("started")).len(), 2);
        let err = running.call(&driver, &last_of("die")).unwrap_err();
        assert!(
            format!("{err:#}").contains("stopped without answering"),
            "{err:#}"
        );
        let asked = lines(&dir.join("asked"));
        assert_eq!(
            asked.iter().filter(|l| l.contains("\"die\"")).count(),
            1,
            "{asked:?}"
        );
        assert_eq!(
            asked.iter().filter(|l| l.contains("\"slow\"")).count(),
            1,
            "{asked:?}"
        );
        assert_eq!(
            running.call(&driver, &last_of("t")).unwrap(),
            Reply::Last { last: 1 }
        );
        assert_eq!(lines(&dir.join("started")).len(), 3);
    }

    /// An idle driver is stopped once it's idle long enough, no call to
    /// come: by its pool's sweep.
    #[test]
    fn an_idle_driver_is_stopped_with_no_call_to_come() {
        let dir = link_test_kit::TempDir::new("store-kept-swept");
        let driver = serving(&dir);
        let running = std::sync::Arc::new(Running {
            kept_idle: Duration::from_millis(100),
            sweep_every: Duration::from_millis(20),
            ..Running::default()
        });
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        Running::sweep(&running, stop.clone());
        running.call(&driver, &last_of("t")).unwrap();
        let pid: i32 = lines(&dir.join("started"))[0].parse().unwrap();
        link_test_kit::wait::eventually("the idle driver stopped", Duration::from_secs(10), || {
            let idle = running
                .idle
                .lock()
                .unwrap()
                .values()
                .map(Vec::len)
                .sum::<usize>();
            // SAFETY: a signal 0 only asks whether it's there.
            idle == 0 && unsafe { libc::kill(pid, 0) } != 0
        });
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// At most so many idle of a driver, and each stopped once idle long
    /// enough.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn idle_drivers_are_few_and_stopped() {
        let dir = link_test_kit::TempDir::new("store-kept-idle");
        let driver = serving(&dir);
        let running = Running {
            kept_idle: Duration::from_millis(300),
            ..Running::default()
        };
        // Four at once: four started, two kept.
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    // Each holds its driver a moment, so all four start.
                    let barrier = Request::Last {
                        agent: Agent::default(),
                        thread: "t".into(),
                    };
                    running.call(&driver, &barrier).unwrap();
                });
            }
        });
        let started = lines(&dir.join("started"));
        let kept = |running: &Running| {
            running
                .idle
                .lock()
                .unwrap()
                .values()
                .map(Vec::len)
                .sum::<usize>()
        };
        assert!(kept(&running) <= BRIDGE_KEPT, "{started:?}");
        assert!(kept(&running) >= 1);
        // Idle past it: stopped, and their processes gone.
        std::thread::sleep(Duration::from_millis(400));
        running.stop_idle();
        assert_eq!(kept(&running), 0);
        for pid in &started {
            let pid: i32 = pid.parse().unwrap();
            // SAFETY: a signal 0 only asks whether it's there.
            assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "{pid} still runs");
        }
        // The next call starts one afresh.
        assert_eq!(
            running.call(&driver, &last_of("t")).unwrap(),
            Reply::Last { last: 1 }
        );
    }
}
