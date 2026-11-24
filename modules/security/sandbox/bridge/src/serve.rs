//! The server every bridge runs on: a long-lived process taking calls,
//! each on a connection of its own, as [`crate`] says a call is.
//!
//! - **Before the proof**, a connection has [`PROOF_WAIT`] from its
//!   challenge to prove who's calling, the whole exchange, however slowly
//!   it's sent, and holds one of [`PEER_PROVING`] places of its peer's
//!   (the user at its other end, as the system names it), apart from
//!   every proved call's. A peer's next connection closes that peer's own
//!   oldest, never another's; and there's room for [`PEERS`](crate::PEERS) peers each
//!   holding all of theirs ([`PROVING`] in all), past which a new peer's
//!   connection is answered busy. So a peer that floods keeps out
//!   neither a proved call nor another peer's proof, though its own
//!   callers, proving as it floods, may be closed with it: on a machine
//!   where every agent runs as one user, that's every caller there.
//! - **Once proved**, a call holds one of the bridge's [`Limits::calls`]
//!   places, one of its agent's [`Limits::per_agent`] and one of its
//!   owner's [`Limits::per_owner`]: past any, it's answered that the
//!   bridge is busy, at once.
//! - **Its action** comes within [`Limits::action_wait`], at most
//!   [`Limits::action_max`] bytes; its answer is taken within
//!   [`Limits::answer_wait`].
//!
//! Every failure is answered with the one error envelope, [`Failure`].

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt as _;
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::Notify;

use crate::take::{line_async, prove, write_async};
use crate::{Failure, Identity, Keys, PEER_PROVING, PROVING, Why, parsed};

/// What one bridge takes at once and waits for: its own, as each bridge
/// says (the store bridge's, the files bridge's, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Its name, as its answers say it: `the store bridge`.
    pub name: &'static str,
    /// The most proved calls it holds at once.
    pub calls: usize,
    /// The most of them one agent's calls hold, so the rest are left to
    /// the others.
    pub per_agent: usize,
    /// The most of them one owner's agents' calls hold together.
    pub per_owner: usize,
    /// The most a call's action is, as JSON.
    pub action_max: u64,
    /// How long a proved call's action may take to arrive.
    pub action_wait: Duration,
    /// How long its answer may wait to be taken.
    pub answer_wait: Duration,
}

/// A bridge on `listener` until `stop` (checked as each connection
/// arrives): each call proved against `keys`, then answered with what
/// `answer` gives for who called and its action, run where it may block
/// (a thread of its own), each within `limits`.
pub fn serve<A, R, F>(
    listener: std::os::unix::net::UnixListener,
    keys: Arc<dyn Keys>,
    limits: &Limits,
    stop: &AtomicBool,
    answer: F,
) -> anyhow::Result<()>
where
    A: DeserializeOwned + Send + 'static,
    R: Serialize + Send + 'static,
    F: Fn(&Identity, A) -> anyhow::Result<R> + Send + Sync + 'static,
{
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let answer = Arc::new(answer);
    let served = runtime.block_on(async {
        let listener = tokio::net::UnixListener::from_std(listener)?;
        serve_with(listener, keys, limits, stop, |who, action: A| {
            let answer = answer.clone();
            async move {
                tokio::task::spawn_blocking(move || answer(&who, action))
                    .await
                    .map_err(|err| {
                        anyhow::anyhow!("the call ended before it was answered: {err}")
                    })?
            }
        })
        .await;
        anyhow::Ok(())
    });
    // Calls still being answered finish on their own.
    runtime.shutdown_background();
    served
}

/// [`serve`], with `answer` a future of the caller's: for a bridge whose
/// answers are async, and may borrow what the bridge holds. Every call is
/// served on the task this runs on.
pub async fn serve_with<A, R, F, Fut>(
    listener: tokio::net::UnixListener,
    keys: Arc<dyn Keys>,
    limits: &Limits,
    stop: &AtomicBool,
    answer: F,
) where
    A: DeserializeOwned,
    R: Serialize,
    F: Fn(Identity, A) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let places = Places::new(*limits);
    accepting(listener, stop, |stream| {
        one(stream, &keys, &places, &answer)
    })
    .await;
}

/// A bridge whose calls are streams ([`crate::stream`]) on `listener`
/// until `stop`, each proved against `keys` and held within `limits` as
/// [`serve_with`]'s are, for as long as its stream is open: `handle` is
/// given who called, its action and the stream, and writes its verdict
/// before anything else. A call it fails before its verdict is answered
/// with the error envelope.
pub async fn serve_streams<A, F, Fut>(
    listener: tokio::net::UnixListener,
    keys: Arc<dyn Keys>,
    limits: &Limits,
    stop: &AtomicBool,
    handle: F,
) where
    A: DeserializeOwned,
    F: Fn(Identity, A, crate::stream::Stream) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    serve_streams_from(listener, keys, limits, stop, |_, who, action, stream| {
        handle(who, action, stream)
    })
    .await;
}

