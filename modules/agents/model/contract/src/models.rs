//! What Link knows of models: each one's window, its longest answer, how
//! it thinks, and what it costs. Facts, not opinions, so data, never
//! code: Link includes them as a file ([`LINK_MODELS`]), refreshed with the
//! models' current figures at every release, and yours
//! (`~/.lnk/safe/models/facts.toml`, [`path`]) adds a model or corrects one,
//! field by field, yours winning ([`facts`]), so a model released after
//! your Link is described without a new one.
//!
//! The harness module hands a harness the facts of its agent's model with
//! its settings (`Settings::model_facts`), so any harness reads them;
//! `lnk model show` prints them.

pub mod providers;
mod refresh;
pub mod route;
pub mod selection;
pub mod show;

use std::collections::BTreeMap;

use anyhow::ensure;
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

pub use refresh::{current_generation, generation, head, plausible_change, print, refresh};

/// The model facts Link includes, refreshed at every release.
pub const LINK_MODELS: &str = include_str!("models/link.toml");

/// The largest models file Link reads: Link's own, refreshed, is about a
/// hundred kilobytes.
pub const MAX_BYTES: u64 = 1 << 20;

/// One model's facts, as a file writes them: each optional, so your file
/// may correct one field of Link's.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    /// Its whole window, the answer included, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// The longest answer it writes, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<u32>,
    /// How it's asked to think.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinks>,
    /// What it costs, in US dollars a million tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    /// Where its price jumps, lowest first: a prompt past a jump's
    /// `tokens`, the whole prompt, the cache's tokens included, costs
    /// that jump's price instead, as its provider charges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub above: Vec<Jump>,
    /// What its provider's prompt cache offers, the one it keeps unasked
    /// first: each lifetime a request may ask its prompt kept, and what
    /// writing it costs ([`crate::caching`]). Empty: Link knows no cache
    /// of it, and chooses nothing of one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache: Vec<Offer>,
}

/// One lifetime a provider's prompt cache offers (`cache`).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    /// How long a prompt written to it is kept from the request that
    /// wrote or last read it (`"5m"`, `"1h"`), as its provider writes it.
    pub lasts: String,
    /// What writing a token to it costs, as a multiple of the prompt's
    /// input price (`1.25`).
    pub write: f64,
}

impl Offer {
    /// How long it keeps a prompt.
    pub fn lifetime(&self) -> Option<std::time::Duration> {
        crate::caching::duration(&self.lasts)
    }
}

/// A price a model charges for a long prompt (`above`): past `tokens`
/// of prompt, the whole call costs `price`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Jump {
    /// The prompt's size past which its price applies, in tokens.
    pub tokens: u32,
    pub price: Price,
}

impl Facts {
    /// What a call with a prompt of `prompt` tokens costs a million
    /// tokens: the price of the last jump it's past, else `price`.
    pub fn price_at(&self, prompt: u64) -> Option<Price> {
        self.price.map(|price| price_at(price, &self.above, prompt))
    }

    /// The largest prompt priced as `price`, in tokens: the first jump's,
    /// where there is one.
    pub fn first_jump(&self) -> Option<u32> {
        self.above.first().map(|jump| jump.tokens)
    }
}

/// `price`, or the price of the last of `above` a prompt of `prompt`
/// tokens is past.
pub fn price_at(price: Price, above: &[Jump], prompt: u64) -> Price {
    above
        .iter()
        .rev()
        .find(|jump| prompt > u64::from(jump.tokens))
        .map_or(price, |jump| jump.price)
}

/// How a model is asked to think, as its provider documents it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Thinks {
    /// Not at all, or not in a way Link knows.
    #[default]
    No,
    /// Anthropic's adaptive thinking, at an effort.
    Adaptive,
    /// Anthropic's thinking within a budget of tokens.
    Budget,
    /// Reasoning at an effort, as OpenAI and OpenRouter ask it.
    Effort,
}

/// What a model costs, in US dollars a million tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// A million tokens of the prompt.
    pub input: f64,
    /// A million tokens of the answer, thinking included.
    pub output: f64,
    /// A million tokens of the prompt read from the provider's cache;
    /// left out, as `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached: Option<f64>,
}

impl Price {
    /// What `input` tokens of the prompt, `cached` of them from the
    /// provider's cache, and `output` of the answer cost, in US dollars.
    pub fn of(&self, input: u64, cached: u64, output: u64) -> f64 {
        let cached = cached.min(input);
        let million = 1_000_000.0;
        (input - cached) as f64 * self.input / million
            + cached as f64 * self.cached.unwrap_or(self.input) / million
            + output as f64 * self.output / million
    }
}

