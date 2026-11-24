//! Agent-owned actions, served on the shared proved bridge protocol.
use anyhow::ensure;
use link_agent::settings::Settings;
use link_agent_contract::{
    action_registry::{self, Caller, Surface},
    namespace as forms,
};
use link_format::WithinExt;
use link_sandbox_client::routes::TOOL_MESSAGE_MAX;
use link_sandbox_route::{Bridge, Call, Route, To, Verdict, Verdicts, http::RequestHead};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    agent: String,
    action: String,
}

fn bridge() -> anyhow::Result<Bridge> {
    Ok(actions(Bridge::of("agent", None)?))
}

/// The agent module's bridge, as its action bridge: `lnk-agent
/// action-bridge` on its own socket. `lnk-agent` never reads its
/// namespace, so the bridge names no group.
fn actions(mut bridge: Bridge) -> Bridge {
    bridge.socket.set_file_name("agent-actions.sock");
    if !bridge.start.is_empty() {
        bridge.start[1] = "action-bridge".into();
        bridge.start[3] = bridge.socket.display().to_string();
    }
    bridge
}
pub(crate) fn routes(agent: &Settings) -> anyhow::Result<Vec<Route>> {
    let bridge = bridge()?;
    let mut routes = Vec::new();
    for (path, action) in link_agent_contract::actions() {
        if !action.allows(Caller::Program, Surface::Route)
            || action.bridge.as_deref() != Some("agent")
        {
            continue;
        }
        let Some((namespace, name)) = path.split_once(' ') else {
            continue;
        };
        let context = matches!(path.as_str(), "agent show" | "memory show" | "tool list");
        if !context {
            let file = crate::namespaces::file(agent, namespace);
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let definition: link_sandbox_client::namespace::Namespace =
                link_files::saved::parse_json(&bytes)?;
            if !definition.actions.contains_key(name) {
                continue;
            }
        }
        let given = serde_json::to_string(&Grant {
            agent: agent.name.clone(),
            action: path.clone(),
        })?;
        routes.push(Route {
            name: format!("agent-action-{}", path.replace(' ', "-")),
            prefix: format!("/_link/{}", path.replace(' ', "/")),
            bridge: bridge.clone(),
            given: given.into(),
            needs_url_secret: true,
            ..Default::default()
        });
    }
    if let Some(route) = routes.first_mut() {
        route
            .env
            .insert("LNK_ACTION_URL".into(), "{proxy}/_link/url/{secret}".into());
    }
    Ok(routes)
}
pub(crate) fn endpoints(
    agent: &Settings,
) -> anyhow::Result<link_sandbox_client::namespace::Endpoints> {
    let mut endpoints = link_sandbox_client::namespace::Endpoints::default();
    let secret = agent.url_secret()?;
    for route in routes(agent)? {
        let path = route
            .prefix
            .strip_prefix("/_link/")
            .within("action prefix")?;
        let (namespace, name) = path.split_once('/').within("action prefix")?;
        endpoints.urls.insert(
            namespace.into(),
            format!(
                "http://127.0.0.1:{}/_link/url/{secret}/{namespace}",
                agent.proxy_port()
            ),
        );
        endpoints.actions.push(format!("{namespace} {name}"));
    }
    // Owner reads load settings without provider credentials. Describing
    // the client endpoint must not rebuild the credential-bearing backend
    // grant; the running proxy authenticates and enforces that grant.
    if agent.model.is_some() && agent.proxy_of(agent.harness()).is_some() {
        endpoints.urls.insert(
            "model".into(),
            link_model_contract::route::actions::url(agent.proxy_port(), &secret, false),
        );
        endpoints.actions.extend(
            link_model_contract::actions()
                .into_iter()
                .filter(|(_, action)| action.allows(Caller::Program, Surface::Route))
                .map(|(path, _)| path),
        );
    }
    Ok(endpoints)
}
pub(crate) async fn serve(started: &link_sandbox_route::bridge::Started) -> anyhow::Result<()> {
    let limits = link_sandbox_binding::Limits {
        name: "the agent action bridge",
        calls: 128,
        per_agent: 16,
        per_owner: 64,
        action_max: 1 << 20,
        action_wait: Duration::from_secs(10),
        answer_wait: Duration::from_secs(10),
    };
    link_sandbox_route::bridge::serve(
        started,
        &limits,
        Duration::from_secs(600),
        |who, call, stream| handle(who.agent, call, stream),
    )
    .await
}
async fn handle(
    identity: String,
    call: Call,
    mut stream: link_sandbox_binding::stream::Stream,
) -> anyhow::Result<()> {
    let grant: Grant = link_sandbox_route::given(&call.given)?;
    let To::Request { head } = call.to else {
        anyhow::bail!("agent actions take requests");
    };
    let request = RequestHead::parse(head.as_bytes()).map_err(anyhow::Error::msg)?;
    let actions = link_agent_contract::actions();
    let (path, action) = action_registry::route(&actions, request.path(), Caller::Program)
        .within("unknown action route")?;
    ensure!(
        path == grant.action && action.bridge.as_deref() == Some("agent"),
        "wrong action grant"
    );
    let limits = action.limits.within("action has no limits")?;
    let answer = tokio::time::timeout(Duration::from_millis(limits.time_ms), async {
        let agent = Settings::load_without_keys(&grant.agent)?;
        ensure!(agent.key() == identity, "caller is another agent");
        let read = matches!(path, "agent show" | "memory show" | "tool list");
        ensure!(
            request.method == if read { "GET" } else { "POST" },
            "wrong action method"
        );
        let length = usize::try_from(request.length().map_err(anyhow::Error::msg)?)?;
        ensure!(length <= TOOL_MESSAGE_MAX, "request too large");
        let mut input = vec![0; length];
        stream.read_exact(&mut input).await?;
        let input = if read {
            ensure!(length == 0, "read actions have no body");
            match request.target.split_once('?') {
                None => json!({}),
                Some((_, "instructions=true")) if path == "agent show" => {
                    json!({"instructions":true})
                }
                _ => anyhow::bail!("unknown query"),
            }
        } else {
            ensure!(request.target == request.path(), "unexpected query");
            serde_json::from_slice(&input)?
        };
        let answer = execute(&agent, path, input).await?;
        let bytes = serde_json::to_vec(&answer)?;
        ensure!(bytes.len() <= limits.bytes, "answer too large");
        Ok::<_, anyhow::Error>(bytes)
    })
    .await;
    match answer {
        Ok(Ok(body)) => {
            stream.say(&Verdict::allowed(grant.action)).await?;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",body.len()).as_bytes()).await?;
            stream.write_all(&body).await?;
            Ok(())
        }
        _ => {
            stream.say(&Verdict::refused(grant.action)).await?;
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            anyhow::bail!("agent action failed or was refused")
        }
    }
}
/// Owner CLI, agent commands and proved routes all reach the same provider and approval check.
pub(crate) async fn execute(agent: &Settings, path: &str, input: Value) -> anyhow::Result<Value> {
    execute_with(agent, path, input, json!({}), Caller::Program).await
}

