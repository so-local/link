//! The way in: messages for the agent from others, taken as their proofs
//! say, handed to the harness, and each answer delivered where it's owed.

use super::*;
use link_format::WithinExt;

link_files::specification!(Owed, include_str!("way_in/owed.schema.toml"));

/// Each owed answer's way, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Owed>()
}

/// `~/.lnk/safe/agents/<agent>/received/<teammate>.json`: how many
/// messages a teammate's had taken today, naming its schema. The count is
/// the connections contract's type, so its format is declared here.
static RECEIVED: std::sync::LazyLock<link_files::schema::Format> = std::sync::LazyLock::new(|| {
    link_files::schema::Format::shipped(include_str!("way_in/received.schema.toml"))
});

/// Each teammate's count of messages taken, which an upgrade converts.
pub(crate) fn saved_received() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::shipped::<Received>(&RECEIVED)
}

impl WayIn {
    pub(crate) fn sent_in_schedule(
        &self,
        thread: &str,
        from: &str,
        connection: &str,
    ) -> anyhow::Result<()> {
        if !from.starts_with("schedule:") {
            return Ok(());
        }
        let mut owed = self.owed(thread);
        if owed.from == from
            && matches!(&owed.answer, link_connection_contract::AnswerTo::Connection(target) if connection_name(target) == connection_name(connection))
        {
            owed.sent = true;
            self.owe(thread, &owed)?;
        }
        Ok(())
    }

    /// Resolve a platform through its module's owner-recipient contract.
    pub(crate) fn send_connection(&self, connection: &str, text: &str) -> anyhow::Result<()> {
        if [crate::chat::TERMINAL, crate::chat::CONNECTION].contains(&connection) {
            return self.held.keep(crate::chat::CONNECTION, text);
        }
        let (program, request) = self
            .connections
            .get(connection)
            .within("that connection is not connected")?;
        let chats = super::asking::owner_chats(program, request)?;
        anyhow::ensure!(
            !chats.is_empty(),
            "that connection has no owner's direct chat"
        );
        for chat in chats {
            self.send_owner(&format!("{connection}:{chat}"), text)?;
        }
        Ok(())
    }

    /// Explicit owner messages must name a supported destination; unlike
    /// turn completion, an unknown thread must never look delivered.
    pub(crate) fn send_owner(&self, thread: &str, text: &str) -> anyhow::Result<()> {
        let (connection, chat) = thread
            .split_once(':')
            .within("a connection thread is required")?;
        anyhow::ensure!(!chat.is_empty(), "a connection chat is required");
        anyhow::ensure!(
            connection == crate::chat::CONNECTION || self.connections.contains_key(connection),
            "the connection is not connected"
        );
        self.deliver(thread, text)
    }

    /// The agent it takes messages for.
    pub(crate) fn agent(&self) -> &str {
        &self.endpoint.agent
    }

