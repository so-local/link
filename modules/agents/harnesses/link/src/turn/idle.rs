//! A quiet conversation's compaction (`[connections] compact_after`, its
//! connection's own over it): kept by the turn with the agent's
//! scheduler and moved by every turn after; when it fires, what's before
//! the owner's last message is summarized, as `/chat:compact` does, told
//! to nobody and said in the log. `"auto"` times it just before the
//! cache the turn asked for ends, so the summary's input is read from
//! it, and keeps it only when the summary costs less than reading the
//! conversation again ([`super::caching`]). It's for a connection
//! holding one long conversation (a Telegram chat), not one whose
//! conversations end (`lnk agent chat`'s, Link's `never`).

use super::*;
use link_scheduler_tool_contract::compaction;

impl Harness {
    /// After a turn's answer: keeps `turn`'s conversation's compaction
    /// due when its connection's `compact_after` says, the one waiting
    /// moved, or, when it says none now, taken away. Nothing for a
    /// conversation on no connection (a subagent's), where that or
    /// `[window] compact_at` is `"never"`, where the conversation holds no
    /// message of the owner's to compact before, or where the agent has
    /// no scheduler; a failure is said, never the turn's.
    pub(super) async fn keep_quiet(&self, turn: &Turn<'_>, config: &Config, toolbox: &Toolbox<'_>) {
        if turn.work != Work::Answer {
            return;
        }
        // A conversation's connection is its name's first part
        // (`telegram:42`), as its other opinions are read.
        let Some((connection, _)) = turn.thread.name().split_once(':') else {
            return;
        };
        let opinions = config.opinions();
        let window = opinions.window.clone().unwrap_or_default();
        let (Some(quiet), Some(_)) = (
            opinions.compact_after(connection),
            window.compact_at.and_then(|at| at.share()),
        ) else {
            return;
        };
        let Ok(events) = off_runtime(|| self.tail(turn.agent, turn.thread)) else {
            return;
        };
        if !events
            .iter()
            .any(|e| e.role == event::USER && e.from.is_none())
        {
            return;
        }
        let after = match quiet {
            link_agent_spec::Quiet::After(after) => Some(after),
            link_agent_spec::Quiet::Auto => {
                let asked = turn.cache.get().cloned().flatten();
                super::caching::Caching::of(config, turn.thread, &events)
                    .and_then(|caching| caching.compaction(asked.as_deref()))
            }
        };
        if let Err(err) = toolbox.keep_compaction(turn.thread.name(), after).await {
            crate::warn!(crate::said::in_chat(turn.agent, turn.thread) => "keeping its compaction for when it's quiet: {err:#}");
        }
    }

    /// Whether `message` is `thread`'s compaction firing: from its own
    /// job, by the job's ID ([`compaction::from`]).
    pub(super) fn is_quiet(thread: &Thread, message: &Event) -> bool {
        message.role == event::USER
            && message.from.as_deref() == Some(compaction::from(thread.name()).as_str())
    }

    /// `thread`'s compaction firing: what's before the owner's last
    /// message summarized, as `/chat:compact` does, the firing itself
    /// kept nowhere. Skipped where the conversation compacted since that
    /// message, while a turn runs, since a message came and its turn
    /// keeps the compaction again, and with nothing left since the
    /// conversation last compacted. What it did comes back as a
    /// command's event, never kept, its answer nobody's.
    pub(super) async fn compact_quiet(
        &self,
        agent: &Agent,
        thread: &Thread,
    ) -> anyhow::Result<Asked> {
        let skipped = |text: &str| {
            Ok(Asked::Answered(Box::new(Event {
                role: event::COMMAND.into(),
                text: text.into(),
                at: event::now(),
                ..Event::default()
            })))
        };
        // Compacted since the owner's last message, by tokens at its turn's
        // end: that summary was never a prompt's start, so the cache
        // wouldn't hold for this one.
        let events = off_runtime(|| self.tail(agent, thread))?;
        let owners = events
            .iter()
            .rposition(|e| e.role == event::USER && e.from.is_none());
        if owners.is_some_and(|at| events[at..].iter().any(|e| e.role == event::SUMMARY)) {
            return skipped(crate::commands::NOTHING_TO_COMPACT);
        }
        let holder = holder()?;
        if off_runtime(|| self.store.take_turn(agent, thread, &holder))? == Took::Held {
            return skipped(crate::commands::BUSY_NOT_COMPACTED);
        }
        // Nobody hears it.
        let (pieces, _) = mpsc::channel(1);
        let told = std::sync::Mutex::new(None);
        let done = self
            .run(
                agent,
                thread,
                &holder,
                None,
                (&pieces, &told),
                Work::Compact,
            )
            .await?;
        crate::note!(crate::said::in_chat(agent, thread) => "quiet, so compacted: {}", done.text);
        Ok(Asked::Answered(Box::new(done)))
    }
}
