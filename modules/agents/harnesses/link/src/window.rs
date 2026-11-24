//! The window: which of a conversation's events the prompt holds. The
//! prompt is the conversation sent the same way each turn, so the
//! provider's cache holds; once it outgrows the budget, the oldest turns
//! drop in steps of half the budget, so its start stays the same for
//! many turns between drops.
//!
//! Where it starts depends on every event before it, so a [`Mark`] keeps
//! how far the walk over them got: a turn reads the events from there
//! on, not the whole conversation. The mark is derived from the events,
//! and made anew from all of them when it's missing or not theirs.

pub use link_model_contract::context::{IMAGE_TOKENS, PDF_TOKENS_MAX, text_tokens};
use serde::{Deserialize, Serialize};

use crate::event::{self, Event, Kind, Part};

/// About how many tokens `event` takes, of its text, its calls and the
/// thinking sent back with them, its parts ([`part_tokens`]), and a few
/// for the message around it ([`text_tokens`]). None for a summary: it's
/// sent at the window's head, in room of its own, never where it was
/// written.
pub fn tokens(event: &Event) -> usize {
    if matches!(event.role.as_str(), event::SUMMARY | event::EVIDENCE) {
        return 0;
    }
    let calls: usize = event
        .calls
        .iter()
        .map(|c| text_tokens(&c.name) + text_tokens(&c.arguments.to_string()) + 4)
        .sum();
    let thinking: usize = event
        .thinking
        .iter()
        .map(|block| text_tokens(&block.to_string()))
        .sum();
    let parts: usize = event.parts.iter().map(part_tokens).sum();
    text_tokens(&event.text) + calls + thinking + parts + 4
}

/// About how many tokens `part` takes, from its kind and size: an image
/// [`IMAGE_TOKENS`], whatever its size; a PDF a token for each 3 of its
/// bytes, at most [`PDF_TOKENS_MAX`]; a text document as its text, 4
/// bytes to a token ([`text_tokens`]'s ASCII), and a few for its name;
/// a voice note a few, its line, since its words are its message's.
pub fn part_tokens(part: &Part) -> usize {
    let bytes = usize::try_from(part.bytes).unwrap_or(usize::MAX);
    match part.kind {
        Kind::Image => IMAGE_TOKENS,
        Kind::Document if part.media.starts_with("text/") => bytes / 4 + 8,
        Kind::Document => (bytes / 3).min(PDF_TOKENS_MAX),
        Kind::Voice => 16,
    }
}

/// `budget`, rounded down to a step of a sixteenth of it or less (a
/// power of two): the budget a window is walked with. A small change to
/// what the prompt holds besides the conversation (a note, a line of the
/// instructions, a tool) seldom moves it, so the window, and its mark,
/// stay as they were; it's never more than `budget`, and at most a
/// sixteenth less.
pub fn steady(budget: usize) -> usize {
    if budget < 32 {
        return budget;
    }
    let step = 1usize << (usize::BITS - 1 - budget.leading_zeros()).saturating_sub(4);
    budget / step * step
}

/// The index of the first event the prompt holds, within `budget`.
///
/// The drop points are fixed by the conversation itself: the first event
/// at or after each multiple of half the budget, counting from the start.
/// The window starts at the first drop point whose rest fits, moved on to
/// a message of the owner's (never a tool's answer, whose call would be
/// left out). As events are added, it moves only when the
/// rest outgrows the budget, and then by about half of it.
pub fn start(events: &[Event], budget: usize) -> usize {
    match events.first() {
        Some(first) => walk(events, &Mark::at(first, budget)).0,
        None => 0,
    }
}

/// How far the walk to a window's start got: the event it goes on from,
/// and what it had counted before it. Walking on from here finds the
/// same start as walking from the first event, since a drop point it
/// passed never fits again: the conversation only grows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// The budget it was walked with.
    pub budget: usize,
    /// The event it goes on from: its number, and its time, so a mark
    /// isn't taken for another conversation's.
    pub seq: u64,
    pub at: String,
    /// The tokens of the events before it.
    pub before: usize,
    /// The next drop point, in tokens from the conversation's start.
    pub next: usize,
    /// The agent's last answer, where the copy for people ends; 0 for
    /// none.
    #[serde(default)]
    pub answered: u64,
    /// The latest summary, which the window starts with; 0 for none.
    #[serde(default)]
    pub summary: u64,
}

