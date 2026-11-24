//! What a turn may fill: the window and the longest answer, in tokens.
//!
//! The model's own figures are facts, never Link Harness's: what Link
//! and you know of the model (`link_model_contract`, handed with the
//! agent's settings), a runtime on this machine's report of the model it
//! runs ([`reported`]), else the agent's spec's small window for a model
//! nobody described (`[window] unknown`). The order is yours, a local
//! runtime's, Link's, then the small one: your models file corrects
//! anything, and a runtime knows the window it runs a model with better
//! than any file.
//!
//! How much of it to fill is the spec's (`[window] max`, and `[window]
//! answer` or `[model] answer` for the answer). A default of Link's spec is held to what the model takes; a
//! value you (or a team) set is taken as you set it, whatever Link knows
//! of the model, so the provider refusing is the only limit, and the
//! harness says so when it does.
//!
//! The window a turn fills ([`Limits::budget`]) is that, less room for
//! the answer, for the system prompt and tools, and for counting tokens
//! roughly ([`crate::window::tokens`]) before the provider counts them
//! (`[window] margin`).

use std::{collections::HashMap, sync::Mutex, time::Duration};

use link_agent_spec::Spec;
use link_harness_contract::harness::Model;
pub use link_model_contract::Thinks;
use link_model_contract::{Known, Price};

/// What a turn may fill, in tokens, how the model is asked to think, and
/// what it costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// The window a turn fills, the answer included.
    pub window: u32,
    /// The longest answer it's asked for.
    pub answer: u32,
    pub thinks: Thinks,
    /// The share of the window kept back for counting roughly.
    pub margin: f64,
    pub price: Option<Price>,
}

/// How long a runtime has to say what it runs.
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

/// A model's own figures, before the spec: its window, and its longest
/// answer where it's known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Own {
    pub window: u32,
    pub answer: Option<u32>,
    pub thinks: Thinks,
    pub price: Option<Price>,
    /// The largest prompt priced as `price`, where its price jumps past
    /// it (the facts' first `above`), in tokens.
    pub jump: Option<u32>,
}

impl Own {
    /// What `known` says, where it says the window.
    pub fn of(known: &Known) -> Option<Self> {
        Some(Self {
            window: known.facts.window?,
            answer: known.facts.answer,
            thinks: known.facts.thinking.unwrap_or_default(),
            price: known.facts.price,
            jump: known.facts.first_jump(),
        })
    }

    /// A model nobody described, or one only its runtime did: a window
    /// of `window`, its longest answer not known.
    pub fn window(window: u32) -> Self {
        Self {
            window,
            answer: None,
            thinks: Thinks::No,
            price: None,
            jump: None,
        }
    }

    /// Within what the agent's `spec` says, for `provider`: its `[window]
    /// max`, held to the model's window when it's Link's (one of `links`,
    /// the keys no team or you set) and taken as set when it's yours; and
    /// the answer, `[model] answer` as you set it, else `[window]
    /// answer`'s share of the window, at most the model's longest. Either
    /// way, held to where the model's price jumps, unless `[window]
    /// past_price_jump`.
    pub fn within(self, spec: &Spec, provider: &str, links: &[String]) -> Limits {
        let window_spec = spec.window.clone().unwrap_or_default();
        let links_key = |key: &str| links.iter().any(|k| k == key);
        let window = match window_spec.max {
            Some(max) if links_key("window.max") => max.min(self.window),
            Some(max) => max,
            None => self.window,
        };
        let window = match (self.jump, window_spec.past_price_jump.unwrap_or(false)) {
            (Some(jump), false) => window.min(jump),
            _ => window,
        };
        let model = spec.model_for(provider);
        let answer_key = match provider_answer(spec, provider) {
            true => format!("model.{provider}.answer"),
            false => "model.answer".to_string(),
        };
        let share = (window as f64 * window_spec.answer.unwrap_or(0.0)) as u32;
        let own = self.answer.unwrap_or(u32::MAX);
        let answer = match model.answer {
            Some(answer) if links_key(&answer_key) => answer.min(own),
            Some(answer) => answer,
            None => share.min(own),
        };
        Limits {
            window,
            answer,
            thinks: self.thinks,
            margin: window_spec.margin.unwrap_or(0.0),
            price: self.price,
        }
    }
}

/// Whether `spec`'s table for `provider` sets the answer itself.
fn provider_answer(spec: &Spec, provider: &str) -> bool {
    let Some(model) = &spec.model else {
        return false;
    };
    let table = match provider {
        "anthropic" => &model.anthropic,
        "openai" => &model.openai,
        "openrouter" => &model.openrouter,
        "ollama" => &model.ollama,
        "local" => &model.local,
        _ => &None,
    };
    table.as_ref().is_some_and(|t| t.answer.is_some())
}

