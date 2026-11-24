//! A turn: the owner's new message in, the agent's answer out. It reads
//! the conversation, the agent's settings and its instructions through
//! the store, calls the model, writes the answer there, and holds
//! nothing after: any process could take the next turn.
//!
//! When the model calls tools, the turn goes in rounds: each of its
//! answers is kept, each call made and its answer kept (the calls of one
//! round at once), and the model asked again with them, until it answers
//! with no call. Each is an event of the conversation, written as it
//! happens, so a turn cut short leaves the conversation as far as it
//! got.
//!
//! One turn runs per conversation, whichever process runs it: it holds
//! the conversation's lease in the store ([`Store::take_turn`]), renewed
//! while it runs. A message that comes while one runs is written at once,
//! conditionally on its number like every event, and then does as the
//! agent's spec says (`[turns] busy`): joins the running turn's next
//! round (steer), waits for a turn of its own (queue), or ends the
//! running turn and takes one (interrupt). A turn whose lease lapsed
//! crashed: the next to take it finishes it first.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use link_agent_spec::Busy;
use link_secretive::Duplicate;
use tokio::sync::{mpsc, watch};

use crate::agent::{Agent, Agents, Thread};
use crate::config::Config;
use crate::event::{self, Event};
use crate::store::{Store, Took};
use crate::tools::{Answered, Kept, Listed, Toolbox};
use crate::{compact, limits, model, window};

mod appending;
mod caching;
mod choosing;
mod command;
mod compaction;
mod configuring;
mod execution;
mod goal;
mod heard;
mod idle;
mod interactions;
mod lease;
mod looking;
mod media;
mod recovery;
mod request;
mod requested;
mod rounds;
mod running;
mod skilling;
use rounds::call_meta;

use lease::{Holding, Renewing, holder};

/// A call's answer when the model calls anyway, told not to.
const TOO_MANY: &str = "Not called: too many calls in one turn.";

/// A call's answer when its turn crashed before the tool answered: the
/// model decides whether to call again, so nothing runs twice unseen.
pub const NOT_MADE_AGAIN: &str = "It was interrupted, and not made again.";

/// The most tokens an event could be counted as: its line's bytes, and
/// its parts' as a text document's (the most of any kind).
const EVENT_TOKENS_MAX: usize =
    (crate::store::EVENT_MAX + event::PARTS_MAX as u64 * event::PART_MAX) as usize;

/// How often a running turn looks for an interruption, and a waiting
/// message for the turn to end.
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// What came of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    /// Its turn's answer: the last round's.
    Answered(Box<Event>),
    /// It joined a turn already running, whose answer answers it too.
    Steered,
}

/// Runs `work`, file reads and writes, where it doesn't hold up the
/// other threads' turns: out of the async workers' way when there are
/// several.
fn off_runtime<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

/// Answers turns, for the agents in its [`Agents`].
pub struct Harness {
    interactions: Option<link_interactions::Store>,
    /// Whether its tools' sources check its work's bindings: its store is
    /// the machine's ([`link_interactions::Host::shared`]).
    sources_check_work: bool,
    agents: Agents,
    store: Arc<dyn Store>,
    client: reqwest::Client,
    /// For tools, on this machine: never through a proxy.
    tools_client: reqwest::Client,
    /// Each tool's calls, as it last listed them: derived, and listed
    /// again by a process without them, which offers what the last one
    /// kept ([`crate::tools::Lists`]) of a tool that fails to list them.
    listed: Listed,
    /// Each tool's session, kept between turns until idle.
    kept: Kept,
    /// The leases its turns hold now ([`lease::Holding`]).
    holding: Holding,
    /// Each conversation's last model request it recorded, so the next
    /// records only what changed ([`requested`]).
    requested: requested::Requested,
    /// What runtimes on this machine said of their models' windows.
    reported: limits::Reported,
    /// The conversation's budget, in tokens, in place of the model's
    /// (tests).
    budget: Option<usize>,
    /// How long a turn waits for a tool to start and list its tools.
    open_timeout: std::time::Duration,
    /// How long a tool's start may run in all, past the turn that began
    /// it ([`crate::tools::start_limit`]).
    start_limit: std::time::Duration,
    /// Where each turn's measures go (`LNK_MEASURES`); none: nowhere.
    measures: Option<std::path::PathBuf>,
    /// The conversations summarizing after an answer.
    compacting: Marks,
    /// The conversations whose goal is pursued, and each one's goal as
    /// last found ([`goal::Goals`]).
    goals: goal::Goals,
    /// The first models found off lately, which a fallback answers for.
    off: crate::fallback::Off,
    /// Each event a turn writes, as it's written, for whoever watches
    /// ([`Self::watch`]).
    written: tokio::sync::broadcast::Sender<Written>,
    /// How many prompts it's answering for clients, and ACP connections
    /// open ([`Self::answering`]).
    answering: Arc<std::sync::atomic::AtomicUsize>,
}