/// [`serve_streams`], also handing the handler the kernel's peer UID.
/// Both key lookup and the action use that same observed peer.
pub async fn serve_streams_from<A, F, Fut>(
    listener: tokio::net::UnixListener,
    keys: Arc<dyn Keys>,
    limits: &Limits,
    stop: &AtomicBool,
    handle: F,
) where
    A: DeserializeOwned,
    F: Fn(u32, Identity, A, crate::stream::Stream) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let places = Places::new(*limits);
    accepting(listener, stop, |stream| {
        crate::stream::one(stream, &keys, &places, &handle)
    })
    .await;
}

/// Each connection `listener` takes until `stop`, served by `each`, all
/// on the task this runs on.
#[expect(
    clippy::disallowed_methods,
    reason = "rests after a failed accept rather than spin"
)]
async fn accepting<'a, E, Fut>(listener: tokio::net::UnixListener, stop: &AtomicBool, each: E)
where
    E: Fn(tokio::net::UnixStream) -> Fut + 'a,
    Fut: Future<Output = ()> + 'a,
{
    let mut calls = futures_util::stream::FuturesUnordered::new();
    // After a failed accept (out of descriptors, say), a moment for
    // some to close, the calls served meanwhile.
    let mut resting = false;
    loop {
        tokio::select! {
            accepted = listener.accept(), if !resting => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match accepted {
                    Ok((stream, _)) => calls.push(each(stream)),
                    Err(_) => resting = true,
                }
            }
            () = tokio::time::sleep(Duration::from_millis(50)), if resting => resting = false,
            Some(()) = calls.next(), if !calls.is_empty() => {}
        }
    }
}

/// One call on `stream`, answered.
async fn one<A, R, F, Fut>(
    stream: tokio::net::UnixStream,
    keys: &Arc<dyn Keys>,
    places: &Places,
    answer: &F,
) where
    A: DeserializeOwned,
    R: Serialize,
    F: Fn(Identity, A) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let peer = peer_of(&stream);
    let (read, mut write) = stream.into_split();
    let mut read = tokio::io::BufReader::new(read);
    let taken = taken(peer, &mut read, &mut write, keys, places, answer).await;
    let (line, _held) = match taken {
        Ok((answer, held)) => match serde_json::to_value(&answer) {
            Ok(answer) => (answer, Some(held)),
            Err(err) => (
                envelope(&Failure::new(Why::Failed, format!("its answer: {err}"))),
                None,
            ),
        },
        Err(failure) => (envelope(&failure), None),
    };
    let _ = tokio::time::timeout(places.limits.answer_wait, write_async(&mut write, &line)).await;
}

/// `failure` as it's written.
pub(crate) fn envelope(failure: &Failure) -> serde_json::Value {
    serde_json::to_value(failure).unwrap_or_default()
}

/// One call, proved and answered: the answer, and the places it holds
/// until it's written.
async fn taken<'p, A, R, F, Fut>(
    peer: u32,
    read: &mut (impl AsyncBufRead + Unpin),
    write: &mut (impl AsyncWrite + Unpin),
    keys: &Arc<dyn Keys>,
    places: &'p Places,
    answer: &F,
) -> Result<(R, Held<'p>), Failure>
where
    A: DeserializeOwned,
    F: Fn(Identity, A) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let limits = &places.limits;
    let waiting = places.wait(peer).ok_or_else(|| busy(limits))?;
    let who = tokio::select! {
        who = prove(read, write, keys) => who?,
        () = waiting.closed() => return Err(busy(limits)),
    };
    drop(waiting);
    let held = places.hold(&who).ok_or_else(|| busy(limits))?;
    let action = tokio::time::timeout(
        limits.action_wait,
        line_async(read, limits.action_max, "the call's action"),
    )
    .await
    .map_err(|_| Failure::new(Why::Refused, "the call's action didn't come in time"))??;
    let action: A = parsed(&action, "the call's action")?;
    let answer = answer(who, action).await.map_err(Failure::of)?;
    Ok((answer, held))
}

/// That the bridge is busy.
pub(crate) fn busy(limits: &Limits) -> Failure {
    Failure::new(Why::Busy, format!("{} is busy: try again", limits.name))
}

