//! A chat's conversations: the stretches of its one log that
//! `/chat:create`, `/chat:fork` and `/chat:resume` begin. Nothing is copied
//! or moved: each command that ran is kept as an event of its own, and
//! a chat's conversations are read back from those events, in order.
//!
//! The chat's first events are its first conversation. `/chat:create`
//! begins one holding nothing before it, `/chat:fork` one holding the
//! conversation it was sent in as it stood, and `/chat:resume <n>` takes
//! conversation `n` up again where it left off. `/chat:rename` names the
//! conversation it's sent in.

use crate::event::{self, Event};
use crate::window::Mark;

/// How a `/chat:create` that ran is kept: as its name, or as `/new`, its
/// alias and its name before the namespaces.
const NEW_RAN: [&str; 2] = ["/chat:create", "/new"];
const FORK: &str = "/chat:fork";
const RESUME: &str = "/chat:resume";
const RENAME: &str = "/chat:rename";

/// What a command that ran began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Began {
    /// A conversation holding nothing before it.
    New,
    /// A conversation holding the one it was sent in, as it stood.
    Fork,
    /// Conversation `n`, from 1, taken up again.
    Resume(usize),
}

/// What `event` began: a command of the owner's that ran, never one
/// refused.
pub fn began(event: &Event) -> Option<Began> {
    if event.role != event::COMMAND || event.failed {
        return None;
    }
    let text = event.text.as_str();
    if NEW_RAN.contains(&text) {
        return Some(Began::New);
    }
    let (name, rest) = text.split_once(' ').unwrap_or((text, ""));
    match name {
        FORK => Some(Began::Fork),
        RESUME => rest.trim().parse().ok().map(Began::Resume),
        _ => None,
    }
}

/// Whether `event` started a new conversation: a `/chat:create` that ran.
pub fn is_new(event: &Event) -> bool {
    began(event) == Some(Began::New)
}

/// The name a `/chat:rename` or `/chat:fork` that ran gave its
/// conversation.
fn renamed(event: &Event) -> Option<&str> {
    if event.role != event::COMMAND || event.failed {
        return None;
    }
    [RENAME, FORK]
        .iter()
        .find_map(|name| event.text.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix(' '))
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// One conversation of a chat: its number, from 1, its name if it has
/// one, and its events, by their index in the chat's.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub number: usize,
    pub name: Option<String>,
    pub events: Vec<usize>,
}

impl Conversation {
    /// What it's called when listed: its name, else the first thing the
    /// owner said in it, cut to a line.
    pub fn called(&self, events: &[Event]) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let first = self
            .events
            .iter()
            .map(|i| &events[*i])
            .find(|e| e.role == event::USER && e.from.is_none());
        match first.map(|e| e.text.lines().next().unwrap_or_default().trim()) {
            Some(text) if text.chars().count() > 60 => {
                format!("{}...", text.chars().take(57).collect::<String>())
            }
            Some(text) if !text.is_empty() => text.to_string(),
            _ => "(nothing said yet)".into(),
        }
    }

    /// The owner's messages and the agent's answers it holds.
    pub fn messages(&self, events: &[Event]) -> usize {
        self.events
            .iter()
            .filter(|i| {
                let e = &events[**i];
                (e.role == event::USER && e.from.is_none()) || e.is_answer()
            })
            .count()
    }
}

/// Each conversation of a chat whose events, from its first, are
/// `events`, and the index of the one it's on.
pub fn conversations(events: &[Event]) -> (Vec<Conversation>, usize) {
    let mut all = vec![Conversation {
        number: 1,
        name: None,
        events: Vec::new(),
    }];
    let mut on = 0;
    for (i, e) in events.iter().enumerate() {
        match began(e) {
            Some(Began::New) => {
                all.push(Conversation {
                    number: all.len() + 1,
                    name: None,
                    events: vec![i],
                });
                on = all.len() - 1;
            }
            Some(Began::Fork) => {
                let mut held = all[on].events.clone();
                held.push(i);
                all.push(Conversation {
                    number: all.len() + 1,
                    name: renamed(e).map(str::to_string),
                    events: held,
                });
                on = all.len() - 1;
            }
            Some(Began::Resume(n)) if (1..=all.len()).contains(&n) => {
                on = n - 1;
                all[on].events.push(i);
            }
            _ => {
                if let Some(name) = renamed(e) {
                    all[on].name = Some(name.to_string());
                }
                all[on].events.push(i);
            }
        }
    }
    (all, on)
}

/// What a command did: what the owner is told, how it's kept when that
/// isn't as it was sent, whether it was refused, which begins nothing,
/// and whether it's kept at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub told: String,
    pub kept: Option<String>,
    pub refused: bool,
    pub unkept: bool,
}

impl From<String> for Done {
    fn from(told: String) -> Self {
        Self::told(told)
    }
}

impl Done {
    fn told(told: String) -> Self {
        Self {
            told,
            kept: None,
            refused: false,
            unkept: false,
        }
    }

    pub fn refused(told: String) -> Self {
        Self {
            refused: true,
            ..Self::told(told)
        }
    }
}