/// One prompt being answered for a client, or one ACP connection open:
/// counted while held.
pub struct Answering(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for Answering {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// An event a turn wrote, and whose conversation it's in.
#[derive(Clone, Debug)]
pub struct Written {
    pub agent: Agent,
    pub thread: Thread,
    pub event: Event,
}

impl Harness {
    /// What sums up a call of `agent`'s tool `name` in its title, as its
    /// tool last listed it ([`Listed::summary`]).
    pub fn summary(&self, agent: &Agent, name: &str) -> Option<(Vec<String>, serde_json::Value)> {
        self.listed.summary(agent, name)
    }

    pub fn new(agents: Agents, store: Arc<dyn Store>) -> anyhow::Result<Self> {
        link_fetch::install_crypto_provider();
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .read_timeout(std::time::Duration::from_secs(300))
            .build()?;
        // A call may wait for you to allow it (`lnk sandbox question list`): as
        // long as its spec says (`[tools] timeout`), held to it per call.
        let tools_client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            interactions: None,
            sources_check_work: false,
            agents,
            store,
            client,
            tools_client,
            listed: Listed::default(),
            kept: Kept::default(),
            requested: requested::Requested::default(),
            holding: Holding::default(),
            reported: limits::Reported::default(),
            budget: None,
            open_timeout: crate::tools::OPEN_TIMEOUT,
            start_limit: crate::tools::start_limit(),
            measures: crate::measures::from_env(),
            compacting: Marks::default(),
            goals: goal::Goals::default(),
            off: crate::fallback::Off::default(),
            written: tokio::sync::broadcast::channel(256).0,
            answering: Arc::default(),
        })
    }

    /// Counts a prompt being answered, or an ACP connection open, until
    /// the guard is dropped, so a stopping harness lets it finish
    /// ([`Self::answers`]).
    pub fn answering(&self) -> Answering {
        self.answering.fetch_add(1, Ordering::SeqCst);
        Answering(self.answering.clone())
    }

    /// How many prompts it's answering, and ACP connections open, now.
    pub fn answers(&self) -> usize {
        self.answering.load(Ordering::SeqCst)
    }

    /// `thread`'s goal pursued here until the guard is dropped; none
    /// while it already is: one pursuit at a time on a chat, of this
    /// harness's, never another's in the same process.
    pub(crate) fn pursuing<'a>(
        &'a self,
        agent: &'a Agent,
        thread: &'a Thread,
    ) -> Option<Released<'a>> {
        self.goals.pursuing.take(agent, thread)
    }

    /// Each event a turn writes from now on, of every agent and
    /// conversation: its answers, their calls, and the tools' answers.
    /// One that falls behind misses some, never holds a turn up.
    pub fn watch(&self) -> tokio::sync::broadcast::Receiver<Written> {
        self.written.subscribe()
    }

    /// Ends each tool's session no turn has used for as long as the
    /// agent's spec keeps one (`[tool.memory] keep`), or started as long
    /// ago as it lives (`max_age`), and its server's process with it.
    pub async fn end_idle_tools(&self) {
        self.kept.end_idle(None).await;
    }

    /// [`Self::end_idle_tools`], with `idle` and `age` in place of what
    /// the spec says (tests).
    pub async fn end_idle_tools_within(&self, idle: std::time::Duration, age: std::time::Duration) {
        self.kept.end_idle(Some((idle, age))).await;
    }

    /// How many tool sessions are kept (tests).
    pub fn tool_sessions(&self) -> usize {
        self.kept.len()
    }

    /// The agents this process answers for with ID `id`, one per owner.
    pub fn agents_with_id(&self, id: &str) -> Vec<Agent> {
        self.agents.with_id(id).into_iter().cloned().collect()
    }

    /// Asks `thread`'s running turn to end, as its answer is: what it
    /// wrote kept, marked interrupted. Nothing when none runs.
    pub fn interrupt(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<()> {
        match off_runtime(|| self.store.turn_running(agent, thread)) {
            true => off_runtime(|| self.store.interrupt_turn(agent, thread)),
            false => Ok(()),
        }
    }

    /// The conversations of `agent`.
    pub fn threads(&self, agent: &Agent) -> anyhow::Result<Vec<Thread>> {
        off_runtime(|| self.store.threads(agent))
    }

    /// Every event of `thread`, as kept.
    pub fn events(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Vec<Event>> {
        off_runtime(|| self.store.read(agent, thread))
    }

    /// The native export path shared by live ASP and stopped state capture.
    pub fn exported_state(
        &self,
        agent: &Agent,
        kinds: &[link_agent_state_protocol::Kind],
    ) -> anyhow::Result<link_agent_state_protocol::State> {
        off_runtime(|| crate::acp::exported_state(self.store.as_ref(), agent, kinds))
    }

    /// Portable session updates, including its stored media and thinking.
    pub fn session_updates(
        &self,
        agent: &Agent,
        thread: &Thread,
    ) -> anyhow::Result<Vec<link_agent_state_protocol::acp::schema::v1::SessionUpdate>> {
        off_runtime(|| {
            let events = self.store.read(agent, thread)?;
            crate::acp::history::archived(&events, |part| {
                self.store.part(agent, thread, &part.file)
            })
        })
    }

    /// Writes `updates` as `thread`'s conversation, one brought from
    /// another harness. Where `thread` holds some already, only what
    /// follows them, when the conversation brought goes on from them;
    /// false, and nothing written, when it doesn't.
    pub fn bring(
        &self,
        agent: &Agent,
        thread: &Thread,
        updates: &[link_agent_state_protocol::acp::schema::v1::SessionUpdate],
    ) -> anyhow::Result<bool> {
        off_runtime(|| {
            let mut before = self.store.read(agent, thread)?;
            let carried = crate::acp::history::carried(updates);
            let Some(new) = crate::acp::history::goes_on(&before, &carried) else {
                return Ok(false);
            };
            for (mut event, media) in carried.into_iter().skip(new) {
                let (parts, left) = self.keep(agent, thread, media);
                anyhow::ensure!(
                    left.is_none(),
                    "an imported part could not be kept: {}",
                    left.unwrap_or_default()
                );
                event.parts = parts;
                event.seq = before.len() as u64 + 1;
                self.store.append(agent, thread, &event, &before)?;
                before.push(event);
            }
            Ok(true)
        })
    }

    /// The folder `agent`'s tools work in: its Home.
    pub fn workspace(&self, agent: &Agent) -> Option<std::path::PathBuf> {
        self.agents.workspace(agent)
    }

    /// The agents this process answers for named `name`, one per owner.
    pub fn agents_named(&self, name: &str) -> Vec<Agent> {
        self.agents.named(name).into_iter().cloned().collect()
    }

    /// Whether this process answers for `agent`.
    pub fn answers_for(&self, agent: &Agent) -> bool {
        self.agents.paths(agent).is_ok()
    }

    /// `agent`'s settings, as its store has them.
    pub fn config(&self, agent: &Agent) -> anyhow::Result<Config> {
        off_runtime(|| self.store.config(agent))
    }

    /// `agent`'s turn on `thread` for `message`, as [`Self::ask`]: its
    /// answer, or an error for a message that joined a running turn.
    pub async fn turn(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: &str,
        pieces: mpsc::Sender<String>,
    ) -> anyhow::Result<Event> {
        match self
            .ask(agent, thread, Event::user(message), pieces, None)
            .await?
        {
            Asked::Answered(answer) => Ok(*answer),
            Asked::Steered => anyhow::bail!("it joined the turn already running"),
        }
    }

    /// Narrows `agent`'s conversation `thread` to `tools`, of those it
    /// may use, from its next turn on.
    pub fn narrow_tools(
        &self,
        agent: &Agent,
        thread: &Thread,
        tools: &[String],
    ) -> anyhow::Result<()> {
        off_runtime(|| self.store.narrow_tools(agent, thread, tools))
    }

    /// `message` for `agent` on `thread`: kept, and answered in a turn,
    /// each piece of the answer sent to `pieces` while it's written. With
    /// tools, each call and its answer kept too, in rounds. While another
    /// turn runs, as the agent's spec says (`[turns] busy`): steered into
    /// it ([`Asked::Steered`]), or answered in a turn of its own once
    /// that one ends, or ends. `begun` is told once it's sure to be
    /// answered here, not steered.
    pub async fn ask(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: Event,
        pieces: mpsc::Sender<String>,
        begun: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> anyhow::Result<Asked> {
        self.ask_told(agent, thread, message, pieces, begun, None, None)
            .await
    }

    /// [`Self::ask`], and `told` its turn's answer as soon as it's whole,
    /// when the turn goes on after it: it summarizes what left the
    /// window. What it returns comes later, once the turn ends. `asked`,
    /// what its sender asked a message sent while another turn runs to
    /// do, in place of the spec's `[turns] busy`: one of [`Self::takes`].
    #[expect(
        clippy::too_many_arguments,
        reason = "the message and each connection its sender hears the turn on"
    )]
    #[expect(
        clippy::disallowed_methods,
        reason = "the turn's own period of looking"
    )]
    pub async fn ask_told(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: Event,
        pieces: mpsc::Sender<String>,
        mut begun: Option<tokio::sync::oneshot::Sender<()>>,
        told: Option<tokio::sync::oneshot::Sender<Event>>,
        mut asked: Option<Busy>,
    ) -> anyhow::Result<Asked> {
        if Self::is_quiet(thread, &message) {
            return self.compact_quiet(agent, thread).await;
        }
        let config = self.config(agent)?;
        let mut message = message;
        if let Some(text) = self.owner_input(agent, thread, &message)? {
            let _ = pieces.send(text.clone()).await;
            return Ok(Asked::Answered(Box::new(Event {
                role: event::COMMAND.into(),
                text,
                ..Event::default()
            })));
        }
        let commands = crate::commands::list(&config);
        let skills = config.opinions().commands.and_then(|c| c.skills) == Some(true);
        // A skill of its name, as its precedence says: a turn of its own.
        if let Some(asked) = self.skill_asked(agent, &message, skills, &commands)? {
            message.text = asked.text;
            message.evidence = Some(asked.evidence.into());
        }
        match crate::commands::asked(&message, &commands) {
            // A turn of its own, asked with the skill.
            Some(run) if run.command.does == crate::commands::Does::Learn => {
                message.text = crate::commands::learn(&run.arguments);
            }
            // A goal set is a turn of its own; shown or cleared, a command.
            Some(run)
                if run.command.does == crate::commands::Does::Goal
                    && !matches!(run.arguments.as_str(), "" | "--clear" | "clear") =>
            {
                message.text = crate::goal::set(&run.arguments);
            }
            // Given a message, that's the one it's for.
            Some(run)
                if matches!(run.command.does, crate::commands::Does::Busy(_))
                    && !run.arguments.is_empty() =>
            {
                if let crate::commands::Does::Busy(busy) = run.command.does {
                    asked = Some(busy);
                }
                message.text = run.arguments;
            }
            Some(run) => return self.command(agent, thread, &message, run, &pieces).await,
            // The owner's message, after a `/chat:queue` or `/chat:steer`
            // alone: as that asked.
            None if asked.is_none() && message.role == event::USER && message.from.is_none() => {
                let events = off_runtime(|| self.store.last_events(agent, thread, 8))?;
                asked = crate::commands::busy_asked(&events);
            }
            None => {}
        }
        let told = std::sync::Mutex::new(told);
        let busy = asked.unwrap_or_else(|| busy(&config));
        if let Some(text) = self.admit_busy_input(agent, thread, &message, busy)? {
            let _ = pieces.send(text.clone()).await;
            return Ok(Asked::Answered(Box::new(Event {
                role: event::COMMAND.into(),
                text,
                ..Event::default()
            })));
        }
        let holder = holder()?;
        // When it came, its connection and who's speaking, at its head as
        // the spec says: kept with it, so it reads the same at every turn.
        if message.role == event::USER && message.now.is_none() {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            message.now = crate::prompt::now(&config, thread, message.from.as_deref(), secs);
        }
        let mut said: Option<Event> = None;
        let mut interrupted = false;
        // It waits for a turn of its own.
        let mut own = false;
        loop {
            let took = off_runtime(|| self.store.take_turn(agent, thread, &holder))?;
            if took != Took::Held {
                // Written while another turn ran, which read it and
                // answered it: nothing more to ask.
                if said.is_some()
                    && let Some(answer) = off_runtime(|| self.answered(agent, thread))?
                {
                    let _ = off_runtime(|| self.store.end_turn(agent, thread, &holder));
                    if begun.take().is_some() {
                        return Ok(Asked::Steered);
                    }
                    // Its reply began: the answer that answered it.
                    let _ = pieces.send(answer.text.clone()).await;
                    return Ok(Asked::Answered(Box::new(answer)));
                }
                // Acknowledgement means the original input is durable, even if admission
                // is fenced or this process disappears before its first model request.
                if said.is_none() {
                    match off_runtime(|| -> anyhow::Result<bool> {
                        let (event, inserted) =
                            self.retain_admitted_message(agent, thread, &message)?;
                        Ok(!inserted
                            && self
                                .answered(agent, thread)?
                                .is_some_and(|answer| answer.seen >= event.seq))
                    }) {
                        Ok(true) => {
                            let _ = off_runtime(|| self.store.end_turn(agent, thread, &holder));
                            return Ok(Asked::Steered);
                        }
                        Ok(_) => {}
                        Err(err) => {
                            let _ = off_runtime(|| self.store.end_turn(agent, thread, &holder));
                            return Err(err);
                        }
                    }
                }
                if let Some(begun) = begun.take() {
                    let _ = begun.send(());
                }
                let answer = self
                    .run(agent, thread, &holder, None, (&pieces, &told), Work::Answer)
                    .await?;
                return Ok(Asked::Answered(Box::new(answer)));
            }
            // Written at once, whatever happens next.
            if said.is_none() {
                let (event, _) =
                    off_runtime(|| self.retain_admitted_message(agent, thread, &message))?;
                said = Some(event);
            }
            // A turn summarizing after its answer has given it: this
            // waits for a turn of its own, even once that ends.
            own |= self.compacting.holds(agent, thread);
            match busy {
                // The running turn reads it at its next round, or, done,
                // finds it after it ends.
                Busy::Steer if !own && off_runtime(|| self.store.turn_running(agent, thread)) => {
                    return Ok(Asked::Steered);
                }
                Busy::Interrupt if !interrupted => {
                    off_runtime(|| self.store.interrupt_turn(agent, thread))?;
                    interrupted = true;
                }
                _ => {}
            }
            // It waits for a turn of its own: sure to be answered here.
            if (busy != Busy::Steer || own)
                && let Some(begun) = begun.take()
            {
                let _ = begun.send(());
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    }

    /// Finishes each turn of `agent`'s that crashed: its unanswered
    /// calls answered as not made again, and the model asked to go on
    /// while a message is still to answer, its answer kept in the
    /// conversation and for the agent's bridge to send to the chat
    /// ([`Self::recovered`]). A turn that had answered before it crashed
    /// only has its lease let go.
    pub async fn recover(&self, agent: &Agent) {
        let threads = off_runtime(|| self.store.lapsed_turns(agent));
        for thread in threads {
            let Ok(holder) = holder() else {
                continue;
            };
            let took = off_runtime(|| self.store.take_turn(agent, &thread, &holder));
            // Another took it, or it ended meanwhile; or it had
            // answered, and only its lease was left.
            let answered = match took {
                Ok(Took::Lapsed) => {
                    !matches!(off_runtime(|| self.answered(agent, &thread)), Ok(None))
                }
                Ok(Took::Took) => true,
                _ => continue,
            };
            if answered {
                if let Some(store) = &self.interactions {
                    let _ = off_runtime(|| store.recover_work(agent.id(), thread.name()));
                }
                let _ = off_runtime(|| self.store.end_turn(agent, &thread, &holder));
                continue;
            }
            let (pieces, _) = mpsc::channel(64);
            let told = std::sync::Mutex::new(None);
            match self
                .run(
                    agent,
                    &thread,
                    &holder,
                    None,
                    (&pieces, &told),
                    Work::Answer,
                )
                .await
            {
                Ok(answer) => {
                    if let Err(err) =
                        off_runtime(|| self.store.set_recovered(agent, &thread, answer.seq))
                    {
                        crate::warn!(crate::said::in_chat(agent, &thread) => "keeping its answer: {err:#}");
                    }
                }
                Err(err) => {
                    crate::warn!(crate::said::in_chat(agent, &thread) => "finishing its turn: {err:#}")
                }
            }
        }
        self.recover_inputs(agent).await;
    }

    /// The answers of `agent`'s turns finished after a crash that nobody
    /// has taken, by their conversation's name, each taken once: what its
    /// bridge sends to the chat. Any turn whose lease lapsed since is
    /// finished first, in a task of its own: a caller that goes doesn't
    /// leave it half done.
    pub async fn recovered(self: &Arc<Self>, agent: &Agent) -> Vec<(String, String)> {
        self.recovering(agent).await;
        let taken = off_runtime(|| self.store.take_recovered(agent));
        self.texts(agent, taken)
            .into_iter()
            .map(|(_, thread, text)| (thread, text))
            .collect()
    }

    /// [`Self::recovered`] in two steps: each answer with its id, kept
    /// until [`Self::delivered`] is told it was sent.
    pub async fn recovered_kept(self: &Arc<Self>, agent: &Agent) -> Vec<(String, String, String)> {
        self.recovering(agent).await;
        let kept = off_runtime(|| self.store.recovered(agent));
        self.texts(agent, kept)
    }

    /// Keeps the answer `seq` of `agent`'s conversation `thread`, whose
    /// caller went before it was told it (its bridge stopped), for the
    /// bridge to send, as a crashed turn's is ([`Self::recovered`]).
    pub fn untaken(&self, agent: &Agent, thread: &Thread, seq: u64) {
        if let Err(err) = self.store.set_recovered(agent, thread, seq) {
            crate::warn!(crate::said::in_chat(agent, thread) => "keeping its answer: {err:#}");
        }
    }

    /// Lets go of each answer of [`Self::recovered_kept`] whose id is in
    /// `ids`: it was sent. An id that isn't one is passed over.
    pub fn delivered(&self, agent: &Agent, ids: &[String]) {
        for id in ids {
            let Some((thread, seq)) = id
                .rsplit_once('/')
                .and_then(|(thread, seq)| Some((Thread::new(thread).ok()?, seq.parse().ok()?)))
            else {
                continue;
            };
            if let Err(err) = off_runtime(|| self.store.drop_recovered(agent, &thread, seq)) {
                crate::warn!(crate::said::in_chat(agent, &thread) => "letting its answer go: {err:#}");
            }
        }
    }

    /// [`Self::recover`], in a task of its own: it runs to its end even
    /// when the caller goes.
    async fn recovering(self: &Arc<Self>, agent: &Agent) {
        let (harness, agent) = (self.clone(), agent.clone());
        let _ = tokio::spawn(async move { harness.recover(&agent).await }).await;
    }

    /// Each answer of `kept` that is one: its id (`<thread>/<number>`),
    /// its conversation's name, and its text.
    fn texts(&self, agent: &Agent, kept: Vec<(Thread, u64)>) -> Vec<(String, String, String)> {
        let mut answers = Vec::new();
        for (thread, seq) in kept {
            let read = off_runtime(|| self.store.read_from(agent, &thread, seq));
            let Some(answer) = read
                .ok()
                .and_then(|events| events.into_iter().find(|e| e.seq == seq))
                .filter(|e| e.is_answer() && !e.text.trim().is_empty())
            else {
                continue;
            };
            let name = thread.name().to_string();
            answers.push((format!("{name}/{seq}"), name, answer.text));
        }
        answers
    }

    /// The conversation's last answer, when it answered every message
    /// before it; none while one is still to answer.
    fn answered(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Option<Event>> {
        let events = self.tail(agent, thread)?;
        Ok(match waiting(&events) {
            true => None,
            false => events.into_iter().rev().find(finished),
        })
    }
}

/// How a turn compacts its conversation (`[window] compact_at`).
struct Compact {
    /// The longest summary, in tokens: the window's room past the
    /// conversation's share, at most the longest answer.
    room: u32,
    /// The conversation's whole share, the room included: the most the
    /// window as last sent may hold to be answered with before what
    /// leaves it is summarized.
    full: usize,
    /// The model saves to its notes first (`[window] save_notes`, and a
    /// tool to save with).
    save: bool,
}

/// What was read of a conversation: its events from the one numbered
/// `from` to its last, as they were then.
pub(super) struct Read {
    events: Vec<Event>,
    from: u64,
}

impl Read {
    /// Its events numbered `first` and on, when it holds them all.
    fn from(&self, first: u64) -> Option<Vec<Event>> {
        (self.from <= first.max(1)).then(|| {
            self.events
                .iter()
                .filter(|e| e.seq >= first)
                .cloned()
                .collect()
        })
    }
}

/// What a turn does with its lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Work {
    /// Answers what it's asked: a turn.
    Answer,
    /// Summarizes what's before the owner's last message, now
    /// (`/chat:compact`), and answers what it did.
    Compact,
}

/// What [`Harness::prepare`] makes ready for a turn: what it's asked
/// with, its conversation's events from where the window's walk goes
/// on, the mark it goes on from and the one kept, and the budget it's
/// walked with.
pub(super) struct Prepared<'a> {
    ask: Ask<'a>,
    events: Vec<Event>,
    mark: Option<window::Mark>,
    kept: window::Mark,
    budget: usize,
}

