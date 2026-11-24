//! A second model for a turn its first can't answer (`lnk agent start
//! --fallback`, [`link_harness_contract::harness::Settings::fallback`]),
//! where the agent's spec turns it on (`[model] fallback`): an agent
//! that thinks with a model on this machine, and a hosted one behind it,
//! or a hosted one and another of its provider's. For one turn at a
//! time, the second answers when the first:
//!
//! - **is off:** its runtime refuses the connection, or sends nothing
//!   back within [`FIRST_BYTE`] ([`crate::model::Unreached`]);
//! - **is too small:** the prompt, once its oldest turns have left the
//!   window, is still past the first's window, so it's never sent what it
//!   would cut. A refusal of the prompt as too long is the same
//!   ([`crate::model::TooLong`]); or
//! - **is overloaded:** its provider answers 429 or a 5xx, busy or
//!   failing ([`crate::model::Overloaded`]).
//!
//! Once it has, the turn's other rounds go to the second too. A first
//! found off is left alone for [`OFF_FOR`]: the turns in that time go to
//! the second at once ([`Off`]), and the turn after tries the first
//! again. An overloaded one isn't: the next turn tries it. A summary is asked the same way. Nothing of an answer has gone out when it
//! falls back: an answer cut off half-way is the first's, failed. The
//! answer says which and why ([`crate::event::Event::fallback`]), and a
//! turn's measures count it ([`crate::measures`]).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use link_harness_contract::harness::Model;
use link_model_contract::Price;
use link_secretive::Duplicate;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::event::Event;
use crate::limits::{self, Reported};
use crate::model::{self, Overloaded, Prompt, Think, TooLong, Unreached};
use crate::window;

/// How long the first model has to send anything back, with a second
/// behind it: a model on this machine loading takes some seconds, one
/// that's off answers at once.
pub const FIRST_BYTE: Duration = Duration::from_secs(60);

/// How long a first model found off is left alone, its turns going to
/// the second at once: a runtime that didn't answer seldom does seconds
/// later, and each try costs up to [`FIRST_BYTE`].
pub const OFF_FOR: Duration = Duration::from_secs(3 * 60);

/// The first models found off lately, each by when: a harness's, for
/// every agent it answers.
pub struct Off {
    lasts: Duration,
    since: Mutex<HashMap<String, Instant>>,
}

impl Off {
    /// Each found off left alone for `lasts`.
    pub fn new(lasts: Duration) -> Self {
        Self {
            lasts,
            since: Mutex::default(),
        }
    }

    /// The model, as one runtime's: where, and which.
    fn key(model: &Model) -> String {
        format!(
            "{} {} {}",
            model.provider,
            model.base_url.as_deref().unwrap_or_default(),
            model.name.as_deref().unwrap_or_default()
        )
    }

    /// Whether `model` was found off within [`Off::new`]'s `lasts`.
    pub fn lately(&self, model: &Model) -> bool {
        let since = self.since.lock().unwrap_or_else(|e| e.into_inner());
        since
            .get(&Self::key(model))
            .is_some_and(|at| at.elapsed() < self.lasts)
    }

    /// `model` was found off now (`off`), or answered.
    fn found(&self, model: &Model, off: bool) {
        let mut since = self.since.lock().unwrap_or_else(|e| e.into_inner());
        match off {
            true => since.insert(Self::key(model), Instant::now()),
            false => since.remove(&Self::key(model)),
        };
    }
}

impl Default for Off {
    fn default() -> Self {
        Self::new(OFF_FOR)
    }
}

/// Why the second model answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The first's runtime refused the connection, or said nothing in time.
    Off,
    /// The prompt was past the first's window.
    Window,
    /// The first's provider answered 429 or a 5xx.
    Overloaded,
}

impl Why {
    /// As an answer keeps it, and its measure names it.
    pub fn name(self) -> &'static str {
        match self {
            Why::Off => "off",
            Why::Window => "window",
            Why::Overloaded => "overloaded",
        }
    }
}

/// The second model, and what a prompt for it holds to, as the first's
/// [`limits::Limits`] are made.
#[derive(Duplicate)]
pub struct Second {
    #[duplicate]
    pub model: Model,
    pub answer: u32,
    pub window: u32,
    pub price: Option<Price>,
    pub above: Vec<link_model_contract::Jump>,
    pub thinking: Think,
    /// Its own model's section of the prompt in place of the first's,
    /// when they differ ([`crate::prompt::Prompt::swap`]).
    pub swap: Option<crate::prompt::Swap>,
}

impl Second {
    /// `config`'s fallback, if it has one and its spec turns it on, with
    /// its limits: its facts as handed, else Link's, within the spec.
    pub async fn of(
        config: &Config,
        reported: &Reported,
        client: &reqwest::Client,
    ) -> Option<Self> {
        let opinions = config.opinions();
        if opinions.model.as_ref().and_then(|m| m.fallback) != Some(true) {
            return None;
        }
        let model = config.fallback.duplicate()?;
        let window = opinions.window.clone().unwrap_or_default();
        let known = limits::known(config.fallback_facts.as_ref(), &model);
        let limits = reported
            .of(
                client,
                &model,
                known.as_ref(),
                window.unknown.unwrap_or(1024),
            )
            .await
            .within(&opinions, &model.provider, &config.links());
        let opinion = opinions.model_for(&model.provider);
        Some(Self {
            answer: limits.answer,
            window: limits.window,
            price: limits.price,
            above: known.map(|k| k.facts.above).unwrap_or_default(),
            thinking: Think {
                thinks: limits.thinks,
                level: opinion.thinking.unwrap_or(link_agent_spec::Thinking::Off),
                budget: opinion.thinking_budget.unwrap_or(0),
            },
            model,
            swap: None,
        })
    }

