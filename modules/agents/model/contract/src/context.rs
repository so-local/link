//! What a model call's prompt is spent on: the census of its request
//! into named parts ([`census`]), the record the model bridge keeps of
//! each call ([`Call`]), and the report of a chat built from them
//! ([`Report`]). Names and numbers only, never a word of the prompt.
//!
//! A part is named by a path of stable names: `instructions`,
//! `tools/<name>`, and under `conversation/`, `user`, `assistant`,
//! `thinking`, `calls/<tool>`, `results/<tool>` and `media/<kind>`.
//! Each carries its exact bytes and a weight, about as many tokens as
//! its kind takes ([`text_tokens`], [`IMAGE_TOKENS`], [`PDF_TOKENS_MAX`]).
//! A call's measured prompt is split among its parts by weight
//! ([`How::Apportioned`]); with no measured usage, the weight stands
//! ([`How::Estimated`]). A caller that composed its system prompt names
//! its ranges ([`Span`]), each counted beneath `instructions` by name.

use serde::{Deserialize, Serialize};

use crate::Price;
use crate::chat::Tokens;

mod census;
pub use census::{Api, census, fingerprint, opening};

/// About as many tokens as an image takes: Anthropic counts width ×
/// height / 750, about 1,600 for an image as large as it reads without
/// scaling it down (1,092 × 1,092), and OpenAI's tiles come to as many.
pub const IMAGE_TOKENS: usize = 1_600;

/// The most tokens a PDF is counted as: 100 pages at about 3,000 each,
/// as Anthropic reads a page, its text and an image of it.
pub const PDF_TOKENS_MAX: usize = 300_000;

/// About how many tokens `text` takes, where the provider hasn't
/// counted: four ASCII characters to a token, and a token for each
/// character outside it, which a tokenizer splits finer (Chinese, an
/// emoji).
pub fn text_tokens(text: &str) -> usize {
    let (ascii, other) = text.chars().fold((0, 0), |(a, o), c| match c.is_ascii() {
        true => (a + 1, o),
        false => (a, o + 1),
    });
    ascii / 4 + other
}

/// The folder of `agent`'s calls in Link's `settings`:
/// an error unless `agent` is an agent's ID.
pub fn dir(settings: &std::path::Path, agent: &str) -> anyhow::Result<std::path::PathBuf> {
    anyhow::ensure!(
        (1..=64).contains(&agent.len())
            && agent
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
            && !agent.starts_with('.'),
        "{:?} isn't an agent's ID",
        link_format::clean(agent)
    );
    Ok(link_files::layout::Hidden::ModelContext
        .at(settings)
        .join(agent))
}

/// Forgets `agent`'s calls in Link's `settings`, as it
/// goes: nothing to forget is no error.
pub fn forget(settings: &std::path::Path, agent: &str) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(dir(settings, agent)?) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// The most distinct parts a call is split into: past it, a part is
/// counted in its kind's `other` (`tools/other`, `conversation/calls/other`).
pub const PARTS_MAX: usize = 256;

/// The longest name a path keeps of a tool, else it's `other`.
pub const NAME_MAX: usize = 128;

/// A range of a system prompt its caller composed, by byte: which part
/// of the instructions it is (`skills/tides`), plain names joined by `/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub name: String,
    pub start: u64,
    pub end: u64,
}

/// `system` split by `spans`, in order and none overlapping, each a part
/// `instructions/<name>`, the bytes in none `instructions`: none when a
/// span isn't so, or isn't on `system`'s characters.
pub fn split(system: &str, spans: &[Span]) -> Option<Vec<Part>> {
    let mut parts: Vec<Part> = Vec::new();
    let mut add = |path: String, text: &str| match parts.iter_mut().find(|p| p.path == path) {
        Some(part) => {
            part.bytes += text.len() as u64;
            part.weight += text_tokens(text) as u64;
        }
        None if !text.is_empty() => parts.push(Part {
            path,
            bytes: text.len() as u64,
            weight: text_tokens(text) as u64,
        }),
        None => {}
    };
    let mut at = 0;
    for span in spans.iter().take(PARTS_MAX) {
        let (start, end) = (
            usize::try_from(span.start).ok()?,
            usize::try_from(span.end).ok()?,
        );
        if start < at || end < start || !plain(&span.name) {
            return None;
        }
        add("instructions".into(), system.get(at..start)?);
        add(
            format!("instructions/{}", span.name),
            system.get(start..end)?,
        );
        at = end;
    }
    add("instructions".into(), system.get(at..)?);
    Some(parts)
}

