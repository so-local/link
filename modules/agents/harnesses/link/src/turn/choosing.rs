//! A conversation's own model, how hard it thinks and whether it's asked
//! fast (`/model:set`): kept as its events, read back
//! at each turn ([`crate::commands::chosen`]). `--every-chat` makes the
//! choice every chat's too, in the agent's settings, until it starts
//! again with its own.

use super::{Harness, off_runtime};
use crate::agent::{Agent, Thread};
use crate::commands::{self, Chosen, Done};
use crate::config::Config;

impl Harness {
    /// `agent`'s settings as the conversation `thread` is on chose them.
    pub(crate) fn conversing(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Config> {
        Ok(self.conversing_read(agent, thread)?.0)
    }

    /// [`Self::conversing`], and what it read of `thread`.
    pub(super) fn conversing_read(
        &self,
        agent: &Agent,
        thread: &Thread,
    ) -> anyhow::Result<(Config, super::Read)> {
        let mut read = off_runtime(|| self.tail_read(agent, thread))?;
        if commands::needs_all(&read.events) {
            read = super::Read {
                events: off_runtime(|| self.store.read(agent, thread))?,
                from: 1,
            };
        }
        let events = commands::conversation(read.events.clone(), None, usize::MAX).0;
        Ok((self.config(agent)?.chosen(&commands::chosen(&events)), read))
    }

    /// `thread`'s conversation runs on `model`, a model or an alias, from
    /// now on, as `from` asked (a subagent's start): kept as a
    /// `/model:set` of its own, from them. Nothing where it's on it;
    /// refused where its spec's `[model] allow` leaves it out.
    pub fn choose_model(
        &self,
        agent: &Agent,
        thread: &Thread,
        model: &str,
        from: Option<&str>,
    ) -> anyhow::Result<()> {
        let config = self.conversing(agent, thread)?;
        let model = config.opinions().model_named(model);
        if config.model.named().as_deref() == Some(model.as_str()) {
            return Ok(());
        }
        // Held to its spec's `[model] allow`, as the proxy holds it.
        if let Some(allow) = config.opinions().model_for(&config.model.provider).allow {
            anyhow::ensure!(
                allow.contains(&model),
                "{model} isn't one of the models its spec allows"
            );
        }
        let asked = commands::models::Asked {
            model: Some(model),
            ..Default::default()
        };
        let mut chose = crate::event::Event {
            role: crate::event::COMMAND.into(),
            text: commands::models::kept(commands::models::SET, &asked),
            from: from.map(str::to_string),
            at: crate::event::now(),
            ..Default::default()
        };
        let mut events = off_runtime(|| self.tail(agent, thread))?;
        off_runtime(|| self.append_next(agent, thread, &mut chose, &mut events))
    }

    /// What `/model:set` does, `arguments` after it, in `thread`: its
    /// conversation switched to the model named, an alias resolved, when
    /// its provider offers it and its spec allows it; with `--every-chat`,
    /// every chat too; `--fast` asked of a provider that offers it.
    pub(super) async fn set_model(
        &self,
        agent: &Agent,
        thread: &Thread,
        arguments: &str,
    ) -> anyhow::Result<Done> {
        let asked = match commands::models::asked(arguments) {
            Ok(asked) => asked,
            Err(why) => return Ok(Done::refused(why)),
        };
        let Some(level) = asked.effort else {
            return self.switch(agent, thread, asked).await;
        };
        // `--effort` beside a model or `--fast` is told with it; alone,
        // it is all that is kept.
        let mut done = match asked.model.is_some() || asked.fast.is_some() {
            true => self.switch(agent, thread, asked.clone()).await?,
            false => Done {
                kept: Some(commands::models::kept(commands::models::SET, &asked)),
                ..Done::from(String::new())
            },
        };
        if done.refused {
            return Ok(done);
        }
        if asked.agent {
            let config = self.config(agent)?.chosen(&Chosen {
                thinking: Some(level),
                ..Chosen::default()
            });
            off_runtime(|| self.store.set_config(agent, &config))?;
        }
        let told = commands::effort(Some(level), Some(asked.agent));
        done.told = match done.told.is_empty() {
            true => told,
            false => format!("{} {told}", done.told),
        };
        Ok(done)
    }

    /// `/model:set` given a model or `--fast`, or neither: its
    /// conversation's model switched, or its models named.
    async fn switch(
        &self,
        agent: &Agent,
        thread: &Thread,
        mut asked: commands::models::Asked,
    ) -> anyhow::Result<Done> {
        let config = self.conversing(agent, thread)?;
        let current = config.model.named().unwrap_or_default();
        let provider = config.model.provider.clone();
        if let Some(name) = asked.model.take() {
            let (current, offered) = match self.offered(&config).await? {
                Ok(offered) => offered,
                Err(told) => return Ok(Done::refused(format!("{told}, so it stays."))),
            };
            let name = config.opinions().model_named(&name);
            let (to, told) = commands::model(&current, &name, &offered, asked.agent);
            let Some(to) = to else {
                return Ok(match name == current && asked.fast.is_some() {
                    true => self.set_fast(&config, &current, asked),
                    false => Done::refused(told),
                });
            };
            if asked.fast == Some(true) && !link_model_contract::chat::offers_fast(&provider, &to) {
                return Ok(Done::refused(commands::no_fast(&to)));
            }
            if asked.agent {
                let mut config = self.config(agent)?;
                config.model.name = Some(to.clone());
                off_runtime(|| self.store.set_config(agent, &config))?;
            }
            asked.model = Some(to);
            return Ok(Done {
                kept: Some(commands::models::kept(commands::models::SET, &asked)),
                ..Done::from(told)
            });
        }
        if asked.fast.is_some() {
            return Ok(self.set_fast(&config, &current, asked));
        }
        if asked.agent {
            return Ok(Done::refused(
                "Name a model: /model:set <name> --every-chat.".into(),
            ));
        }
        let level = config.opinions().model_for(&config.model.provider).thinking;
        let told = match self.offered(&config).await? {
            Ok((current, offered)) => commands::model(&current, "", &offered, false).1,
            Err(told) => told,
        };
        Ok(Done::from(format!(
            "{told} {}",
            commands::effort(level, None)
        )))
    }

    /// `/model:set --fast`, `on` or `off`, for this conversation's
    /// `model`: refused where its provider offers no faster service.
    fn set_fast(&self, config: &Config, model: &str, asked: commands::models::Asked) -> Done {
        let on = asked.fast == Some(true);
        if asked.agent {
            return Done::refused("Fast is this conversation's: leave out --every-chat.".into());
        }
        if on && !link_model_contract::chat::offers_fast(&config.model.provider, model) {
            return Done::refused(commands::no_fast(model));
        }
        Done {
            kept: Some(commands::models::kept(commands::models::SET, &asked)),
            ..Done::from(commands::fast(model, on))
        }
    }

    /// What `/chat:verbose` does, `arguments` after it, in `thread`:
    /// whether its conversation's chat shows each tool call, `on` or
    /// `off`, alone the other of what it does now, its choice or its
    /// spec's `[connections] activity` for its connection.
    pub(super) fn verbose(
        &self,
        agent: &Agent,
        thread: &Thread,
        arguments: &str,
    ) -> anyhow::Result<Done> {
        let config = self.conversing(agent, thread)?;
        let connection = thread.name().split(':').next().unwrap_or_default();
        let now = config.verbose.unwrap_or_else(|| {
            config
                .opinions()
                .connection(connection)
                .activity
                .is_some_and(link_agent_spec::Activity::shows)
        });
        let on = match arguments {
            "" => !now,
            "on" => true,
            "off" => false,
            other => return Ok(Done::refused(format!("No {other}: on or off."))),
        };
        let word = if on { "on" } else { "off" };
        Ok(Done {
            kept: Some(format!("{} {word}", commands::models::VERBOSE)),
            ..Done::from(commands::verbose(on).to_string())
        })
    }

    /// The model `config` runs, and the models its provider offers that
    /// its spec allows; what the owner is told when they can't be read.
    pub(super) async fn offered(
        &self,
        config: &Config,
    ) -> anyhow::Result<Result<(String, Vec<String>), String>> {
        let current = config.model.named().unwrap_or_default();
        let mut offered = match crate::model::listed(&self.client, &config.model).await {
            Ok(offered) => offered,
            Err(why) => {
                return Ok(Err(format!(
                    "Model: {current}\nIts provider's list couldn't be read ({why})"
                )));
            }
        };
        if let Some(allow) = config.opinions().model_for(&config.model.provider).allow {
            offered.retain(|m| allow.contains(m));
        }
        Ok(Ok((current, offered)))
    }
}
