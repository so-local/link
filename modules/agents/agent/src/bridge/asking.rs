//! Owner delivery and notices over the interaction engine's durable records.
use super::WayIn;
use link_format::WithinExt;
use link_interactions::{approval::EffectState, *};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
const LOOK: Duration = Duration::from_millis(500);

/// The outbox reader agent `agent`'s owner delivery registers in the
/// interaction store, which keeps every event after its cursor.
pub fn owner_reader(agent: &str) -> String {
    format!("owner-delivery:{agent}")
}

/// Removes agent `agent`'s outbox reader from `store`, when the agent is
/// deleted or leaves this machine: the events only it would read leave
/// in that write. A stopped
/// agent's stays, holding what it reads when it returns. Answers whether
/// it was registered.
pub fn forget_owner_reader(store: &Store, agent: &str) -> anyhow::Result<bool> {
    store.unsubscribe(&owner_reader(agent))
}

impl WayIn {
    fn owner_threads(&self) -> BTreeMap<String, Vec<String>> {
        let mut threads = BTreeMap::new();
        for (connection, (program, request)) in &self.connections {
            match owner_chats(program, request) {
                Ok(chats) if !chats.is_empty() => {
                    threads.insert(
                        connection.clone(),
                        chats
                            .iter()
                            .map(|chat| format!("{connection}:{chat}"))
                            .collect(),
                    );
                }
                Ok(_) => {}
                Err(err) => link_format::daemon::warn!("asking the owner on {connection}: {err:#}"),
            }
        }
        threads
    }

