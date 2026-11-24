//! ASP's agent state, for a harness whose `initialize` advertises it:
//! what it exports holds a thread it answered, and a conversation it
//! imports goes on, the model sent what came in.

use super::*;
use link_harness_contract::asp::forms::{By, Message, Session};
use link_harness_contract::asp::methods::{Export, Import};
use link_harness_contract::asp::{AgentState, Kind};

/// The conversation the check imports, on its thread.
const BROUGHT: &str = "conformance:asp";
const SAID: &str = "Brought from another harness";
pub(super) const NEXT_ASK: &str = "What should you do next?";
pub(super) const NEXT_ANSWER: &str =
    "Next: check the invoice for the green delivery. Do not send it yet.";

impl Kit {
    /// After the endpoint's checks, which answered `conformance:1`.
    pub(super) fn state_checks(&self, url: &str) -> anyhow::Result<()> {
        let advertised = self.runtime.block_on(async {
            let lines = saying::opened(url, Some(AGENT), Some(KEY))
                .await?
                .map_err(|said| anyhow::anyhow!("its ACP refused: {}", said.status))?;
            acp::Client
                .builder()
                .connect_with(lines, async |cx: acp::ConnectionTo<acp::Agent>| {
                    let up = cx
                        .send_request(acp::schema::v1::InitializeRequest::new(
                            acp::schema::ProtocolVersion::V1,
                        ))
                        .block_task()
                        .await?;
                    let Some(kinds) = AgentState::of(&up.agent_capabilities) else {
                        return Ok(None);
                    };
                    if !kinds.export.contains(&Kind::Sessions)
                        || !kinds.import.contains(&Kind::Sessions)
                    {
                        return Ok(None);
                    }
                    let asked = Export {
                        kinds: vec![Kind::Sessions],
                    };
                    let exported = cx.send_request(asked).block_task().await?.state;
                    let brought = Session::of_messages(
                        acp::schema::v1::SessionId::new(BROUGHT),
                        None,
                        &[Message {
                            by: By::User,
                            text: SAID.into(),
                        }],
                    );
                    let state = link_harness_contract::asp::State {
                        sessions: Some(vec![brought]),
                        ..Default::default()
                    };
                    let imported = cx.send_request(Import { state }).block_task().await?;
                    Ok(Some((exported, imported)))
                })
                .await
                .map_err(|err| anyhow::anyhow!("asking its agent state: {err}"))
        })?;
        let Some((exported, imported)) = advertised else {
            return Ok(());
        };
        let sessions = exported.sessions.unwrap_or_default();
        let first = sessions
            .iter()
            .find(|s| &*s.session_id.0 == "conformance:1")
            .with_context(|| "its export holds no conformance:1, a thread it answered")?;
        ensure!(
            first.messages().iter().any(|m| m.text.contains("Hello")),
            "its export of conformance:1 lost what was said: {:?}",
            first.messages()
        );
        let thread = imported
            .sessions
            .get(&acp::schema::v1::SessionId::new(BROUGHT))
            .with_context(|| format!("its import named no session for {BROUGHT}"))?
            .0
            .to_string();
        ensure!(
            thread == BROUGHT,
            "the conversation came in as {thread}, not {BROUGHT}"
        );
        let said = self.runtime.block_on(say(
            url,
            Saying {
                agent: Some(AGENT),
                key: Some(KEY),
                thread: BROUGHT,
                text: "Go on",
                ..Saying::default()
            },
        ))?;
        ensure!(
            said.ok(),
            "a request on an imported thread was answered {}",
            said.status
        );
        let sent = self.last_asked();
        ensure!(
            sent.contains(SAID),
            "the model wasn't sent the conversation that came in: {sent}"
        );
        self.personal_pairs(url)?;
        Ok(())
    }

