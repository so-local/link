//! The store bridge: a driver called from a sandbox ([`Bridged`]), and
//! the bridge outside it that proves each call and runs the driver
//! ([`serve_bridge`], [`serve_storage`]).

use super::*;
use link_format::sentence;

/// A driver behind the store bridge, as a harness in its sandbox calls
/// it: each call is the agent's identity, the [`Request`], and its proof
/// ([`crate::bridge`]), on a connection of its own to the bridge's
/// socket. The harness holds no driver, credential or way to the store:
/// the bridge, outside the sandbox, runs the driver, and only for the
/// agent the proof is of.
#[derive(Duplicate, Debug)]
pub struct Bridged {
    pub socket: PathBuf,
    #[duplicate]
    pub binding: crate::bridge::Binding,
}

/// Where a harness reaches its store bridge, when it has one
/// ([`crate::harness::Settings::store`]): the driver the bridge runs, and
/// its socket, in a folder the harness's sandbox reaches. A conversation
/// kept by another driver is read as before, the driver run where the
/// harness runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeAt {
    pub driver: String,
    pub socket: PathBuf,
}

/// How long a harness waits for its store bridge to be there, as both
/// start at once.
pub const BRIDGE_WAIT: Duration = Duration::from_secs(10);

/// The most a store bridge's answer is: a read of many events.
pub const ANSWER_MAX: u64 = 1 << 30;

impl Store for Bridged {
    fn name(&self) -> &str {
        "the store bridge"
    }

    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        crate::bridge::retrying(|| self.call_once(request))
            .map_err(|err| anyhow::anyhow!("the store bridge: {err:#}"))
    }
}

impl Bridged {
    /// One call, on a connection of its own: a refusal, busy included, an
    /// error that is that failure.
    #[expect(clippy::disallowed_methods, reason = "retries while the bridge starts")]
    fn call_once(&self, request: &Request) -> anyhow::Result<Reply> {
        let started = std::time::Instant::now();
        let stream = loop {
            match std::os::unix::net::UnixStream::connect(&self.socket) {
                Ok(stream) => break stream,
                // Starting beside the harness: there in a moment.
                Err(_) if started.elapsed() < BRIDGE_WAIT => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("reaching the store bridge at {}", self.socket.display())
                    });
                }
            }
        };
        stream.set_read_timeout(Some(CALL_TIMEOUT + Duration::from_secs(5)))?;
        crate::bridge::call(&stream, &self.binding, request)?;
        crate::bridge::read_answer(&stream, ANSWER_MAX)
    }
}

/// Which driver answers a call to the store bridge, for the agent that
/// proved it's calling: an error to refuse it.
pub type DriverOf = dyn Fn(&crate::bridge::Identity) -> anyhow::Result<Driver> + Send + Sync;

/// Which driver answers a call to the storage bridge, for the agent that
/// proved it's calling, the collection the call is of (where the agent's
/// machine keeps it) and the call itself (whether it changes a file of
/// it, [`Request::changed_file`]): an error to refuse it.
pub type PlacedDriver = dyn Fn(&crate::bridge::Identity, crate::storage::Collection, &Request) -> anyhow::Result<Driver>
    + Send
    + Sync;

/// The most proved calls the store bridge holds at once, one agent's or
/// many: one more is answered at once that it's busy, so no caller,
/// however many it opens, holds more of the machine than this.
pub const BRIDGE_CONNECTIONS: usize = 16;

/// The most of them one agent's calls hold: a driver that never answers
/// one agent's calls (each stopped past [`CALL_TIMEOUT`]) leaves the rest
/// to the others.
pub const BRIDGE_AGENT_CONNECTIONS: usize = 8;

/// The most drivers the store bridge runs at once, for proved calls; the
/// rest wait their turn.
pub const BRIDGE_DRIVERS: usize = 4;

/// How long the store bridge waits for a proved call's request: it comes
/// at once, after the proof.
pub const BRIDGE_READ_WAIT: Duration = Duration::from_secs(5);

/// What the store bridge takes at once and waits for
/// ([`crate::bridge::serve`]).
pub const BRIDGE_LIMITS: crate::bridge::Limits = crate::bridge::Limits {
    name: "the store bridge",
    calls: BRIDGE_CONNECTIONS,
    per_agent: BRIDGE_AGENT_CONNECTIONS,
    per_owner: BRIDGE_CONNECTIONS,
    action_max: crate::bridge::CALL_MAX,
    action_wait: BRIDGE_READ_WAIT,
    answer_wait: Duration::from_secs(30),
};

