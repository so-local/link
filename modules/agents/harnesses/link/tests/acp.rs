//! Link Harness as an ACP agent, end to end in process: a client of the
//! protocol's own SDK opens a WebSocket with the agent's key, starts a
//! session on a chat's thread, prompts it, and is streamed the answer,
//! told the commands, and replayed the conversation.

use crate::common;

use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use link_agent_state_protocol::acp::schema::ProtocolVersion;
use link_agent_state_protocol::acp::schema::v1::{
    ContentBlock, InitializeRequest, ListSessionsRequest, LoadSessionRequest, NewSessionRequest,
    PromptRequest, SessionId, SessionNotification, StopReason, TextContent,
};
use link_agent_state_protocol::acp::{self, ConnectionTo, Lines};
use link_connection_contract::acp::{Meta, Said};
use link_harness::{
    agent::{Agent, Agents},
    config::Config,
    server::{self, Server},
    store::Files,
    turn::Harness,
};
use link_harness_contract::harness::Model;
use link_test_kit::TempDir;
use serde_json::json;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

/// An agent on a fake model, which answers "Noted": its port.
async fn world(root: &TempDir) -> u16 {
    world_pausing(root, std::time::Duration::ZERO).await
}

/// [`world`], the model's answer after its first piece `pause` later.
async fn world_pausing(root: &TempDir, pause: std::time::Duration) -> u16 {
    let (port, _) = common::fake_model_with(axum::Router::new(), pause, |_, _| {
        vec![
            common::delta(json!({"content": "Not"})),
            common::delta(json!({"content": "ed"})),
        ]
    })
    .await;
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Files::new(agents.clone());
    link_harness::store::Store::set_config(
        &store,
        &agent,
        &Config {
            model: Model {
                provider: "openai".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                ..Model::default()
            },
            endpoint_key: Some("the-key".to_string().into()),
            ..Config::default()
        },
    )
    .unwrap();
    let server = Arc::new(Server::new(
        Harness::new(agents, Arc::new(store)).unwrap(),
        agent,
    ));
    common::serve(server::router(server)).await
}