/// The user at `stream`'s other end, as the system names it; one no
/// user is, when it can't say.
pub(crate) fn peer_of(stream: &tokio::net::UnixStream) -> u32 {
    stream.peer_cred().map_or(u32::MAX, |cred| cred.uid())
}

/// The places a bridge's connections hold: before their proof, by peer,
/// and after, of all, of each agent and of each owner.
pub(crate) struct Places {
    pub(crate) limits: Limits,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The next connection's number.
    next: u64,
    /// Connections waiting for their proof, by peer, oldest first, and
    /// how each is told it's closed for a newer one of its peer's.
    waiting: HashMap<u32, VecDeque<(u64, Arc<Notify>)>>,
    /// Proved calls held.
    calls: usize,
    /// Those of each agent, by its ID and owner.
    agents: HashMap<(String, String), usize>,
    /// Those of each owner's agents.
    owners: HashMap<String, usize>,
}

impl Places {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A place for one of `peer`'s connections until it's proved: its
    /// peer's oldest closed for it when the peer holds all of its own;
    /// none when it's a peer's first past the room for
    /// [`PEERS`](crate::PEERS).
    pub(crate) fn wait(&self, peer: u32) -> Option<Waiting<'_>> {
        let mut state = self.state();
        let all: usize = state.waiting.values().map(VecDeque::len).sum();
        let mine = state.waiting.get(&peer).map_or(0, VecDeque::len);
        if mine >= PEER_PROVING {
            if let Some((_, oldest)) = state.waiting.get_mut(&peer).and_then(VecDeque::pop_front) {
                oldest.notify_one();
            }
        } else if all >= PROVING {
            return None;
        }
        let number = state.next;
        state.next += 1;
        let closed = Arc::new(Notify::new());
        state
            .waiting
            .entry(peer)
            .or_default()
            .push_back((number, closed.clone()));
        Some(Waiting {
            places: self,
            peer,
            number,
            closed,
        })
    }

    /// A place for `who`'s proved call, if one is free: of all, of its
    /// agent's and of its owner's.
    pub(crate) fn hold(&self, who: &Identity) -> Option<Held<'_>> {
        let agent = (who.agent.clone(), who.owner.clone());
        let mut state = self.state();
        let of_agent = state.agents.get(&agent).copied().unwrap_or(0);
        let of_owner = state.owners.get(&who.owner).copied().unwrap_or(0);
        if state.calls >= self.limits.calls
            || of_agent >= self.limits.per_agent
            || of_owner >= self.limits.per_owner
        {
            return None;
        }
        state.calls += 1;
        state.agents.insert(agent.clone(), of_agent + 1);
        state.owners.insert(who.owner.clone(), of_owner + 1);
        Some(Held {
            places: self,
            agent,
        })
    }
}

/// One less of `key` in `counts`, gone at none.
fn let_go<K: std::hash::Hash + Eq>(counts: &mut HashMap<K, usize>, key: &K) {
    if let Some(n) = counts.get_mut(key) {
        *n -= 1;
        if *n == 0 {
            counts.remove(key);
        }
    }
}

/// A connection's place while it proves who's calling.
pub(crate) struct Waiting<'p> {
    places: &'p Places,
    peer: u32,
    number: u64,
    closed: Arc<Notify>,
}

impl Waiting<'_> {
    /// Once it's closed for a newer connection of its peer's.
    pub(crate) async fn closed(&self) {
        self.closed.notified().await;
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let mut state = self.places.state();
        if let Some(waiting) = state.waiting.get_mut(&self.peer) {
            waiting.retain(|(number, _)| *number != self.number);
            if waiting.is_empty() {
                state.waiting.remove(&self.peer);
            }
        }
    }
}

