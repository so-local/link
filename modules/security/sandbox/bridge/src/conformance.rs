//! The test every bridge passes ([`check`]), run against the bridge
//! itself, listening: what [`crate`] and [`crate::serve`] promise of every
//! bridge, whatever it does with a call.
//!
//! - a call proved with a wrong key is refused;
//! - a proof replayed on a new connection is refused;
//! - a [`crate::Proven`] that's malformed, names an identity no ID could
//!   be, or is overlong, is refused, and nothing more of the call is read;
//! - a proved call whose action isn't one is refused;
//! - none of those, nor a call turned away busy, runs its action;
//! - a caller that never answers its challenge, or sends its proof a byte
//!   now and then, is cut off within [`PROOF_WAIT`];
//! - a proved caller that sends its action a byte now and then is cut off
//!   within [`Limits::action_wait`];
//! - a peer flooding the bridge with connections that never prove keeps
//!   out no proof of its own made at once: its oldest are closed, busy;
//! - one agent's calls past [`Limits::per_agent`], and anyone's past
//!   [`Limits::calls`], are answered that it's busy, and taken again once
//!   one of them goes;
//! - a caller that never takes its answer gives its place back within
//!   [`Limits::answer_wait`] (for a bridge with an answer larger than a
//!   connection holds, [`Subject::big`]);
//! - every failure is the error envelope ([`Failure`]): one line, then the
//!   connection closed.
//!
//! Every connection the test makes is one peer's, its own user's: what
//! one peer can't do to another's ([`crate::serve`]) is tested on the
//! server's places alone.
//!
//! Each case panics, saying what failed. They take about the longest of
//! [`PROOF_WAIT`], [`Limits::action_wait`] and [`Limits::answer_wait`].

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    Binding, Challenge, Failure, Identity, Limits, PEER_PROVING, PROOF_WAIT, PROVEN_MAX, Why,
};

/// A bridge to check: where it listens; agents bound on it, each of an
/// owner of its own, the first whose action `action` is, and enough of
/// them to hold every one of its [`Limits::calls`] and one more (one
/// alone, for a bridge only one agent ever calls, whose
/// [`Limits::per_agent`] is then all of its calls); an action it takes
/// from the first (answered as the bridge answers it, but never refused
/// or busy); how many actions it has run so far; an action whose answer
/// is larger than a connection holds, where it has one; and its limits.
pub struct Subject {
    pub socket: PathBuf,
    pub agents: Vec<Binding>,
    pub action: serde_json::Value,
    pub ran: Arc<dyn Fn() -> usize + Send + Sync>,
    pub big: Option<serde_json::Value>,
    pub limits: Limits,
}

impl Subject {
    fn first(&self) -> &Binding {
        &self.agents[0]
    }

    fn name(&self) -> &str {
        self.limits.name
    }
}

/// `limits`, its waits as short as the check can still tell apart: what
/// the check tests is that each wait is kept, not how long it is. A
/// bridge's test serves it, and checks it, within these, so it waits
/// seconds, not its real waits. A call held outlasts [`PROOF_WAIT`],
/// which tells a call taken from one turned away.
pub fn brief(limits: Limits) -> Limits {
    Limits {
        action_wait: limits.action_wait.min(PROOF_WAIT + Duration::from_secs(3)),
        answer_wait: limits.answer_wait.min(Duration::from_secs(3)),
        ..limits
    }
}

/// Runs every case against `subject`: those that hold its places one at
/// a time, as every connection here is one peer's.
pub fn check(subject: &Subject) {
    let limits = &subject.limits;
    assert!(!subject.agents.is_empty(), "{}: no agent", subject.name());
    match subject.agents.len() {
        1 => assert_eq!(
            limits.per_agent,
            limits.calls,
            "{}: one agent calls it, so its calls are all one agent's",
            subject.name()
        ),
        n => assert!(
            limits.per_agent < limits.calls && n > limits.calls.div_ceil(limits.per_agent),
            "{}: give agents enough to hold every call and one more",
            subject.name()
        ),
    }
    a_proved_call_is_answered(subject);
    a_refused_call_never_runs(subject);
    a_peers_flood_keeps_out_none_of_its_proofs(subject);
    std::thread::scope(|scope| {
        let slow = [
            scope.spawn(|| a_caller_that_never_answers_is_cut_off(subject)),
            scope.spawn(|| a_proof_sent_a_byte_at_a_time_is_cut_off(subject)),
            scope.spawn(|| an_action_sent_a_byte_at_a_time_is_cut_off(subject)),
        ];
        for case in slow {
            if let Err(panic) = case.join() {
                std::panic::resume_unwind(panic);
            }
        }
    });
    calls_past_their_places_are_busy(subject);
    if subject.big.is_some() {
        an_answer_never_taken_gives_its_place_back(subject);
    }
}