/// A turn's own: whose, where, where its pieces go, whether it's asked
/// to end, the last event it was asked for, and its measures.
struct Turn<'a> {
    admit_queued: bool,
    binding: Option<&'a link_interactions::runtime::Binding>,
    attempt: &'a str,
    work: Work,
    agent: &'a Agent,
    thread: &'a Thread,
    pieces: &'a mpsc::Sender<String>,
    cancelled: watch::Receiver<bool>,
    /// The number of the last event when it began: its message's, or
    /// what it found to answer. Any after came while it ran.
    own: &'a AtomicU64,
    measuring: Option<&'a crate::measures::Measuring>,
    /// Told the answer once it's whole, when the turn goes on after it
    /// (it summarizes): whoever waits needn't.
    told: &'a std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Event>>>,
    /// Its answer was whole before it summarized, and measured then.
    whole: std::sync::atomic::AtomicBool,
    written: &'a tokio::sync::broadcast::Sender<Written>,
    /// What its requests asked of the provider's cache, once chosen.
    cache: std::sync::OnceLock<Option<String>>,
}

impl Turn<'_> {
    /// Tells whoever waits the turn's answer, once.
    fn tell(&self, answer: &Event) {
        let told = self.told.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(told) = told {
            let _ = told.send(answer.clone());
        }
    }

    /// It wrote `event`, of its own: its measures count up to it.
    fn wrote(&self, event: &Event) {
        if let Some(measuring) = self.measuring {
            measuring.wrote(event.seq);
        }
        let _ = self.written.send(Written {
            agent: self.agent.clone(),
            thread: self.thread.clone(),
            event: event.clone(),
        });
    }
}

