//! Goals: `/goal <condition>` sets one on a chat, and after each answer a
//! model judges whether it holds; while it doesn't, and its spec's
//! `[goals]` limits allow, the agent takes another turn toward it on its
//! own. A goal is read from the conversation's events, never kept apart:
//! the owner's `/goal` message sets it, and a command's event ends it
//! (`/goal --clear`, or the judge's or the limits' word), as `/new` does.

use crate::event::{self, Event};

/// How a turn toward a goal begins: the agent's own, never the owner's.
pub const GOING_ON: &str = "[Your goal isn't reached yet. Go on toward it:]";

/// The word the judge's answer starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It holds: the goal is reached.
    Holds,
    /// Work remains, and the agent can go on.
    Going,
    /// It can't be reached as things are.
    Impossible,
}

/// A chat's goal, while it's pursued.
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    pub condition: String,
    /// The turns it took on its own toward it so far.
    pub turns: u32,
    /// The tokens they spent, prompts and answers.
    pub tokens: u64,
}

/// The message a goal's first turn is asked with: what the owner set.
pub fn set(condition: &str) -> String {
    format!(
        "/goal {condition}\n\n[Your owner set you a goal. Work toward it; after each answer \
         it's judged whether it holds, and while it doesn't you go on, on your own:]\n\n\
         {condition}"
    )
}

/// The message of a turn the agent takes toward `goal`, and why the
/// judge said it doesn't hold yet.
pub fn going_on(goal: &Goal, why: &str) -> String {
    format!("{GOING_ON}\n\n{}\n\n(Judged: {why})", goal.condition)
}

/// The goal `events`, a conversation from its last `/new` on, pursues:
/// the latest the owner set and nothing ended since, with what its own
/// turns spent.
pub fn current(events: &[Event]) -> Option<Goal> {
    pursued(advance(None, events))
}

/// `goal`, as found of a conversation's events so far, gone on through
/// `events`, the ones after them: a goal the owner set among them, its
/// turns and tokens counted, or ended. One set with no condition is held
/// as it's counted, and pursues nothing ([`pursued`]).
pub fn advance(mut goal: Option<Goal>, events: &[Event]) -> Option<Goal> {
    for event in events {
        let text = event.text.as_str();
        match event.role.as_str() {
            event::USER if event.from.is_none() => {
                if let Some(rest) = text.strip_prefix("/goal ") {
                    let condition = rest.split("\n\n").next().unwrap_or_default().trim();
                    goal = Some(Goal {
                        condition: condition.to_string(),
                        turns: 0,
                        tokens: 0,
                    });
                } else if text.starts_with(GOING_ON)
                    && let Some(goal) = &mut goal
                {
                    goal.turns += 1;
                }
            }
            event::ASSISTANT => {
                if let (Some(goal), Some(tokens)) = (&mut goal, &event.tokens) {
                    goal.tokens += tokens.input + tokens.output;
                }
            }
            event::COMMAND if text.starts_with(ENDED) || CLEAR_RAN.contains(&text) => goal = None,
            _ => {}
        }
    }
    goal
}

/// The goal `goal` ([`advance`]) pursues: none for one with no condition.
pub fn pursued(goal: Option<Goal>) -> Option<Goal> {
    goal.filter(|goal| !goal.condition.is_empty())
}

/// How a command's event that ends a goal begins.
pub const ENDED: &str = "/goal ended:";

/// How a `/chat:goal --clear` that ran is kept: as its name, or as `/goal`,
/// its alias and its name before the namespaces.
const CLEAR_RAN: [&str; 2] = ["/chat:goal --clear", "/goal --clear"];

/// What the judge is told: the goal, and the agent's last answer.
pub const JUDGE: &str = "You judge whether an agent's goal holds, from its last answer. \
    Answer with one word first, then one short line why: HOLDS if the goal holds now, GOING \
    if work remains and the agent can go on toward it, IMPOSSIBLE if it can't be reached as \
    things are.";

/// The judge's question about `goal`, after `answer`.
pub fn asked(goal: &Goal, answer: &str) -> String {
    format!(
        "The goal:\n{}\n\nThe agent's last answer:\n{answer}",
        goal.condition
    )
}

