//! Link conversation history and forks, from the same store as a turn.

use super::*;
use link_api::types::{RemoteConversation, RemoteConversations};
use serde_json::json;

impl Files {
    /// The owner's Link threads, never another connection's conversations.
    pub fn list_conversations(&self, agent: &Agent) -> anyhow::Result<RemoteConversations> {
        let mut threads = Vec::new();
        for (named, folder) in self.each_folder(agent) {
            if folder.kind("fork.pending").is_some() {
                continue;
            }
            let Some(way) =
                kept_in(&folder)?.or_else(|| folder.kind("thread.json").map(|_| Way::Log))
            else {
                continue;
            };
            let thread = self.thread_in(agent, &folder, &named, &way)?;
            if is_link(&thread) {
                threads.push(thread.name().to_string());
            }
        }
        threads.sort();
        threads.dedup();
        Ok(serde_json::from_value(
            json!({"threads": threads, "fork": true}),
        )?)
    }

    /// Text meant for the owner, with its event number and time. Tool
    /// arguments, hidden thinking and summaries never enter the browser.
    pub fn history(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<RemoteConversation> {
        anyhow::ensure!(
            is_link(thread),
            "only link conversations may be opened here"
        );
        anyhow::ensure!(
            self.folder(agent, thread, false)?
                .is_none_or(|folder| folder.kind("fork.pending").is_none()),
            "fork is not complete"
        );
        let messages: Vec<_> = self.read(agent, thread)?.into_iter()
            .filter(|event| (event.role == crate::event::USER && event.from.is_none()) || event.is_answer())
            .map(|event| json!({"seq": event.seq, "role": event.role, "text": event.text, "at": event.at}))
            .collect();
        Ok(serde_json::from_value(
            json!({"thread": thread.name(), "messages": messages, "fork": true}),
        )?)
    }

    /// Copies the immutable event prefix before an owner's message.
    /// Keeping every event unchanged preserves the model's cacheable
    /// prefix, including thinking, tool calls, summaries and window marks.
    pub fn fork_conversation(
        &self,
        agent: &Agent,
        source: &Thread,
        before: u64,
    ) -> anyhow::Result<RemoteConversation> {
        anyhow::ensure!(
            is_link(source),
            "only link conversations may be forked here"
        );
        let events = self.read(agent, source)?;
        anyhow::ensure!(
            events
                .iter()
                .any(|e| e.seq == before && e.role == crate::event::USER && e.from.is_none()),
            "fork before an owner's message in this conversation"
        );
        let thread = Thread::new(&format!("link:fork-{}", link_files::random_token()?))?;
        let holder = link_files::random_token()?;
        anyhow::ensure!(
            self.take_turn(agent, &thread, &holder)? == Took::Took,
            "new thread is busy"
        );
        let copied = (|| {
            anyhow::ensure!(
                self.read(agent, &thread)?.is_empty(),
                "new thread already exists"
            );
            let folder = self
                .folder(agent, &thread, true)?
                .within("new thread's folder")?;
            folder.write("fork.pending", b"")?;
            if let Some(tools) = self.tools(agent, source)? {
                self.narrow_tools(agent, &thread, &tools)?;
            }
            let mut prefix = Vec::new();
            for event in events.into_iter().take_while(|e| e.seq < before) {
                // Its images and documents first, under the same names.
                for part in &event.parts {
                    let bytes = self.part(agent, source, &part.file)?;
                    self.keep_part(agent, &thread, &part.file, &bytes)?;
                }
                self.append(agent, &thread, &event, &prefix)?;
                prefix.push(event);
            }
            // Empty forks still have a discoverable, unambiguous thread.
            let folder = self
                .folder(agent, &thread, true)?
                .within("new thread's folder")?;
            folder.write("thread.json", &serde_json::to_vec(thread.name())?)?;
            folder.dir.remove_file(OsStr::new("fork.pending"))?;
            self.history(agent, &thread)
        })();
        self.end_turn(agent, &thread, &holder)?;
        copied
    }
}

fn is_link(thread: &Thread) -> bool {
    thread
        .name()
        .parse::<link_api::types::RemoteThread>()
        .is_ok()
}