/// What a message sent while a turn runs may ask to do: wait or
/// interrupt it, always; steer it, where the spec steers, since a
/// running turn reads what joins it only then.
pub fn takes(config: &Config) -> Vec<Busy> {
    let mut takes = vec![Busy::Queue, Busy::Interrupt];
    if busy(config) == Busy::Steer {
        takes.insert(0, Busy::Steer);
    }
    takes
}

/// What a message waiting for a turn does: as the agent's spec says
/// (`[turns] busy`), else as Link's does.
fn busy(config: &Config) -> Busy {
    let of = |spec: link_agent_spec::Spec| spec.turns.and_then(|t| t.busy);
    of(config.opinions())
        .or_else(|| of(link_agent_spec::link()))
        .unwrap_or(Busy::Steer)
}

/// Whether `event` is an answer that ended its turn: the agent's, with
/// no call, not cut short.
fn finished(event: &Event) -> bool {
    event.role == event::ASSISTANT && event.calls.is_empty() && !event.interrupted
}

/// Whether a message of `events` is still to answer: one after the last
/// the model read before an answer that ended its turn (an answer that
/// doesn't say, kept by an older harness, read every event before it).
fn waiting(events: &[Event]) -> bool {
    let read = events
        .iter()
        .rev()
        .find(|e| finished(e))
        .map_or(0, |answer| match answer.seen {
            0 => answer.seq.saturating_sub(1),
            seen => seen,
        });
    events.iter().any(|e| e.role == event::USER && e.seq > read)
}