/// [`execute`], with `meta` laid under the call's own `_meta`: what only
/// the owner's command line says, such as `lnk agent task list --all`
/// (`link.local/all`), which no model's call carries; and who made it,
/// `caller`. Only [`Caller::Owner`], the owner's own command line, skips
/// a provider's argument asks (a recurring schedule's): a route is
/// reached from inside the sandbox, by a harness or what runs in it.
pub(crate) async fn execute_with(
    agent: &Settings,
    path: &str,
    input: Value,
    meta: Value,
    caller: Caller,
) -> anyhow::Result<Value> {
    let context = match path {
        "agent show" => Some("show"),
        "memory show" => Some("memory_show"),
        "tool list" => Some("tool_list"),
        _ => None,
    };
    if let Some(context) = context {
        return crate::context_mcp::context(agent, context, input);
    }
    let (namespace, name) = path.split_once(' ').within("action needs a namespace")?;
    let mut input = input;
    let mut meta = meta;
    // Every provider-specific result is projected into the namespace's declared type.
    match path {
        "web fetch" => {
            let _: link_web_tool_contract::WebToolFetchArgs =
                serde_json::from_value(input.clone())?;
            // The agent's own `[tool.web]` (its part's size and hosts), as
            // its harness sends it with a model's call: never the caller's.
            meta["link.local/spec"] = serde_json::to_value(agent.spec()?.tool("web"))?;
        }
        "web search" => {
            let _: link_web_tool_contract::WebToolSearchArgs =
                serde_json::from_value(input.clone())?;
        }
        "agent send" => {
            let request: forms::Send = serde_json::from_value(input.clone())?;
            meta["link.local/thread"] = json!(request.thread);
            input
                .as_object_mut()
                .within("arguments must be object")?
                .remove("thread");
        }
        "cron create" => {
            let request: forms::Schedule = serde_json::from_value(input.clone())?;
            meta["link.local/thread"] = json!(request.thread);
            input
                .as_object_mut()
                .within("arguments must be object")?
                .remove("thread");
        }
        _ => {}
    }
    thread_given(namespace, path, &mut input, &mut meta)?;
    let answer = invoke_by(
        &crate::namespaces::file(agent, namespace),
        name,
        input,
        meta,
        caller,
    )
    .await?;
    let structured = answer
        .get("structuredContent")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(match path {
        "web fetch" => serde_json::to_value(
            serde_json::from_value::<link_web_tool_contract::Page>(structured)?,
        )?,
        "web search" => serde_json::to_value(serde_json::from_value::<
            link_web_tool_contract::Results,
        >(structured)?)?,
        "agent send" => serde_json::to_value(serde_json::from_value::<forms::Sent>(structured)?)?,
        "cron create" => {
            serde_json::to_value(serde_json::from_value::<forms::Scheduled>(structured)?)?
        }
        _ => answer,
    })
}
pub(crate) async fn invoke(
    file: &Path,
    action: &str,
    input: Value,
    meta: Value,
) -> anyhow::Result<Value> {
    invoke_by(file, action, input, meta, Caller::Program).await
}