    /// `prompt`, as this model is asked it.
    fn prompt<'a>(&'a self, prompt: &Prompt<'a>) -> Prompt<'a> {
        Prompt {
            system: prompt.system,
            messages: prompt.messages,
            tools: prompt.tools,
            no_calls: prompt.no_calls,
            answer: self.answer,
            window: self.window,
            price: self.price,
            above: &self.above,
            thinking: self.thinking,
            fast: prompt.fast,
            chat: prompt.chat,
            spans: prompt.spans,
            parent: prompt.parent,
            cache: prompt.cache,
        }
    }
}

/// Whether `prompt` is past its window: its tokens, counted as the
/// window's walk counts them, and its answer's.
pub fn past_window(prompt: &Prompt<'_>) -> bool {
    let tools: usize = prompt
        .tools
        .iter()
        .map(|t| {
            window::text_tokens(&t.name)
                + window::text_tokens(&t.description)
                + window::text_tokens(&t.schema.to_string())
        })
        .sum();
    let messages: usize = prompt
        .messages
        .iter()
        .map(|m| {
            window::text_tokens(&m.text)
                + m.answers
                    .iter()
                    .map(|a| window::text_tokens(&a.text))
                    .sum::<usize>()
                + m.calls
                    .iter()
                    .map(|c| {
                        window::text_tokens(&c.name) + window::text_tokens(&c.arguments.to_string())
                    })
                    .sum::<usize>()
        })
        .sum();
    let tokens = window::text_tokens(prompt.system) + tools + messages + prompt.answer as usize;
    tokens > prompt.window as usize
}

/// Asks `first` to answer `prompt`, or `second` when there is one and
/// the first can't (above), was found off lately (`off`, which this
/// keeps), or the turn already fell back (`using`, which this sets when
/// it does). The answer names the second and why.
#[expect(
    clippy::too_many_arguments,
    reason = "the turn's model call and its fallback's state, each used apart"
)]
pub async fn answer(
    client: &reqwest::Client,
    first: &Model,
    second: Option<&Second>,
    off: &Off,
    prompt: &Prompt<'_>,
    using: &mut Option<Why>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Event> {
    answer_observed(
        client, first, second, off, prompt, using, pieces, separate, cancelled, None,
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "fallback model inputs and optional per-attempt observation"
)]
pub async fn answer_observed(
    client: &reqwest::Client,
    first: &Model,
    second: Option<&Second>,
    off: &Off,
    prompt: &Prompt<'_>,
    using: &mut Option<Why>,
    pieces: &mpsc::Sender<String>,
    separate: bool,
    cancelled: tokio::sync::watch::Receiver<bool>,
    observer: Option<&dyn model::RequestObserver>,
) -> anyhow::Result<Event> {
    let Some(second) = second else {
        return model::answer_observed(
            client, first, prompt, pieces, separate, cancelled, None, observer,
        )
        .await;
    };
    if using.is_none() && past_window(prompt) {
        *using = Some(Why::Window);
    }
    if using.is_none() && off.lately(first) {
        *using = Some(Why::Off);
    }
    if using.is_none() {
        let asked = model::answer_observed(
            client,
            first,
            prompt,
            pieces,
            separate,
            cancelled.clone(),
            Some(FIRST_BYTE),
            observer,
        )
        .await;
        match asked {
            Err(err) if err.is::<Unreached>() => {
                off.found(first, true);
                *using = Some(Why::Off);
            }
            Err(err) if err.is::<TooLong>() => *using = Some(Why::Window),
            Err(err) if err.is::<Overloaded>() => *using = Some(Why::Overloaded),
            asked => {
                if asked.is_ok() {
                    off.found(first, false);
                }
                return asked;
            }
        }
    }
    let why = using.unwrap_or(Why::Off);
    let system = second.swap.as_ref().map(|swap| swap.apply(prompt.system));
    let asked = Prompt {
        system: system.as_deref().unwrap_or(prompt.system),
        ..second.prompt(prompt)
    };
    let mut answer = model::answer_observed(
        client,
        &second.model,
        &asked,
        pieces,
        separate,
        cancelled,
        None,
        observer,
    )
    .await?;
    answer.fallback = Some(why.name().to_string());
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fallback is asked in the same chat, its spans and its parent the
    /// first's, at its own limits: its calls are the chat's.
    #[test]
    fn a_fallback_is_asked_in_the_same_chat() {
        let second = Second {
            model: Model::default(),
            answer: 100,
            window: 1000,
            price: None,
            above: Default::default(),
            thinking: Think::default(),
            swap: None,
        };
        let spans = [link_model_contract::context::Span {
            name: "base".into(),
            start: 0,
            end: 1,
        }];
        let first = Prompt {
            system: "s",
            messages: &[],
            tools: &[],
            no_calls: false,
            answer: 1,
            window: 2,
            price: None,
            above: Default::default(),
            thinking: Think::default(),
            fast: true,
            chat: Some("main"),
            spans: &spans,
            parent: Some("telegram:42"),
            cache: None,
        };
        let asked = second.prompt(&first);
        assert!(asked.fast);
        assert_eq!(asked.chat, Some("main"));
        assert_eq!(asked.parent, Some("telegram:42"));
        assert_eq!(asked.spans, &spans);
        assert_eq!((asked.answer, asked.window), (100, 1000));
    }
}
