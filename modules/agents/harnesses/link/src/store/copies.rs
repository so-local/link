//! The copy for people (`conversation.md`), made and grown from the events.

use super::*;
use crate::event;

impl Files {
    /// Brings the copy for people up to `event`, just written after
    /// `before`: the sections since the agent's last answer added to its
    /// end when the copy ends with that answer, else the copy made anew
    /// from every event.
    pub(super) fn copy(
        &self,
        agent: &Agent,
        thread: &Thread,
        folder: &Held,
        event: &Event,
        before: &[Event],
    ) -> anyhow::Result<()> {
        let md = folder.at(MARKDOWN);
        // A link there isn't followed: the copy is made anew in its place.
        let exists = folder.kind(MARKDOWN) == Some(Kind::File);
        // The copy stops at the last answer: only an answer adds to it.
        if exists && !event.is_answer() {
            return Ok(());
        }
        let follows = before.last().map_or(event.seq == 1, |last| {
            last.seq.checked_add(1) == Some(event.seq)
        });
        let whole = follows
            && before
                .first()
                .map_or(event.seq == 1, |first| first.seq == 1);
        let last = before.iter().rposition(Event::is_answer);
        // What the copy ends with, when it's up to date so far.
        let so_far = match last {
            Some(i) => section(&before[i]),
            None if whole => Some(header(thread)),
            None => None,
        };
        let from = last.map_or(0, |i| i + 1);
        if exists
            && follows
            && let Some(so_far) = so_far
            && ends_with(folder, MARKDOWN, so_far.as_bytes(), last.is_none())?
        {
            let mut added = String::new();
            for said in before[from..].iter().chain([event]) {
                added += &section(said).unwrap_or_default();
            }
            let mut file = folder
                .open(MARKDOWN, libc::O_WRONLY | libc::O_APPEND, 0)
                .within(format!("writing {}", md.display()))?;
            return file
                .write_all(added.as_bytes())
                .within(format!("writing {}", md.display()));
        }
        let copy = match whole {
            true => markdown(thread, &[before, std::slice::from_ref(event)].concat()),
            false => markdown(thread, &self.read(agent, thread)?),
        };
        folder
            .write(MARKDOWN, copy.as_bytes())
            .within(format!("writing {}", md.display()))
    }
}

/// What the copy for people reads of `event`, its text as `text`
/// ([`Event::said`]: a line naming each of its parts first).
pub(super) fn said<'a>(event: &'a Event, text: &'a str) -> Said<'a> {
    Said {
        role: event.copied_role(),
        text,
        at: &event.at,
        calls: !event.calls.is_empty(),
        from: event.from.as_deref(),
    }
}

/// The conversation as people read it, and memory searches it
/// ([`copy::markdown`]).
pub fn markdown(thread: &Thread, events: &[Event]) -> String {
    let texts: Vec<_> = events.iter().map(Event::said).collect();
    let said: Vec<Said> = events
        .iter()
        .zip(&texts)
        .filter(|(event, _)| event.role != event::EVIDENCE)
        .map(|(event, text)| said(event, text))
        .collect();
    copy::markdown(thread.name(), &said)
}

/// The copy's first line.
fn header(thread: &Thread) -> String {
    copy::header(thread.name())
}

/// `event` in the copy ([`copy::section`]).
fn section(event: &Event) -> Option<String> {
    if event.role == event::EVIDENCE {
        return None;
    }
    copy::section(&said(event, &event.said()))
}