/// Whether `name` is plain names joined by `/`, each at most
/// [`NAME_MAX`] bytes.
fn plain(name: &str) -> bool {
    name.split('/').all(|part| {
        (1..=NAME_MAX).contains(&part.len())
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
    })
}

/// A part of a call's prompt: its path, its bytes in the request, and
/// about how many tokens it takes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub path: String,
    pub bytes: u64,
    pub weight: u64,
}

/// One model call, as the bridge keeps it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    /// Its place in its chat, from 1.
    pub seq: u64,
    /// When it was asked, in seconds since the Unix epoch.
    pub at: u64,
    /// The model it asked, as the request named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The window it was sent, in tokens, where the request said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// What the provider measured; none when its answer gave no usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    /// What the model cost when it was asked, where Link knew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    /// Where that price jumps for a long prompt, where it does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub above: Vec<crate::Jump>,
    /// The chat that asked for its chat's work, where its caller said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub parts: Vec<Part>,
}

/// How a share's tokens are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// The provider's measured prompt, split by each part's weight.
    Apportioned,
    /// Its weight alone: the provider measured nothing.
    Estimated,
}

/// A part's tokens, as reported.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Share {
    pub path: String,
    pub bytes: u64,
    pub tokens: u64,
    pub how: How,
    /// What its tokens cost, in US dollars, at the model's price; none
    /// where the price, or a call's, isn't known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// One call's prompt, part by part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub seq: u64,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    /// What its answer cost, in US dollars, where its price is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_cost: Option<f64>,
    pub parts: Vec<Share>,
}

/// A chat the bridge keeps calls of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    pub chat: String,
    /// Its calls so far, those dropped from its file among them.
    pub calls: u64,
    /// Its latest call's time.
    pub at: u64,
    /// The chat that asked for its work, as its latest call said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// A chat's report: its latest call, and each part summed over its
/// kept calls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub chat: String,
    pub calls: u64,
    pub latest: Snapshot,
    /// Each part's tokens summed over the chat's kept calls: what it
    /// cost over the chat, a part re-sent with each call counted each
    /// time.
    pub over: Vec<Share>,
    /// What its kept calls' answers cost, in US dollars; none where one
    /// call's price isn't known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_cost: Option<f64>,
}

impl Call {
    /// `price` for this call's prompt: past a jump of `above`, its.
    fn at(&self, price: Price) -> Price {
        let prompt = self.tokens.as_ref().map_or(0, |t| t.input);
        crate::price_at(price, &self.above, prompt)
    }