impl Mark {
    /// The first event a turn reads by this mark: where the window
    /// starts, or the agent's last answer or the latest summary when
    /// either is earlier.
    pub fn first(&self) -> u64 {
        [self.seq, self.answered, self.summary]
            .into_iter()
            .filter(|seq| *seq > 0)
            .min()
            .unwrap_or(1)
    }

    /// Whether a walk could have made this mark, at its event numbered
    /// `seq` (the first is 1), when no event is over `most` tokens: its
    /// drop point a step of the budget's, and no further than a step past
    /// what it counted, which is no more than the events before it hold.
    /// A mark that isn't is made anew from every event.
    pub fn fits(&self, most: usize) -> bool {
        let step = (self.budget / 2).max(1);
        let before_most = (self.seq.saturating_sub(1) as usize).saturating_mul(most);
        self.next >= step
            && self.next.is_multiple_of(step)
            && self.next <= self.before.saturating_add(step)
            && self.before <= before_most
    }

    /// At `first`, the conversation's first event, nothing counted yet.
    pub fn at(first: &Event, budget: usize) -> Self {
        Self {
            budget,
            seq: first.seq,
            at: first.at.clone(),
            before: 0,
            next: (budget / 2).max(1),
            answered: 0,
            summary: 0,
        }
    }

    /// Made anew from `events`, a conversation's from its start (or from
    /// a `/new`): at its first, with its last answer and latest summary.
    /// None without events.
    pub fn of(events: &[Event], budget: usize) -> Option<Self> {
        events.first().map(|first| Self {
            answered: events
                .iter()
                .rev()
                .find(|e| e.is_answer())
                .map_or(0, |e| e.seq),
            summary: crate::compact::head(events).map_or(0, |e| e.seq),
            ..Self::at(first, budget)
        })
    }
}

