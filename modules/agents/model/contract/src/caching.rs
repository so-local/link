//! What caching a conversation's prompt costs, and so which lifetime to
//! ask of its provider's cache and when, if ever, a quiet conversation is
//! worth compacting. Pure arithmetic over figures it's given: what the
//! provider's cache offers and the model costs (the model's facts,
//! [`crate::Offer`], [`crate::Price`]), and what the conversation is
//! (its sizes and pauses, [`Conversation`]). It reads and sends nothing,
//! so any harness can ask it.
//!
//! Costs are in multiples of the model's input price. A request reads
//! the prompt its last request cached at `r`, the cache's read price, if
//! the cache still holds it, and writes what it adds at the offer's
//! `write`; a request after the cache ended writes the whole prompt
//! again. So each lifetime costs, for the request now and the next:
//!
//! `write · written + prompt · (r · F(lasts) + write · (1 − F(lasts)))`
//!
//! where `F(lasts)` is the share of the conversation's pauses shorter
//! than the lifetime. [`lifetime`] chooses the least.
//!
//! A quiet conversation is compacted just before its cache ends, while
//! the summary's input is read from it, and only when the summary costs
//! less than it saves ([`compaction`]).

use std::sync::LazyLock;
use std::time::Duration;

use serde::Deserialize;

use crate::{Offer, Price};

/// What `caching.toml` names.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Figures {
    /// How long before its cache ends a quiet conversation is compacted.
    pub margin: String,
    /// How many cached turns a conversation takes after a pause.
    pub turns: u32,
    /// A summary's expected length, in tokens, before the conversation
    /// has one.
    pub summary: u64,
    /// How many of a conversation's latest pauses its choices learn from.
    pub kept: usize,
}

/// `caching.toml`, read once.
pub fn figures() -> &'static Figures {
    static READ: LazyLock<Figures> = LazyLock::new(|| {
        toml::from_str(include_str!("caching.toml")).expect("caching.toml is Figures")
    });
    &READ
}

/// `text` as a duration (`"5m"`), as every spec writes one.
pub fn duration(text: &str) -> Option<Duration> {
    link_spec::duration_ms(text)
        .ok()
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
}

/// A conversation as its caching costs it, in tokens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation {
    /// What the next request reads of it: its last prompt, and the answer.
    pub prompt: u64,
    /// Of `prompt`, what a summary would replace: what's before its last
    /// message, but for any summary already there.
    pub history: u64,
    /// What its last request wrote to the cache.
    pub written: u64,
    /// How long its summary is expected to be.
    pub summary: u64,
    /// Its pauses between messages: what's assumed of a conversation
    /// first (`[connections] pauses`), then its own latest
    /// ([`Figures::kept`]).
    pub pauses: Vec<Duration>,
}

/// The prices it's costed at, as multiples of the input price.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rates {
    read: f64,
    output: f64,
}

impl Rates {
    fn of(price: &Price) -> Option<Rates> {
        (price.input > 0.0).then(|| Rates {
            read: price.cached.unwrap_or(price.input) / price.input,
            output: price.output / price.input,
        })
    }
}

/// The share of `pauses` shorter than `lasts`: how likely the next
/// message finds a cache of that lifetime holding the prompt.
fn within(pauses: &[Duration], lasts: Duration) -> f64 {
    match pauses.len() {
        0 => 0.0,
        all => pauses.iter().filter(|p| **p < lasts).count() as f64 / all as f64,
    }
}

/// What `offer` is expected to cost the request now and the next.
fn cost(offer: &Offer, lasts: Duration, rates: Rates, conversation: &Conversation) -> f64 {
    let held = within(&conversation.pauses, lasts);
    offer.write * conversation.written as f64
        + conversation.prompt as f64 * (rates.read * held + offer.write * (1.0 - held))
}

/// Which of `offers` to ask of the provider's cache, by its index: the
/// one expected to cost least, the provider's own (the first) on a tie.
/// None where there's no offer or no price to cost them by.
pub fn lifetime(offers: &[Offer], price: &Price, conversation: &Conversation) -> Option<usize> {
    let rates = Rates::of(price)?;
    let mut best: Option<(usize, f64)> = None;
    for (index, offer) in offers.iter().enumerate() {
        let Some(lasts) = offer.lifetime() else {
            continue;
        };
        let cost = cost(offer, lasts, rates, conversation);
        if best.is_none_or(|(_, least)| cost < least) {
            best = Some((index, cost));
        }
    }
    best.map(|(index, _)| index)
}

/// How long after its last request a quiet conversation, cached as
/// `offer`, is worth compacting: just inside the cache's lifetime
/// ([`Figures::margin`]), when what the summary saves is more than it
/// costs. None when it never is.
///
/// The summary reads the prompt from the cache and writes the summary,
/// which the cache then keeps. It saves what the conversation would
/// otherwise read again: the history it replaces, written again by the
/// next message if that comes after the cache ended (the share of its
/// pauses that long, of those past the compaction), and read by each of
/// the turns after ([`Figures::turns`]).
pub fn compaction(offer: &Offer, price: &Price, conversation: &Conversation) -> Option<Duration> {
    let rates = Rates::of(price)?;
    let lasts = offer.lifetime()?;
    let at = lasts.checked_sub(duration(&figures().margin)?)?;
    if at.is_zero() || conversation.history <= conversation.summary {
        return None;
    }
    let later = conversation.pauses.iter().filter(|p| **p >= at).count();
    let cold = match later {
        0 => 1.0,
        later => conversation.pauses.iter().filter(|p| **p >= lasts).count() as f64 / later as f64,
    };
    let turns = f64::from(figures().turns);
    let (summary, prompt) = (conversation.summary as f64, conversation.prompt as f64);
    let costs = prompt * rates.read + summary * (rates.output + offer.write);
    let replaced = (conversation.history - conversation.summary) as f64;
    let saves = replaced
        * (cold * (offer.write + (turns - 1.0) * rates.read) + (1.0 - cold) * turns * rates.read);
    (saves > costs).then_some(at)
}

#[cfg(test)]
mod tests;
