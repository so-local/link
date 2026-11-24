use super::*;
use crate::store::Repeat;
use link_interactions::{
    runtime::{Binding, Step, WorkAdmission},
    sources,
};

pub(super) struct WorkGuard(
    pub  (
        link_interactions::Store,
        std::sync::Arc<link_secretive::Secret>,
    ),
);
impl Drop for WorkGuard {
    fn drop(&mut self) {
        let _ = self.0.0.finish_work(self.0.1.expose(), None);
    }
}

impl Harness {
    /// The trusted host's coordinator: shared with owner ingress and
    /// source gates where it's the machine's, else its own.
    pub fn with_interactions(mut self, host: link_interactions::Host) -> Self {
        self.interactions = Some(host.store);
        self.sources_check_work = host.shared;
        self
    }
    /// The binding a tool call of `turn` carries: its work's, where its
    /// tools' sources check it; none where they can't see it.
    pub(super) fn carried<'a>(&self, turn: &Turn<'a>) -> Option<&'a Binding> {
        turn.binding.filter(|_| self.sources_check_work)
    }
    pub(super) fn retain_admitted_message(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: &Event,
    ) -> anyhow::Result<(Event, bool)> {
        let mut events = self.tail(agent, thread)?;
        let repeat = match (&message.delivery, &message.from) {
            (Some(delivery), _) => Some(Repeat::Delivery(delivery.clone())),
            (None, Some(from))
                if thread.name().starts_with("task-") && from.starts_with("task:") =>
            {
                Some(Repeat::From(from.clone()))
            }
            _ => None,
        };
        if let Some(repeat) = repeat {
            // Found by where its key was first said, not by reading the
            // whole conversation at every message.
            if let Some(previous) = self.store.first_of(agent, thread, &repeat)? {
                anyhow::ensure!(
                    previous.text == message.text
                        && previous.hops == message.hops
                        && previous.from == message.from
                        && previous.occurrence == message.occurrence,
                    "source retried a task with changed input"
                );
                return Ok((previous, false));
            }
        }
        let mut event = message.clone();
        self.append_next(agent, thread, &mut event, &mut events)?;
        Ok((event, true))
    }
    pub(super) fn admit_busy_input(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: &Event,
        busy: Busy,
    ) -> anyhow::Result<Option<String>> {
        let Some(store) = &self.interactions else {
            return Ok(None);
        };
        if message.role != event::USER || message.from.is_some() {
            return Ok(None);
        }
        let work = off_runtime(|| store.works(agent.name()))?
            .into_iter()
            .find(|b| b.state.work.agent == agent.id() && b.state.work.session == thread.name());
        let Some(work) = work else { return Ok(None) };
        if !work.active
            && work.state.pause_reasons.is_empty()
            && work.state.lifecycle == link_interactions::control::Lifecycle::Open
            && work.state.observed == link_interactions::control::Execution::Prepared
        {
            return Ok(None);
        }
        let verb = match busy {
            Busy::Steer => "steer",
            Busy::Queue => "queue",
            Busy::Interrupt => "redirect",
        };
        off_runtime(|| {
            link_interactions::owner::command(
                store,
                &sources::owner(),
                agent.name(),
                thread.name(),
                &format!("/work {verb} {}", message.text),
                link_interactions::TransportEventId::new(link_files::random_token()?)
                    .map_err(anyhow::Error::msg)?,
            )
        })
        .map(Some)
    }
    pub(super) fn owner_input(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: &Event,
    ) -> anyhow::Result<Option<String>> {
        if message.role != event::USER
            || message.from.is_some()
            || !link_interactions::owner::is_command(&message.text)
        {
            return Ok(None);
        }
        let Some(store) = &self.interactions else {
            return Ok(None);
        };
        off_runtime(|| {
            if link_interactions::owner::stops_nothing_retained(
                store,
                agent.name(),
                thread.name(),
                &message.text,
            ) {
                return Ok(None);
            }
            link_interactions::owner::command(
                store,
                &sources::owner(),
                agent.name(),
                thread.name(),
                &message.text,
                link_interactions::TransportEventId::new(link_files::random_token()?)
                    .map_err(anyhow::Error::msg)?,
            )
            .map(Some)
        })
    }
    pub(super) async fn recover_inputs(&self, agent: &Agent) {
        self.delete_retained_work(agent);
        let Some(store) = &self.interactions else {
            return;
        };
        let ready = match off_runtime(|| store.ready_work(agent.name())) {
            Ok(ready) => ready,
            Err(_) => return,
        };
        for work in ready {
            if work.agent != agent.id() {
                continue;
            }
            let Ok(thread) = Thread::new(&work.session) else {
                continue;
            };
            let Ok(holder) = holder() else { continue };
            if !matches!(
                off_runtime(|| self.store.take_turn(agent, &thread, &holder)),
                Ok(Took::Took)
            ) {
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
                    let _ = off_runtime(|| self.store.set_recovered(agent, &thread, answer.seq));
                }
                Err(err) => {
                    crate::warn!(crate::said::in_chat(agent, &thread) => "continuing retained work: {err:#}")
                }
            }
        }
    }
    fn delete_retained_work(&self, agent: &Agent) {
        let Some(store) = &self.interactions else {
            return;
        };
        let Ok(pending) = off_runtime(|| store.pending_deletions(agent.id())) else {
            return;
        };
        for member in pending {
            let link_interactions::control::ControlOperation::Delete { scope } = &member.operation
            else {
                continue;
            };
            let Ok(thread) = Thread::new(&member.work.session) else {
                continue;
            };
            let Ok(holder) = holder() else { continue };
            if !matches!(
                off_runtime(|| self.store.take_turn(agent, &thread, &holder)),
                Ok(Took::Took)
            ) {
                continue;
            }
            let result = off_runtime(|| self.store.delete_work(agent, &thread, scope));
            let _ = off_runtime(|| self.store.end_turn(agent, &thread, &holder));
            match result {
                Ok(()) => {
                    let _ = off_runtime(|| store.deleted_work(&member));
                }
                Err(err) => {
                    let _ = store.record_receipt(
                        member.id.as_str(),
                        &member.work,
                        link_interactions::control::ReceiptStage::Unknown,
                        &format!("Deletion did not confirm completion: {err:#}"),
                        link_interactions::now(),
                    );
                }
            }
        }
    }
    pub(super) fn bind_work(
        &self,
        agent: &Agent,
        thread: &Thread,
        attempt: &str,
        message: Option<&Event>,
    ) -> anyhow::Result<Option<Binding>> {
        self.interactions
            .as_ref()
            .map(|store| {
                store.recover_work(agent.id(), thread.name())?;
                let last = || {
                    self.tail(agent, thread)
                        .ok()
                        .and_then(|events| events.into_iter().rev().find(|e| e.role == event::USER))
                };
                let occurrence = message
                    .and_then(|m| m.occurrence.clone())
                    .or_else(|| last().and_then(|e| e.occurrence));
                // A subagent's lineage, as its scheduler records it in the
                // machine's store: a store of the harness's own learns it
                // from the parent conversation its first message names.
                if !self.sources_check_work
                    && let Some(id) = thread.name().strip_prefix("task-")
                    && let Some(parent) = message
                        .and_then(|m| m.parent.clone())
                        .or_else(|| last().and_then(|e| e.parent))
                {
                    store.record_child(agent.id(), &parent, id)?;
                }
                let kind = if let Some(occurrence) = occurrence {
                    // The scheduler projects its schedules into the
                    // machine's store, and checks an occurrence there
                    // before it fires. A store of the harness's own sees
                    // none of them, so it keeps the projection the
                    // occurrence names, or it would admit only a schedule's
                    // first occurrence and refuse every one after.
                    if !self.sources_check_work {
                        store.register_schedule(
                            agent.id(),
                            &occurrence.schedule,
                            occurrence.revision,
                            None,
                        )?;
                    }
                    link_interactions::WorkKind::Occurrence { occurrence }
                } else if let Some(id) = thread.name().strip_prefix("task-") {
                    link_interactions::WorkKind::Child { id: id.into() }
                } else {
                    link_interactions::WorkKind::Conversation
                };
                store.register_work(WorkAdmission {
                    owner_route: thread.name().contains(':').then(|| thread.name().into()),
                    kind,
                    owner: sources::owner(),
                    agent: agent.id().into(),
                    routing_agent: agent.name().into(),
                    session: thread.name().into(),
                    attempt: attempt.into(),
                    parent_attempt: None,
                    checkpoint: Some(thread.name().into()),
                })
            })
            .transpose()
    }

    pub(super) fn step_work(
        &self,
        turn: &Turn<'_>,
        step: Step,
    ) -> anyhow::Result<Option<link_interactions::WorkRef>> {
        if let (Some(store), Some(binding)) = (&self.interactions, turn.binding) {
            let _fence = off_runtime(|| store.dispatch_fence())?;
            return off_runtime(|| store.work_step(binding.token.expose(), step)).map(Some);
        }
        Ok(None)
    }

    pub(super) fn same_step(
        &self,
        turn: &Turn<'_>,
        expected: &Option<link_interactions::WorkRef>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.step_work(turn, Step::Tool)? == *expected,
            "control changed while the model generated these calls"
        );
        Ok(())
    }

    /// Whether the work goes on as it did at `expected`, its step, but
    /// for a message steered in: a steer moves the work's control
    /// revision and nothing else, while a stop, pause, redirect or archive
    /// halts the work, which [`Self::step_work`] refuses. What the model
    /// wrote meanwhile, a summary, stands; a call it made doesn't
    /// ([`Self::same_step`]).
    pub(super) fn same_work(
        &self,
        turn: &Turn<'_>,
        expected: &Option<link_interactions::WorkRef>,
    ) -> anyhow::Result<()> {
        let unsteered = |work: &Option<link_interactions::WorkRef>| {
            work.clone().map(|work| link_interactions::WorkRef {
                control_revision: 0,
                ..work
            })
        };
        anyhow::ensure!(
            unsteered(&self.step_work(turn, Step::Tool)?) == unsteered(expected),
            "the work changed while the model wrote"
        );
        Ok(())
    }

    pub(super) fn take_inputs(
        &self,
        turn: &Turn<'_>,
        events: &mut Vec<Event>,
        new_turn: bool,
    ) -> anyhow::Result<()> {
        if let (Some(store), Some(binding)) = (&self.interactions, turn.binding) {
            for input in off_runtime(|| {
                store.waiting_inputs(binding.token.expose(), new_turn && turn.admit_queued)
            })? {
                let mut event = Event::user(&input.content.text);
                // The durable event identity makes a crash between transcript and receipt harmless.
                event.input = Some(input.id.clone());
                let existing = events
                    .iter()
                    .find(|e| e.input.as_ref() == Some(&input.id))
                    .map(|e| e.seq);
                let seq = match existing {
                    Some(seq) => seq,
                    None => {
                        off_runtime(|| {
                            self.append_next(turn.agent, turn.thread, &mut event, events)
                        })?;
                        event.seq
                    }
                };
                off_runtime(|| store.input_delivered(binding.token.expose(), &input.id, seq))?;
            }
        }
        Ok(())
    }
}
