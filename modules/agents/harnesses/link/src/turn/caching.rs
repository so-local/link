//! A conversation's caching, as the agent's spec and its model's facts
//! say: how long its requests ask the provider's cache to keep its
//! prompt, and when, if ever, a quiet conversation is compacted. The
//! choice is [`link_model_contract::caching`]'s, from what this reads of
//! the conversation: its size, its summary and its pauses.

use std::time::Duration;

use link_model_contract::caching::{self, Conversation};
use link_model_contract::{Offer, Price};

use super::*;

/// What a conversation's caching is chosen from.
pub(super) struct Caching {
    offers: Vec<Offer>,
    price: Price,
    conversation: Conversation,
    /// The lifetime its spec names (`[connections] cache`); none, the
    /// cheapest.
    named: Option<String>,
}

impl Caching {
    /// `thread`'s, whose events since its window's start are `events`,
    /// as `config` says: none where its model's cache or price isn't
    /// known.
    pub(super) fn of(config: &Config, thread: &Thread, events: &[Event]) -> Option<Caching> {
        let known = crate::limits::known(config.model_facts.as_ref(), &config.model)?;
        let price = known.facts.price?;
        if known.facts.cache.is_empty() {
            return None;
        }
        let opinions = config.opinions();
        // A conversation's connection is its name's first part
        // (`telegram:42`), as its other opinions are read.
        let connection = thread.name().split_once(':').map(|(c, _)| c);
        let mut pauses = connection.map(|c| opinions.pauses(c)).unwrap_or_default();
        pauses.extend(seen(events));
        Some(Caching {
            offers: known.facts.cache,
            price,
            conversation: sizes(events, pauses),
            named: connection.and_then(|c| opinions.cache_for(c)),
        })
    }

    /// The offer its requests ask for: the one its spec names, where the
    /// provider offers it, else the cheapest.
    fn offer(&self) -> Option<&Offer> {
        if let Some(named) = &self.named {
            return self.offers.iter().find(|o| &o.lasts == named);
        }
        caching::lifetime(&self.offers, &self.price, &self.conversation)
            .and_then(|index| self.offers.get(index))
    }

    /// What its requests ask of the provider's cache: none for the one
    /// the provider keeps unasked (the first).
    pub(super) fn asked(&self) -> Option<String> {
        let offer = self.offer()?;
        (self.offers.first() != Some(offer)).then(|| offer.lasts.clone())
    }

    /// How long after its last request it's worth compacting when quiet,
    /// its requests having asked for `asked`: just before that cache ends,
    /// when the summary costs less than reading it again. None when it
    /// isn't.
    pub(super) fn compaction(&self, asked: Option<&str>) -> Option<Duration> {
        let offer = match asked {
            Some(asked) => self.offers.iter().find(|o| o.lasts == asked)?,
            None => self.offers.first()?,
        };
        caching::compaction(offer, &self.price, &self.conversation)
    }
}

/// The pauses between the owner's messages in `events`, the latest
/// [`caching::Figures::kept`].
fn seen(events: &[Event]) -> Vec<Duration> {
    let times: Vec<u64> = events
        .iter()
        .filter(|e| e.role == event::USER && e.from.is_none())
        .filter_map(|e| event::seconds(&e.at))
        .collect();
    let pauses: Vec<Duration> = times
        .windows(2)
        .map(|pair| Duration::from_secs(pair[1].saturating_sub(pair[0])))
        .collect();
    let kept = caching::figures().kept;
    pauses[pauses.len().saturating_sub(kept)..].to_vec()
}

/// `events` as their caching costs them, their pauses `pauses`: the
/// prompt and its writes as the provider last counted them, else
/// estimated.
fn sizes(events: &[Event], pauses: Vec<Duration>) -> Conversation {
    let head = compact::head(events);
    let covered = head.map_or(0, |h| h.covers);
    let estimated: u64 = events.iter().map(|e| window::tokens(e) as u64).sum();
    let counted = events.iter().rev().find_map(|e| e.tokens.as_ref());
    let (prompt, written) = match counted {
        Some(t) => (
            t.input + t.output,
            t.input.saturating_sub(t.cached) + t.output,
        ),
        None => (estimated, estimated),
    };
    let last = events
        .iter()
        .rposition(|e| e.role == event::USER && e.from.is_none())
        .unwrap_or(events.len());
    let history = events[..last]
        .iter()
        .filter(|e| {
            e.seq >= covered
                && !matches!(
                    e.role.as_str(),
                    event::SUMMARY | event::COMMAND | event::EVIDENCE
                )
        })
        .map(|e| window::tokens(e) as u64)
        .sum();
    Conversation {
        prompt,
        history,
        written,
        summary: head.map_or(caching::figures().summary, |h| {
            window::text_tokens(&h.text) as u64
        }),
        pauses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(at: u64, role: &str, from: Option<&str>) -> Event {
        Event {
            role: role.into(),
            at: event::timestamp(at),
            from: from.map(str::to_string),
            text: "word ".repeat(200),
            ..Event::default()
        }
    }

    /// The pauses are between the owner's own messages, others' left out.
    #[test]
    fn pauses_are_between_the_owners_messages() {
        let events = [
            said(0, event::USER, None),
            said(30, event::ASSISTANT, None),
            said(60, event::USER, None),
            said(600, event::USER, Some("schedule:aaaaaaaaaaa1")),
            said(1260, event::USER, None),
        ];
        assert_eq!(
            seen(&events),
            [Duration::from_secs(60), Duration::from_secs(1200)]
        );
    }

    /// The history a summary would replace is before the owner's last
    /// message; the prompt, as the provider last counted it.
    #[test]
    fn sizes_count_what_a_summary_replaces() {
        let mut answer = said(30, event::ASSISTANT, None);
        answer.tokens = Some(event::Tokens {
            input: 9000,
            output: 500,
            cached: 8000,
        });
        let events = [
            said(0, event::USER, None),
            answer,
            said(60, event::USER, None),
        ];
        let sized = sizes(&events, Vec::new());
        assert_eq!((sized.prompt, sized.written), (9500, 1500));
        let each = window::tokens(&events[0]) as u64;
        assert_eq!(sized.history, each + window::tokens(&events[1]) as u64);
        assert_eq!(sized.summary, caching::figures().summary);
    }
}