/// A proved call's place, given back once it's answered.
pub(crate) struct Held<'p> {
    places: &'p Places,
    agent: (String, String),
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let mut state = self.places.state();
        state.calls -= 1;
        let_go(&mut state.agents, &self.agent);
        let_go(&mut state.owners, &self.agent.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Binding, PEERS};
    use std::sync::atomic::AtomicUsize;

    const LIMITS: Limits = Limits {
        name: "the test's bridge",
        calls: 4,
        per_agent: 2,
        per_owner: 2,
        action_max: 1 << 20,
        action_wait: Duration::from_secs(3),
        answer_wait: Duration::from_secs(3),
    };

    /// Agent `n`, of an owner of its own.
    fn agent(n: usize) -> Binding {
        Binding {
            identity: Identity {
                agent: format!("agent{n:07}"),
                service: String::new(),
                owner: format!("acct-{n}"),
                teams: Vec::new(),
            },
            key: format!("{n:0>32}").into(),
        }
    }

    fn binding() -> Binding {
        agent(1)
    }

    /// Every agent of `agents` bound, each with its own key.
    struct Bound(Vec<Binding>);

    impl Keys for Bound {
        fn key_of(&self, identity: &Identity) -> Option<crate::Secret> {
            self.0.iter().find_map(|b| b.key_of(identity))
        }
    }

    /// A folder of its own for a test, removed when dropped, so a test
    /// that fails leaves nothing. Not `link_test_kit::TempDir`:
    /// link-core depends on this crate, so this one can't on it.
    struct Dir(std::path::PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "lnk-bridge-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A bridge that says who called and what it asked, on the server,
    /// counting what it ran, and answering `big` with 8 MiB, passes the
    /// test every bridge passes.
    #[test]
    fn a_bridge_on_the_server_passes_the_test_every_bridge_passes() {
        let dir = Dir::new("conformance");
        let socket = dir.0.join("bridge.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let agents: Vec<Binding> = (1..=4).map(agent).collect();
        let keys: Arc<dyn Keys> = Arc::new(Bound(agents.iter().map(Binding::duplicate).collect()));
        let ran = Arc::new(AtomicUsize::new(0));
        let running = ran.clone();
        std::thread::spawn(move || {
            let stop = AtomicBool::new(false);
            serve(
                listener,
                keys,
                &LIMITS,
                &stop,
                move |who, action: serde_json::Value| {
                    running.fetch_add(1, Ordering::SeqCst);
                    Ok(match action == "big" {
                        true => serde_json::json!({"big": "x".repeat(8 << 20)}),
                        false => serde_json::json!({"who": who.agent, "asked": action}),
                    })
                },
            )
        });
        crate::conformance::check(&crate::conformance::Subject {
            socket: socket.clone(),
            agents,
            action: serde_json::json!({"say": "hello"}),
            ran: Arc::new(move || ran.load(Ordering::SeqCst)),
            big: Some(serde_json::json!("big")),
            limits: LIMITS,
        });
        let answer: serde_json::Value =
            crate::call_at(&socket, &binding(), &serde_json::json!({"say": "hi"}), 4096).unwrap();
        assert_eq!(
            answer,
            serde_json::json!({"who": "agent0000001", "asked": {"say": "hi"}})
        );
    }

    /// A bridge that proves its calls and turns away those past its
    /// places as every bridge does, but never gives a place back: each
    /// call it takes holds its place for good, answered or gone.
    fn never_freeing(socket: &std::path::Path, agents: &[Binding]) {
        use std::io::{BufRead, Write};
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let keys: Arc<dyn Keys> = Arc::new(Bound(agents.iter().map(Binding::duplicate).collect()));
        let held: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (keys, held) = (keys.clone(), held.clone());
                std::thread::spawn(move || {
                    let mut write = &stream;
                    let say = |mut write: &std::os::unix::net::UnixStream,
                               value: serde_json::Value| {
                        let _ = writeln!(write, "{value}");
                    };
                    let challenge = crate::challenge().unwrap();
                    say(
                        write,
                        serde_json::json!(crate::Challenge::new(challenge.clone())),
                    );
                    let mut read = std::io::BufReader::new(&stream);
                    let mut line = String::new();
                    let _ = read.read_line(&mut line);
                    let who = serde_json::from_str::<crate::Proven>(&line)
                        .map_err(|_| Failure::new(Why::Refused, "who's calling"))
                        .and_then(|proven| crate::checked(&challenge, &proven, &*keys));
                    let who = match who {
                        Ok(who) => who,
                        Err(failure) => return say(write, envelope(&failure)),
                    };
                    {
                        let mut held = held.lock().unwrap();
                        let all: usize = held.values().sum();
                        let mine = held.get(&who.agent).copied().unwrap_or(0);
                        if all >= LIMITS.calls || mine >= LIMITS.per_agent {
                            return say(write, envelope(&busy(&LIMITS)));
                        }
                        held.insert(who.agent.clone(), mine + 1);
                    }
                    line.clear();
                    if read.read_line(&mut line).is_ok_and(|n| n > 0) {
                        let _ = writeln!(write, "{}", line.trim_end());
                    }
                });
            }
        });
    }

    /// A bridge that never gives a place back fails the test every bridge
    /// passes, however its rounds are held: the first round's places are
    /// never given back, so the next round's first call is never taken.
    #[test]
    #[should_panic(expected = "no call of one agent's was taken")]
    fn a_bridge_that_never_frees_a_place_fails_the_test() {
        let dir = Dir::new("leaks");
        let socket = dir.0.join("bridge.sock");
        let agents: Vec<Binding> = (1..=4).map(agent).collect();
        never_freeing(&socket, &agents);
        crate::conformance::calls_past_their_places_are_busy(&crate::conformance::Subject {
            socket,
            agents,
            action: serde_json::json!({"say": "hello"}),
            ran: Arc::new(|| 0),
            big: None,
            limits: LIMITS,
        });
    }

    /// A call that fails is answered with the envelope, and reads as an
    /// error that is that failure; a conflict stays one.
    #[test]
    fn a_failed_call_reads_as_its_failure() {
        let dir = Dir::new("failed");
        let socket = dir.0.join("bridge.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let stop = AtomicBool::new(false);
            serve(
                listener,
                Arc::new(binding()),
                &LIMITS,
                &stop,
                |_, action: String| -> anyhow::Result<()> {
                    match action.as_str() {
                        "conflict" => Err(Failure::new(Why::Conflict, "it changed").into()),
                        _ => anyhow::bail!("no such thing"),
                    }
                },
            )
        });
        let failed = crate::call_at::<_, ()>(&socket, &binding(), &"x", 4096).unwrap_err();
        let failure = failed.downcast_ref::<Failure>().unwrap();
        assert_eq!(
            (failure.why, failure.error.as_str()),
            (Why::Failed, "no such thing")
        );
        let failed = crate::call_at::<_, ()>(&socket, &binding(), &"conflict", 4096).unwrap_err();
        assert_eq!(failed.downcast_ref::<Failure>().unwrap().why, Why::Conflict);
    }

    /// A bridge stops once told, at the next connection.
    #[test]
    fn a_bridge_stops_when_told() {
        let dir = Dir::new("stop");
        let socket = dir.0.join("bridge.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let serving = std::thread::spawn({
            let stop = stop.clone();
            move || {
                serve(
                    listener,
                    Arc::new(binding()),
                    &LIMITS,
                    &stop,
                    |_, _: serde_json::Value| Ok(()),
                )
            }
        });
        stop.store(true, Ordering::Relaxed);
        let _ = std::os::unix::net::UnixStream::connect(&socket);
        serving.join().unwrap().unwrap();
    }

    /// One peer flooding closes only its own connections proving: another
    /// peer's, proving as it floods, are never closed for it.
    #[test]
    fn one_peers_flood_never_closes_anothers_proof() {
        let places = Places::new(LIMITS);
        let honest = places.wait(2).unwrap();
        let flood: Vec<_> = (0..PEER_PROVING * 10)
            .map(|_| places.wait(1).unwrap())
            .collect();
        let closed = |waiting: &Waiting<'_>| {
            futures_util::FutureExt::now_or_never(waiting.closed()).is_some()
        };
        assert!(!closed(&honest), "another peer's proof was closed");
        // The flood closed its own, all but its newest share.
        let open = flood.iter().filter(|w| !closed(w)).count();
        assert_eq!(open, PEER_PROVING);
    }

    /// Every one of the most peers holding its share leaves the last its
    /// own; a peer past them is turned away, until one goes.
    #[test]
    fn every_peer_has_room_to_prove() {
        let places = Places::new(LIMITS);
        let held: Vec<_> = (0..PEERS as u32)
            .flat_map(|peer| (0..PEER_PROVING).map(move |_| peer))
            .map(|peer| places.wait(peer).expect("a peer's share"))
            .collect();
        assert!(
            places.wait(PEERS as u32).is_none(),
            "room past every peer's"
        );
        drop(held);
        assert!(places.wait(PEERS as u32).is_some());
    }

    /// One owner's agents hold at most their owner's share together;
    /// another owner's call is still taken.
    #[test]
    fn one_owner_holds_at_most_its_share() {
        let places = Places::new(Limits {
            per_agent: 2,
            per_owner: 3,
            calls: 8,
            ..LIMITS
        });
        let of = |agent: &str, owner: &str| Identity {
            agent: agent.into(),
            service: String::new(),
            owner: owner.into(),
            teams: Vec::new(),
        };
        let held = [
            places.hold(&of("a", "o1")).unwrap(),
            places.hold(&of("a", "o1")).unwrap(),
            places.hold(&of("b", "o1")).unwrap(),
        ];
        assert!(places.hold(&of("a", "o1")).is_none(), "its agent's share");
        assert!(places.hold(&of("c", "o1")).is_none(), "its owner's share");
        assert!(places.hold(&of("c", "o2")).is_some(), "another owner's");
        drop(held);
        assert!(places.hold(&of("c", "o1")).is_some());
    }
}