impl Limits {
    /// How many tokens of the conversation a turn sends: the window,
    /// less the longest answer, the margin kept back, and `fixed`, the
    /// system prompt's and tools' tokens. Never under one.
    pub fn budget(&self, fixed: usize) -> usize {
        let window = self.window as usize;
        let kept = (window as f64 * self.margin.clamp(0.0, 1.0)) as usize;
        window
            .saturating_sub(self.answer as usize)
            .saturating_sub(kept)
            .saturating_sub(fixed)
            .max(1)
    }
}

/// What each runtime on this machine said of a model, by its address
/// and the model's name: derived, asked again by a process without it.
#[derive(Default)]
pub struct Reported(Mutex<HashMap<(String, String), Option<u32>>>);

impl Reported {
    /// `model`'s own figures: your models file's window, else its
    /// runtime's on this machine (asked once, at most `unknown` where the
    /// runtime says only what the model was trained for), else Link's,
    /// else a window of `unknown`. `known`: what Link and you know of it.
    pub async fn of(
        &self,
        client: &reqwest::Client,
        model: &Model,
        known: Option<&Known>,
        unknown: u32,
    ) -> Own {
        let described = known.and_then(Own::of);
        let yours = known.is_some_and(|k| k.yours.iter().any(|f| f == "window"));
        if let (Some(own), true) = (described, yours) {
            return own;
        }
        let runtime = match (model.named(), model.base_url.clone()) {
            (Some(name), Some(base)) if model.is_local() => {
                let key = (base.clone(), name.clone());
                let asked = self
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&key)
                    .copied();
                match asked {
                    Some(window) => window,
                    None => {
                        let window = match &model.actions_url {
                            Some(url) => {
                                match link_model_contract::chat::client::canonical::client() {
                                    Ok(client) => {
                                        link_model_contract::chat::client::canonical::get::<
                                            link_model_contract::show::Shown,
                                        >(
                                            &client, url, &format!("show?name={name}")
                                        )
                                        .await
                                        .ok()
                                        .and_then(|shown| window_of(&shown, unknown))
                                    }
                                    Err(_) => None,
                                }
                            }
                            None => reported(client, &base, &name, unknown).await,
                        };
                        self.0
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key, window);
                        window
                    }
                }
            }
            _ => None,
        };
        match (runtime, described) {
            (Some(window), Some(own)) => Own { window, ..own },
            (Some(window), None) => Own::window(window),
            (None, Some(own)) => own,
            (None, None) => Own::window(unknown),
        }
    }
}

/// The window a runtime at `base` says it runs model `name` with, when
/// it says: asked in Link's shape, at the local model route's
/// `/_link/model/show`, whatever runs the model ([`window_of`]).
pub async fn reported(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    unknown: u32,
) -> Option<u32> {
    let target = link_model_contract::show::target(name)?;
    let answer = client
        .get(format!("{}{target}", base.trim_end_matches('/')))
        .timeout(ASK_TIMEOUT)
        .send()
        .await
        .ok()
        .filter(|answer| answer.status().is_success())?;
    let shown = serde_json::from_slice(&answer.bytes().await.ok()?).ok()?;
    window_of(&shown, unknown)
}

/// The window to give a model its runtime shows as `shown`: the one it
/// runs it with, else the one it was trained for at most `unknown`, since
/// a runtime (Ollama's) runs a model with a smaller window of its own
/// unless told. None below 1,024 tokens.
pub fn window_of(shown: &link_model_contract::show::Shown, unknown: u32) -> Option<u32> {
    shown
        .context
        .or(shown.trained.map(|trained| trained.min(unknown)))
        .filter(|window| *window >= 1024)
}