    pub fn new(agent: &Settings, plan: &Plan) -> Self {
        let mut connections = BTreeMap::new();
        for connection in &plan.connections {
            if let (Some(request), Some(program)) = (
                request(agent, plan, connection),
                link_core::find_module(connection),
            ) {
                connections.insert(connection.clone(), (program, request));
            }
        }
        let endpoint = link_connection_contract::Bridge {
            agent: agent.name.clone(),
            identity: agent.identity(),
            endpoint: plan.endpoint.clone(),
            key: plan.key.duplicate(),
            proof: plan.proof,
            ..Default::default()
        };
        let interactions = link_interactions::Store::machine().ok();
        if let Some(store) = &interactions
            && let Err(err) = store.register_agent_route(&agent.key(), &agent.name)
        {
            link_format::daemon::warn!("registering the owner's work route: {err:#}");
        }
        // Its spec, read once: its terms and its scheduler's caps.
        let spec = agent.spec().ok();
        let scheduler = spec.as_ref().map(|spec| spec.spec.tool("scheduler"));
        Self {
            interaction_agent: agent.key(),
            interactions,
            terms: link_connection_contract::Terms {
                name: agent.name.clone(),
                me: endpoint.identity(),
                tools: plan.tools,
                strangers: spec
                    .as_ref()
                    .map(|spec| spec.spec.strangers())
                    .unwrap_or_default(),
            },
            keys: Arc::new(link_sandbox_binding::Keyring::here()),
            owed: settings::config_of(&agent.name).join("owed"),
            endpoint,
            connections,
            teams: settings::teams_dir(),
            held: crate::chat::Held::of(&agent.name),
            received: settings::config_of(&agent.name).join("received"),
            per_day: scheduler.as_ref().and_then(|tool| tool.per_day),
            hops: scheduler.as_ref().and_then(|tool| tool.hops),
            chains: link_interactions::chains::Chains::machine().ok(),
            counting: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// Counts a message from teammate `sender` (its ID) toward today's,
    /// refused past the agent's own `per_day`: a teammate's messages,
    /// its answers to this one's included, are taken at most that many
    /// times a day, however many its own cap lets it send.
    fn receive(&self, sender: &str) -> anyhow::Result<()> {
        self.count(sender, |received| {
            received
                .receive(received.day, self.per_day, true)
                .map_err(refused)
        })
    }

    /// Counts another agent's `firing` toward its conversation's chain,
    /// refused past the scheduler's `hops`: its own hops, or, on a chat,
    /// the agents' messages since a person last spoke there. A
    /// conversation only agents write in (`agent-<id>`, `team-<id>`) has
    /// no person to end its chain, so its own hops, checked as each
    /// message is sent, bound it alone.
    fn chain(&self, firing: &link_connection_contract::Firing) -> anyhow::Result<()> {
        let hops = self
            .hops
            .within("its spec sets no [tool.scheduler] hops: another agent's message isn't taken")
            .map_err(refused)?;
        if firing.hops > hops {
            return Err(refused(anyhow::anyhow!(
                "{} messages in a chain, past the cap of {hops} ([tool.scheduler] hops): not taken",
                firing.hops
            )));
        }
        if !is_chat(&firing.thread) {
            return Ok(());
        }
        let chains = self
            .chains
            .as_ref()
            .within("the machine's interactions' folder: another agent's message isn't taken")?;
        match chains.agent_said(&self.terms.me.agent, &firing.thread, hops) {
            Err(err) if err.is::<link_interactions::chains::Capped>() => Err(refused(err)),
            said => said.map(|_| ()),
        }
    }

    /// Gives back a chain's count for a message the harness never took.
    fn unchain(&self, thread: &str) {
        if is_chat(thread)
            && let Some(chains) = &self.chains
            && let Err(err) = chains.unsay(&self.terms.me.agent, thread)
        {
            link_format::daemon::warn!("giving back a conversation's count: {err:#}");
        }
    }

    /// Gives back a receipt for a message the harness never took.
    fn unreceive(&self, sender: &str) -> anyhow::Result<()> {
        self.count(sender, |received| {
            received.receive(received.day, self.per_day, false)
        })
    }

    /// Changes teammate `sender`'s count for today under the lock, read
    /// from its file (none is a count of none; one unreadable is
    /// refused, never taken as none) and written back.
    fn count(
        &self,
        sender: &str,
        change: impl FnOnce(&mut Received) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        link_machine::machine::check_id(sender)?;
        let _one = self.counting.lock().unwrap_or_else(|e| e.into_inner());
        link_files::create_private_dir(&self.received)?;
        let path = self.received.join(format!("{sender}.json"));
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() / 86_400);
        let mut received: Received = match std::fs::read(&path) {
            Ok(bytes) => {
                RECEIVED
                    .read_json(&bytes)
                    .within(format!("reading {}", path.display()))?
                    .spec
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Received::default(),
            Err(e) => return Err(e).within(format!("reading {}", path.display())),
        };
        if received.day != today {
            received = Received {
                day: today,
                taken: 0,
            };
        }
        change(&mut received)?;
        link_files::write_private(&path, &RECEIVED.print_json(&received)?)
            .within(format!("writing {}", path.display()))
    }

    /// What reaches the harness's endpoint.
    pub fn endpoint(&self) -> &link_connection_contract::Bridge {
        &self.endpoint
    }

    /// The teams' files, as they say now ([`link_connection_contract::Terms`]
    /// reads them for a message from another agent).
    fn teams(&self) -> Vec<link_agent_spec::Team> {
        settings::teams_in(&self.teams).unwrap_or_default()
    }

    /// Takes messages at the way in, at `path`, on the server every bridge
    /// runs on ([`link_sandbox_binding::serve`], within [`WAY_IN`]), each
    /// on a thread of its own, until `stop` says (it's then woken by a
    /// connection).
    pub fn listen(self, path: &std::path::Path, stop: &AtomicBool) -> anyhow::Result<()> {
        self.listen_within(path, stop, &WAY_IN)
    }

    /// [`WayIn::listen`], within `limits`: a test's, briefer.
    pub(crate) fn listen_within(
        self,
        path: &std::path::Path,
        stop: &AtomicBool,
        limits: &link_sandbox_binding::Limits,
    ) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            link_files::create_private_dir(dir)?;
        }
        let listener = link_sandbox_bridge::socket::listen_private(path)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let keys = self.keys.clone();
        let way_in = self;
        link_sandbox_binding::serve(
            listener,
            keys,
            limits,
            stop,
            move |caller, firing: link_connection_contract::Firing| {
                way_in
                    .terms
                    .caller(caller, &firing, &way_in.teams())
                    .map_err(refused)?;
                let answer = runtime.block_on(way_in.fire(firing))?;
                Ok(link_connection_contract::Fired { answer })
            },
        )
    }