/// A models file: Link's or yours.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Each model's facts, by the provider's or a router's name for it.
    #[serde(default)]
    pub model: BTreeMap<String, Facts>,
}

/// A model's facts as Link and you know them, and which are yours.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Known {
    /// The model, as asked.
    pub name: String,
    #[serde(flatten)]
    pub facts: Facts,
    /// The fields your file set (`window`), which win over anything Link
    /// knows or a runtime reports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub yours: Vec<String>,
}

link_files::specification!(File, include_str!("models.schema.toml"));

/// Parses `text` as a models file, strictly, its values checked.
pub fn parse(text: &str) -> anyhow::Result<File> {
    let file: File = link_files::saved::parse(text)?;
    for (name, facts) in &file.model {
        check(name, facts).within(format!("in [model.{:?}]", link_format::clean(name)))?;
    }
    Ok(file)
}

/// Checks one model's facts: a name a provider could give, figures a
/// model could have.
pub fn check(name: &str, facts: &Facts) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control),
        "{:?} isn't a model's name",
        link_format::clean(name)
    );
    if let Some(window) = facts.window {
        ensure!(window >= 1024, "window is at least 1024 tokens");
    }
    if let Some(answer) = facts.answer {
        ensure!(answer >= 256, "answer is at least 256 tokens");
    }
    let jumps = facts.above.iter().map(|jump| ("above.price", &jump.price));
    for (name, price) in facts.price.iter().map(|p| ("price", p)).chain(jumps) {
        for (key, value) in [
            ("input", Some(price.input)),
            ("output", Some(price.output)),
            ("cached", price.cached),
        ] {
            if let Some(value) = value {
                ensure!(
                    value.is_finite() && value >= 0.0,
                    "{name}.{key} is dollars a million tokens, not {value}"
                );
            }
        }
    }
    if !facts.above.is_empty() {
        ensure!(facts.price.is_some(), "above needs a price to jump from");
    }
    let mut longest = std::time::Duration::ZERO;
    for offer in &facts.cache {
        let lasts = offer.lifetime().filter(|lasts| *lasts > longest);
        ensure!(
            lasts.is_some(),
            "cache's lifetimes are durations, each longer than the last, not {:?}",
            link_format::clean(&offer.lasts)
        );
        longest = lasts.unwrap_or_default();
        ensure!(
            offer.write.is_finite() && offer.write >= 1.0,
            "cache's write is a multiple of the input price, at least 1, not {}",
            offer.write
        );
    }
    let mut last = 0;
    for jump in &facts.above {
        ensure!(
            jump.tokens > last,
            "above's jumps are each past the last, lowest first, not {}",
            jump.tokens
        );
        if let Some(window) = facts.window {
            ensure!(
                jump.tokens < window,
                "above's {} tokens are within the window, {window}",
                jump.tokens
            );
        }
        last = jump.tokens;
    }
    Ok(())
}

/// The models Link's file no longer describes, each by its [`plain`]
/// name, with the current model of its line, as Link's file names it.
pub const RETIRED: &str = include_str!("models/retired.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Retired {
    retired: BTreeMap<String, String>,
}

/// [`RETIRED`], parsed once: each retired model's plain name, and the
/// model that replaces it.
pub fn retired() -> &'static BTreeMap<String, String> {
    static RETIRED_MODELS: std::sync::LazyLock<BTreeMap<String, String>> =
        std::sync::LazyLock::new(|| {
            toml::from_str::<Retired>(RETIRED)
                .expect("retired.toml is checked by its test")
                .retired
        });
    &RETIRED_MODELS
}

/// Link's facts, parsed once.
pub fn link() -> File {
    // Checked by a test, and by the release before it ships a refreshed
    // file: Link's own always parses.
    static LINK: std::sync::LazyLock<File> =
        std::sync::LazyLock::new(|| parse(LINK_MODELS).unwrap_or_default());
    LINK.clone()
}

/// Where your models file is: `~/.lnk/safe/models/facts.toml`.
pub fn path() -> anyhow::Result<std::path::PathBuf> {
    link_files::layout::Hidden::ModelFacts.try_path()
}