/// What Link and you know of `model`: as handed with the agent's
/// settings when they're of this model, else Link's own file's.
pub fn known(handed: Option<&Known>, model: &Model) -> Option<Known> {
    let name = model.named()?;
    if let Some(handed) = handed.filter(|k| k.name == name) {
        return Some(handed.clone());
    }
    link_model_contract::facts(
        &name,
        &link_model_contract::link(),
        &link_model_contract::File::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: Own = Own {
        window: 1_000_000,
        answer: Some(128_000),
        thinks: Thinks::Adaptive,
        price: None,
        jump: None,
    };

    fn spec(text: &str) -> (Spec, Vec<String>) {
        let own = match text.is_empty() {
            true => None,
            false => Some(link_agent_spec::parse(&format!("schema = 1\n{text}")).unwrap()),
        };
        let known = link_agent_spec::Known {
            name: "main".into(),
        };
        let resolved = link_agent_spec::resolve(own.as_ref(), &[], &known).unwrap();
        let links = resolved.links();
        (resolved.spec, links)
    }

    #[test]
    fn links_window_is_held_to_the_models_and_yours_wins() {
        // Link's spec: 128,000 at most, a quarter of it for the answer.
        let (links_spec, links) = spec("");
        let limits = CLAUDE.within(&links_spec, "anthropic", &links);
        assert_eq!((limits.window, limits.answer), (128_000, 32_000));
        assert_eq!(limits.margin, 0.125);
        // A small model keeps its own.
        let small = Own::window(32_768).within(&links_spec, "ollama", &links);
        assert_eq!((small.window, small.answer), (32_768, 8_192));
        // Yours is taken as you set it, above what Link knows of it.
        let (yours, links) = spec("[window]\nmax = 2000000\n[model]\nanswer = 200000\n");
        let limits = CLAUDE.within(&yours, "anthropic", &links);
        assert_eq!((limits.window, limits.answer), (2_000_000, 200_000));
        // A provider's own answer of yours, Link's window held as before;
        // a share of a big window, at most the model's longest.
        let (yours, links) = spec("[model.anthropic]\nanswer = 9000\n");
        let limits = CLAUDE.within(&yours, "anthropic", &links);
        assert_eq!((limits.window, limits.answer), (128_000, 9_000));
        let (yours, links) = spec("[window]\nmax = 1000000\n");
        let limits = CLAUDE.within(&yours, "anthropic", &links);
        assert_eq!((limits.window, limits.answer), (1_000_000, 128_000));
    }

    #[test]
    fn a_window_stops_where_the_price_jumps_unless_past_it() {
        let haiku = known(
            None,
            &Model {
                provider: "anthropic".into(),
                name: Some("claude-haiku-5-5".into()),
                ..Model::default()
            },
        )
        .and_then(|k| Own::of(&k))
        .unwrap();
        assert_eq!(haiku.jump, Some(100_000));
        let (links_spec, links) = spec("");
        assert_eq!(
            haiku.within(&links_spec, "anthropic", &links).window,
            100_000
        );
        let (yours, links) = spec("[window]\nmax = 1000000\n");
        assert_eq!(haiku.within(&yours, "anthropic", &links).window, 100_000);
        let (past, links) = spec("[window]\nmax = 1000000\npast_price_jump = true\n");
        assert_eq!(haiku.within(&past, "anthropic", &links).window, 1_000_000);
    }

    #[test]
    fn the_budget_leaves_room_for_the_answer_and_the_count() {
        let limits = Limits {
            window: 32_768,
            answer: 4_096,
            thinks: Thinks::No,
            margin: 0.125,
            price: None,
        };
        assert_eq!(limits.budget(0), 32_768 - 4_096 - 4_096);
        assert_eq!(limits.budget(1 << 30), 1);
        let none = Limits {
            margin: 0.0,
            ..limits
        };
        assert_eq!(none.budget(100), 32_768 - 4_096 - 100);
    }

    #[test]
    fn the_window_run_with_or_the_trained_one_held_to_the_small_one() {
        use link_model_contract::show::Shown;
        let shown = |context, trained| Shown {
            name: "qwen3:8b".into(),
            context,
            trained,
            ..Shown::default()
        };
        assert_eq!(
            window_of(&shown(Some(65_536), Some(131_072)), 32_768),
            Some(65_536)
        );
        assert_eq!(window_of(&shown(None, Some(131_072)), 32_768), Some(32_768));
        assert_eq!(window_of(&shown(None, Some(8_192)), 32_768), Some(8_192));
        assert_eq!(window_of(&shown(Some(512), None), 32_768), None);
        assert_eq!(window_of(&shown(None, None), 32_768), None);
    }

    #[test]
    fn what_is_known_of_a_model() {
        let model = |name: &str| Model {
            provider: "anthropic".into(),
            name: Some(name.into()),
            ..Model::default()
        };
        let sonnet = known(None, &model("claude-sonnet-5-5")).unwrap();
        assert_eq!(Own::of(&sonnet).unwrap().window, 1_000_000);
        assert!(known(None, &model("someone-elses-model")).is_none());
        // What came with the settings, for the model they're of.
        let handed = Known {
            name: "someone-elses-model".into(),
            facts: link_model_contract::Facts {
                window: Some(50_000),
                ..Default::default()
            },
            yours: vec!["window".into()],
        };
        let found = known(Some(&handed), &model("someone-elses-model")).unwrap();
        assert_eq!(found.facts.window, Some(50_000));
        assert!(
            known(Some(&handed), &model("claude-sonnet-5-5"))
                .unwrap()
                .yours
                .is_empty()
        );
    }
}