/// The store bridge, on the server every bridge runs on
/// ([`crate::bridge::serve`], within [`BRIDGE_LIMITS`]): takes calls on
/// `listener` until `stop`, and answers each with the driver `driver_of`
/// gives for the agent that proved it's calling (`keys`), outside every
/// sandbox, at most [`BRIDGE_DRIVERS`] at once. A driver whose contract
/// says `serve` is kept running between calls: at most [`BRIDGE_KEPT`]
/// idle of each, each stopped once idle for [`BRIDGE_KEPT_IDLE`], and all
/// with the bridge. A call for another agent's conversations than the
/// caller's, or whose proof isn't the agent's it names, is refused, its
/// request never read.
pub fn serve_bridge(
    listener: std::os::unix::net::UnixListener,
    keys: std::sync::Arc<dyn crate::bridge::Keys>,
    driver_of: std::sync::Arc<DriverOf>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    serve_storage(
        listener,
        keys,
        std::sync::Arc::new(move |identity: &crate::bridge::Identity, _, _: &Request| {
            driver_of(identity)
        }),
        stop,
    )
}

/// The storage bridge: [`serve_bridge`], each call answered by the driver
/// `driver_for` gives for the agent and the collection the call is of
/// ([`Request::collection`]), so each of an agent's collections is kept
/// at its own place.
pub fn serve_storage(
    listener: std::os::unix::net::UnixListener,
    keys: std::sync::Arc<dyn crate::bridge::Keys>,
    driver_for: std::sync::Arc<PlacedDriver>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let drivers = std::sync::Arc::new(Slots::new(BRIDGE_DRIVERS));
    let running = std::sync::Arc::new(Running::default());
    // Idle drivers stopped once they've been idle long enough, whether or
    // not another call comes; this ends with the bridge.
    Running::sweep(&running, stop.clone());
    let served = crate::bridge::serve(
        listener,
        keys,
        &BRIDGE_LIMITS,
        &stop,
        move |identity, request: Request| {
            answer_bridged(identity, &request, driver_for.as_ref(), &drivers, &running)
        },
    );
    if let Err(err) = served {
        crate::warned(sentence!("The store bridge stopped."), &err);
    }
}

/// So many of something at once: a slot is held while its guard lives.
struct Slots {
    free: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
}

/// A slot held, given back when it's dropped.
struct Slot(std::sync::Arc<Slots>);

impl Slots {
    fn new(most: usize) -> Self {
        Self {
            free: std::sync::Mutex::new(most),
            freed: std::sync::Condvar::new(),
        }
    }

    /// A slot, once one is free.
    fn take(self: &std::sync::Arc<Self>) -> Slot {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
        Slot(self.clone())
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.0.freed.notify_one();
    }
}