/// The WebSocket to `port`'s ACP, opened with `key`.
async fn open(
    port: u16,
    key: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Error,
> {
    let address = format!(
        "ws://127.0.0.1:{port}{}",
        link_connection_contract::acp::ACP_PATH
    );
    let mut request = address.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {key}").parse().unwrap());
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

/// ACP's lines over the WebSocket to `port`, opened with the agent's key.
async fn lines(port: u16) -> impl acp::ConnectTo<acp::Client> {
    let socket = open(port, "the-key").await.expect("the key opens it");
    let (sink, stream) = socket.split();
    let outgoing = sink
        .sink_map_err(std::io::Error::other)
        .with(|line: String| async move {
            Ok::<_, std::io::Error>(tungstenite::Message::Text(line.into()))
        });
    let incoming = stream.filter_map(|frame| async move {
        match frame {
            Ok(tungstenite::Message::Text(text)) => Some(Ok(text.to_string())),
            Ok(_) => None,
            Err(err) => Some(Err(std::io::Error::other(err))),
        }
    });
    Lines::new(outgoing, incoming)
}

fn text(update: &serde_json::Value) -> Option<&str> {
    update["content"]["text"].as_str()
}

#[tokio::test]
async fn a_client_prompts_a_session_and_is_streamed_its_answer() {
    let root = TempDir::new("acp-prompt");
    let port = world(&root).await;
    let lines = lines(port).await;
    let updates: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let seen = updates.clone();
    acp::Client
        .builder()
        .on_receive_notification(
            async move |said: SessionNotification, _cx| {
                assert_eq!(said.session_id.0.as_ref(), "telegram:42");
                seen.lock()
                    .unwrap()
                    .push(serde_json::to_value(&said.update).unwrap());
                Ok(())
            },
            acp::on_receive_notification!(),
        )
        .connect_with(lines, async |cx: ConnectionTo<acp::Agent>| {
            let up = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            assert!(up.agent_capabilities.load_session);
            assert!(up.agent_capabilities.prompt_capabilities.image);

            // A prompt on a session not opened here is refused.
            let unopened = PromptRequest::new(
                SessionId::new("telegram:42"),
                vec![ContentBlock::Text(TextContent::new("hello"))],
            );
            assert!(
                cx.send_request(unopened.clone())
                    .block_task()
                    .await
                    .is_err()
            );

            let on_chat = Said {
                thread: Some("telegram:42".into()),
                ..Said::default()
            };
            let new =
                NewSessionRequest::new(root.join("files")).meta(on_chat.into_meta(Meta::new()));
            let session = cx.send_request(new).block_task().await?.session_id;
            assert_eq!(session.0.as_ref(), "telegram:42");

            let ended = cx.send_request(unopened).block_task().await?;
            assert_eq!(ended.stop_reason, StopReason::EndTurn);
            {
                let updates = updates.lock().unwrap();
                let kinds: Vec<&str> = updates
                    .iter()
                    .map(|u| u["sessionUpdate"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    kinds,
                    [
                        "available_commands_update",
                        "agent_message_chunk",
                        "agent_message_chunk"
                    ]
                );
                let commands = updates[0]["availableCommands"].to_string();
                assert!(commands.contains("\"chat:create\"") && commands.contains("\"model:set\""));
                assert_eq!(text(&updates[1]), Some("Not"));
            }

            // A command is the owner's text, answered without the model.
            let help = PromptRequest::new(
                session.clone(),
                vec![ContentBlock::Text(TextContent::new("/help"))],
            );
            cx.send_request(help).block_task().await?;
            assert!(
                text(updates.lock().unwrap().last().unwrap())
                    .is_some_and(|t| t.starts_with("Commands:"))
            );

            // Listed, and replayed as it streamed.
            let listed = cx
                .send_request(ListSessionsRequest::new())
                .block_task()
                .await?;
            assert!(listed.sessions.iter().any(|s| s.session_id == session));
            updates.lock().unwrap().clear();
            cx.send_request(LoadSessionRequest::new(session, root.join("files")))
                .block_task()
                .await?;
            let replayed: Vec<(String, Option<String>)> = updates
                .lock()
                .unwrap()
                .iter()
                .map(|u| {
                    (
                        u["sessionUpdate"].as_str().unwrap().to_string(),
                        text(u).map(str::to_string),
                    )
                })
                .collect();
            assert_eq!(
                replayed[..3],
                [
                    ("user_message_chunk".to_string(), Some("hello".to_string())),
                    ("agent_message_chunk".to_string(), Some("Noted".to_string())),
                    ("user_message_chunk".to_string(), Some("/help".to_string())),
                ]
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_connection_without_the_agents_key_is_refused() {
    let root = TempDir::new("acp-key");
    let port = world(&root).await;
    match open(port, "not-the-key").await {
        Err(tungstenite::Error::Http(response)) => assert_eq!(response.status(), 401),
        other => panic!("opened without the key: {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_cancel_ends_the_turn_and_its_prompt_says_so() {
    let root = TempDir::new("acp-cancel");
    let port = world_pausing(&root, std::time::Duration::from_secs(30)).await;
    acp::Client
        .builder()
        .connect_with(lines(port).await, async |cx: ConnectionTo<acp::Agent>| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = cx
                .send_request(NewSessionRequest::new(root.join("files")))
                .block_task()
                .await?
                .session_id;
            let prompt = PromptRequest::new(
                session.clone(),
                vec![ContentBlock::Text(TextContent::new("take your time"))],
            );
            let asked = cx.send_request(prompt);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            cx.send_notification(
                link_agent_state_protocol::acp::schema::v1::CancelNotification::new(session),
            )?;
            let ended =
                tokio::time::timeout(std::time::Duration::from_secs(10), asked.block_task())
                    .await
                    .expect("the turn ended well before the model's answer")?;
            assert_eq!(ended.stop_reason, StopReason::Cancelled);
            Ok(())
        })
        .await
        .unwrap();
}

/// ASP's agent state: its conversations given whole, and taken in as
/// conversations of their own, each replayed as it came.
#[tokio::test]
async fn its_conversations_are_exported_and_imported_over_asp() {
    use link_agent_state_protocol::acp::schema::v1::SessionUpdate;
    use link_agent_state_protocol::forms::By;
    use link_agent_state_protocol::methods::{Export, Import};
    let root = TempDir::new("acp-state");
    let port = world(&root).await;
    common::prompt(
        &common::acp(port),
        "the-key",
        "telegram:42",
        vec![ContentBlock::Text(TextContent::new("hello"))],
    )
    .await
    .unwrap();
    acp::Client
        .builder()
        .connect_with(lines(port).await, async |cx: ConnectionTo<acp::Agent>| {
            let up = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let kinds = link_agent_state_protocol::AgentState::of(&up.agent_capabilities).unwrap();
            assert_eq!(kinds.import, [link_agent_state_protocol::Kind::Sessions]);
            let asked = Export {
                kinds: vec![link_agent_state_protocol::Kind::Sessions],
            };
            let exported = cx.send_request(asked).block_task().await?.state;
            let mut sessions = exported.sessions.unwrap();
            assert_eq!(sessions.len(), 1);
            assert!(sessions[0].native_evidence()?.is_some());
            let said: Vec<(By, String)> = sessions[0]
                .messages()
                .into_iter()
                .map(|m| (m.by, m.text))
                .collect();
            assert_eq!(
                said,
                [(By::User, "hello".into()), (By::Agent, "Noted".into())]
            );

            // A copy that went on elsewhere goes on in its thread; another
            // under a name nothing holds is a conversation of its own.
            let mut other = sessions[0].clone();
            other.session_id = SessionId::new("slack:C1");
            let more: Vec<SessionUpdate> = serde_json::from_value(serde_json::json!([
                {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "again"}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Noted again"}}
            ]))
            .unwrap();
            sessions[0].updates.extend(more);
            let parted = sessions[0].clone();
            sessions.push(other);
            let state = link_agent_state_protocol::State {
                sessions: Some(sessions),
                instructions: Some("# Agent\n".into()),
                tools: Some(serde_json::from_value(serde_json::json!([
                    {"name": "gmail", "command": "gmail-mcp", "args": [], "env": []}
                ]))
                .unwrap()),
                ..link_agent_state_protocol::State::default()
            };
            let imported = cx.send_request(Import { state }).block_task().await?;
            assert_eq!(imported.taken, [link_agent_state_protocol::Kind::Sessions]);
            assert_eq!(imported.not_taken[0].kind, link_agent_state_protocol::Kind::Instructions);
            // What stays behind is named, each server its owner may add.
            let tools = &imported.not_taken[1];
            assert_eq!(tools.kind, link_agent_state_protocol::Kind::Tools);
            assert!(tools.reason.contains("MCP server gmail"), "{}", tools.reason);
            for name in ["telegram:42", "slack:C1"] {
                assert_eq!(imported.sessions[&SessionId::new(name)], SessionId::new(name));
            }
            // One that parted from its thread comes in beside it, named for
            // the harness it came from.
            let mut parted = parted;
            parted.updates.drain(..2);
            parted.harness = Some("hermes".into());
            let state = link_agent_state_protocol::State {
                sessions: Some(vec![parted]),
                ..link_agent_state_protocol::State::default()
            };
            let imported = cx.send_request(Import { state }).block_task().await?;
            assert_eq!(
                imported.sessions[&SessionId::new("telegram:42")],
                SessionId::new("telegram:42~hermes")
            );
            let listed = cx
                .send_request(ListSessionsRequest::new())
                .block_task()
                .await?;
            assert_eq!(listed.sessions.len(), 3);
            let asked = Export {
                kinds: vec![link_agent_state_protocol::Kind::Sessions],
            };
            let again = cx.send_request(asked).block_task().await?.state.sessions.unwrap();
            let texts = |name: &str| -> Vec<String> {
                let session = again.iter().find(|s| &*s.session_id.0 == name).unwrap();
                session.messages().into_iter().map(|m| m.text).collect()
            };
            assert_eq!(texts("telegram:42"), ["hello", "Noted", "again", "Noted again"]);
            let slack = again.iter().find(|s| &*s.session_id.0 == "slack:C1").unwrap();
            let evidence = slack.native_evidence()?.unwrap();
            assert!(evidence.events.is_empty(), "an imported manifest is not local execution");
            assert!(!evidence.imported.is_empty());
            let replayed: Vec<String> = slack.messages().into_iter().map(|m| m.text).collect();
            assert_eq!(replayed, ["hello", "Noted"]);
            Ok(())
        })
        .await
        .unwrap();
}