/// [`invoke`], for `caller`: the owner's own calls are said to the
/// namespace on its command line (`--by owner`), which nothing in a
/// sandbox can give it.
async fn invoke_by(
    file: &Path,
    action: &str,
    input: Value,
    meta: Value,
    caller: Caller,
) -> anyhow::Result<Value> {
    let program =
        link_core::find_module("sandbox").within("the sandbox module is not installed")?;
    let by = match caller {
        Caller::Owner => "owner",
        Caller::Agent | Caller::Program | Caller::Trigger => "model",
    };
    let mut child = tokio::process::Command::from(link_secretive::process::command(program))
        .arg("mcp-namespace")
        .arg(file)
        .args(["--by", by])
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().within("namespace input")?;
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"link-action","version":env!("CARGO_PKG_VERSION")}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":action,"arguments":input,"_meta":meta}}),
    ] {
        let mut bytes = serde_json::to_vec(&message)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes).await?;
    }
    drop(stdin);
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .within("namespace output")?
        .take(TOOL_MESSAGE_MAX as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(
        bytes.len() <= TOOL_MESSAGE_MAX,
        "namespace answer too large"
    );
    ensure!(child.wait().await?.success(), "namespace failed");
    for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        let message: Value = serde_json::from_slice(line)?;
        if message["id"] == 1 {
            return answer(&message);
        }
    }
    anyhow::bail!("namespace returned no action answer")
}

