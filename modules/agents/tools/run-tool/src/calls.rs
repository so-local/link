//! The calls a session runs at once, and how one is cancelled.

use std::sync::Mutex;

use serde_json::Value;

/// The calls a session is running, by their request's ID: at most
/// [`link_run_tool_contract::Limits::calls_at_once`], each one's process
/// group stopped when its caller cancels it.
#[derive(Default)]
pub struct Calls {
    running: Mutex<std::collections::HashMap<String, Call>>,
}

/// A call running: its command's process group once it started, and
/// whether its caller cancelled it.
#[derive(Default)]
pub(crate) struct Call {
    pub(crate) group: Option<i32>,
    cancelled: bool,
}

impl Calls {
    pub(crate) fn running(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, Call>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Call `id` entered, while it runs: none past the most at once, or
    /// with a call of its ID already running.
    pub(crate) fn enter(&self, id: &Value) -> Option<Entered<'_>> {
        let key = id.to_string();
        let mut running = self.running();
        if running.len() >= link_run_tool_contract::limits().calls_at_once
            || running.contains_key(&key)
        {
            return None;
        }
        running.insert(key.clone(), Call::default());
        Some(Entered { calls: self, key })
    }

    /// Call `id` cancelled: its process group, once started, killed.
    pub(crate) fn cancel(&self, id: &Value) {
        if let Some(call) = self.running().get_mut(&id.to_string()) {
            call.cancelled = true;
            if let Some(group) = call.group {
                kill_group(group);
            }
        }
    }
}

/// A call entered in [`Calls`], left when dropped.
pub(crate) struct Entered<'a> {
    calls: &'a Calls,
    key: String,
}

impl Entered<'_> {
    /// Its command started as process group `group`, which a cancel now
    /// kills: whether it was cancelled already, and so is to be killed.
    pub(crate) fn started(&self, group: i32) -> bool {
        let mut running = self.calls.running();
        let call = running.entry(self.key.clone()).or_default();
        call.group = Some(group);
        call.cancelled
    }

    /// Its command's group about to be reaped: no cancel names it after.
    pub(crate) fn ending(&self) {
        if let Some(call) = self.calls.running().get_mut(&self.key) {
            call.group = None;
        }
    }
}

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.calls.running().remove(&self.key);
    }
}

/// SIGKILL to process group `group`; never group 1 or below, since
/// `kill(-1)` is every process.
pub(crate) fn kill_group(group: i32) {
    if group > 1 {
        // SAFETY: a signal to a process group a child of ours leads.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}