/// What a turn asks the model with, every round.
struct Ask<'a> {
    config: &'a Config,
    instructions: &'a str,
    /// Where each part of the instructions is, by name.
    spans: &'a [link_model_contract::context::Span],
    /// The longest answer, in tokens.
    answer: u32,
    /// The window, in tokens.
    window: u32,
    /// What the model costs, when it's known.
    price: Option<link_model_contract::Price>,
    /// Where its price jumps for a long prompt.
    above: Vec<link_model_contract::Jump>,
    /// How it thinks.
    thinking: model::Think,
    /// Asked of the provider's faster service (`/model:set --fast`).
    fast: bool,
    /// The most rounds of calls in one turn: the model is asked once
    /// more after them, told to call nothing (`[turns] rounds`).
    rounds: usize,
    /// The most calls one answer makes; the model's others are dropped,
    /// never kept or made, so a round costs at most so many events and
    /// calls (`[turns] calls`).
    calls: usize,
    /// A message come while it runs joins it (`[turns] busy`).
    steer: bool,
    /// What leaves the window is summarized first; none, it just leaves.
    compact: Option<Compact>,
    /// Its fallback model, if it has one.
    second: Option<crate::fallback::Second>,
    /// How long the provider's cache is asked to keep its prompt; none,
    /// as long as it keeps one unasked ([`caching::Caching`]).
    cache: Option<String>,
}