/// A call made as the first agent: its answer, a JSON line.
pub fn a_proved_call_is_answered(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&subject.first().prove(&challenge));
    call.send(&subject.action);
    let answer = call.answer();
    if let Ok(failure) = serde_json::from_value::<Failure>(answer.clone()) {
        assert!(
            !matches!(failure.why, Why::Refused | Why::Busy),
            "{}: a proved call was answered {answer}",
            subject.name()
        );
    }
}

/// Every refused call, each with its action sent: refused, and none of
/// their actions run.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn a_refused_call_never_runs(subject: &Subject) {
    let before = (subject.ran)();
    a_wrong_key_is_refused(subject);
    a_proof_replayed_on_a_new_connection_is_refused(subject);
    a_malformed_proven_is_refused(subject);
    an_overlong_proven_is_refused(subject);
    an_action_it_cant_read_is_refused(subject);
    // Settled: a bridge that ran one would have by now.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        (subject.ran)(),
        before,
        "{}: a refused call's action ran",
        subject.name()
    );
}

/// A call proved with a key that isn't its agent's.
pub fn a_wrong_key_is_refused(subject: &Subject) {
    let wrong = Binding {
        key: subject.first().key.expose_map(|key| format!("{key}x")),
        ..subject.first().duplicate()
    };
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&wrong.prove(&challenge));
    call.send(&subject.action);
    call.refused("a wrong key");
}

/// A proof made for one connection's challenge, sent on another.
pub fn a_proof_replayed_on_a_new_connection_is_refused(subject: &Subject) {
    let mut first = Call::open(&subject.socket);
    let seen = subject.first().prove(&first.challenge());
    let mut second = Call::open(&subject.socket);
    let _ = second.challenge();
    second.send(&seen);
    second.send(&subject.action);
    second.refused("a proof replayed on a new connection");
    // The proof was good where it was made, but this call never sends
    // its action: let go, refused.
    drop(first);
}

/// A first line that isn't a [`crate::Proven`], and one naming who no ID
/// could be.
pub fn a_malformed_proven_is_refused(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let _ = call.challenge();
    call.write(b"not a proof\n");
    call.send(&subject.action);
    call.refused("a first line that isn't JSON");

    let mut call = Call::open(&subject.socket);
    let _ = call.challenge();
    call.send(&serde_json::json!({"identity": "me"}));
    call.send(&subject.action);
    call.refused("a first line that isn't a proof");

    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    let mut sneaky = subject.first().duplicate();
    sneaky.identity = Identity {
        agent: format!("../{}{}[2J", sneaky.identity.agent, char::from(27u8)),
        ..sneaky.identity
    };
    call.send(&sneaky.prove(&challenge));
    call.send(&subject.action);
    let failure = call.refused("an identity no ID could be");
    assert!(
        !failure.error.contains(char::from(27u8)) && !failure.error.contains("../"),
        "{}: a refusal showed what the caller named: {:?}",
        subject.name(),
        failure.error
    );
}

/// A first line longer than [`PROVEN_MAX`], refused once that much is
/// read: never read to its end.
pub fn an_overlong_proven_is_refused(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let _ = call.challenge();
    let started = Instant::now();
    call.write(&vec![b' '; PROVEN_MAX as usize + 1]);
    let failure = call.refused("a first line longer than its most");
    assert!(
        failure.error.contains("longer"),
        "{}: {:?}",
        subject.name(),
        failure.error
    );
    assert!(
        started.elapsed() < PROOF_WAIT,
        "{}: an overlong proof was waited on",
        subject.name()
    );
}

/// A proved call whose action isn't one: refused, never failed.
pub fn an_action_it_cant_read_is_refused(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&subject.first().prove(&challenge));
    call.write(b"{not json\n");
    call.refused("an action that isn't JSON");
}