/// `identity`'s call to the store bridge, proved: its reply, or why it
/// failed.
fn answer_bridged(
    identity: &crate::bridge::Identity,
    request: &Request,
    driver_for: &PlacedDriver,
    drivers: &std::sync::Arc<Slots>,
    running: &Running,
) -> anyhow::Result<Reply> {
    let whose = request.agent();
    anyhow::ensure!(
        whose.id == identity.agent && whose.owner == identity.owner,
        "agent {} asked for agent {}'s conversations: refused",
        link_format::clean(&identity.agent),
        link_format::clean(&whose.id)
    );
    match request {
        Request::Append { event, .. } => anyhow::ensure!(
            serde_json::to_vec(event)?.len() as u64 <= EVENT_MAX,
            "the event is larger than {EVENT_MAX} bytes"
        ),
        Request::JobPut { job, .. } => check_job(job)?,
        Request::JobGet { id, .. }
        | Request::JobShift { id, .. }
        | Request::JobRemove { id, .. } => check_job_id(id)?,
        Request::FileRead { path, .. } | Request::FileRemove { path, .. } => {
            check_file_path(path, false)?
        }
        Request::FileList { folder, .. } => check_file_path(folder, true)?,
        Request::FileWrite { path, data, .. } | Request::FileAppend { path, data, .. } => {
            check_file_path(path, false)?;
            file_data(path, data)?;
        }
        Request::ToolsNarrow { tools, .. } => check_tools(tools)?,
        _ => {}
    }
    let collection = request
        .collection()
        .context("a path in none of the agent's collections")?;
    let driver = driver_for(identity, collection, request)?;
    let _running = drivers.take();
    match running.call(&driver, request)? {
        Reply::Error { error } => bail!("{}", link_format::clean(&error)),
        reply => Ok(reply),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::lines;
    use super::*;

    /// The store bridge answers an agent only while it's bound: a call
    /// proved with a guessed key, or with the key of a binding that went,
    /// is refused, and the driver never runs for it.
    #[test]
    fn the_store_bridge_answers_an_agent_only_while_its_bound() {
        use crate::bridge::{Binding, Identity, Keyring};
        let dir = link_test_kit::TempDir::new("store-bridge-unit");
        let program = dir.join("lnk-fake");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\necho ran >> '{}'\necho '{{\"reply\":\"last\",\"last\":7}}'\n",
                dir.join("ran").display()
            ),
        )
        .unwrap();
        let driver = Driver::at("fake", program, &dir);
        let keyring = Keyring::at(&dir.join("keys"));
        let socket = dir.join("store.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        {
            let keyring = keyring.clone();
            std::thread::spawn(move || {
                serve_bridge(
                    listener,
                    std::sync::Arc::new(keyring),
                    std::sync::Arc::new(move |_: &Identity| Ok(driver.clone())),
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                )
            });
        }
        let who = Identity {
            agent: "aaaaaaaaaaaa".into(),
            service: String::new(),
            owner: "acct-1".into(),
            teams: Vec::new(),
        };
        let agent = Agent {
            id: who.agent.clone(),
            owner: who.owner.clone(),
        };
        let ran = || {
            std::fs::read_to_string(dir.join("ran"))
                .unwrap_or_default()
                .lines()
                .count()
        };
        let through = |binding: Binding| Bridged {
            socket: socket.clone(),
            binding,
        };
        // Unbound: a key guessed for it proves nothing.
        let guessed = Binding {
            identity: who.clone(),
            key: link_secretive::Secret::from("0".repeat(64)),
        };
        assert!(through(guessed).last(&agent, "t").is_err());
        assert_eq!(ran(), 0);
        // Bound: answered, the driver run twice: asked once whether it
        // serves (`contract`), then for the call.
        let bound = keyring
            .bind(&who, &link_secretive::Secret::from("c".repeat(64)))
            .unwrap();
        assert_eq!(through(bound.duplicate()).last(&agent, "t").unwrap(), 7);
        assert_eq!(ran(), 2);
        // Unbound: what it held proves nothing.
        keyring.unbind_key(&bound, &link_secret_contract::keeper::RouteRevoker);
        let refused = through(bound).last(&agent, "t").unwrap_err();
        assert!(
            format!("{refused:#}").contains("isn't bound here"),
            "{refused:#}"
        );
        assert_eq!(ran(), 2);
    }

    /// The store bridge passes the test every bridge passes.
    #[test]
    fn the_store_bridge_is_a_bridge() {
        let dir = link_test_kit::TempDir::new("store-bridge-conformance");
        let program = dir.join("lnk-fake");
        let ran = dir.join("ran");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh\necho ran >> '{}'\necho '{{\"reply\":\"last\",\"last\":3}}'\n",
                ran.display()
            ),
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("fake", program, &dir));
        let mut agents = vec![bridged.binding.duplicate()];
        agents.extend(
            ["bbbbbbbbbbbb", "cccccccccccc"].map(|id| crate::testing::bound(&dir.join("keys"), id)),
        );
        let action = Request::Last {
            agent: Agent {
                id: "aaaaaaaaaaaa".into(),
                owner: "me@box".into(),
            },
            thread: "t".into(),
        };
        link_sandbox_bridge::conformance::check(&link_sandbox_bridge::conformance::Subject {
            socket: bridged.socket.clone(),
            agents,
            action: serde_json::to_value(&action).unwrap(),
            ran: std::sync::Arc::new(move || {
                std::fs::read_to_string(&ran)
                    .unwrap_or_default()
                    .lines()
                    .count()
            }),
            big: None,
            limits: BRIDGE_LIMITS,
        });
    }

    /// One agent's calls made all at once, past its peer's places for
    /// proving and its own for calls, are each answered: those answered
    /// busy are made again.
    #[test]
    fn calls_made_at_once_are_all_answered() {
        let dir = link_test_kit::TempDir::new("store-bridge-at-once");
        let program = dir.join("lnk-fake");
        link_test_kit::write_program(
            &program,
            "#!/bin/sh\nsleep 0.2\necho '{\"reply\":\"last\",\"last\":3}'\n",
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("fake", program, &dir));
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        let at_once = 3 * crate::bridge::PEER_PROVING.max(BRIDGE_AGENT_CONNECTIONS);
        let barrier = std::sync::Barrier::new(at_once);
        std::thread::scope(|s| {
            let calls: Vec<_> = (0..at_once)
                .map(|_| {
                    let (bridged, agent, barrier) = (&bridged, &agent, &barrier);
                    s.spawn(move || {
                        barrier.wait();
                        bridged.last(agent, "t")
                    })
                })
                .collect();
            for call in calls {
                assert_eq!(call.join().unwrap().unwrap(), 3);
            }
        });
    }

    /// Connections that never prove don't keep the store bridge's calls
    /// out, however many: they're held apart from its places, and a proved
    /// call is answered.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn the_store_bridge_is_never_held_by_connections_that_dont_prove() {
        let dir = link_test_kit::TempDir::new("store-bridge-busy");
        let program = dir.join("lnk-fake");
        link_test_kit::write_program(
            &program,
            "#!/bin/sh\necho '{\"reply\":\"last\",\"last\":3}'\n",
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("fake", program, &dir));
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        // Callers that connect and say nothing, more than it holds.
        let idle: Vec<_> = (0..BRIDGE_CONNECTIONS * 2)
            .map(|_| std::os::unix::net::UnixStream::connect(&bridged.socket).unwrap())
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        let started = std::time::Instant::now();
        assert_eq!(bridged.last(&agent, "t").unwrap(), 3);
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(idle);
    }

    /// A driver whose contract doesn't say `jobs` is never asked of jobs
    /// by the store bridge: refused, the driver run only for its
    /// contract, and its conversations still answered.
    #[test]
    fn the_store_bridge_asks_jobs_only_of_a_driver_that_keeps_them() {
        let dir = link_test_kit::TempDir::new("store-bridge-no-jobs");
        let program = dir.join("lnk-old");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh
echo \"$1\" >> '{}'
                 case \"$1\" in
                 contract) echo '{{\"version\":1}}' ;;
                 call) echo '{{\"reply\":\"last\",\"last\":5}}' ;;
                 esac