/// Your facts; none when there's no file.
pub fn yours() -> anyhow::Result<File> {
    let path = path()?;
    let text = match std::fs::metadata(&path) {
        Ok(meta) => {
            ensure!(
                meta.len() <= MAX_BYTES,
                "{} is more than {MAX_BYTES} bytes",
                path.display()
            );
            std::fs::read_to_string(&path).within(format!("reading {}", path.display()))?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(err) => return Err(err).within(format!("reading {}", path.display())),
    };
    parse(&text).within(format!("in {}", path.display()))
}

/// `name` as Link's files match it: its own, after a router's prefix
/// (`anthropic/claude-sonnet-5.5`), without a dated snapshot's suffix
/// (Anthropic's `-20251001`, OpenAI's `-2024-08-06`), lowercase, `.` read
/// as `-`.
pub fn plain(name: &str) -> String {
    let name = name.rsplit('/').next().unwrap_or(name);
    let plain = name.to_ascii_lowercase().replace('.', "-");
    // `-` then the date's shape, `d` a digit.
    for shape in ["-dddddddd", "-dddd-dd-dd"] {
        let Some(at) = plain.len().checked_sub(shape.len()) else {
            continue;
        };
        let dated = plain.as_bytes()[at..]
            .iter()
            .zip(shape.bytes())
            .all(|(b, s)| {
                if s == b'd' {
                    b.is_ascii_digit()
                } else {
                    *b == s
                }
            });
        if at > 0 && dated {
            return plain[..at].to_string();
        }
    }
    plain
}

impl File {
    /// `name`'s entry: by its own name, else by its [`plain`] one.
    pub fn find(&self, name: &str) -> Option<&Facts> {
        if let Some(facts) = self.model.get(name) {
            return Some(facts);
        }
        let wanted = plain(name);
        self.model
            .iter()
            .find(|(known, _)| plain(known) == wanted)
            .map(|(_, facts)| facts)
    }
}

/// `name`'s facts: Link's (`link`), each field yours where `yours` sets
/// it. None when neither knows the model.
pub fn facts(name: &str, link: &File, yours: &File) -> Option<Known> {
    let theirs = link.find(name);
    let mine = yours.find(name);
    if theirs.is_none() && mine.is_none() {
        return None;
    }
    let theirs = theirs.cloned().unwrap_or_default();
    let mine = mine.cloned().unwrap_or_default();
    let mut set = Vec::new();
    for (key, is) in [
        ("window", mine.window.is_some()),
        ("answer", mine.answer.is_some()),
        ("thinking", mine.thinking.is_some()),
        ("price", mine.price.is_some()),
        ("above", !mine.above.is_empty()),
        ("cache", !mine.cache.is_empty()),
    ] {
        if is {
            set.push(key.to_string());
        }
    }
    Some(Known {
        name: name.to_string(),
        facts: Facts {
            window: mine.window.or(theirs.window),
            answer: mine.answer.or(theirs.answer),
            thinking: mine.thinking.or(theirs.thinking),
            price: mine.price.or(theirs.price),
            above: match mine.above.is_empty() {
                true => theirs.above,
                false => mine.above,
            },
            cache: match mine.cache.is_empty() {
                true => theirs.cache,
                false => mine.cache,
            },
        },
        yours: set,
    })
}

/// `name`'s facts as this machine knows them: Link's, and yours.
pub fn known(name: &str) -> anyhow::Result<Option<Known>> {
    Ok(facts(name, &link(), &yours()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lookup, on figures of its own: what the release's refresh brings
    /// changes none of it.
    #[test]
    fn a_model_by_any_of_its_names() {
        let link = parse(
            "[model.\"claude-sonnet-5-5\"]\nwindow = 1000000\n\
             [model.\"claude-sonnet-4-6\"]\nwindow = 900000\n\
             [model.\"claude-haiku-4-5\"]\nwindow = 200000\n\
             [model.\"gpt-4o\"]\nwindow = 128000\n\
             [model.\"gpt-5\"]\nwindow = 400000\n",
        )
        .unwrap();
        let window = |name: &str| link.find(name).and_then(|facts| facts.window);
        assert_eq!(window("claude-sonnet-5-5"), Some(1_000_000));
        assert_eq!(window("anthropic/claude-sonnet-4.6"), Some(900_000));
        assert_eq!(window("claude-haiku-4-5-20251001"), Some(200_000));
        assert_eq!(window("gpt-4o-2024-08-06"), Some(128_000));
        assert_eq!(window("openai/gpt-5"), Some(400_000));
        assert_eq!(window("qwen3:8b"), None);
        assert_eq!(window("claude-sonnet-5-5-x"), None);
        assert_eq!(plain("Anthropic/Claude-Sonnet-4.6"), "claude-sonnet-4-6");
        assert_eq!(plain("2024-08-06"), "2024-08-06");
    }

    /// A model's line is its name without numbers, and its generation the
    /// numbers in order, however its maker placed them.
    #[test]
    fn a_models_line_and_generation_are_its_names() {
        for (name, line, numbers) in [
            ("claude-sonnet-5-5", "claude-sonnet", vec![5, 5]),
            ("anthropic/claude-sonnet-4.5", "claude-sonnet", vec![4, 5]),
            ("anthropic/claude-3.5-sonnet", "claude-sonnet", vec![3, 5]),
            ("claude-opus-4-8-20260101", "claude-opus", vec![4, 8]),
            ("gpt-6.1-sol", "gpt-sol", vec![6, 1]),
            ("openai/gpt-4o-mini", "gpt-o-mini", vec![4]),
            ("openrouter/auto", "auto", vec![]),
        ] {
            assert_eq!(generation(name), (line.to_string(), numbers), "{name}");
        }
        let link = link();
        // Link's own first-party models are each the newest of their line.
        let owned = link
            .model
            .keys()
            .filter(|name| selection::owner(name).is_some());
        for name in owned {
            assert!(!current_generation(&link, name), "{name}");
        }
        assert!(current_generation(&link, "anthropic/claude-opus-6"));
        assert!(current_generation(&link, "openai/gpt-6.2-sol"));
        assert!(current_generation(&link, "deepseek/deepseek-v4"));
        for old in [
            "anthropic/claude-opus-4.5",
            "anthropic/claude-3-opus",
            "openai/gpt-5.5",
        ] {
            assert!(!current_generation(&link, old), "{old}");
        }
    }

    /// Link's file describes only current models: none it retired, and
    /// the model that replaces each.
    #[test]
    fn links_file_describes_no_retired_model() {
        let link = link();
        assert!(!retired().is_empty());
        for (old, now) in retired() {
            assert_eq!(*old, plain(old), "{old} is named by its plain name");
            assert!(link.find(old).is_none(), "{old} is retired");
            assert!(link.model.contains_key(now), "{old}'s {now} is described");
            assert!(!retired().contains_key(&plain(now)), "{now} is retired");
        }
    }

    #[test]
    fn yours_wins_field_by_field() {
        let link = parse(
            "[model.m]\nwindow = 200000\nanswer = 8000\nthinking = \"effort\"\n\
             price = { input = 1.0, output = 2.0 }\n",
        )
        .unwrap();
        let yours = parse("[model.\"router/M\"]\nwindow = 300000\n").unwrap();
        let known = facts("m", &link, &yours).unwrap();
        assert_eq!(known.facts.window, Some(300_000));
        assert_eq!(known.facts.answer, Some(8_000));
        assert_eq!(known.facts.thinking, Some(Thinks::Effort));
        assert_eq!(known.yours, ["window"]);
        // A model only you know.
        let new = facts("router/M", &File::default(), &yours).unwrap();
        assert_eq!(new.facts.window, Some(300_000));
        assert_eq!(new.facts.answer, None);
        assert!(facts("other", &link, &yours).is_none());
    }

    #[test]
    fn a_wrong_figure_is_refused() {
        for bad in [
            "[model.m]\nwindow = 10\n",
            "[model.m]\nanswer = 1\n",
            "[model.m]\nprice = { input = -1.0, output = 1.0 }\n",
            "[model.m]\nwindows = 100000\n",
            "[model.\"\"]\nwindow = 100000\n",
            "[model.m]\nthinking = \"lots\"\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_release_refreshes_the_figures_and_keeps_the_rest() {
        let current = parse(
            "[model.\"claude-sonnet-5-5\"]\nwindow = 200000\nanswer = 64000\n\
             thinking = \"adaptive\"\n\
             [model.\"gpt-6.1-sol\"]\nwindow = 1050000\nthinking = \"effort\"\n",
        )
        .unwrap();
        let model = |id: &str, window: u64, reasoning: bool| {
            serde_json::json!({"id": id, "context_length": window,
                "top_provider": {"max_completion_tokens": 128000},
                "pricing": {"prompt": "0.000002", "completion": "0.00001",
                    "input_cache_read": "0.0000002"},
                "supported_parameters": if reasoning { vec!["reasoning"] } else { vec![] }})
        };
        let mut data = vec![
            model("anthropic/claude-sonnet-5.5", 1_000_000, true),
            model("openai/gpt-7-sol", 2_000_000, true),
            model("meta-llama/llama-9", 131_072, false),
            model("anthropic/claude-haiku-4.5", 200_000, true),
            model("claude-opus-4-8-20260101", 1_000_000, true),
            model("anthropic/claude-sonnet-4.5", 1_000_000, true),
            model("anthropic/claude-3.5-haiku", 200_000, false),
            model("openai/gpt-4o-mini", 128_000, false),
            model("openai/gpt-6-sol", 1_050_000, true),
            model("anthropic/claude-mythos-5.2", 1_000_000, true),
            serde_json::json!({"id": "openrouter/auto", "context_length": 2000000,
                "pricing": {"prompt": "-1", "completion": "-1"}}),
            serde_json::json!({"id": "tiny", "context_length": 10}),
        ];
        for n in 0..10 {
            data.push(model(&format!("x/filler-{n}"), 8192, false));
        }
        let (refreshed, refused) = refresh(&current, &serde_json::json!({ "data": data })).unwrap();
        assert!(refused.is_empty(), "{refused:?}");
        // Matched by its plain name: its figures new, how it thinks kept.
        let sonnet = &refreshed.model["claude-sonnet-5-5"];
        assert_eq!(sonnet.window, Some(1_000_000));
        assert_eq!(sonnet.answer, Some(128_000));
        assert_eq!(sonnet.thinking, Some(Thinks::Adaptive));
        let price = sonnet.price.unwrap();
        assert_eq!(
            (price.input, price.output, price.cached),
            (2.0, 10.0, Some(0.2))
        );
        assert!(!refreshed.model.contains_key("anthropic/claude-sonnet-5.5"));
        // New ones by their own name.
        assert_eq!(
            refreshed.model["openai/gpt-7-sol"].thinking,
            Some(Thinks::Effort)
        );
        assert_eq!(
            refreshed.model["meta-llama/llama-9"].thinking,
            Some(Thinks::No)
        );
        // A router's price that isn't one is no price; nonsense is left out.
        assert_eq!(refreshed.model["openrouter/auto"].price, None);
        assert!(!refreshed.model.contains_key("tiny"));
        // An older generation of a first party's line stays out, as does
        // a line Link doesn't describe; a newer generation enters.
        for old in [
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-3.5-haiku",
            "openai/gpt-4o-mini",
            "openai/gpt-6-sol",
            "anthropic/claude-mythos-5.2",
        ] {
            assert!(!refreshed.model.contains_key(old), "{old}");
        }
        // A retired model stays out, under any router's name for it.
        assert!(
            refreshed
                .model
                .keys()
                .all(|name| !retired().contains_key(&plain(name)))
        );
        // Printed, it reads back the same; too few models is refused.
        let printed = print("# head", &refreshed).unwrap();
        assert_eq!(parse(&printed).unwrap(), refreshed);
        assert!(refresh(&current, &serde_json::json!({"data": []})).is_err());
        // Link's own file prints as it reads.
        let link = link();
        assert_eq!(
            parse(&print(&head(LINK_MODELS), &link).unwrap()).unwrap(),
            link
        );
    }

    /// A refresh that would shrink a window past half, or move a price
    /// tenfold, to nothing or from nothing, keeps the reviewed figures
    /// and says so; a model new to the file is taken as listed.
    #[test]
    fn a_refresh_is_bound_by_the_reviewed_file() {
        let current = parse(
            "[model.a]\nwindow = 200000\nprice = { input = 3.0, output = 15.0, cached = 0.3 }\n\
             [model.b]\nwindow = 200000\nprice = { input = 3.0, output = 15.0 }\n\
             [model.c]\nwindow = 200000\nprice = { input = 3.0, output = 15.0 }\n\
             [model.d]\nwindow = 200000\nprice = { input = 3.0, output = 15.0 }\n",
        )
        .unwrap();
        let model = |id: &str, window: u64, input: &str, output: &str| {
            serde_json::json!({"id": id, "context_length": window,
                "pricing": {"prompt": input, "completion": output}})
        };
        let mut data = vec![
            model("x/a", 1024, "0.000003", "0.000015"),
            model("x/b", 200_000, "0", "0"),
            model("x/c", 200_000, "0.01", "0.000015"),
            model("x/d", 128_000, "0.000002", "0.00002"),
        ];
        for n in 0..10 {
            data.push(model(&format!("x/new-{n}"), 8192, "0.000001", "0.000001"));
        }
        let (refreshed, refused) = refresh(&current, &serde_json::json!({ "data": data })).unwrap();
        for name in ["a", "b", "c"] {
            assert_eq!(refreshed.model[name], current.model[name], "{name}");
        }
        assert_eq!(refused.len(), 3, "{refused:?}");
        assert!(
            refused[0].contains("window would go from 200000 to 1024"),
            "{refused:?}"
        );
        // Within bounds: taken.
        let d = &refreshed.model["d"];
        assert_eq!(d.window, Some(128_000));
        assert_eq!(d.price.map(|p| (p.input, p.output)), Some((2.0, 20.0)));
        assert_eq!(refreshed.model["x/new-0"].window, Some(8192));
    }

    /// Aliases of one entry can't walk it past the bound a step at a
    /// time: several, none its own id, leave it as reviewed; its own id
    /// listed is the one taken; a longest answer more than halved is
    /// refused too.
    #[test]
    fn aliases_cant_chain_a_refresh_past_its_bound() {
        let current = parse(
            "[model.claude-x]\nwindow = 1000000\nanswer = 64000\n\
             [model.claude-y]\nwindow = 200000\n\
             [model.claude-z]\nwindow = 200000\nanswer = 64000\n",
        )
        .unwrap();
        let model = |id: &str, window: u64, answer: u64| {
            serde_json::json!({"id": id, "context_length": window,
                "top_provider": {"max_completion_tokens": answer}})
        };
        let mut data = vec![
            model("a/claude-x", 600_000, 64000),
            model("b/claude-x", 360_000, 64000),
            model("c/claude-x-20260101", 200_000, 64000),
            model("claude-y", 150_000, 8192),
            model("a/claude-y", 1024, 8192),
            model("claude-z", 200_000, 1024),
        ];
        for n in 0..10 {
            data.push(model(&format!("x/new-{n}"), 8192, 1024));
        }
        let (refreshed, refused) = refresh(&current, &serde_json::json!({ "data": data })).unwrap();
        assert_eq!(refreshed.model["claude-x"], current.model["claude-x"]);
        assert_eq!(refreshed.model["claude-y"].window, Some(150_000));
        assert_eq!(refreshed.model["claude-z"], current.model["claude-z"]);
        assert!(
            refused.iter().any(|r| r.contains("more than one model")),
            "{refused:?}"
        );
        assert!(
            refused.iter().any(|r| r.contains("longest answer")),
            "{refused:?}"
        );
    }

    #[test]
    fn a_long_prompt_costs_its_jumps_price() {
        let file = parse(
            "[model.m]\nwindow = 1000000\n\
             price = { input = 0.1, output = 0.5, cached = 0.01 }\n\
             above = [{ tokens = 100000, price = { input = 0.5, output = 2.5, cached = 0.05 } }]\n",
        )
        .unwrap();
        let facts = &file.model["m"];
        assert_eq!(facts.first_jump(), Some(100_000));
        assert_eq!(facts.price_at(100_000).unwrap().input, 0.1);
        assert_eq!(facts.price_at(100_001).unwrap().input, 0.5);
        assert_eq!(facts.price_at(100_001).unwrap().cached, Some(0.05));
        // Printed, it reads back the same.
        let printed = print("# m", &file).unwrap();
        assert!(
            printed.contains("above = [{ tokens = 100000, price = {"),
            "{printed}"
        );
        // Jumps lowest first, within the window, from a price.
        for wrong in [
            "[model.m]\nprice = { input = 1.0, output = 1.0 }\n\
             above = [{ tokens = 200, price = { input = 2.0, output = 2.0 } }, \
             { tokens = 100, price = { input = 3.0, output = 3.0 } }]\n",
            "[model.m]\nwindow = 100000\nprice = { input = 1.0, output = 1.0 }\n\
             above = [{ tokens = 100000, price = { input = 2.0, output = 2.0 } }]\n",
            "[model.m]\nabove = [{ tokens = 100, price = { input = 2.0, output = 2.0 } }]\n",
        ] {
            assert!(parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_price_counts_the_cache() {
        let price = Price {
            input: 2.0,
            output: 10.0,
            cached: Some(0.2),
        };
        let cost = price.of(1_000_000, 500_000, 100_000);
        assert!((cost - (1.0 + 0.1 + 1.0)).abs() < 1e-9, "{cost}");
        let plain = Price {
            cached: None,
            ..price
        };
        assert!((plain.of(1_000, 2_000, 0) - 0.002).abs() < 1e-12);
    }
}
