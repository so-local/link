//! Hosted chat echoes `you said: <text>` in two UTF-8-safe pieces, followed
//! by the whole answer. `chats.jsonl` records each agent, thread and text.
//!
//! Fixture controls: `chat-steered` joins a running turn; `chat-refused`
//! supplies a refusal. `chat-stalls` publishes `chat.pid` after flushing the
//! first reply, then waits so tests can exercise cancellation.

use std::io::Write;

use anyhow::{Context, ensure};
use link_agent_place::{self as cloud, Request};
use serde_json::json;

use crate::host::{Host, read_json};

impl Host {
    /// `agent-chat`: the answer in pieces, then whole, as JSON lines.
    #[expect(
        clippy::disallowed_methods,
        reason = "a fake host that stalls on purpose"
    )]
    pub(crate) fn chat(&self, request: &Request) -> anyhow::Result<()> {
        let agent = request.agent.as_ref().context("no agent")?;
        let dir = self.agent_dir(&agent.id)?;
        ensure!(dir.is_dir(), "no agent {} here", agent.id);
        let thread = request.thread.as_deref().context("no thread")?;
        link_connection_contract::check_link_thread(thread)?;
        let text = request.text.as_deref().context("no message")?;
        let mut chats = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("chats.jsonl"))?;
        writeln!(
            chats,
            "{}",
            json!({"agent": agent.id, "thread": thread, "text": text})
        )?;
        let lines = if self.dir.join("chat-steered").exists() {
            vec![cloud::Chatting::Steered]
        } else if let Ok(refused) = read_json(&self.dir.join("chat-refused")) {
            vec![cloud::Chatting::Refused(serde_json::from_value(refused)?)]
        } else {
            let answer = format!("you said: {text}");
            let mut half = answer.len() / 2;
            while !answer.is_char_boundary(half) {
                half -= 1;
            }
            let (first, rest) = answer.split_at(half);
            vec![
                cloud::Chatting::Piece(first.to_string()),
                cloud::Chatting::Piece(rest.to_string()),
                cloud::Chatting::Answer(answer.clone()),
            ]
        };
        let mut out = std::io::stdout().lock();
        for line in lines {
            writeln!(out, "{}", serde_json::to_string(&line)?)?;
            out.flush()?;
            if self.dir.join("chat-stalls").exists() {
                std::fs::write(self.dir.join("chat.pid"), std::process::id().to_string())?;
                std::thread::sleep(std::time::Duration::from_secs(600));
            }
        }
        Ok(())
    }
}