    pub(super) fn ask_owner(&self, agent: &str, stop: &AtomicBool) {
        std::thread::scope(|scope| {
            scope.spawn(|| self.native_questions(agent, stop));
            self.owner_messages(agent, stop);
        });
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "observes durable owner deliveries without a separate service"
    )]
    fn native_questions(&self, agent: &str, stop: &AtomicBool) {
        if !native::Desktop::available() {
            return;
        }
        let Some(store) = &self.interactions else {
            return;
        };
        let mut attempted = std::collections::BTreeSet::new();
        while !stop.load(Ordering::Relaxed) {
            if let Ok(pending) = store.pending(now()) {
                for snapshot in pending.into_iter().filter(|s| {
                    s.question.requester.source.as_str() == "agent"
                        && link_connection_contract::questions::asked_of(
                            store,
                            &s.question.requester,
                            agent,
                        )
                }) {
                    if attempted.insert(snapshot.question.id.clone())
                        && let Err(err) = native::present_until(
                            store,
                            &snapshot.question.id,
                            &native::Desktop,
                            &|| stop.load(Ordering::Relaxed),
                        )
                    {
                        link_format::daemon::warn!(
                            "native question uses CLI/chat fallback: {err:#}"
                        );
                    }
                }
            }
            std::thread::sleep(LOOK);
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "observes durable interaction outbox without a separate service"
    )]
    fn owner_messages(&self, agent: &str, stop: &AtomicBool) {
        let Some(store) = &self.interactions else {
            return;
        };
        let owners = self.owner_threads();
        let _presence = (!owners.is_empty())
            .then(|| store.presence(&format!("agent:{agent}"), None).ok())
            .flatten();
        let subscriber = owner_reader(agent);
        // Registered before it reads, so the outbox keeps what it hasn't.
        let mut cursor = store.subscribe(&subscriber).unwrap_or_default();
        while !stop.load(Ordering::Relaxed) {
            match store.pending(now()) {
                Ok(waiting) => {
                    for snapshot in waiting.into_iter().filter(|s| {
                        link_connection_contract::questions::asked_of(
                            store,
                            &s.question.requester,
                            agent,
                        )
                    }) {
                        let turns = store
                            .active_routes(&format!("turn:{agent}"))
                            .unwrap_or_default();
                        let origin = snapshot
                            .question
                            .requester
                            .work
                            .as_ref()
                            .and_then(|work| store.owner_route(work).ok().flatten());
                        let routes = origin.map(|route| vec![route]).unwrap_or(turns);
                        for thread in where_to_ask(&owners, &routes) {
                            let surface = thread.split(':').next().unwrap_or_default();
                            let delivered = (|| -> anyhow::Result<()> {
                                let e = store.plan_delivery(
                                    &snapshot.question.id,
                                    &thread,
                                    surface,
                                    &render::text_capabilities(),
                                    None,
                                    now(),
                                )?;
                                if !e.delivery.delivered {
                                    let messages =
                                        self.deliver_receipt(&thread, &render::delivery(&e))?;
                                    store.delivered_ids(&e.delivery.id, &messages)?;
                                }
                                Ok(())
                            })();
                            if let Err(err) = delivered {
                                link_format::daemon::warn!("delivering owner question: {err:#}");
                            }
                        }
                    }
                }
                Err(err) => link_format::daemon::warn!("reading owner questions: {err:#}"),
            }
            if let Ok(events) = store.events(cursor) {
                for event in events {
                    let notice = (|| -> anyhow::Result<()> {
                        if let Some(receipt) = &event.receipt
                            && receipt.target.owner == sources::owner()
                            && store
                                .works(agent)?
                                .iter()
                                .any(|b| b.state.work.agent == receipt.target.agent)
                        {
                            let origin = store
                                .owner_route(&receipt.target)?
                                .into_iter()
                                .collect::<Vec<_>>();
                            for route in where_to_ask(&owners, &origin) {
                                self.deliver(
                                    &route,
                                    &format!(
                                        "{}: {:?} — {}",
                                        receipt.correlation,
                                        receipt.stage,
                                        link_format::clean(&receipt.evidence)
                                    ),
                                )?;
                            }
                        }
                        if let Some(id) = &event.request {
                            let snapshot = store.snapshot(id, now())?;
                            if link_connection_contract::questions::asked_of(
                                store,
                                &snapshot.question.requester,
                                agent,
                            ) && event.kind != "created"
                            {
                                let text = notice(&snapshot);
                                for delivery in
                                    store.deliveries(id)?.into_iter().filter(|d| d.delivered)
                                {
                                    if owners.values().flatten().any(|t| t == &delivery.route)
                                        || delivery.route.starts_with("link:")
                                    {
                                        self.deliver(&delivery.route, &text)?;
                                    }
                                }
                            }
                        }
                        store.acknowledge(&subscriber, event.id)?;
                        Ok(())
                    })();
                    if let Err(err) = notice {
                        link_format::daemon::warn!("delivering question outcome: {err:#}");
                        break;
                    }
                    cursor = event.id;
                }
            }
            std::thread::sleep(LOOK);
        }
    }
}
fn notice(snapshot: &Snapshot) -> String {
    match &snapshot.effect {
        EffectState::Authorized => "Allowed; awaiting source checks and application.".into(),
        EffectState::Applying { .. } => "Allowed; applying the reviewed operation.".into(),
        EffectState::Applied { detail, .. } => format!("Applied: {detail}"),
        EffectState::Failed { reason, .. } => format!("Operation failed: {reason}"),
        EffectState::Unknown { reason, .. } => format!("Operation outcome unknown: {reason}"),
        EffectState::Denied => "Permission refused or expired; no operation authorized.".into(),
        EffectState::Revoked { reason } => format!("Permission closed: {reason}"),
        EffectState::RequiresClarification => {
            "Qualification retained; clarification is required before execution.".into()
        }
        _ => snapshot
            .resolution
            .as_ref()
            .map(|r| {
                r.closed_reason
                    .as_ref()
                    .map(|s| format!("Question closed: {s}"))
                    .unwrap_or_else(|| {
                        format!(
                            "Answer sent to {}{}.",
                            snapshot.question.requester.label,
                            snapshot
                                .answered_on
                                .as_ref()
                                .map(|s| format!(" (answered on {s})"))
                                .unwrap_or_default()
                        )
                    })
            })
            .unwrap_or_else(|| "Awaiting response.".into()),
    }
}
fn where_to_ask(owners: &BTreeMap<String, Vec<String>>, turns: &[String]) -> Vec<String> {
    let mut threads = Vec::new();
    for turn in turns {
        let connection = turn.split(':').next().unwrap_or_default();
        let routes = if connection == crate::chat::CONNECTION {
            vec![turn.clone()]
        } else {
            owners.get(connection).cloned().unwrap_or_default()
        };
        for route in routes {
            if !threads.contains(&route) {
                threads.push(route);
            }
        }
    }
    if threads.is_empty() {
        threads = owners.values().flatten().cloned().collect();
    }
    threads
}

/// What `lnk-<connection> owner` says the owner's direct chats are.
pub(super) fn owner_chats(
    program: &std::path::Path,
    request: &link_connection_contract::Bridge,
) -> anyhow::Result<Vec<String>> {
    let mut child = link_secretive::process::ours(program)
        .arg("owner")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .within(format!("running {} owner", program.display()))?;
    let mut stdin = child.stdin.take().within("no stdin")?;
    stdin.write_all(link_connection_contract::stated(request)?.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;
    // A connection with no `owner` command (webhook's): no chat of the
    // owner's to ask on.
    if !output.status.success() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&output.stdout).within("reading the owner's chats")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_delivery_preserves_initiating_connection_and_direct_chat_fallback() {
        let owners = BTreeMap::from([
            ("telegram".into(), vec!["telegram:42".into()]),
            ("slack".into(), vec!["slack:D1".into()]),
        ]);
        assert_eq!(
            where_to_ask(&owners, &["telegram:-1007".into()]),
            ["telegram:42"]
        );
        assert_eq!(where_to_ask(&owners, &["link:main".into()]), ["link:main"]);
        assert_eq!(
            where_to_ask(&owners, &["missing:thread".into()]),
            ["slack:D1", "telegram:42"]
        );
        assert_eq!(where_to_ask(&owners, &[]), ["slack:D1", "telegram:42"]);
    }
}
