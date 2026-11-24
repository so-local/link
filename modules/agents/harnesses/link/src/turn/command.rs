//! A command run in place of a turn ([`crate::commands`]).

use tokio::sync::mpsc;

use super::{Asked, Harness, off_runtime};
use crate::agent::{Agent, Thread};
use crate::commands::{self, Does, Run};
use crate::event::{self, Event};
use link_agent_contract::namespace::tasks;

impl Harness {
    /// Runs `run`, the command the owner's `message` names: its event
    /// kept as the conversation's next, and what the owner is told sent
    /// as an answer is, in its place. It takes no turn: one running goes
    /// on as it began, and the next reads the conversation as the
    /// command left it.
    pub(super) async fn command(
        &self,
        agent: &Agent,
        thread: &Thread,
        message: &Event,
        run: Run,
        pieces: &mpsc::Sender<String>,
    ) -> anyhow::Result<Asked> {
        let mut events = off_runtime(|| self.tail(agent, thread))?;
        let done = match run.command.does {
            Does::Resume | Does::Fork | Does::Rename | Does::Export => {
                events = off_runtime(|| self.store.read(agent, thread))?;
                let asked = run.arguments.as_str();
                match run.command.does {
                    Does::Resume => commands::resume(&events, asked),
                    Does::Fork => commands::fork(&events, asked),
                    Does::Rename => commands::rename(&events, asked),
                    _ => commands::Done::from(self.export(agent, thread, &events)?),
                }
            }
            Does::Busy(busy) => commands::Done::from(commands::busy(busy).to_string()),
            Does::ModelSet => self.set_model(agent, thread, &run.arguments).await?,
            Does::ModelGet => self.set_model(agent, thread, "").await?,
            Does::Verbose => self.verbose(agent, thread, &run.arguments)?,
            Does::Compact => {
                commands::Done::from(self.compact_command(agent, thread, pieces).await?)
            }
            // Neither the question nor its answer is kept.
            Does::Btw => {
                let told = match run.arguments.as_str() {
                    "" => {
                        commands::response(&self.config(agent)?.opinions(), commands::NO_QUESTION)
                    }
                    question => self.btw(agent, thread, question).await?,
                };
                let _ = pieces.send(told.clone()).await;
                return Ok(Asked::Answered(Box::new(Event {
                    role: event::COMMAND.into(),
                    text: told,
                    at: message.at.clone(),
                    ..Event::default()
                })));
            }
            _ => commands::Done::from(self.told(agent, thread, &events, &run).await?),
        };
        let mut ran = Event {
            role: event::COMMAND.into(),
            text: done.kept.clone().unwrap_or_else(|| {
                format!("/{} {}", run.command.name, run.arguments)
                    .trim_end()
                    .to_string()
            }),
            failed: done.refused,
            at: message.at.clone(),
            ..Event::default()
        };
        off_runtime(|| self.append_next(agent, thread, &mut ran, &mut events))?;
        let told = match run.command.does {
            Does::Compact => done.told,
            _ => commands::response(&self.config(agent)?.opinions(), &done.told),
        };
        let _ = pieces.send(told.clone()).await;
        Ok(Asked::Answered(Box::new(Event { text: told, ..ran })))
    }

    /// What the owner is told of `run`, a command that keeps nothing
    /// but its own event, in `thread`, whose events since its window's
    /// start are `events`.
    async fn told(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: &[Event],
        run: &Run,
    ) -> anyhow::Result<String> {
        Ok(match &run.command.does {
            Does::Action(path) => {
                let answer = link_agent_contract::namespace::client::command(
                    &self.config(agent)?.actions,
                    path,
                    &self.given(agent, thread, path, &run.arguments)?,
                    thread.name(),
                )
                .await?;
                if path == "task stop"
                    && let Some(Ok(stopping)) = tasks::stopping(&run.arguments)
                {
                    self.stop_too(agent, thread, stopping, &answer)?;
                }
                serde_json::to_string(&answer)?
            }
            Does::Help => commands::help(&commands::list(&self.config(agent)?)),
            Does::ModelHelp => commands::help(
                &commands::list(&self.config(agent)?)
                    .into_iter()
                    .filter(|command| command.name.starts_with("model:"))
                    .collect::<Vec<_>>(),
            ),
            Does::Status => self.status(agent, thread, events).await?,
            Does::New => commands::NEW.to_string(),
            Does::Skills => commands::skills::listed(&self.found_skills(agent)?),
            Does::Stop => {
                let running = off_runtime(|| self.store.turn_running(agent, thread));
                self.interrupt(agent, thread)?;
                match running {
                    true => commands::STOPPED,
                    false => commands::NOTHING_TO_STOP,
                }
                .to_string()
            }
            Does::ModelShow => {
                let config = self.conversing(agent, thread)?;
                if config
                    .actions
                    .actions
                    .iter()
                    .any(|name| name == "model show")
                {
                    let input = serde_json::json!({"name":config.model.named()});
                    serde_json::to_string(
                        &link_agent_contract::namespace::client::call(
                            &config.actions,
                            "model show",
                            input,
                        )
                        .await?,
                    )?
                } else {
                    let name = config.model.named().unwrap_or_default();
                    let known = crate::limits::known(config.model_facts.as_ref(), &config.model);
                    commands::shown(&name, &config.model.provider, known.as_ref())
                }
            }
            Does::ModelList => {
                let config = self.conversing(agent, thread)?;
                if config
                    .actions
                    .actions
                    .iter()
                    .any(|name| name == "model list")
                {
                    serde_json::to_string(
                        &link_agent_contract::namespace::client::call(
                            &config.actions,
                            "model list",
                            serde_json::json!({}),
                        )
                        .await?,
                    )?
                } else {
                    match self.offered(&config).await? {
                        Ok((current, offered)) => commands::listed(&current, &offered),
                        Err(told) => told,
                    }
                }
            }
            // Asked as a turn, never run here.
            Does::Learn => anyhow::bail!("/learn is a turn of its own"),
            Does::Goal => match run.arguments.as_str() {
                "--clear" => crate::goal::CLEARED.to_string(),
                "clear" => anyhow::bail!("No clear here: --clear ends a goal."),
                _ => {
                    let since = self.current(agent, thread, events.to_vec())?;
                    crate::goal::shown(&since)
                }
            },
            // Each keeps more than its own event, above.
            Does::Resume
            | Does::Fork
            | Does::Rename
            | Does::Export
            | Does::Busy(_)
            | Does::Compact
            | Does::Btw
            | Does::ModelSet
            | Does::ModelGet
            | Does::Verbose => {
                anyhow::bail!("/{} is run on its own", run.command.name)
            }
        })
    }