    /// Asks the harness `firing`, as from its sender, and sends the
    /// answer where it goes: a subagent's back to its parent's
    /// conversation, as a message from it, one hop further.
    pub(crate) fn fire(
        &self,
        firing: link_connection_contract::Firing,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>
    {
        Box::pin(async move {
            let taken = self.take(firing).await?;
            self.finish(taken).await
        })
    }

    /// Hands the harness `firing`, as from its sender, once it passes
    /// the way in's checks: done when the harness has taken it, its turn
    /// still running ([`Self::finish`] reads its answer and sends it).
    pub(crate) async fn take(
        &self,
        firing: link_connection_contract::Firing,
    ) -> anyhow::Result<Taken> {
        // On the way in's terms: a stranger's on its own conversation
        // with the strangers' tools, a teammate's from a teammate.
        let firing = self.terms.firing(firing, &self.teams()).map_err(refused)?;
        // Another agent's message counts toward the day's as it's taken,
        // and not if the harness never took it.
        // And toward its conversation's chain since a person last spoke,
        // and never past the hops a chain may have.
        let teammate = match firing.from.split_once(':') {
            Some(("agent", sender)) => {
                self.chain(&firing)?;
                if let Err(e) = self.receive(sender) {
                    self.unchain(&firing.thread);
                    return Err(e);
                }
                Some(sender.to_owned())
            }
            _ => None,
        };
        let thread = firing.thread.clone();
        match (self.hand(firing).await, teammate) {
            (Err(e), Some(sender)) => {
                self.unchain(&thread);
                Err(match self.unreceive(&sender) {
                    Ok(()) => e,
                    Err(back) => e.context(format!("and its count wasn't given back: {back:#}")),
                })
            }
            (taken, _) => taken,
        }
    }

    /// Hands the harness `firing`, checked, and owes its answer where it
    /// goes.
    async fn hand(&self, firing: link_connection_contract::Firing) -> anyhow::Result<Taken> {
        // Where its answer goes, kept until it went: a turn its
        // harness finishes after a crash answers there too. A
        // subagent's conversation answers where it was first asked
        // from, as itself, whoever writes to it since.
        let owed = match self.owed_path(&firing.thread).exists() && is_subagents(&firing.thread) {
            true => self.owed(&firing.thread),
            false => Owed {
                from: firing.from.clone(),
                hops: firing.hops,
                answer: firing.answer.clone(),
                standing: firing.standing,
                sent: false,
            },
        };
        self.owe(&firing.thread, &owed)?;
        let accepted = link_connection_contract::ask_occurrence(&self.endpoint, &firing).await?;
        Ok(Taken {
            thread: firing.thread,
            owed,
            accepted,
        })
    }

    /// Reads the answer of a message the harness took and sends it where
    /// it goes; joined to a turn already running, its answer goes where
    /// that turn's goes.
    pub(crate) fn finish(
        &self,
        taken: Taken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>
    {
        Box::pin(async move {
            use link_connection_contract::Asked;
            let (progress, _) = tokio::sync::watch::channel(String::new());
            let Taken {
                thread,
                owed,
                accepted,
            } = taken;
            let (answered, receipt) = accepted.answer_received(&progress).await?;
            let Asked::Answer(answer) = answered else {
                self.paid(&thread);
                return Ok(String::new());
            };
            anyhow::ensure!(
                !self.completion_fenced(&thread)?,
                "the result is retained while this work is fenced"
            );
            self.answer_received(&thread, &owed, &answer, receipt.as_deref())
                .await?;
            Ok(answer)
        })
    }

    /// Sends `answer`, of conversation `thread`, where `owed` says: the
    /// chat it is, another conversation (a subagent's parent, as a
    /// message from it, one hop further, its answer going where that one's
    /// goes), or nowhere; then it's no longer owed.
    pub(super) async fn send_answer(
        &self,
        thread: &str,
        owed: &Owed,
        answer: &str,
        delivery: &link_connection_contract::TransportEventId,
    ) -> anyhow::Result<Option<Taken>> {
        use link_connection_contract::{AnswerTo, Firing};
        let mut continuation = None;
        let already_sent = owed.from.starts_with("schedule:") && {
            let current = self.owed(thread);
            current.from == owed.from && current.answer == owed.answer && current.sent
        };
        if !answer.trim().is_empty() && !already_sent {
            match &owed.answer {
                AnswerTo::Nowhere => {}
                AnswerTo::Chat => self.deliver(thread, answer)?,
                AnswerTo::OwnerThread(destination) => self.send_owner(destination, answer)?,
                AnswerTo::Connection(connection) => self.send_connection(connection, answer)?,
                AnswerTo::Thread(parent) => {
                    let taken = self
                        .take(Firing {
                            parent: None,
                            delivery: Some(delivery.clone()),
                            occurrence: None,
                            thread: parent.clone(),
                            from: owed.from.clone(),
                            text: answer.to_string(),
                            answer: self.answer_of(parent),
                            hops: owed.hops.saturating_add(1),
                            // The parent keeps its own.
                            tools: None,
                            model: None,
                            // A subagent's answer stands as it did.
                            standing: owed.standing,
                        })
                        .await?;
                    continuation = Some(taken);
                }
            }
        }
        self.paid(thread);
        Ok(continuation)
    }

    pub(super) fn completion_fenced(&self, thread: &str) -> anyhow::Result<bool> {
        use link_interactions::control::{Execution, Lifecycle};
        let Some(store) = &self.interactions else {
            return Ok(false);
        };
        Ok(store
            .work_status(&self.interaction_agent, thread)?
            .is_some_and(|state| {
                state.lifecycle != Lifecycle::Open
                    || !state.pause_reasons.is_empty()
                    || !state.in_flight_effects.is_empty()
                    || matches!(
                        state.observed,
                        Execution::Pausing
                            | Execution::Stopping
                            | Execution::Stopped
                            | Execution::Unknown
                    )
            }))
    }

    /// Asks the harness for the answers of turns it finished after a
    /// crash, or whose caller went, and sends each where its turn's answer
    /// was to go: when the bridge starts, again each time the harness
    /// answers after it couldn't be reached (it stopped, so it may have
    /// crashed), and every half minute meanwhile, for a turn a bridge
    /// stopping cut off. Between, it holds a connection to the harness,
    /// which ends when it stops.
    pub(super) fn deliver_recovered(self, stop: &AtomicBool) {
        let Ok(port) = link_harness_contract::harness::endpoint_port(&self.endpoint.endpoint)
        else {
            return;
        };
        let mut wait = RETRY_FIRST;
        let mut failing = Failing::default();
        while !stop.load(Ordering::Relaxed) {
            if let Err(err) = link_runtime::block_on(self.recover()) {
                // Not answering yet (it's starting): asked again, and said
                // only once it's been a while.
                if let Some(for_) = failing.failed(Instant::now()) {
                    link_format::daemon::warn!(
                        "asking what was finished after a crash: no answer for {} s: {err:#}",
                        for_.as_secs()
                    );
                }
                if sleep_unless(wait, stop) {
                    return;
                }
                wait = (wait * 2).min(RETRY_MOST);
                continue;
            }
            failing = Failing::default();
            wait = RETRY_FIRST;
            // Held until it's gone or a while has passed: then asked again,
            // for the answer of a turn whose caller went (a bridge stopped
            // as it answered), which it finished since.
            while let Held::Ended = held(port, ASK_AGAIN) {
                // It ended a connection it had answered on, and may run
                // on: looked at again in a moment.
                if sleep_unless(RETRY_FIRST, stop) {
                    return;
                }
            }
        }
    }

    /// One ask for what the harness finished after a crash: each answer
    /// sent where it goes, and the harness told which went, so it keeps
    /// the rest. A harness that doesn't keep them hands each over once.
    pub(super) async fn recover(&self) -> anyhow::Result<()> {
        self.recover_for(None, &[]).await.map(drop)
    }

    /// [`Self::recover`], or, for a chat on `link` thread `mine`, that
    /// thread's answers owed to its chat alone, in order, for the chat to
    /// show, but for those kept as `except` says (ids it knew of before).
    /// The bridge leaves a `link` thread's chat answer its harness keeps
    /// with it, for the next chat on that thread; one it doesn't keep is
    /// held ([`crate::chat::Held`]).
    pub async fn recover_for(
        &self,
        mine: Option<&str>,
        except: &[String],
    ) -> anyhow::Result<Vec<String>> {
        use link_connection_contract::AnswerTo;
        self.delete_selected_deliveries()?;
        self.retry_answers().await?;
        let answers = link_connection_contract::recovered(&self.endpoint).await?;
        let (mut done, mut handed) = (Vec::new(), Vec::new());
        for recovered in answers {
            if self.completion_fenced(&recovered.thread)? {
                continue;
            }
            let owed = self.owed(&recovered.thread);
            let excepted = recovered.id.as_ref().is_some_and(|id| except.contains(id));
            if mine == Some(recovered.thread.as_str()) && owed.answer == AnswerTo::Chat && !excepted
            {
                self.paid(&recovered.thread);
                if !recovered.text.trim().is_empty() {
                    handed.push(recovered.text);
                }
                done.extend(recovered.id);
                continue;
            }
            let left = match mine {
                // A chat takes its own thread's chat answers alone: the
                // rest stay kept for the bridge, which sends each once, and
                // for other chats. One handed over once (from a harness
                // that keeps nothing) goes where it goes, or it's lost.
                Some(_) => recovered.id.is_some(),
                // A `link` thread's chat answer, kept for the next chat on
                // it.
                None => {
                    crate::chat::is_link(&recovered.thread)
                        && recovered.id.is_some()
                        && owed.answer == AnswerTo::Chat
                }
            };
            if left {
                continue;
            }
            match self
                .answer_received(
                    &recovered.thread,
                    &owed,
                    &recovered.text,
                    recovered.id.as_deref(),
                )
                .await
            {
                Ok(()) => done.extend(recovered.id),
                Err(err) => link_format::daemon::warn!(
                    "an answer {} finished after a crash didn't go{}: {err:#}",
                    link_format::clean(&recovered.thread),
                    match recovered.id {
                        Some(_) => ", kept for the next time",
                        None => "",
                    }
                ),
            }
        }
        link_connection_contract::recovered_done(&self.endpoint, done).await?;
        Ok(handed)
    }

    /// The ids of the answers in `thread` its harness keeps now, none
    /// taken.
    pub async fn kept(&self, thread: &str) -> anyhow::Result<Vec<String>> {
        let answers = link_connection_contract::recovered(&self.endpoint).await?;
        Ok(answers
            .into_iter()
            .filter(|answer| answer.thread == thread)
            .filter_map(|answer| answer.id)
            .collect())
    }

    /// Where the answer of conversation `thread` goes: as its last
    /// firing said while it's owed, else to its chat; a subagent's that
    /// isn't known, nowhere.
    pub(super) fn owed(&self, thread: &str) -> Owed {
        let kept = std::fs::read(self.owed_path(thread))
            .ok()
            .and_then(|bytes| link_files::saved::parse_json(&bytes).ok());
        // Not known: as no one, a stranger, the least.
        kept.unwrap_or_else(|| Owed {
            from: String::new(),
            hops: 0,
            answer: match is_chat(thread) {
                true => link_connection_contract::AnswerTo::Chat,
                false => link_connection_contract::AnswerTo::Nowhere,
            },
            standing: link_connection_contract::access::Standing::Stranger,
            sent: false,
        })
    }

    /// Where an answer in conversation `thread`, a message to it, goes: a
    /// subagent's conversation answers where it was asked from, up the
    /// chain; any other, to its chat.
    fn answer_of(&self, thread: &str) -> link_connection_contract::AnswerTo {
        match is_subagents(thread) {
            true => self.owed(thread).answer,
            false => link_connection_contract::AnswerTo::Chat,
        }
    }

    /// Keeps where conversation `thread`'s answer goes.
    pub(super) fn owe(&self, thread: &str, owed: &Owed) -> anyhow::Result<()> {
        link_files::create_private_dir(&self.owed)?;
        let path = self.owed_path(thread);
        link_files::write_private(&path, &link_files::saved::print_json(owed)?)
            .within(format!("writing {}", path.display()))
    }

    /// Conversation `thread`'s answer went. A subagent's conversation
    /// keeps where it answers, for its own subagents' answers.
    pub(super) fn paid(&self, thread: &str) {
        if !is_subagents(thread) {
            let _ = std::fs::remove_file(self.owed_path(thread));
        }
    }

    /// Its file: the conversation's name in hex, so any is one name.
    pub(super) fn owed_path(&self, thread: &str) -> PathBuf {
        let hex: String = thread.bytes().map(|b| format!("{b:02x}")).collect();
        self.owed.join(format!("{hex}.json"))
    }

    /// Sends `text` to the chat `thread` is (`telegram:42`), through its
    /// connection's module (`lnk-<connection> send`); held for the next chat on
    /// it, for a `link` thread; nothing for a conversation that isn't a
    /// carried connection's (a subagent's).
    pub(super) fn deliver(&self, thread: &str, text: &str) -> anyhow::Result<()> {
        self.deliver_receipt(thread, text).map(drop)
    }

    pub(super) fn deliver_receipt(&self, thread: &str, text: &str) -> anyhow::Result<Vec<String>> {
        let Some((connection, chat)) = thread.split_once(':') else {
            return Ok(Vec::new());
        };
        if connection == crate::chat::CONNECTION {
            self.held.keep(thread, text)?;
            return Ok(Vec::new());
        }
        let Some((program, request)) = self.connections.get(connection) else {
            return Ok(Vec::new());
        };
        let sending = link_connection_contract::Sending {
            bridge: request.duplicate(),
            chat: chat.to_string(),
            text: text.to_string(),
        };
        let mut child = link_secretive::process::ours(program)
            .arg("send")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .within(format!("running {} send", program.display()))?;
        let mut stdin = child.stdin.take().within("no stdin")?;
        stdin.write_all(link_connection_contract::stated(&sending)?.as_bytes())?;
        drop(stdin);
        let output = child.wait_with_output()?;
        anyhow::ensure!(
            output.status.success(),
            "lnk-{connection} send: {}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
        let receipt: link_connection_contract::SentMessages =
            serde_json::from_slice(&output.stdout)
                .within("connection did not return its delivery receipt")?;
        Ok(receipt.messages)
    }
}

/// Whether `thread` is a subagent's conversation (`task-<id>`).
fn is_subagents(thread: &str) -> bool {
    thread.starts_with("task-")
}

/// Whether `thread` is a chat a connection carries (`telegram:42`), where
/// a person may speak; every other conversation of an agent's is named
/// without a connection (`task-<id>`, `agent-<id>`, `team-<id>`).
fn is_chat(thread: &str) -> bool {
    thread.contains(':')
}

/// `err` as a refusal ([`link_sandbox_binding::Why::Refused`]): the way in
/// read the message and won't take it, by its terms or a cap, so trying it
/// again changes nothing, and its sender says so rather than retrying.
fn refused(err: anyhow::Error) -> anyhow::Error {
    link_sandbox_binding::Failure::new(link_sandbox_binding::Why::Refused, format!("{err:#}"))
        .into()
}

/// The first and longest wait before asking a harness that didn't answer
/// again.
pub(super) const RETRY_FIRST: Duration = Duration::from_secs(2);

/// How long a harness may not answer before the bridge says so: a
/// harness starting (Link Harness in a second, DeepSeek's in several)
/// isn't an error, and a first start would otherwise print one.
const QUIET: Duration = Duration::from_secs(30);

/// Since when asking the harness has failed, without an answer between.
#[derive(Default)]
pub(super) struct Failing {
    since: Option<Instant>,
}

impl Failing {
    /// Another failure at `now`: how long it has failed, if that's long
    /// enough ([`QUIET`]) to say.
    pub(super) fn failed(&mut self, now: Instant) -> Option<Duration> {
        let since = *self.since.get_or_insert(now);
        let for_ = now.saturating_duration_since(since);
        (for_ >= QUIET).then_some(for_)
    }
}
pub(super) const RETRY_MOST: Duration = Duration::from_secs(60);

/// How long a connection to a harness is held before it's asked again
/// for what it finished that nobody took.
const ASK_AGAIN: Duration = Duration::from_secs(30);

/// What came of holding a connection to a harness.
enum Held {
    /// Nothing answers: it's down, or starting.
    Down,
    /// It answered, then ended the connection.
    Ended,
    /// It answered, and still held it after the while given.
    Up,
}

/// A connection to the harness on `port`, held until it ends it, it
/// stops, or `within` has passed.
fn held(port: u16, within: Duration) -> Held {
    use std::io::Read;
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return Held::Down;
    };
    let host = format!("HEAD / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    if stream.write_all(host.as_bytes()).is_err() {
        return Held::Down;
    }
    let mut buffer = [0u8; 1024];
    // Its answer, then whatever ends: nothing, unless it's a harness.
    match stream.read(&mut buffer) {
        Ok(0) | Err(_) => Held::Down,
        Ok(_) => {
            let until = Instant::now() + within;
            loop {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
                    return Held::Up;
                }
                match stream.read(&mut buffer) {
                    Ok(n) if n > 0 => {}
                    Ok(_) => return Held::Ended,
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        return Held::Up;
                    }
                    Err(_) => return Held::Ended,
                }
            }
        }
    }
}
