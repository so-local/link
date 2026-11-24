//! Keeping a conversation's idle compaction (`[window] compact_after`)
//! with the agent's scheduler: its `compact` call, made by the harness
//! and never offered to the model
//! ([`link_scheduler_tool_contract::compaction`]).

use super::*;
use link_scheduler_tool_contract::compaction;

impl Toolbox<'_> {
    /// Keeps `thread`'s compaction due `after` from now, moving the one
    /// waiting, or with none, takes it away: whether the agent has a
    /// scheduler to keep it. A session with it is started as a call's
    /// is, and ends with the turn.
    pub async fn keep_compaction(
        &self,
        thread: &str,
        after: Option<std::time::Duration>,
    ) -> anyhow::Result<bool> {
        let names = compaction::compaction();
        let Some((server, started)) = self.servers.iter().find(|(s, _)| s.name == names.tool)
        else {
            return Ok(false);
        };
        let mut started = started.lock().await;
        if matches!(*started, Started::Not) {
            let starting = Session::start_new(&self.client, server, self.timeout);
            *started = match tokio::time::timeout(self.timeout, starting).await {
                Ok(Ok(session)) => Started::Open(Arc::new(session)),
                Ok(Err(err)) => Started::Failed(format!("{err:#}")),
                Err(_) => Started::Failed("it took too long to start".into()),
            };
        }
        let session = match &*started {
            Started::Open(session) => session.clone(),
            Started::Failed(why) => anyhow::bail!("the scheduler is unavailable: {why}"),
            Started::Not => anyhow::bail!("the scheduler didn't start"),
        };
        drop(started);
        let arguments = match after {
            Some(after) => json!({names.after.as_str(): after.as_secs().max(1)}),
            None => json!({}),
        };
        let params = json!({"name": names.call, "arguments": arguments,
            "_meta": {"link.local/thread": thread}});
        let called = tokio::time::timeout(
            self.timeout,
            session.request("tools/call", params, &AtomicU64::new(0)),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the scheduler took too long to keep it"))??;
        anyhow::ensure!(
            called["isError"] != true,
            "the scheduler refused it: {}",
            called["content"][0]["text"].as_str().unwrap_or_default()
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The server compaction is kept through is a namespace every agent
    /// is handed, and its call that namespace's harness-only action, so
    /// the model is never offered it and the scheduler still checks it.
    #[test]
    fn compaction_is_a_handed_namespaces_harness_action() {
        let names = compaction::compaction();
        assert!(link_harness_contract::harness::NAMESPACES.contains(&names.tool.as_str()));
        let declared = &link_harness_contract::harness::namespaces::declared()[&names.tool];
        let action = &declared[&names.call];
        assert!(action.harness);
        assert!(action.targets.iter().any(|target| {
            target.kind == link_harness_contract::harness::namespaces::Kind::Schedule
                && target.native == names.call
        }));
    }
}