    /// What `/task:stop`'s words stop here beside what its providers
    /// did, `stopped` their answer: `--all` the answer being written in
    /// `thread` too, and a subagent's turn, under way in its own
    /// conversation, each one stopped or refused, as it's already begun.
    fn stop_too(
        &self,
        agent: &Agent,
        thread: &Thread,
        stopping: tasks::Stopping,
        stopped: &serde_json::Value,
    ) -> anyhow::Result<()> {
        if stopping == tasks::Stopping::All {
            self.interrupt(agent, thread)?;
        }
        let agents = ["stopped", "failed"]
            .iter()
            .flat_map(|key| stopped[key].as_array().into_iter().flatten())
            .filter(|said| said["work"] == tasks::Work::Agents.word())
            .filter_map(|said| said["id"].as_str());
        for id in agents {
            if let Ok(theirs) = Thread::new(&format!("task-{id}")) {
                self.interrupt(agent, &theirs)?;
            }
        }
        Ok(())
    }

    /// `arguments`, an action's JSON, as `path` is given it: a subagent
    /// started by `/task:agent` without a model runs on the one this
    /// conversation runs on.
    fn given(
        &self,
        agent: &Agent,
        thread: &Thread,
        path: &str,
        arguments: &str,
    ) -> anyhow::Result<String> {
        if path != "task agent" {
            return Ok(arguments.to_string());
        }
        let model = self.conversing(agent, thread)?.model.named();
        Ok(commands::with_model(arguments, model.as_deref()))
    }

    /// What `/chat:compact` says once its turn has run, its lease taken
    /// as a turn's is, waiting for one running to end.
    #[expect(
        clippy::disallowed_methods,
        reason = "the turn's own period of looking, as a message waiting for its turn"
    )]
    async fn compact_command(
        &self,
        agent: &Agent,
        thread: &Thread,
        pieces: &mpsc::Sender<String>,
    ) -> anyhow::Result<String> {
        let holder = super::holder()?;
        loop {
            if off_runtime(|| self.store.take_turn(agent, thread, &holder))?
                != crate::store::Took::Held
            {
                break;
            }
            tokio::time::sleep(super::LOOK_EVERY).await;
        }
        let told = std::sync::Mutex::new(None);
        let done = self
            .run(
                agent,
                thread,
                &holder,
                None,
                (pieces, &told),
                super::Work::Compact,
            )
            .await?;
        Ok(done.text)
    }

    /// The events of the conversation `thread` is on, of `events`, read
    /// from its window's start: every event of it read for it when
    /// those can't say ([`commands::needs_all`]).
    pub(super) fn current(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: Vec<Event>,
    ) -> anyhow::Result<Vec<Event>> {
        let events = match commands::needs_all(&events) {
            true => off_runtime(|| self.store.read(agent, thread))?,
            false => events,
        };
        Ok(commands::conversation(events, None, usize::MAX).0)
    }

    /// What `/chat:export` says, once it has written the conversation
    /// `thread` is on, whose every event is `events`, as people read
    /// it, to a file of its own in the agent's `exports/`.
    fn export(&self, agent: &Agent, thread: &Thread, events: &[Event]) -> anyhow::Result<String> {
        let (all, on) = commands::conversations(events);
        let held: Vec<Event> = all[on].events.iter().map(|i| events[*i].clone()).collect();
        let name = format!("{}-{}.md", thread.folder(), on + 1);
        let path = off_runtime(|| {
            self.store.export(
                agent,
                &name,
                crate::store::markdown(thread, &held).as_bytes(),
            )
        })?;
        Ok(format!(
            "Exported conversation {} to {path}, in the agent's folder.",
            on + 1
        ))
    }

    /// What `/status` says of `thread`, whose window's `events` are
    /// read: from its last `/new` on.
    async fn status(
        &self,
        agent: &Agent,
        thread: &Thread,
        events: &[Event],
    ) -> anyhow::Result<String> {
        let config = self.conversing(agent, thread)?;
        let opinions = config.opinions();
        let unknown = opinions.window.clone().unwrap_or_default().unknown;
        let known = crate::limits::known(config.model_facts.as_ref(), &config.model);
        let window = self
            .reported
            .of(
                &self.client,
                &config.model,
                known.as_ref(),
                unknown.unwrap_or(1024),
            )
            .await
            .within(&opinions, &config.model.provider, &config.links())
            .window;
        let events = self.current(agent, thread, events.to_vec())?;
        let model = config.model.named().unwrap_or_else(|| "none".into());
        let status = commands::Status::of(
            &events,
            model,
            config.model.provider.clone(),
            thread.name(),
            window,
        );
        Ok(status.told())
    }
}