/// A namespace's refusal of one call, with the reason it gave: the
/// namespace's own words (a protocol error's message, or the text of a
/// tool answer marked an error), which a caller may pass on whole.
#[derive(Debug)]
pub(crate) struct Refused {
    /// Who refused: the namespace itself, or the provider behind it.
    pub(crate) by: &'static str,
    /// Why, as it said it; empty when it gave no reason.
    pub(crate) reason: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} refused action", self.by)?;
        if !self.reason.is_empty() {
            write!(f, ": {}", self.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for Refused {}

/// The result in the call's answer `message`, or its refusal with the
/// reason the namespace gave.
fn answer(message: &Value) -> anyhow::Result<Value> {
    if let Some(error) = message.get("error") {
        return Err(Refused {
            by: "namespace",
            reason: error["message"].as_str().unwrap_or_default().to_string(),
        }
        .into());
    }
    let result = message
        .get("result")
        .within("namespace returned no result")?;
    if result["isError"] == true {
        let reason = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(Refused {
            by: "provider",
            reason,
        }
        .into());
    }
    Ok(result.clone())
}

/// A conversation-scoped namespace's `thread`, as a terminal action
/// names it in its request: taken out of the arguments and given as the
/// call's conversation. Task, cron and the browser, whose page is each
/// conversation's own, are scoped by it.
fn thread_given(
    namespace: &str,
    path: &str,
    input: &mut Value,
    meta: &mut Value,
) -> anyhow::Result<()> {
    if matches!(namespace, "task" | "cron" | "browser")
        && !matches!(path, "cron create")
        && let Some(thread) = input
            .as_object_mut()
            .and_then(|input| input.remove("thread"))
    {
        ensure!(thread.is_string(), "thread must be a string");
        meta[link_connection_contract::access::THREAD_META] = thread;
    }
    Ok(())
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    #[test]
    fn the_action_bridge_starts_as_lnk_agent_reads_it() {
        use clap::Parser;
        let at = Path::new("/run/qa");
        let bridge = actions(Bridge::at(
            at,
            at,
            Path::new("lnk-agent"),
            "agent",
            None,
            at,
        ));
        let parsed = link_agent::cli::Cli::try_parse_from(&bridge.start)
            .unwrap_or_else(|err| panic!("{:?}: {err}", bridge.start));
        let link_agent::cli::Command::AgentActionBridge(given) = parsed.command else {
            panic!("{:?} isn't the action bridge", bridge.start);
        };
        assert_eq!(Path::new(&given.socket), bridge.socket);
        assert!(bridge.socket.ends_with("agent-actions.sock"));
    }

    #[test]
    fn a_terminal_actions_thread_is_its_conversation() {
        for path in ["browser open", "task list", "cron delete"] {
            let namespace = path.split_once(' ').unwrap().0;
            let mut input = json!({"url": "https://example.com", "thread": "link:main"});
            let mut meta = json!({});
            thread_given(namespace, path, &mut input, &mut meta).unwrap();
            assert_eq!(meta["link.local/thread"], "link:main", "{path}");
            assert!(input.get("thread").is_none(), "{path}");
        }
        let mut input = json!({"query": "x", "thread": "link:main"});
        let mut meta = json!({});
        thread_given("web", "web search", &mut input, &mut meta).unwrap();
        assert!(meta.get("link.local/thread").is_none() && input.get("thread").is_some());
        let mut bad = json!({"thread": 7});
        assert!(thread_given("browser", "browser read", &mut bad, &mut json!({})).is_err());
    }

    #[test]
    fn a_refusal_keeps_the_reason_the_namespace_gave() {
        let cap = "Not sent: 9 messages in a chain, past the cap of 8";
        let provider = answer(&json!({"id": 1, "result": {
            "content": [{"type": "text", "text": cap}], "isError": true}}))
        .unwrap_err();
        let refused = provider.downcast_ref::<Refused>().expect("a refusal");
        assert_eq!((refused.by, refused.reason.as_str()), ("provider", cap));
        assert_eq!(
            provider.to_string(),
            format!("provider refused action: {cap}")
        );
        let namespace = answer(&json!({"id": 1, "error": {"code": -32602,
            "message": "you may read only"}}))
        .unwrap_err();
        assert_eq!(
            namespace.to_string(),
            "namespace refused action: you may read only"
        );
        let bare = answer(&json!({"id": 1, "error": {"code": -32602}})).unwrap_err();
        assert_eq!(bare.to_string(), "namespace refused action");
        let ok = json!({"structuredContent": {"sent": []}});
        assert_eq!(answer(&json!({"id": 1, "result": ok})).unwrap(), ok);
    }
}