/// Where the window as last sent starts in `events`, when the window
/// starts with `head` (the summary that covers the events before number
/// `covered`), or with the first event without one; none when those
/// events weren't read, or it holds more than `full` tokens, its head
/// included.
fn sent(events: &[Event], head: Option<&Event>, covered: u64, full: usize) -> Option<usize> {
    let first = events.first()?;
    let sent = match head {
        Some(_) if first.seq <= covered => events.iter().position(|e| e.seq >= covered)?,
        None if first.seq == 1 => 0,
        _ => return None,
    };
    let held = head.map_or(0, |h| window::text_tokens(&h.text) + 4)
        + events[sent..].iter().map(window::tokens).sum::<usize>();
    (held <= full).then_some(sent)
}

/// The turn's instructions: as composed again, else as it began.
fn system<'b>(recomposed: &'b Option<String>, ask: &Ask<'b>) -> &'b str {
    recomposed.as_deref().unwrap_or(ask.instructions)
}

/// Conversations marked, by their agent's owner and ID and their
/// thread, in one harness: those whose turn summarizes after its answer
/// until its lease is let go (a message that comes meanwhile waits for a
/// turn of its own, since the answer it would join is given), and those
/// whose goal is pursued.
#[derive(Default)]
struct Marks(std::sync::Mutex<std::collections::HashSet<(String, String, Thread)>>);