/// The index of the first event the prompt holds, walking on from
/// `mark`, whose event is `events[0]`; and the mark to go on from next
/// time.
pub fn walk(events: &[Event], mark: &Mark) -> (usize, Mark) {
    let budget = mark.budget;
    let sizes: Vec<usize> = events.iter().map(tokens).collect();
    let total = sizes
        .iter()
        .fold(mark.before, |sum, size| sum.saturating_add(*size));
    if total <= budget || events.is_empty() {
        return (0, mark.clone());
    }
    let step = (budget / 2).max(1);
    let mut before = mark.before; // tokens before event `i`
    let mut next_drop = mark.next;
    let mut start = events.len() - 1;
    let mut resume = mark.clone();
    for (i, size) in sizes.iter().enumerate() {
        if before >= next_drop {
            resume = Mark {
                seq: events[i].seq,
                at: events[i].at.clone(),
                before,
                next: next_drop,
                ..mark.clone()
            };
            // The first drop point after `before`, however far.
            let steps = (before - next_drop) / step + 1;
            next_drop = steps
                .checked_mul(step)
                .and_then(|n| n.checked_add(next_drop))
                .unwrap_or(usize::MAX);
            if total - before <= budget {
                start = i;
                break;
            }
        }
        before = before.saturating_add(*size);
    }
    // A window starts with the owner speaking.
    let start = (start..events.len())
        .find(|i| events[*i].role == event::USER)
        .unwrap_or(start);
    (start, resume)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(seq: u64, words: usize) -> [Event; 2] {
        let text = "word ".repeat(words);
        [
            Event {
                seq: seq * 2 - 1,
                ..Event::user(&text)
            },
            Event {
                seq: seq * 2,
                role: event::ASSISTANT.into(),
                text,
                ..Event::default()
            },
        ]
    }

    #[test]
    fn a_budget_is_steady_through_small_changes() {
        assert_eq!(steady(20), 20);
        assert_eq!(steady(200), 200);
        assert_eq!(steady(450), 448);
        for budget in [100, 1_000, 33_333, 150_000, 1_000_000] {
            let walked = steady(budget);
            assert!(
                walked <= budget && walked >= budget - budget / 16,
                "{budget}"
            );
        }
        // A note of a few dozen tokens leaves it as it was.
        assert_eq!(steady(150_000), steady(150_000 - 40));
        assert_eq!(steady(150_000), 147_456);
    }

    #[test]
    fn a_short_conversation_is_whole() {
        let events: Vec<Event> = (1..=3).flat_map(|n| turn(n, 10)).collect();
        assert_eq!(start(&events, 1000), 0);
        assert_eq!(start(&[], 1000), 0);
    }

    #[test]
    fn the_window_moves_in_large_steps_and_fits() {
        let budget = 1000;
        let mut events = Vec::new();
        let mut starts = Vec::new();
        for n in 1..=200 {
            events.extend(turn(n, 20));
            let first = start(&events, budget);
            let kept: usize = events[first..].iter().map(tokens).sum();
            assert!(kept <= budget, "turn {n}: {kept} tokens kept");
            assert_eq!(events[first].role, event::USER);
            starts.push(first);
        }
        // Only forward, and rarely: each move drops about half the budget.
        assert!(starts.windows(2).all(|w| w[0] <= w[1]));
        let moves = starts.windows(2).filter(|w| w[0] != w[1]).count();
        let total: usize = events.iter().map(tokens).sum();
        assert!(moves <= total / (budget / 2) + 1, "{moves} moves");
        assert!(moves >= 10, "{moves} moves");
        // The kept part never shrinks below about half the budget.
        let kept: usize = events[*starts.last().unwrap()..].iter().map(tokens).sum();
        assert!(kept >= budget / 2 - 60, "{kept}");
    }

    /// Going on from the last mark finds the same start as walking the
    /// whole conversation, every time.
    #[test]
    fn a_mark_walks_on_to_the_same_start() {
        let budget = 1000;
        let mut events = Vec::new();
        let mut mark: Option<Mark> = None;
        for n in 1..=300 {
            events.extend(turn(n, 5 + (n as usize * 7) % 40));
            let mark_now = mark.take().unwrap_or_else(|| Mark::at(&events[0], budget));
            let from = events.iter().position(|e| e.seq == mark_now.seq).unwrap();
            let (start_here, next) = walk(&events[from..], &mark_now);
            assert_eq!(from + start_here, start(&events, budget), "turn {n}");
            assert!(next.seq >= mark_now.seq);
            assert!(next.seq <= events[from + start_here].seq);
            mark = Some(next);
        }
        assert!(mark.unwrap().seq > 100);
    }

    /// A mark planted with a huge count is walked on at once, never a
    /// step at a time, and doesn't fit the events it names.
    #[test]
    fn a_forged_mark_is_refused_and_never_spins() {
        let events: Vec<Event> = (1..=3).flat_map(|n| turn(n, 10)).collect();
        let forged = Mark {
            before: 1 << 62,
            next: 1,
            ..Mark::at(&events[2], 24_000)
        };
        assert!(!forged.fits(1 << 20));
        let started = std::time::Instant::now();
        let (start, resume) = walk(&events[2..], &forged);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(resume.before, 1 << 62);
        assert!(start < 4);
        let (_, resume) = walk(
            &events[2..],
            &Mark {
                before: usize::MAX,
                ..forged
            },
        );
        assert_eq!(resume.before, usize::MAX);

        // What walks make, fits.
        let mut events = Vec::new();
        for n in 1..=200 {
            events.extend(turn(n, 20));
        }
        let (_, mark) = walk(&events, &Mark::at(&events[0], 1000));
        assert!(mark.seq > 1);
        assert!(mark.fits(1 << 20) && Mark::at(&events[0], 1000).fits(1));
        assert!(
            !Mark {
                next: mark.before + 501,
                ..mark.clone()
            }
            .fits(1 << 20)
        );
        assert!(
            !Mark {
                next: mark.next + 1,
                ..mark.clone()
            }
            .fits(1 << 20)
        );
        assert!(
            !Mark {
                before: mark.before,
                ..Mark::at(&events[0], 1000)
            }
            .fits(1 << 20)
        );
    }

    #[test]
    fn a_message_bigger_than_the_budget_is_sent_alone() {
        let mut events: Vec<Event> = (1..=3).flat_map(|n| turn(n, 10)).collect();
        events.push(Event {
            seq: 7,
            ..Event::user(&"x".repeat(10_000))
        });
        assert_eq!(start(&events, 100), 6);
    }
}