",
                dir.join("ran").display()
            ),
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("old", program, &dir));
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        let refused = bridged.job_list(&agent, Place::Waiting).unwrap_err();
        assert!(
            format!("{refused:#}").contains("keeps no jobs"),
            "{refused:#}"
        );
        assert_eq!(bridged.last(&agent, "t").unwrap(), 5);
        assert_eq!(lines(&dir.join("ran")), ["contract", "call"]);
    }

    /// A driver whose contract doesn't say `files` is never asked of files
    /// by the store bridge: refused, its conversations still answered.
    /// Nor is any driver asked to write a file the bridge refuses: a path
    /// it doesn't keep, data that isn't base64 or is larger than a file.
    #[test]
    fn the_store_bridge_asks_files_only_of_a_driver_that_keeps_them() {
        let dir = link_test_kit::TempDir::new("store-bridge-no-files");
        let program = dir.join("lnk-old");
        link_test_kit::write_program(
            &program,
            format!(
                "#!/bin/sh
echo \"$1\" >> '{}'
                 case \"$1\" in
                 contract) echo '{{\"version\":1}}' ;;
                 call) echo '{{\"reply\":\"last\",\"last\":5}}' ;;
                 esac
",
                dir.join("ran").display()
            ),
        )
        .unwrap();
        let bridged =
            crate::testing::store_bridge(&dir, "aaaaaaaaaaaa", Driver::at("old", program, &dir));
        let agent = Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        };
        for refused in [
            bridged
                .file_write(&agent, "instructions/a.md", b"x", 0)
                .map(drop),
            bridged
                .file_append(&agent, "instructions/a.md", b"x")
                .map(drop),
            bridged
                .file_remove(&agent, "instructions/a.md", 1)
                .map(drop),
            bridged.file_read(&agent, "instructions/a.md").map(drop),
        ] {
            let refused = refused.unwrap_err();
            assert!(
                format!("{refused:#}").contains("keeps no files"),
                "{refused:#}"
            );
        }
        // Past the client's checks, the bridge's own.
        for request in [
            Request::FileWrite {
                agent: agent.clone(),
                path: "instructions/a.md".into(),
                data: "not base64!".into(),
                version: 0,
            },
            Request::FileAppend {
                agent: agent.clone(),
                path: "instructions/a.md".into(),
                data: "A".repeat(FILE_MAX as usize * 2),
            },
            Request::FileWrite {
                agent: agent.clone(),
                path: "chats/t/events.jsonl".into(),
                data: "eA==".into(),
                version: 0,
            },
            Request::FileRemove {
                agent: agent.clone(),
                path: "../x".into(),
                version: 1,
            },
        ] {
            assert!(bridged.call(&request).is_err(), "{request:?}");
        }
        assert_eq!(bridged.last(&agent, "t").unwrap(), 5);
        assert_eq!(lines(&dir.join("ran")), ["contract", "call"]);
    }
}