impl Marks {
    fn key(agent: &Agent, thread: &Thread) -> (String, String, Thread) {
        (agent.owner().into(), agent.id().into(), thread.clone())
    }

    fn set(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashSet<(String, String, Thread)>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Marked for `thread`, until [`Self::release`].
    fn mark(&self, agent: &Agent, thread: &Thread) {
        self.set().insert(Self::key(agent, thread));
    }

    /// Marked for `thread` until the guard is dropped; none when it
    /// already was.
    fn take<'a>(&'a self, agent: &'a Agent, thread: &'a Thread) -> Option<Released<'a>> {
        let taken = self.set().insert(Self::key(agent, thread));
        // Made only when taken: dropped, it lets the mark go.
        taken.then(|| Released(self, agent, thread))
    }

    fn release(&self, agent: &Agent, thread: &Thread) {
        self.set().remove(&Self::key(agent, thread));
    }

    fn holds(&self, agent: &Agent, thread: &Thread) -> bool {
        self.set().contains(&Self::key(agent, thread))
    }
}

/// Releases its conversation's [`Marks`] mark when dropped.
pub(crate) struct Released<'a>(&'a Marks, &'a Agent, &'a Thread);

impl Drop for Released<'_> {
    fn drop(&mut self) {
        self.0.release(self.1, self.2);
    }
}

/// The number of the event after `events`.
fn next_seq(events: &[Event]) -> u64 {
    events.last().map_or(1, |e| e.seq.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chat's goal is pursued once at a time by a harness, and a
    /// pursuit of another harness's in the same process never stops it:
    /// that one has agents and a store of its own.
    #[test]
    fn a_goal_is_pursued_once_at_a_time_by_its_harness() {
        let harness = || {
            let agents = Agents::default();
            Harness::new(agents.clone(), Arc::new(crate::store::Files::new(agents))).unwrap()
        };
        let (one, other) = (harness(), harness());
        let agent = Agent::new("main").unwrap();
        let thread = Thread::new("telegram:42").unwrap();
        let pursued = one.pursuing(&agent, &thread).expect("pursued");
        assert!(one.pursuing(&agent, &thread).is_none(), "pursued twice");
        assert!(
            other.pursuing(&agent, &thread).is_some(),
            "another's held it"
        );
        drop(pursued);
        assert!(one.pursuing(&agent, &thread).is_some(), "never let go");
    }
}
