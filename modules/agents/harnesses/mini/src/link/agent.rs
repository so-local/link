//! One prompt through the harness: serialize turns within a thread, read
//! its history, discover allowed tools, ask the model, answer tool calls,
//! and save the final answer. Model and MCP wire details live in their
//! own modules so the loop below shows the harness's job.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    model, threads,
    tools::{self, Tools},
};

pub(crate) struct Harness {
    client: reqwest::Client,
    tools_from_env: bool,
    /// One turn at a time per thread.
    turns: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Each thread's MCP servers, as its session was given them.
    servers: Mutex<HashMap<String, Vec<(String, String)>>>,
}

impl Harness {
    pub(crate) fn new(tools_from_env: bool) -> Self {
        Self {
            client: reqwest::Client::new(),
            tools_from_env,
            turns: Mutex::default(),
            servers: Mutex::default(),
        }
    }
    /// Remember the MCP servers supplied when a session opens or resumes.
    pub(crate) fn given(
        &self,
        thread: &str,
        servers: &[agent_client_protocol::schema::v1::McpServer],
    ) {
        self.servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(thread.to_string(), super::acp::http(servers));
    }

    /// Answer one message using the thread's history and allowed tools.
    /// Stream text through `pieces` and persist the completed answer.
    pub(crate) async fn turn(
        &self,
        options: &super::acp::Options,
        thread: &str,
        message: &str,
        said: link_connection_contract::acp::Said,
        pieces: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        let (requested_tools, hops) = (said.tools, said.hops);
        // Its sender's standing, at most its firing's.
        let standing = link_connection_contract::access::Standing::of(said.from.as_deref());
        let standing = said.standing.map_or(standing, |cap| standing.at_most(cap));
        let lock = self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(thread.to_string())
            .or_default()
            .clone();
        let _turn = lock.lock().await;
        if let Some(only) = requested_tools {
            threads::narrow(thread, only)?;
        }
        let only = threads::tools(thread)?;
        let messages = threads::read(thread)?;
        crate::runtime::conversation::check_complete(&messages)?;
        let instructions = match &options.instructions {
            Some(path) => crate::runtime::instructions::read_instructions(path)?,
            None => String::new(),
        };
        threads::append(thread, "user", message)?;
        let messages = threads::replayed(threads::read(thread)?, &link_spec::follow::replayed);
        let servers = match self.tools_from_env {
            true => tools::from_env(),
            false => (self.servers.lock().unwrap_or_else(|e| e.into_inner()))
                .get(thread)
                .cloned()
                .unwrap_or_default(),
        };
        let mut tools = Tools::open(&self.client, servers).await;
        if let Some(only) = only {
            tools.restrict(&only);
        }
        // Where each call is made: this thread, its hops, its tools, and
        // the standing of the message it answers.
        let names = tools.names();
        let meta = json!({"link.local/thread": thread, "link.local/hops": hops,
            "link.local/tools": names, "link.local/standing": standing});
        let offered = tools.offered();
        let request = model::Request {
            client: &self.client,
            model: &options.model,
            known: options.model_facts.as_ref(),
            window: options.window,
            instructions: &instructions,
            tools: &offered,
            pieces: &pieces,
            thread,
        };
        let request = &request;
        let tools_ref = &tools;
        let client = &self.client;
        let meta = &meta;
        let answer = crate::core::run_turn(
            messages,
            |messages| async move { request.ask_model(&messages).await },
            |call| async move { tools_ref.call_tool(client, &call, meta).await },
            |message| threads::append_message(thread, message),
        )
        .await;
        tools.close(&self.client).await;
        answer
    }
}