/// A caller that reads its challenge and sends nothing.
pub fn a_caller_that_never_answers_is_cut_off(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let _ = call.challenge();
    let started = Instant::now();
    call.refused("a caller that never answers");
    within(
        subject,
        started.elapsed(),
        PROOF_WAIT,
        "a caller that never answers",
    );
}

/// A caller that sends its proof a byte at a time, each well within any
/// one read's wait: cut off all the same, once the proof's time is up.
pub fn a_proof_sent_a_byte_at_a_time_is_cut_off(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    let started = Instant::now();
    let mut proof = serde_json::to_vec(&subject.first().prove(&challenge)).unwrap();
    proof.push(b'\n');
    call.drip(&proof, started, PROOF_WAIT);
    call.refused("a proof sent a byte at a time");
    within(
        subject,
        started.elapsed(),
        PROOF_WAIT,
        "a proof sent a byte at a time",
    );
}

/// A proved caller that sends its action a byte at a time: cut off once
/// the action's time is up.
pub fn an_action_sent_a_byte_at_a_time_is_cut_off(subject: &Subject) {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&subject.first().prove(&challenge));
    let started = Instant::now();
    let mut action = serde_json::to_vec(&subject.action).unwrap();
    // Never a whole line, however long its time.
    action.resize(action.len().max(1 << 16), b' ');
    call.drip(&action, started, subject.limits.action_wait);
    call.refused("an action sent a byte at a time");
    within(
        subject,
        started.elapsed(),
        subject.limits.action_wait,
        "an action sent a byte at a time",
    );
}

/// One peer flooding the bridge with connections that never prove, many
/// times its share: a proof it makes at once is answered all the same,
/// and its oldest connections are closed, answered busy.
pub fn a_peers_flood_keeps_out_none_of_its_proofs(subject: &Subject) {
    let mut flood: Vec<Call> = (0..PEER_PROVING * 4)
        .map(|_| {
            let mut call = Call::open(&subject.socket);
            let _ = call.challenge();
            call
        })
        .collect();
    a_proved_call_is_answered(subject);
    let oldest = flood.remove(0);
    let failure = oldest.failed("the oldest of a peer's connections that never proved");
    assert_eq!(failure.why, Why::Busy, "{}: {failure:?}", subject.name());
    drop(flood);
}

/// One agent's calls holding every place it has: one more is answered
/// that the bridge is busy, and another agent's isn't; every place held,
/// anyone's is busy; one of them gone, a call is taken again.
///
/// The test sees the bridge's places only through its answers, which a
/// loaded machine slows: a dropped call's place comes free only once the
/// bridge reads that it's gone, and a call it turns away may be answered
/// after the test took it as held. So each round starts from a call of
/// the agent's the bridge took ([`a_call_taken`]), and a round whose
/// answers can't say (a call it took as held answered after all, or the
/// held calls' action wait come first) is dropped and held again, at
/// most [`ROUNDS`] times, then failed with what each round saw. A bridge
/// that holds one call past its places, past every proof's wait, is
/// failed at once; one that keeps getting a claim wrong fails every
/// round, and so the case.
pub fn calls_past_their_places_are_busy(subject: &Subject) {
    let mut said = Vec::new();
    for _ in 0..ROUNDS {
        match places_round(subject) {
            Ok(held) => return one_gone_is_taken_again(subject, held),
            Err(Round::Wrong(wrong)) => panic!("{wrong}"),
            Err(Round::Unclear(unclear)) => said.push(unclear),
        }
    }
    panic!(
        "{}: no round of holding its places could tell, in {ROUNDS}: {said:?}",
        subject.name()
    );
}

/// How many rounds a case of the bridge's places tries.
const ROUNDS: usize = 5;

/// What a round of holding the bridge's places found, short of what it
/// claims: wrong, past doubt; or unclear, its answers too slow to say.
enum Round {
    Wrong(String),
    Unclear(String),
}

