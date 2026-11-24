//! A goal judged after an answer ([`crate::goal`]).

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::mpsc;

use super::{Harness, Marks, off_runtime};
use crate::agent::{Agent, Thread};
use crate::event::{self, Event};
use crate::goal::{self, Goal, Verdict};
use crate::model;

/// The most conversations whose goal is remembered: past it, the one
/// found longest ago is forgotten, and found again from its first event.
const FOUND_MAX: usize = 64;

/// The conversations whose goal is pursued, and the goal each was last
/// found to pursue, so finding it after the next turn reads only the
/// events added since ([`Harness::pursued`]). Derived: a process without
/// it reads a conversation once, and one that finds the event it was read
/// to changed, or a conversation begun since, reads it again.
#[derive(Default)]
pub(super) struct Goals {
    /// The conversations whose goal is pursued now
    /// ([`Harness::pursuing`]).
    pub(super) pursuing: Marks,
    found: Mutex<(u64, HashMap<Key, Found>)>,
}

/// A conversation, by its agent's owner and ID and its thread.
type Key = (String, String, Thread);

/// A conversation's goal as found when its chat's last event was the one
/// numbered `seq`, written `at`: unfiltered ([`goal::advance`]), and the
/// agent's last answer in the conversation the chat is on.
struct Found {
    seq: u64,
    at: String,
    goal: Option<Goal>,
    answer: Option<String>,
    used: u64,
}

impl Goals {
    fn take(&self, key: &Key) -> Option<Found> {
        self.found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(key)
    }

    fn keep(&self, key: Key, mut found: Found) {
        let mut held = self.found.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, conversations) = &mut *held;
        *tick += 1;
        found.used = *tick;
        conversations.insert(key, found);
        while conversations.len() > FOUND_MAX {
            let Some(oldest) = conversations
                .iter()
                .min_by_key(|(_, found)| found.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            conversations.remove(&oldest);
        }
    }
}

impl Harness {
    /// The goal `thread` pursues, and the agent's last answer, in the
    /// conversation its chat is on: none when it pursues none. Read on
    /// from the window, from where it was last found, when that event is
    /// still there as it was and nothing since began a conversation; else
    /// from the chat's first event. So a turn's goal costs what the turn
    /// added, however long the chat.
    pub(crate) fn pursued(
        &self,
        agent: &Agent,
        thread: &Thread,
    ) -> anyhow::Result<Option<(Goal, Option<String>)>> {
        off_runtime(|| {
            let key = (
                agent.owner().to_string(),
                agent.id().to_string(),
                thread.clone(),
            );
            let found = self.goals.take(&key);
            let tail = self.tail_read(agent, thread)?;
            let on = found.and_then(|found| {
                let at = tail
                    .events
                    .iter()
                    .position(|e| e.seq == found.seq && e.at == found.at)?;
                let added = &tail.events[at + 1..];
                if added.iter().any(|e| crate::commands::began(e).is_some()) {
                    return None;
                }
                let answer = added.iter().rev().find(|e| e.is_answer());
                Some(Found {
                    goal: goal::advance(found.goal, added),
                    answer: answer.map(|e| e.text.clone()).or(found.answer),
                    ..found
                })
            });
            let last = tail.events.last().map(|e| (e.seq, e.at.clone()));
            let found = match (on, last) {
                (Some(on), Some((seq, at))) => Found { seq, at, ..on },
                _ => {
                    let events = self.store.read(agent, thread)?;
                    let Some(last) = events.last() else {
                        return Ok(None);
                    };
                    let (seq, at) = (last.seq, last.at.clone());
                    let (events, _) = crate::commands::conversation(events, None, usize::MAX);
                    Found {
                        seq,
                        at,
                        goal: goal::advance(None, &events),
                        answer: events
                            .iter()
                            .rev()
                            .find(|e| e.is_answer())
                            .map(|e| e.text.clone()),
                        used: 0,
                    }
                }
            };
            let pursued = goal::pursued(found.goal.clone()).map(|g| (g, found.answer.clone()));
            self.goals.keep(key, found);
            Ok(pursued)
        })
    }

    /// Whether `goal` holds after `answer`, as its agent's model judges,
    /// asked alone: no conversation, no tools.
    pub async fn judged(
        &self,
        agent: &Agent,
        thread: &Thread,
        goal: &Goal,
        answer: &str,
    ) -> anyhow::Result<(Verdict, String)> {
        // Its calls are the thread's, apart from its turns'.
        let chat = format!("{}/goal", thread.name());
        let config = self.config(agent)?;
        let messages = [model::Message {
            role: crate::event::USER.into(),
            text: goal::asked(goal, answer),
            ..model::Message::default()
        }];
        let prompt = model::Prompt {
            system: goal::JUDGE,
            messages: &messages,
            tools: &[],
            no_calls: false,
            answer: 256,
            window: 32768,
            price: None,
            above: &[],
            thinking: model::Think::default(),
            fast: false,
            chat: Some(&chat),
            spans: &[],
            parent: Some(thread.name()),
            cache: None,
        };
        // Nobody reads it as it's written.
        let (unread, _) = mpsc::channel(1);
        let (_, cancelled) = tokio::sync::watch::channel(false);
        let said = model::answer(
            &self.client,
            &config.model,
            &prompt,
            &unread,
            false,
            cancelled,
        )
        .await?;
        Ok(goal::verdict(&said.text))
    }

    /// Ends `thread`'s goal, saying why: an event of its own, never sent
    /// to the model, that `/goal` names.
    pub fn end_goal(&self, agent: &Agent, thread: &Thread, why: &str) -> anyhow::Result<()> {
        let mut events = off_runtime(|| self.tail(agent, thread))?;
        let mut ended = Event {
            role: event::COMMAND.into(),
            text: format!("{} {why}", goal::ENDED),
            at: event::now(),
            ..Event::default()
        };
        off_runtime(|| self.append_next(agent, thread, &mut ended, &mut events))
    }
}