    /// Every personal destination exercises the other three source archives.
    /// Adapter codecs have their own pinned-format fixtures. This boundary
    /// checks exact archive retention and the next model input in all 12 pairs.
    fn personal_pairs(&self, url: &str) -> anyhow::Result<()> {
        let destination = self
            .adapter
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let destination = match destination {
            "lnk-harness" => "link",
            "lnk-hermes" => "hermes",
            "lnk-openclaw" => "openclaw",
            "lnk-deepseek" => "deepseek",
            _ => return Ok(()),
        };
        for source in ["link", "hermes", "openclaw", "deepseek"]
            .into_iter()
            .filter(|source| *source != destination)
        {
            let thread = format!("conformance:{source}-to-{destination}");
            let mut brought = Session::of_messages(
                thread.clone().into(),
                None,
                &[
                    Message {
                        by: By::User,
                        text: "Prepare a delivery. The initial choice was blue.".into(),
                    },
                    Message {
                        by: By::Agent,
                        text: "Decision: check the invoice before sending anything.".into(),
                    },
                    Message {
                        by: By::User,
                        text: "Correction: green replaces blue. The invoice is unfinished.".into(),
                    },
                ],
            );
            brought.harness = Some(source.into());
            let updates = serde_json::from_value::<Vec<acp::schema::v1::SessionUpdate>>(
                serde_json::json!([
                    {"sessionUpdate":"tool_call","toolCallId":"historical-call","title":"Read invoice","name":"file__read","rawInput":{"file_path":"invoice"}},
                    {"sessionUpdate":"tool_call_update","toolCallId":"historical-call","status":"completed","rawOutput":"Invoice has not been checked"},
                    {"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":""},"_meta":{"link":{"nativeRecord":{"extension":"retain-exactly"}}}}
                ]),
            )?;
            brought.updates.extend(updates);
            let tool_calls = self
                .seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .called
                .len();
            let imported = self.runtime.block_on(async {
                let lines = saying::opened(url, Some(AGENT), Some(KEY))
                    .await?
                    .map_err(|said| anyhow::anyhow!("ACP refused {}", said.status))?;
                acp::Client
                    .builder()
                    .connect_with(lines, async |cx: acp::ConnectionTo<acp::Agent>| {
                        cx.send_request(acp::schema::v1::InitializeRequest::new(
                            acp::schema::ProtocolVersion::V1,
                        ))
                        .block_task()
                        .await?;
                        let imported = cx
                            .send_request(Import {
                                state: link_harness_contract::asp::State {
                                    sessions: Some(vec![brought.clone()]),
                                    ..Default::default()
                                },
                            })
                            .block_task()
                            .await?;
                        Ok(imported)
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })?;
            ensure!(
                imported.sessions.contains_key(&brought.session_id),
                "{source} to {destination} lost its routing identity"
            );
            let said = self.runtime.block_on(say(
                url,
                Saying {
                    agent: Some(AGENT),
                    key: Some(KEY),
                    thread: &thread,
                    text: NEXT_ASK,
                    ..Saying::default()
                },
            ))?;
            ensure!(said.ok(), "{source} to {destination} could not continue");
            ensure!(
                said.text.contains(NEXT_ANSWER),
                "{source} to {destination} lost its next decision: {}",
                said.text
            );
            ensure!(
                self.seen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .called
                    .len()
                    == tool_calls,
                "{source} to {destination} replayed a historical tool effect"
            );
            let sent = self.last_asked();
            ensure!(
                sent.contains("green replaces blue")
                    && sent.contains("invoice")
                    && sent.contains("unfinished"),
                "{source} to {destination} lost the correction or open work in its next model input: {sent}"
            );
            let state = self.runtime.block_on(async {
                let lines = saying::opened(url, Some(AGENT), Some(KEY))
                    .await?
                    .map_err(|said| anyhow::anyhow!("ACP refused {}", said.status))?;
                acp::Client
                    .builder()
                    .connect_with(lines, async |cx: acp::ConnectionTo<acp::Agent>| {
                        cx.send_request(acp::schema::v1::InitializeRequest::new(
                            acp::schema::ProtocolVersion::V1,
                        ))
                        .block_task()
                        .await?;
                        let state = cx
                            .send_request(Export {
                                kinds: vec![Kind::Sessions],
                            })
                            .block_task()
                            .await?
                            .state;
                        Ok(state)
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })?;
            let session = state
                .sessions
                .iter()
                .flatten()
                .find(|s| s.session_id == brought.session_id)
                .context("continued session is absent from export")?;
            ensure!(
                session.updates.starts_with(&brought.updates),
                "{source} to {destination} changed the portable archive"
            );
        }
        Ok(())
    }
}