/// One round of [`calls_past_their_places_are_busy`], up to every place
/// held: its calls, still held, or what it found.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn places_round(subject: &Subject) -> Result<Vec<Call>, Round> {
    let limits = &subject.limits;
    let name = subject.name();
    let held_at = Instant::now();
    let mut held = held_calls(subject, subject.first(), limits.per_agent)?;
    match one_more(subject, subject.first(), &mut held, held_at) {
        Seen::Busy => {}
        Seen::Taken => {
            return Err(Round::Wrong(format!(
                "{name}: {} of one agent's calls held, and one more wasn't busy",
                limits.per_agent
            )));
        }
        Seen::Unclear(why) => return Err(Round::Unclear(format!("{name}: {why}"))),
    }
    // Turned away with its action sent: the action never runs.
    let before = (subject.ran)();
    let mut busy = Call::open(&subject.socket);
    let challenge = busy.challenge();
    busy.send(&subject.first().prove(&challenge));
    busy.send(&subject.action);
    let failure = busy.failed("a call past its agent's places");
    if failure.why != Why::Busy {
        return Err(Round::Unclear(format!("{name}: {failure:?}")));
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!((subject.ran)(), before, "{name}: a busy call's action ran");
    if limits.per_agent < limits.calls {
        if probe(subject, &subject.agents[1]) == Some(Why::Busy) {
            return Err(Round::Unclear(format!(
                "{name}: another agent's call was busy"
            )));
        }
        // Every place held, each agent holding all of its own but the
        // last's.
        let mut agents = subject.agents[1..].iter();
        while held.len() < limits.calls {
            let agent = agents.next().expect("agents enough to hold every call");
            let n = limits.per_agent.min(limits.calls - held.len());
            held.extend(held_calls(subject, agent, n)?);
        }
        let fresh = agents.next().expect("one more agent than every call needs");
        match one_more(subject, fresh, &mut held, held_at) {
            Seen::Busy => {}
            Seen::Taken => {
                return Err(Round::Wrong(format!(
                    "{name}: {} calls held, and another agent's wasn't busy",
                    limits.calls
                )));
            }
            Seen::Unclear(why) => return Err(Round::Unclear(format!("{name}: {why}"))),
        }
    }
    Ok(held)
}

/// One of the first agent's calls in `held` gone: its call is taken again.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn one_gone_is_taken_again(subject: &Subject, mut held: Vec<Call>) {
    drop(held.remove(0));
    let started = Instant::now();
    while probe(subject, subject.first()) == Some(Why::Busy) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{}: still busy once one of its calls went",
            subject.name()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A call whose answer is larger than its connection holds, never taken:
/// the bridge gives its place back once the answer's time is up. Its
/// places are held in rounds, as [`calls_past_their_places_are_busy`]'s.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn an_answer_never_taken_gives_its_place_back(subject: &Subject) {
    let big = subject.big.as_ref().expect("an action with a large answer");
    let limits = &subject.limits;
    let name = subject.name();
    let mut said = Vec::new();
    for _ in 0..ROUNDS {
        let held_at = Instant::now();
        let mut held = match held_calls(subject, subject.first(), limits.per_agent - 1) {
            Ok(held) => held,
            Err(Round::Unclear(why) | Round::Wrong(why)) => {
                said.push(why);
                continue;
            }
        };
        let mut stuck = Call::open(&subject.socket);
        let challenge = stuck.challenge();
        stuck.send(&subject.first().prove(&challenge));
        stuck.send(big);
        // Answered, as far as it fits, then held writing the rest: only
        // then is one more call past its places.
        match stuck.answer_begun(PROOF_WAIT) {
            Some(true) => {}
            begun => {
                said.push(format!("{name}: the call held writing began {begun:?}"));
                continue;
            }
        }
        match one_more(subject, subject.first(), &mut held, held_at) {
            Seen::Busy => {}
            Seen::Taken => panic!("{name}: its places held, one more wasn't busy"),
            Seen::Unclear(why) => {
                said.push(format!("{name}: {why}"));
                continue;
            }
        }
        let started = Instant::now();
        while probe(subject, subject.first()) == Some(Why::Busy) {
            assert!(
                started.elapsed() < limits.answer_wait + Duration::from_secs(5),
                "{name}: an answer never taken held its place past {:?}",
                limits.answer_wait
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        held.clear();
        drop(stuck);
        return;
    }
    panic!("{name}: no round of holding its places could tell, in {ROUNDS}: {said:?}");
}

/// A call of `agent`'s the bridge took, waited for: each call before it
/// that was turned away (a place the case before let go not yet free) is
/// dropped and made again, until one is held, for at most 10 seconds.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_call_taken(subject: &Subject, agent: &Binding) -> Call {
    let started = Instant::now();
    loop {
        let mut call = Call::open(&subject.socket);
        let challenge = call.challenge();
        call.send(&agent.prove(&challenge));
        call.reads_within(Duration::from_millis(500));
        if !call.answered() {
            return call;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{}: no call of one agent's was taken for 10 seconds",
            subject.name()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `n` calls of `agent`'s held, the first one the bridge took
/// ([`a_call_taken`]); unclear if one is never held.
fn held_calls(subject: &Subject, agent: &Binding, n: usize) -> Result<Vec<Call>, Round> {
    let mut held = vec![a_call_taken(subject, agent)];
    for _ in 1..n {
        let call = hold(subject, agent).ok_or_else(|| {
            Round::Unclear(format!("{}: a proved call was never held", subject.name()))
        })?;
        held.push(call);
    }
    Ok(held)
}

/// A call of `agent`'s, proved and holding its place, its action never
/// sent; made again if the bridge answers it at once (closed for a newer
/// connection of this peer's while it proved). None if none is, in 20.
fn hold(subject: &Subject, agent: &Binding) -> Option<Call> {
    for _ in 0..20 {
        let mut call = Call::open(&subject.socket);
        let challenge = call.challenge();
        call.send(&agent.prove(&challenge));
        call.reads_within(Duration::from_millis(50));
        if !call.answered() {
            return Some(call);
        }
    }
    None
}

/// What the bridge says to a proved call of `agent`'s: busy, or what kind
/// of failure (none when it's taken, its action never sent).
fn probe(subject: &Subject, agent: &Binding) -> Option<Why> {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&agent.prove(&challenge));
    call.reads_within(Duration::from_millis(500));
    if !call.answered() {
        return None;
    }
    serde_json::from_value::<Failure>(call.answer())
        .ok()
        .map(|failure| failure.why)
}

/// What [`one_more`] saw of a call past those held.
enum Seen {
    /// One of the calls, it or one held, turned away busy.
    Busy,
    /// Every call taken, past every proof's wait: one too many.
    Taken,
    /// Too slow to say: why.
    Unclear(String),
}

/// A call of `agent`'s past those `held` (held since `held_at`): whether
/// the bridge turned one of the calls away busy, it or one of those,
/// held only as far as the test could tell, whose proof the bridge read
/// after its own (that one is then let go, and the call taken held after
/// the rest). Watched until a call is answered, or past [`PROOF_WAIT`],
/// when every call is proved or cut off; or until the held calls' action
/// wait, which answers them, comes first, when it can't say.
fn one_more(subject: &Subject, agent: &Binding, held: &mut Vec<Call>, held_at: Instant) -> Seen {
    let mut call = Call::open(&subject.socket);
    let challenge = call.challenge();
    call.send(&agent.prove(&challenge));
    let proved = Instant::now() + PROOF_WAIT + Duration::from_secs(1);
    let action_wait = (held_at + subject.limits.action_wait)
        .checked_sub(Duration::from_millis(200))
        .unwrap_or(held_at);
    loop {
        if let Some(why) = call.answered_why() {
            return match why {
                Some(Why::Busy) => Seen::Busy,
                other => Seen::Unclear(format!("one more call was answered {other:?}")),
            };
        }
        for at in 0..held.len() {
            if let Some(why) = held[at].answered_why() {
                held.remove(at);
                return match why {
                    Some(Why::Busy) => {
                        held.push(call);
                        Seen::Busy
                    }
                    other => Seen::Unclear(format!("a call held was answered {other:?}")),
                };
            }
        }
        let now = Instant::now();
        if now >= action_wait {
            return Seen::Unclear(format!(
                "its calls held as long as their action's wait ({:?})",
                subject.limits.action_wait
            ));
        }
        if now >= proved {
            return Seen::Taken;
        }
    }
}

/// That `took` is within `wait`, and the slack a busy machine needs.
fn within(subject: &Subject, took: Duration, wait: Duration, what: &str) {
    assert!(
        took <= wait + Duration::from_secs(3),
        "{}: {what} was held {took:?}, past {wait:?}",
        subject.name()
    );
}

/// One connection to the bridge, as a caller sees it.
struct Call {
    stream: UnixStream,
    read: BufReader<UnixStream>,
    name: String,
}

impl Call {
    fn open(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket)
            .unwrap_or_else(|err| panic!("reaching the bridge at {}: {err}", socket.display()));
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("a read timeout");
        Self {
            read: BufReader::new(stream.try_clone().expect("the connection")),
            stream,
            name: socket.display().to_string(),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        // A bridge that refused already may have closed it: what it said
        // is read next.
        let _ = self.stream.write_all(bytes);
    }

    fn send(&mut self, value: &impl serde::Serialize) {
        let mut line = serde_json::to_vec(value).unwrap();
        line.push(b'\n');
        self.write(&line);
    }

    /// `bytes` a byte every quarter second, until the bridge closes the
    /// connection or twice `wait` from `started`.
    #[expect(clippy::disallowed_methods, reason = "a slow sender on purpose")]
    fn drip(&mut self, bytes: &[u8], started: Instant, wait: Duration) {
        for byte in bytes {
            std::thread::sleep(Duration::from_millis(250));
            if self.stream.write_all(&[*byte]).is_err() || started.elapsed() > wait * 2 {
                break;
            }
        }
    }

    fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        match (&mut self.read).take(1 << 26).read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }

    /// How long a read waits. A Mac refuses this with `EINVAL` once the
    /// bridge has closed its side, where a read has nothing to wait for.
    fn reads_within(&self, wait: Duration) {
        if let Err(err) = self.stream.set_read_timeout(Some(wait)) {
            assert_eq!(
                err.raw_os_error(),
                Some(libc::EINVAL),
                "{}: {err}",
                self.name
            );
        }
    }

    /// Whether the bridge has written something to read, within the
    /// connection's read timeout.
    fn answered(&mut self) -> bool {
        self.read.fill_buf().is_ok_and(|buf| !buf.is_empty())
    }

    /// Whether the bridge has answered yet, looked at a moment: none if
    /// not; else the failure it answered, or none for an answer that
    /// isn't one.
    fn answered_why(&mut self) -> Option<Option<Why>> {
        self.reads_within(Duration::from_millis(20));
        if !self.answered() {
            return None;
        }
        self.reads_within(Duration::from_secs(60));
        Some(
            self.line()
                .and_then(|line| serde_json::from_str::<Failure>(&line).ok())
                .map(|failure| failure.why),
        )
    }

    /// Whether the bridge has begun its answer, within `wait`: none if
    /// not; else whether it's an answer rather than the error envelope.
    fn answer_begun(&mut self, wait: Duration) -> Option<bool> {
        self.reads_within(wait);
        let begun = self.read.fill_buf().ok().filter(|buf| !buf.is_empty())?;
        Some(!begun.starts_with(b"{\"error\""))
    }

    fn challenge(&mut self) -> String {
        let line = self
            .line()
            .unwrap_or_else(|| panic!("{}: no challenge", self.name));
        let challenge: Challenge = serde_json::from_str(&line)
            .unwrap_or_else(|err| panic!("{}: its challenge {line:?}: {err}", self.name));
        assert_eq!(challenge.challenge.len(), 64, "{}: {line}", self.name);
        challenge.challenge
    }

    fn answer(&mut self) -> serde_json::Value {
        let line = self
            .line()
            .unwrap_or_else(|| panic!("{}: no answer", self.name));
        serde_json::from_str(&line)
            .unwrap_or_else(|err| panic!("{}: its answer {line:?}: {err}", self.name))
    }

    /// The bridge's answer, the error envelope, and then nothing: the
    /// connection closed.
    fn failed(mut self, what: &str) -> Failure {
        let answer = self.answer();
        let failure: Failure = serde_json::from_value(answer.clone()).unwrap_or_else(|err| {
            panic!(
                "{}: {what} wasn't answered with the error envelope: {answer} ({err})",
                self.name
            )
        });
        assert!(!failure.error.is_empty(), "{}: {what}: {answer}", self.name);
        let mut rest = Vec::new();
        let _ = self.read.read_to_end(&mut rest);
        assert!(
            rest.is_empty(),
            "{}: {what}: more after the envelope: {:?}",
            self.name,
            String::from_utf8_lossy(&rest)
        );
        failure
    }

    /// As [`Call::failed`], refused.
    fn refused(self, what: &str) -> Failure {
        let name = self.name.clone();
        let failure = self.failed(what);
        assert_eq!(failure.why, Why::Refused, "{name}: {what}: {failure:?}");
        failure
    }
}