/// What `/chat:resume` does, `asked` after it, in a chat whose events,
/// from its first, are `events`: alone, lists its conversations; given
/// a number or a name, takes that one up, kept by its number.
pub fn resume(events: &[Event], asked: &str) -> Done {
    let (all, on) = conversations(events);
    if asked.is_empty() {
        let mut told = String::from("Conversations:");
        for c in &all {
            told.push_str(&format!(
                "\n{}. {}, {}",
                c.number,
                c.called(events),
                messages(c.messages(events))
            ));
            if c.number == on + 1 {
                told.push_str(" (on)");
            }
        }
        return Done::told(told);
    }
    let found = match asked.parse::<usize>() {
        Ok(n) => all.iter().find(|c| c.number == n),
        Err(_) => all.iter().rev().find(|c| c.name.as_deref() == Some(asked)),
    };
    match found {
        None => Done::refused(format!(
            "No conversation {asked} in this chat: /chat:resume lists them."
        )),
        Some(c) if c.number == on + 1 => Done::refused(format!("Already on {}.", c.number)),
        Some(c) => Done {
            kept: Some(format!("{RESUME} {}", c.number)),
            ..Done::told(format!(
                "Resumed {}: {}, {}.",
                c.number,
                c.called(events),
                messages(c.messages(events))
            ))
        },
    }
}

/// `n` messages, in words.
fn messages(n: usize) -> String {
    match n {
        1 => "1 message".into(),
        n => format!("{n} messages"),
    }
}

/// What `/chat:fork` does, `name` after it: a conversation holding this
/// one, numbered after the chat's last.
pub fn fork(events: &[Event], name: &str) -> Done {
    if let Some(refused) = unnamable(events, name) {
        return refused;
    }
    let (all, on) = conversations(events);
    let named = match name {
        "" => String::new(),
        name => format!(", {name}"),
    };
    Done::told(format!(
        "Forked: conversation {}{named}, holding {} as it stands, which stays to resume.",
        all.len() + 1,
        on + 1
    ))
}

/// What `/chat:rename` does, `name` after it: alone, says this
/// conversation's name.
pub fn rename(events: &[Event], name: &str) -> Done {
    let (all, on) = conversations(events);
    if name.is_empty() {
        return Done::told(match &all[on].name {
            Some(name) => format!("Conversation {} is {name}.", on + 1),
            None => format!(
                "Conversation {} has no name: /chat:rename <name> gives it one.",
                on + 1
            ),
        });
    }
    if let Some(refused) = unnamable(events, name) {
        return refused;
    }
    Done::told(format!("Conversation {} is {name}.", on + 1))
}

/// Why `name` can't name a conversation of the chat of `events`: a
/// number, which `/chat:resume` would take for one, or another's name.
fn unnamable(events: &[Event], name: &str) -> Option<Done> {
    if name.is_empty() {
        return None;
    }
    if name.parse::<usize>().is_ok() {
        return Some(Done::refused(format!(
            "A name isn't a number: /chat:resume takes {name} for conversation {name}."
        )));
    }
    let (all, on) = conversations(events);
    all.iter()
        .find(|c| c.number != on + 1 && c.name.as_deref() == Some(name))
        .map(|c| {
            Done::refused(format!(
                "Conversation {} is named {name} already.",
                c.number
            ))
        })
}

/// Whether `events`, read from later than the chat's first, can't say
/// which conversation the chat is on: their last beginning is a fork or
/// a resume, which holds events that came earlier. The chat's every
/// event is read for it then.
pub fn needs_all(events: &[Event]) -> bool {
    events.first().is_some_and(|e| e.seq > 1)
        && matches!(
            events.iter().rev().find_map(began),
            Some(Began::Fork | Began::Resume(_))
        )
}

