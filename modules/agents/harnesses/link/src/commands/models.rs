//! What a conversation chose for itself: its model, how hard it thinks
//! and whether it's asked fast, each by a `/model:set` that ran in it,
//! and whether its chat shows each tool
//! call (`/chat:verbose`), each kept as its event. Another conversation,
//! of this chat or another, keeps the agent's own.

use link_agent_spec::Thinking;

use crate::event::{self, Event};

pub const SET: &str = "/model:set";
pub const VERBOSE: &str = "/chat:verbose";
/// What says the choice is the agent's, for every chat.
pub const AGENT: &str = "--every-chat";
const FAST: &str = "--fast";
const NO_FAST: &str = "--no-fast";
const EFFORT: &str = "--effort";

/// What a conversation chose, of `/model:set`: none
/// of each it didn't.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chosen {
    pub model: Option<String>,
    pub thinking: Option<Thinking>,
    pub fast: bool,
    pub verbose: Option<bool>,
}

/// What the conversation of `events` chose, the last of each winning.
pub fn chosen(events: &[Event]) -> Chosen {
    let mut chosen = Chosen::default();
    for e in events {
        if e.role != event::COMMAND || e.failed {
            continue;
        }
        let (name, rest) = e.text.split_once(' ').unwrap_or((&e.text, ""));
        match (name, asked(rest)) {
            (SET, Ok(set)) => {
                if let Some(model) = set.model {
                    chosen.model = Some(model);
                }
                if let Some(fast) = set.fast {
                    chosen.fast = fast;
                }
                if let Some(level) = set.effort {
                    chosen.thinking = Some(level);
                }
            }
            (VERBOSE, _) => match rest {
                "on" => chosen.verbose = Some(true),
                "off" => chosen.verbose = Some(false),
                _ => {}
            },
            _ => {}
        }
    }
    chosen
}

/// What `/model:set` was asked: a model, `--every-chat`, `--fast` or
/// `--no-fast`, and `--effort` with a level after it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asked {
    pub model: Option<String>,
    pub agent: bool,
    pub fast: Option<bool>,
    pub effort: Option<Thinking>,
}

/// What's asked by `arguments`, what follows the command's name; why
/// it can't be read, when it can't.
pub fn asked(arguments: &str) -> Result<Asked, String> {
    let mut asked = Asked::default();
    let mut words = arguments.split_whitespace().peekable();
    while let Some(word) = words.next() {
        match word {
            AGENT => asked.agent = true,
            FAST if matches!(words.peek(), Some(&"on" | &"off")) => {
                return Err(format!("{FAST} takes no value: {NO_FAST} turns it off."));
            }
            FAST => asked.fast = Some(true),
            NO_FAST => asked.fast = Some(false),
            EFFORT => {
                let word = words
                    .next()
                    .ok_or("Name a level: --effort off, low, medium or high.")?;
                asked.effort = Some(
                    level(word).ok_or_else(|| format!("No {word}: off, low, medium or high."))?,
                );
            }
            flag if flag.starts_with("--") => return Err(format!("No {flag} here.")),
            name if asked.model.is_none() => asked.model = Some(name.to_string()),
            more => return Err(format!("One at a time: {more} is one too many.")),
        }
    }
    Ok(asked)
}

/// How `asked` is kept: `/model:set` and the model it chose, by its own
/// name, an alias resolved; its `--fast` or `--no-fast`, and its
/// `--effort` level.
pub fn kept(command: &str, asked: &Asked) -> String {
    let mut kept = command.to_string();
    if let Some(model) = &asked.model {
        kept.push(' ');
        kept.push_str(model);
    }
    if let Some(fast) = asked.fast {
        kept.push_str(match fast {
            true => " --fast",
            false => " --no-fast",
        });
    }
    if let Some(level) = asked.effort {
        kept.push_str(" --effort ");
        kept.push_str(level.word());
    }
    if asked.agent {
        kept.push(' ');
        kept.push_str(AGENT);
    }
    kept
}

/// The level `word` names: `off`, `low`, `medium` or `high`.
pub fn level(word: &str) -> Option<Thinking> {
    [
        Thinking::Off,
        Thinking::Low,
        Thinking::Medium,
        Thinking::High,
    ]
    .into_iter()
    .find(|level| level.word() == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(text: &str) -> Event {
        Event {
            role: event::COMMAND.into(),
            ..Event::user(text)
        }
    }

    #[test]
    fn a_conversation_keeps_what_it_chose_the_last_winning() {
        let mut refused = ran("/model:set c");
        refused.failed = true;
        let events = vec![
            ran("/model:set a"),
            ran("/model:set --effort high"),
            ran("/model:set --fast"),
            ran("/model:set b --every-chat"),
            ran("/chat:verbose on"),
            refused,
            Event::user("/model:set d"),
        ];
        assert_eq!(
            chosen(&events),
            Chosen {
                model: Some("b".into()),
                thinking: Some(Thinking::High),
                fast: true,
                verbose: Some(true),
            }
        );
        let off = [events, vec![ran("/model:set --no-fast")]].concat();
        assert!(!chosen(&off).fast);
        assert_eq!(chosen(&[]), Chosen::default());
    }

    #[test]
    fn what_was_asked_is_read_and_kept_as_it_resolved() {
        let set = asked("high --every-chat").unwrap();
        assert_eq!((set.model.as_deref(), set.agent), (Some("high"), true));
        assert_eq!(asked("--fast").unwrap().fast, Some(true));
        assert_eq!(asked("--no-fast").unwrap().fast, Some(false));
        assert_eq!(asked("--effort low").unwrap().effort, Some(Thinking::Low));
        assert!(asked("--effort").is_err() && asked("--effort max").is_err());
        assert!(asked("a b").is_err() && asked("--slow").is_err());
        assert!(asked("--fast off").unwrap_err().contains("--no-fast"));
        let resolved = Asked {
            model: Some("claude-opus-5-5".into()),
            ..set
        };
        assert_eq!(
            kept(SET, &resolved),
            "/model:set claude-opus-5-5 --every-chat"
        );
        assert_eq!(level("medium"), Some(Thinking::Medium));
        assert_eq!(level("max"), None);
    }
}
