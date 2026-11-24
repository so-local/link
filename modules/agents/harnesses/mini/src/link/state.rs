//! The mini harness's agent state over ASP, answered by `lnk-mini-harness acp`
//! itself: its sessions, each thread's file of messages. It keeps no
//! instructions, skills, tools, memory or settings.

use super::threads;
use link_harness_contract::asp::acp::schema::v1::SessionId;
use link_harness_contract::asp::forms::{By, Message, Session};
use link_harness_contract::asp::methods::{Described, Imported};
use link_harness_contract::asp::{AgentState, Kind, State};

/// What it gives and takes: sessions.
pub(crate) fn kinds() -> AgentState {
    AgentState {
        export: vec![Kind::Sessions],
        import: vec![Kind::Sessions],
    }
}

pub(crate) fn describe() -> Described {
    Described {
        kinds: [(Kind::Sessions, threads::names().len())].into(),
    }
}

/// Its sessions, when asked for.
pub(crate) fn export(kinds: &[Kind]) -> anyhow::Result<State> {
    let sessions = kinds
        .contains(&Kind::Sessions)
        .then(|| {
            threads::names()
                .into_iter()
                .map(|thread| {
                    let messages: Vec<Message> = threads::read(&thread)?
                        .iter()
                        .filter_map(|line| {
                            let by = match line.role.as_str() {
                                "user" => By::User,
                                "assistant" => By::Agent,
                                _ => return None,
                            };
                            let text = line.text.clone();
                            if text.is_empty() {
                                return None;
                            }
                            Some(Message { by, text })
                        })
                        .collect();
                    Ok(Session::of_messages(
                        SessionId::new(thread),
                        None,
                        &messages,
                    ))
                })
                .collect::<anyhow::Result<Vec<_>>>()
        })
        .transpose()?;
    Ok(State {
        sessions,
        ..State::default()
    })
}

/// Each session a thread of its own: its name where that's free, else
/// a new one. Every other kind isn't taken.
pub(crate) fn import(state: State) -> anyhow::Result<Imported> {
    let mut imported = Imported::default();
    for kind in state.kinds() {
        if kind != Kind::Sessions {
            imported
                .not_taken
                .push(link_harness_contract::asp::methods::NotTaken {
                    kind,
                    reason: "the mini harness keeps none".into(),
                });
        }
    }
    let Some(sessions) = state.sessions else {
        return Ok(imported);
    };
    let taken = threads::names();
    for (n, session) in sessions.iter().enumerate() {
        let old = session.session_id.0.to_string();
        let clean = threads::check(&old).is_ok();
        let thread = match clean && !taken.contains(&old) {
            true => old.clone(),
            false => format!("imported-{}-{n}", std::process::id()),
        };
        for message in session.messages() {
            let role = match message.by {
                By::User => "user",
                By::Agent => "assistant",
            };
            threads::append(&thread, role, &message.text)?;
        }
        imported
            .sessions
            .insert(session.session_id.clone(), SessionId::new(thread));
    }
    imported.taken.push(Kind::Sessions);
    Ok(imported)
}