/// The events of the conversation the chat is on, of `events`, and the
/// window's mark for them, walked with `budget`. `events` are the chat's
/// from its first, or, where [`needs_all`] doesn't hold, from where a
/// turn reads on: all of them, with `mark`, when nothing in them began
/// a conversation. Else its events, `mark` kept only where it's at one
/// of them, else made anew from them.
pub fn conversation(
    events: Vec<Event>,
    mark: Option<Mark>,
    budget: usize,
) -> (Vec<Event>, Option<Mark>) {
    if !events.iter().any(|e| began(e).is_some()) {
        return (events, mark);
    }
    let (all, on) = conversations(&events);
    let mut taken: Vec<Option<Event>> = events.into_iter().map(Some).collect();
    let held: Vec<Event> = all[on]
        .events
        .iter()
        .filter_map(|i| taken[*i].take())
        .collect();
    match mark {
        Some(mark) if held.iter().any(|e| e.seq == mark.seq && e.at == mark.at) => {
            (held, Some(mark))
        }
        _ => {
            let mark = Mark::of(&held, budget);
            (held, mark)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(seq: u64, text: &str) -> Event {
        Event {
            seq,
            at: format!("t{seq}"),
            ..Event::user(text)
        }
    }

    fn ran(seq: u64, text: &str) -> Event {
        Event {
            role: event::COMMAND.into(),
            ..numbered(seq, text)
        }
    }

    fn texts(events: &[Event]) -> Vec<&str> {
        events.iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn a_new_conversation_starts_after_the_last_new() {
        let before = vec![numbered(1, "hello"), numbered(2, "again")];
        let (events, mark) = conversation(before.clone(), None, 1000);
        assert_eq!((events, mark), (before, None));

        let events = vec![
            numbered(1, "hello"),
            ran(2, "/chat:create"),
            numbered(3, "one"),
            ran(4, "/new"),
            numbered(5, "two"),
        ];
        let old = Mark::at(&events[0], 1000);
        let (after, mark) = conversation(events.clone(), Some(old), 1000);
        assert_eq!(after, events[3..].to_vec());
        assert_eq!(mark, Some(Mark::at(&events[3], 1000)));
        // A mark made there, or after, stays.
        for made in [Mark::at(&events[3], 1000), Mark::at(&events[4], 1000)] {
            let (_, mark) = conversation(events.clone(), Some(made.clone()), 1000);
            assert_eq!(mark, Some(made));
        }
        // Nothing after it yet: itself.
        let (after, mark) = conversation(events[..4].to_vec(), None, 1000);
        assert_eq!(after, vec![events[3].clone()]);
        assert_eq!(mark, Some(Mark::at(&events[3], 1000)));
    }

    #[test]
    fn a_fork_holds_its_conversation_and_a_resume_takes_one_up() {
        let events = vec![
            numbered(1, "first"),
            ran(2, "/chat:rename work"),
            ran(3, "/chat:create"),
            numbered(4, "second"),
            ran(5, "/chat:fork"),
            numbered(6, "forked"),
            ran(7, "/chat:resume 1"),
            numbered(8, "back"),
        ];
        let (all, on) = conversations(&events);
        assert_eq!(all.len(), 3);
        assert_eq!((on, all[0].name.as_deref()), (0, Some("work")));
        assert_eq!(all[2].events, vec![2, 3, 4, 5]);
        assert_eq!(all[1].called(&events), "second");
        let (held, mark) = conversation(events.clone(), None, 1000);
        assert_eq!(
            texts(&held),
            ["first", "/chat:rename work", "/chat:resume 1", "back"]
        );
        assert_eq!(mark, Some(Mark::of(&held, 1000).unwrap()));
        // A mark at one of its events stays; one at another's doesn't.
        let at = Mark::at(&events[7], 1000);
        assert_eq!(
            conversation(events.clone(), Some(at.clone()), 1000).1,
            Some(at)
        );
        let elsewhere = Mark::at(&events[5], 1000);
        assert_ne!(
            conversation(events.clone(), Some(elsewhere.clone()), 1000).1,
            Some(elsewhere)
        );
        // Read from later than the first, a fork or resume needs them all.
        assert!(needs_all(&events[4..]));
        assert!(!needs_all(&events));
        assert!(!needs_all(&events[2..4]));
    }

    #[test]
    fn resume_lists_the_conversations_and_takes_one_up_by_number_or_name() {
        let events = vec![
            numbered(1, "first"),
            ran(2, "/chat:rename work"),
            ran(3, "/chat:create"),
            numbered(4, "second"),
        ];
        let listed = resume(&events, "").told;
        assert_eq!(
            listed,
            "Conversations:\n1. work, 1 message\n2. second, 1 message (on)"
        );
        for asked in ["1", "work"] {
            let done = resume(&events, asked);
            assert_eq!(done.kept.as_deref(), Some("/chat:resume 1"), "{asked}");
            assert!(!done.refused && done.told.starts_with("Resumed 1: work"));
        }
        for asked in ["2", "3", "play"] {
            assert!(resume(&events, asked).refused, "{asked}");
        }
    }

    #[test]
    fn a_name_is_never_a_number_or_another_conversations() {
        let events = vec![ran(1, "/chat:rename work"), ran(2, "/chat:create")];
        assert!(rename(&events, "7").refused);
        assert!(rename(&events, "work").refused);
        assert!(fork(&events, "work").refused);
        assert_eq!(rename(&events, "play").told, "Conversation 2 is play.");
        assert!(rename(&events, "").told.contains("has no name"));
        let forked = fork(&events, "play");
        assert!(!forked.refused && forked.told.starts_with("Forked: conversation 3, play"));
        let named = [events, vec![ran(3, "/chat:fork play")]].concat();
        assert_eq!(conversations(&named).0[2].name.as_deref(), Some("play"));
    }

    #[test]
    fn a_refused_command_begins_nothing() {
        let mut refused = ran(2, "/chat:resume 1");
        refused.failed = true;
        assert_eq!(began(&refused), None);
        assert_eq!(began(&ran(2, "/chat:resume nine")), None);
        assert_eq!(began(&ran(2, "/chat:resume 9")), Some(Began::Resume(9)));
        // One out of range takes nothing up.
        let events = vec![numbered(1, "a"), ran(2, "/chat:resume 9"), numbered(3, "b")];
        assert_eq!(conversations(&events).0.len(), 1);
        assert!(is_new(&ran(1, "/chat:create")) && is_new(&ran(1, "/new")));
        assert!(!is_new(&numbered(1, "/chat:create")));
    }
}
