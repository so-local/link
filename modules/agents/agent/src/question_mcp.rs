//! The host-side tool adapter waits on a handle; response ingress stays independent.
use anyhow::{Context, ensure};
use link_agent::settings::Settings;
use link_interactions::{sources, *};
use serde_json::Value;

pub(crate) fn offered() -> Value {
    serde_json::json!({"name":"question_create",
        "description":"Ask the owner one related set of confirmation, text or choice questions and wait for its exact typed result. Mark recommendations explicitly. Preserve user qualifications and distinguish selected supplied wording from user-written text. Clarify material conflicts. Ordinary answers and automatic fallbacks grant no execution authority.",
        "inputSchema": QuestionProposal::schema()})
}

pub(crate) fn create(
    agent: &Settings,
    args: Value,
    meta: &Value,
) -> anyhow::Result<(Store, RequestId)> {
    let proposal: QuestionProposal = serde_json::from_value(args)?;
    let thread = meta["link.local/thread"]
        .as_str()
        .context("question requires its calling conversation")?;
    let attempt = meta["link.local/attempt"]
        .as_str()
        .context("this harness cannot bind a question to an exact attempt")?;
    let call = meta["link.local/call"]
        .as_str()
        .context("question requires its original tool call")?;
    ensure!(
        !thread.is_empty()
            && thread.len() <= 512
            && !thread.chars().any(char::is_control)
            && !attempt.is_empty()
            && !call.is_empty(),
        "invalid question continuation"
    );
    let store = Store::machine()?;
    let token = meta["link.local/work"]
        .as_str()
        .context("this harness cannot authenticate the exact work")?;
    let spec = agent.spec()?.spec;
    let wait = spec
        .connections
        .as_ref()
        .and_then(|c| c.ask_wait)
        .context("agent has no question wait")?;
    let fallback_wait = wait.get();
    let id = sources::create_question(
        &store,
        sources::QuestionCaller {
            revision: meta["link.local/control_revision"]
                .as_u64()
                .context("question call has no control revision")?,
            token: &link_secretive::Secret::new(token.to_owned()),
            agent: &agent.key(),
            service: agent
                .service
                .as_deref()
                .context("this agent acts as no service, so it asks nothing")?,
            label: &agent.name,
            session: thread,
            attempt,
            call,
        },
        proposal,
        &spec.approvals.unwrap_or_default(),
        fallback_wait,
        now(),
    )?;
    Ok((store, id))
}

type Output = std::sync::Arc<std::sync::Mutex<link_format::streams::Protocol>>;
const MAX_PENDING: usize = 32;

#[derive(Default)]
pub(crate) struct Waits {
    pending: std::collections::BTreeMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
    workers: Vec<(String, std::thread::JoinHandle<()>)>,
}
impl Waits {
    /// Return from admission immediately; each original MCP call has its own waiter.
    pub fn start(
        &mut self,
        agent: &Settings,
        args: Value,
        meta: &Value,
        call: Value,
        output: Output,
    ) -> anyhow::Result<()> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let mut active = Vec::new();
        for (key, worker) in self.workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
                self.pending.remove(&key);
            } else {
                active.push((key, worker));
            }
        }
        self.workers = active;
        let key = call.to_string();
        if self.pending.contains_key(&key) {
            return Ok(());
        }
        ensure!(
            self.pending.len() < MAX_PENDING,
            "too many pending questions in this session"
        );
        let (store, id) = create(agent, args, meta)?;
        let stop = Arc::new(AtomicBool::new(false));
        self.pending.insert(key.clone(), stop.clone());
        self.workers.push((key, std::thread::spawn(move || {
            let result = store.wait(&id, || stop.load(Ordering::Relaxed)).and_then(|snapshot|
                serde_json::to_value(snapshot.resolution.context("question has no result")?).map_err(Into::into));
            let reply = match result {
                Ok(answer) => serde_json::json!({"jsonrpc":"2.0","id":call,"result":{
                    "content":[{"type":"text","text":answer.to_string()}],"structuredContent":answer}}),
                Err(err) => serde_json::json!({"jsonrpc":"2.0","id":call,"error":{"code":-32603,"message":format!("Question failed: {err:#}")}}),
            };
            if let Ok(mut output) = output.lock() {
                use std::io::Write;
                let _ = writeln!(output, "{reply}");
                let _ = output.flush();
            }
        })));
        Ok(())
    }
    pub fn cancel(&self, call: &Value) {
        if let Some(stop) = self.pending.get(&call.to_string()) {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
impl Drop for Waits {
    fn drop(&mut self) {
        for stop in self.pending.values() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        for (_, worker) in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