    /// Its parts' tokens: the measured prompt split by weight, the
    /// remainder to the largest remainders so they add up to it exactly;
    /// the weights where nothing was measured.
    pub fn shares(&self) -> Vec<Share> {
        let measured = self.tokens.as_ref().map(|t| t.input);
        let total: u128 = self.parts.iter().map(|p| u128::from(p.weight)).sum();
        let (how, tokens) = match measured {
            Some(input) if total > 0 => (How::Apportioned, apportion(input, &self.parts, total)),
            Some(input) if !self.parts.is_empty() => {
                // Nothing weighs anything: all of it to the first.
                let mut tokens = vec![0; self.parts.len()];
                tokens[0] = input;
                (How::Apportioned, tokens)
            }
            _ => (
                How::Estimated,
                self.parts.iter().map(|p| p.weight).collect(),
            ),
        };
        // The prompt's cost, its cache's share in it, split as its tokens.
        let prompt = self.tokens.as_ref().filter(|t| t.input > 0);
        let per_token = self
            .price
            .zip(prompt)
            .map(|(price, t)| self.at(price).of(t.input, t.cached, 0) / t.input as f64);
        self.parts
            .iter()
            .zip(tokens)
            .map(|(part, tokens)| Share {
                path: part.path.clone(),
                bytes: part.bytes,
                tokens,
                how,
                cost: per_token.map(|cost| cost * tokens as f64),
            })
            .collect()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            seq: self.seq,
            at: self.at,
            model: self.model.clone(),
            window: self.window,
            tokens: self.tokens.clone(),
            answer_cost: self
                .price
                .zip(self.tokens.as_ref())
                .map(|(price, tokens)| self.at(price).of(0, 0, tokens.output)),
            parts: self.shares(),
        }
    }
}

/// `input` split among `parts` by weight (`total` of them), each its
/// floor, and what the floors leave to the largest remainders.
fn apportion(input: u64, parts: &[Part], total: u128) -> Vec<u64> {
    let input = u128::from(input);
    let mut tokens = Vec::with_capacity(parts.len());
    let mut rests = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let exact = input * u128::from(part.weight);
        tokens.push((exact / total) as u64);
        rests.push((exact % total, i));
    }
    let given: u128 = tokens.iter().map(|&t| u128::from(t)).sum();
    rests.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in rests.iter().take((input - given) as usize) {
        tokens[i] += 1;
    }
    tokens
}

impl Report {
    /// `child`'s parts and answers over its chat counted in this one's,
    /// each beneath `under` (`compaction`, `subagents/task-abc`): the work
    /// this chat asked for, at what it cost.
    pub fn adopt(&mut self, under: &str, child: &Report) {
        for share in &child.over {
            let path = format!("{under}/{}", share.path);
            match self.over.iter_mut().find(|s| s.path == path) {
                Some(sum) => {
                    sum.bytes += share.bytes;
                    sum.tokens += share.tokens;
                    sum.cost = sum.cost.zip(share.cost).map(|(a, b)| a + b);
                }
                None => self.over.push(Share {
                    path,
                    ..share.clone()
                }),
            }
        }
        self.answer_cost = self.answer_cost.zip(child.answer_cost).map(|(a, b)| a + b);
    }
}

/// Where a chat's work is counted in its `parent`'s report: beneath what
/// follows the parent's name (`main/compaction`: `compaction`), else as
/// a subagent (`subagents/task-abc`).
pub fn adopted_under(parent: &str, child: &str) -> String {
    match child
        .strip_prefix(parent)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(rest) if !rest.is_empty() => rest.to_owned(),
        _ => format!("subagents/{child}"),
    }
}

/// A chat's report from its kept `calls`, oldest first; none without one.
pub fn report(chat: &str, calls: &[Call]) -> Option<Report> {
    let latest = calls.last()?;
    let mut over: Vec<Share> = Vec::new();
    let mut answer_cost = Some(0.0);
    for call in calls {
        answer_cost = answer_cost
            .zip(call.snapshot().answer_cost)
            .map(|(a, b)| a + b);
        for share in call.shares() {
            match over.iter_mut().find(|s| s.path == share.path) {
                Some(sum) => {
                    sum.bytes += share.bytes;
                    sum.tokens += share.tokens;
                    // Unknown once one call's is: never a partial sum.
                    sum.cost = sum.cost.zip(share.cost).map(|(a, b)| a + b);
                    if share.how == How::Estimated {
                        sum.how = How::Estimated;
                    }
                }
                None => over.push(share),
            }
        }
    }
    Some(Report {
        chat: chat.to_owned(),
        calls: calls.len() as u64,
        latest: latest.snapshot(),
        over,
        answer_cost,
    })
}

#[cfg(test)]
mod tests;
