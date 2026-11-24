//! Compaction: what a conversation's oldest turns leave behind. Once the
//! conversation fills the share of the window its spec says (`[window]
//! compact_at`), the window's start moves on at a drop point, as it
//! always does, and what leaves is summarized. The turn answers first,
//! with the window it last sent, which the room `compact_at` leaves
//! still holds; then the model is asked, with that same prompt and its
//! answer, so the provider's cache holds for all of it, to save what
//! matters to its notes where it has a tool to (`[window] save_notes`),
//! and to write a summary of what leaves. The summary is an event of its
//! own, never changed, and the window starts with it until the next drop
//! point, so the provider's cache holds between the two. A window that
//! outgrew even that room is summarized first, from what leaves alone.
//!
//! The turn makes the calls ([`crate::turn`]); here is what they're made
//! of.

use crate::event::{self, Event};
use crate::window;

/// The tool that saves to the agent's notes: the only one called while
/// it compacts.
pub const REMEMBER: &str = "memory__remember";

/// What the model is asked to write, when it has no notes to save to.
pub const SUMMARIZE: &str = "[Link: everything above is about to leave your window. \
Answer with a summary of it to keep in its place: who and what the conversation is about, \
what was asked, decided and done, and anything still open, with the names, numbers and \
dates that matter. Write it as notes to yourself, with nothing else.]";

/// What it's asked when it has: to save first, then write.
pub const SAVE_AND_SUMMARIZE: &str = "[Link: everything above is about to leave your window. \
First save anything in it worth remembering past this conversation to your notes, with \
memory__remember: what you learned about your owner, their preferences, and what was \
decided. Then answer with a summary of it to keep in its place: who and what the \
conversation is about, what was asked, decided and done, and anything still open, with the \
names, numbers and dates that matter. Write it as notes to yourself, with nothing else.]";

/// A call of another tool while it compacts and saves.
pub const NOT_WHILE_SAVING: &str =
    "Not called: only memory__remember is called while the conversation is summarized.";

/// A call while it compacts with nothing to save.
pub const NOT_WHILE_SUMMARIZING: &str =
    "Not called: nothing is called while the conversation is summarized.";

/// How much of the first message that stays a request quotes.
const QUOTED: usize = 80;

/// What the model is asked, saving first or not, when what's above it
/// leaves up to `staying`, the first message that stays: everything, with
/// none ([`SUMMARIZE`], [`SAVE_AND_SUMMARIZE`]); else everything before
/// that message, quoted by its start.
pub fn request(save: bool, staying: Option<&Event>) -> String {
    let whole = match save {
        true => SAVE_AND_SUMMARIZE,
        false => SUMMARIZE,
    };
    let Some(staying) = staying else {
        return whole.to_string();
    };
    let line = staying
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut start: String = line.chars().take(QUOTED).collect();
    if start.len() < line.len() {
        start.push_str("...");
    }
    let before = format!(
        "[Link: everything above the message that begins \"{}\" is about to leave your window; \
that message and what follows it stay.",
        start.replace('"', "'")
    );
    whole.replacen(
        "[Link: everything above is about to leave your window.",
        &before,
        1,
    )
}

/// The summary a window starts with: the latest of `events`.
pub fn head(events: &[Event]) -> Option<&Event> {
    events.iter().rev().find(|e| e.role == event::SUMMARY)
}

/// What leaves the window for a summary, when it starts at `events[start]`
/// and the summary before covers the events before number `covered`:
/// those in between, but summaries, as many of the newest as fit
/// `budget`, from a message of the owner's. Any older were never read
/// with it (a conversation compaction never read), and leave unsummarized.
pub fn leaving(events: &[Event], start: usize, covered: u64, budget: usize) -> &[Event] {
    let before = &events[..start.min(events.len())];
    let from = before
        .iter()
        .position(|e| e.seq >= covered)
        .unwrap_or(before.len());
    let mut kept = 0usize;
    let mut first = before.len();
    for (i, event) in before.iter().enumerate().skip(from).rev() {
        kept = kept.saturating_add(window::tokens(event));
        if kept > budget {
            break;
        }
        first = i;
    }
    // From the owner speaking, so no call is left without its question.
    let first = (first..before.len())
        .find(|i| before[*i].role == event::USER)
        .unwrap_or(before.len());
    &before[first..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(seq: u64, role: &str, words: usize) -> Event {
        Event {
            seq,
            role: role.into(),
            text: "word ".repeat(words),
            ..Event::default()
        }
    }

    #[test]
    fn a_request_says_what_stays() {
        assert_eq!(request(false, None), SUMMARIZE);
        assert_eq!(request(true, None), SAVE_AND_SUMMARIZE);
        let staying = said(9, event::USER, 40);
        let asked = request(true, Some(&staying));
        assert!(asked.starts_with("[Link: everything above the message that begins \"word word"));
        assert!(asked.contains("...\" is about to leave your window; that message"));
        assert!(asked.contains("memory__remember"));
        assert!(asked.ends_with("with nothing else.]"));
        assert!(!request(false, Some(&staying)).contains("memory__remember"));
    }

    #[test]
    fn the_head_is_the_latest_summary() {
        let mut events: Vec<Event> = (1..=6)
            .map(|n| said(n, [event::USER, event::ASSISTANT][n as usize % 2], 4))
            .collect();
        assert!(head(&events).is_none());
        events.push(Event {
            covers: 3,
            ..said(7, event::SUMMARY, 4)
        });
        events.push(said(8, event::USER, 4));
        events.push(Event {
            covers: 5,
            ..said(9, event::SUMMARY, 4)
        });
        assert_eq!(head(&events).unwrap().covers, 5);
    }

    #[test]
    fn what_leaves_is_what_the_summary_before_didnt_cover() {
        // 1 user, 2 agent, 3 user, 4 agent, 5 summary (covers 3), 6 user ...
        let mut events = vec![
            said(1, event::USER, 40),
            said(2, event::ASSISTANT, 40),
            said(3, event::USER, 40),
            said(4, event::ASSISTANT, 40),
            Event {
                covers: 3,
                ..said(5, event::SUMMARY, 40)
            },
            said(6, event::USER, 40),
            said(7, event::ASSISTANT, 40),
            said(8, event::USER, 40),
        ];
        // The window starts at 8: 3, 4, 6 and 7 leave, the summary not.
        let left: Vec<u64> = leaving(&events, 7, 3, 10_000)
            .iter()
            .filter(|e| e.role != event::SUMMARY)
            .map(|e| e.seq)
            .collect();
        assert_eq!(left, [3, 4, 6, 7]);
        // No summary before: from the first.
        assert_eq!(leaving(&events, 7, 1, 10_000)[0].seq, 1);
        // Only the newest that fit, from the owner speaking.
        assert_eq!(leaving(&events, 7, 1, 120)[0].seq, 6);
        assert!(leaving(&events, 7, 1, 5).is_empty());
        // Nothing leaves at the start.
        events.truncate(2);
        assert!(leaving(&events, 0, 1, 10_000).is_empty());
    }
}