/// The judge's answer, read: its first word, and its reason. Anything
/// else is going on, so a judge that answers oddly costs a turn, never
/// a goal ended for nothing; the limits still hold.
pub fn verdict(said: &str) -> (Verdict, String) {
    let said = said.trim();
    let (word, why) = said.split_once(char::is_whitespace).unwrap_or((said, ""));
    let word = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
    let why = why.trim().trim_start_matches(['-', ':', '.']).trim();
    let verdict = match word.to_ascii_uppercase().as_str() {
        "HOLDS" => Verdict::Holds,
        "IMPOSSIBLE" => Verdict::Impossible,
        _ => Verdict::Going,
    };
    let why: String = link_format::clean(why).chars().take(280).collect();
    (verdict, why)
}

/// Why `goal` is left, if its spec's `[goals]` limits are reached.
pub fn spent(goal: &Goal, limits: &link_agent_spec::Goals) -> Option<String> {
    if let Some(turns) = limits.turns
        && goal.turns >= turns
    {
        return Some(format!("it took {turns} turns, its most ([goals] turns)"));
    }
    match limits.tokens {
        Some(tokens) if goal.tokens >= tokens => Some(format!(
            "it spent {} tokens, past its most of {tokens} ([goals] tokens)",
            goal.tokens
        )),
        _ => None,
    }
}

/// What `/goal` says of the goal of `events`, a conversation from its
/// last `/new` on: the one pursued, or how the last one ended.
pub fn shown(events: &[Event]) -> String {
    if let Some(goal) = current(events) {
        return format!(
            "Goal: {}\n{} turns toward it on its own so far, {} tokens.",
            goal.condition, goal.turns, goal.tokens
        );
    }
    let last = events.iter().rev().find_map(|e| {
        (e.role == event::COMMAND)
            .then(|| e.text.strip_prefix(ENDED))
            .flatten()
    });
    match last {
        Some(why) => format!(
            "No goal: the last ended, {}. /goal <condition> sets one.",
            why.trim()
        ),
        None => "No goal: /goal <condition> sets one.".into(),
    }
}

/// What the owner is told once `/goal --clear` has run.
pub const CLEARED: &str = "Goal cleared: it won't go on toward it.";

#[cfg(test)]
mod tests {
    use super::*;

    fn said(role: &str, text: &str) -> Event {
        Event {
            role: role.into(),
            text: text.into(),
            ..Event::default()
        }
    }

    #[test]
    fn a_goal_is_read_from_its_conversation() {
        let mut answer = said(event::ASSISTANT, "done some");
        answer.tokens = Some(event::Tokens {
            input: 900,
            output: 100,
            cached: 0,
        });
        let goal = Goal {
            condition: "tests pass".into(),
            turns: 0,
            tokens: 0,
        };
        let mut events = vec![
            said(event::USER, &set("tests pass")),
            answer.clone(),
            said(event::USER, &going_on(&goal, "two fail")),
            answer,
        ];
        let now = current(&events).unwrap();
        assert_eq!(
            (now.condition.as_str(), now.turns, now.tokens),
            ("tests pass", 1, 2000)
        );
        // Someone else's "/goal" sets none.
        let mut other = said(event::USER, "/goal be mine");
        other.from = Some("member:telegram:7".into());
        events.push(other);
        assert_eq!(current(&events).unwrap().condition, "tests pass");
        events.push(said(event::COMMAND, "/goal --clear"));
        assert_eq!(current(&events), None);
        events.push(said(event::USER, &set("ship it")));
        events.push(said(event::COMMAND, &format!("{ENDED} it holds")));
        assert_eq!(current(&events), None);
    }

    #[test]
    fn the_judges_word_and_the_limits() {
        assert_eq!(
            verdict("HOLDS - all green"),
            (Verdict::Holds, "all green".into())
        );
        assert_eq!(verdict("**Impossible**: no key").0, Verdict::Impossible);
        assert_eq!(verdict("GOING two fail").0, Verdict::Going);
        assert_eq!(verdict("maybe?").0, Verdict::Going);
        let goal = Goal {
            condition: "x".into(),
            turns: 3,
            tokens: 500,
        };
        let limits = |turns, tokens| link_agent_spec::Goals { turns, tokens };
        assert!(
            spent(&goal, &limits(Some(3), None))
                .unwrap()
                .contains("3 turns")
        );
        assert!(
            spent(&goal, &limits(Some(4), Some(500)))
                .unwrap()
                .contains("500 tokens")
        );
        assert_eq!(spent(&goal, &limits(Some(4), Some(1000))), None);
    }
}
