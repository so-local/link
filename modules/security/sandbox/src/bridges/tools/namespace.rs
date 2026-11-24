//! One MCP server over a host-defined group of existing servers. Every
//! call retains the original server's confinement and approval policy.

use super::{
    MAX_MESSAGE, load, route,
    session::{Session, Started},
};
use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::namespace::Namespace;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

mod schema;
use schema::{check_arguments, one_shape, selected};

struct Peer {
    session: Arc<Session>,
    offered: BTreeMap<String, Value>,
    /// What its server tells a model of its tools (its `initialize`
    /// answer's `instructions`), kept for the namespace's own.
    instructions: Option<String>,
}

struct Server {
    namespace: Namespace,
    tools: route::Tools,
    /// The providers open now, by name.
    peers: tokio::sync::RwLock<BTreeMap<String, Arc<Peer>>>,
    opening: tokio::sync::Mutex<()>,
}

impl Server {
    async fn peer(&self, name: &str) -> anyhow::Result<Peer> {
        let tool = load(&self.namespace.dir, name)?;
        let initialize = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"link-namespace","version":env!("CARGO_PKG_VERSION")}}});
        let (session, answer) = route::start(
            &link_files::layout::Root::Runtime.try_path()?,
            &self.namespace.dir,
            &tool,
            &initialize,
        )
        .await
        .map_err(|(_, why)| anyhow::anyhow!(why))?;
        ensure!(
            answer.get("error").is_none(),
            "provider refused initialization"
        );
        let instructions = answer
            .pointer("/result/instructions")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        session
            .send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        let mut offered = BTreeMap::new();
        let mut cursor = None;
        let mut seen = std::collections::BTreeSet::new();
        for page in 0..16 {
            let mut message = json!({"jsonrpc":"2.0","id":page+1,"method":"tools/list"});
            if let Some(cursor) = &cursor {
                message["params"] = json!({"cursor":cursor});
            }
            let listed = session.request(&message).await?;
            ensure!(listed.get("error").is_none(), "provider refused discovery");
            session.keep_hints(&listed);
            for tool in listed
                .pointer("/result/tools")
                .and_then(Value::as_array)
                .within("provider returned no tool list")?
            {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .within("a tool without a name")?;
                ensure!(
                    offered.insert(name.to_string(), tool.clone()).is_none(),
                    "duplicate provider tool"
                );
                ensure!(offered.len() <= 128, "provider listed too many tools");
            }
            cursor = listed
                .pointer("/result/nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            match &cursor {
                None => {
                    return Ok(Peer {
                        session,
                        offered,
                        instructions,
                    });
                }
                Some(cursor) => ensure!(
                    seen.insert(cursor.clone()),
                    "provider repeated discovery cursor"
                ),
            }
        }
        anyhow::bail!("provider discovery exceeded its page budget")
    }
    /// The providers open now, each provider in `wanted` not open yet
    /// opened first, 20 seconds each at most: one that didn't answer is
    /// tried again at the next listing or call that wants it. Each still
    /// unavailable is returned with why, and said on stderr.
    async fn peers(
        &self,
        wanted: std::collections::BTreeSet<String>,
    ) -> (BTreeMap<String, Arc<Peer>>, BTreeMap<String, String>) {
        // Nothing to open: no wait behind another listing's retry.
        {
            let open = self.peers.read().await;
            if wanted.iter().all(|name| open.contains_key(name)) {
                return (open.clone(), BTreeMap::new());
            }
        }
        // One opening at a time, so two listings never start a provider twice.
        let _opening = self.opening.lock().await;
        let missing: Vec<String> = {
            let open = self.peers.read().await;
            wanted
                .into_iter()
                .filter(|name| !open.contains_key(name))
                .collect()
        };
        let deadline =
            Duration::from_millis(self.namespace.timeout_ms).min(Duration::from_secs(20));
        let opened = futures_util::future::join_all(missing.into_iter().map(|name| async move {
            let peer = match tokio::time::timeout(deadline, self.peer(&name)).await {
                Ok(Ok(peer)) => Ok(peer),
                Ok(Err(err)) => Err(format!("{err:#}")),
                Err(_) => Err(format!(
                    "it didn't answer in {} seconds",
                    deadline.as_secs()
                )),
            };
            (name, peer)
        }))
        .await;
        let mut unavailable = BTreeMap::new();
        let mut open = self.peers.write().await;
        for (name, peer) in opened {
            match peer {
                Ok(peer) => {
                    open.insert(name, Arc::new(peer));
                }
                Err(why) => {
                    link_format::aside(
                        link_format::Report::warning(link_format::sentence!(
                            "The namespace provider {} is unavailable.",
                            link_format::Name::new("provider", &name)
                        ))
                        .why(link_format::prose!(
                            "{}. It's tried again at the next listing or call.",
                            link_format::Foreign::new(&why)
                        )),
                    );
                    unavailable.insert(name, why);
                }
            }
        }
        (open.clone(), unavailable)
    }

    /// Every provider the namespace's actions reach.
    fn providers(&self) -> std::collections::BTreeSet<String> {
        self.namespace
            .actions
            .values()
            .flat_map(|action| action.targets.values().map(|target| target.server.clone()))
            .collect()
    }

    /// The open providers' sessions, for a notice every one gets.
    async fn sessions(&self) -> Vec<Arc<Session>> {
        let open = self.peers.read().await;
        open.values().map(|peer| peer.session.clone()).collect()
    }

    async fn listed(&self) -> anyhow::Result<Value> {
        let (peers, _) = self.peers(self.providers()).await;
        let mut tools = Vec::new();
        // An unlisted action is offered to no one: only its name reaches it.
        for (name, action) in self.namespace.actions.iter().filter(|(_, a)| !a.unlisted) {
            let mut variants = Vec::new();
            let mut description = Vec::new();
            // It only reads when every original it reaches says so.
            let mut reads = true;
            for (kind, target) in &action.targets {
                let Some(original) = peers
                    .get(&target.server)
                    .and_then(|peer| peer.offered.get(&target.tool))
                else {
                    continue;
                };
                let mut schema = original
                    .get("inputSchema")
                    .cloned()
                    .within("a tool without its input schema")?;
                if action.select || action.targets.len() > 1 {
                    schema["properties"]
                        .as_object_mut()
                        .within("an input schema without properties")?
                        .insert("kind".into(), json!({"type": "string", "enum": [kind]}));
                    if !action.broadcast && action.targets.len() > 1 {
                        let required = schema
                            .as_object_mut()
                            .within("an input schema object")?
                            .entry("required")
                            .or_insert_with(|| json!([]));
                        required
                            .as_array_mut()
                            .within("a required array")?
                            .push(json!("kind"));
                    }
                }
                reads &= original.pointer("/annotations/readOnlyHint") == Some(&json!(true));
                variants.push(schema);
                description.push(format!(
                    "{kind}: {}",
                    original
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or(&target.tool)
                ));
            }
            if variants.is_empty() {
                continue;
            }
            let schema = if variants.len() == 1 {
                variants.remove(0)
            } else {
                one_shape(&variants)
            };
            let mut tool =
                json!({"name": name, "description": description.join("\n"), "inputSchema": schema});
            if reads {
                tool["annotations"] = json!({"readOnlyHint": true});
            }
            tools.push(tool);
        }
        Ok(json!({"tools": tools}))
    }

    async fn called(&self, message: &Value) -> anyhow::Result<Value> {
        let name = message
            .pointer("/params/name")
            .and_then(Value::as_str)
            .within("a call needs its action name")?;
        let action = self
            .namespace
            .actions
            .get(name)
            .within("unknown namespace action")?;
        let mut arguments = message
            .pointer("/params/arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let arguments = arguments
            .as_object_mut()
            .within("arguments must be an object")?;
        let kind = if action.select || action.targets.len() > 1 {
            arguments
                .remove("kind")
                .map(|kind| {
                    kind.as_str()
                        .map(str::to_string)
                        .within("kind must be a string")
                })
                .transpose()?
        } else {
            None
        };
        let targets = selected(action, kind.as_deref())?;
        let (peers, unavailable) = self
            .peers(targets.iter().map(|(_, t)| t.server.clone()).collect())
            .await;
        let mut results = Vec::new();
        for (kind, target) in targets {
            let Some(peer) = peers.get(&target.server) else {
                anyhow::bail!(
                    "the namespace provider {} is unavailable: {}",
                    target.server,
                    unavailable
                        .get(&target.server)
                        .map_or("it isn't open", String::as_str)
                );
            };
            let tool = load(&self.namespace.dir, &target.server)?;
            ensure!(
                peer.session.started == Started::of(&tool),
                "{} changed: reconnect the namespace",
                tool.name
            );
            ensure!(
                peer.offered.contains_key(&target.tool),
                "the server does not offer this action"
            );
            check_arguments(
                &json!(arguments),
                &peer.offered[&target.tool]["inputSchema"],
            )?;
            let mut forwarded = message.clone();
            forwarded["params"]["name"] = json!(target.tool);
            forwarded["params"]["arguments"] = json!(arguments);
            let reviewed = route::gate(&self.tools, &tool, &peer.session, &target.tool, &forwarded)
                .await
                .map_err(anyhow::Error::msg)?;
            let store = crate::bridges::asks::store(&self.tools.dir)?;
            let fence = if reviewed.is_some()
                || forwarded["params"]["_meta"]["link.local/work"].is_string()
            {
                Some(tokio::task::spawn_blocking(move || store.dispatch_fence()).await??)
            } else {
                None
            };
            route::work_of(&self.tools, &forwarded).map_err(anyhow::Error::msg)?;
            let policy_dir = self.tools.dir.clone();
            let policy =
                tokio::task::spawn_blocking(move || super::policy_fence(&policy_dir)).await??;
            let current = load(&self.namespace.dir, &target.server)?;
            ensure!(
                peer.session.started == Started::of(&current)
                    && (reviewed.is_some()
                        || !current.asks_given_by(
                            &target.tool,
                            &forwarded["params"]["arguments"],
                            self.tools.by,
                        )),
                "provider or approval policy changed before dispatch"
            );
            let application =
                route::application(&self.tools, &target.server, &forwarded, reviewed)?;
            let mut fences: Vec<_> = fence.into_iter().collect();
            fences.push(policy);
            let answer = peer
                .session
                .observed_request(&forwarded, fences, application)
                .await?;
            ensure!(
                answer.get("error").is_none(),
                "{} refused the action: {}",
                tool.name,
                answer["error"]
            );
            results.push((kind, answer["result"].clone()));
        }
        if results.len() == 1 {
            return Ok(results.remove(0).1);
        }
        let mut content = Vec::new();
        let mut combined = serde_json::Map::new();
        let mut failed = false;
        for (kind, result) in results {
            failed |= result["isError"].as_bool().unwrap_or(false);
            combined.insert(kind.clone(), result.clone());
            content.push(json!({"type": "text", "text": format!("{kind}:\n{}", serde_json::to_string(&result)?)}));
        }
        Ok(json!({"content": content, "structuredContent":{"results":combined}, "isError": failed}))
    }

    async fn answer(&self, message: &Value) -> anyhow::Result<Value> {
        let result = match message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "initialize" => {
                let (peers, _) = self.peers(self.providers()).await;
                let mut result = json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                    "serverInfo": {"name": self.namespace.name, "version": env!("CARGO_PKG_VERSION")}});
                if let Some(instructions) = instructions(&self.namespace, &peers) {
                    result["instructions"] = json!(instructions);
                }
                result
            }
            "tools/list" => self.listed().await?,
            "tools/call" => self.called(message).await?,
            "ping" => json!({}),
            _ => anyhow::bail!("unsupported namespace method"),
        };
        Ok(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
    }
}

/// Each provider's instructions, under the kinds that reach it: a
/// provider's guidance names its own tools, which the namespace serves
/// under its actions. None when no provider gives any.
fn instructions(namespace: &Namespace, peers: &BTreeMap<String, Arc<Peer>>) -> Option<String> {
    let mut kinds: BTreeMap<&str, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for action in namespace.actions.values() {
        for (kind, target) in &action.targets {
            kinds.entry(&target.server).or_default().insert(kind);
        }
    }
    let parts: Vec<String> = peers
        .iter()
        .filter_map(|(server, peer)| {
            let text = peer.instructions.as_deref()?;
            let kinds: Vec<String> = kinds
                .get(server.as_str())
                .into_iter()
                .flatten()
                .map(|kind| format!("{kind:?}"))
                .collect();
            Some(format!(
                "With kind {}, from {server}, whose tools are this server's actions:\n{text}",
                kinds.join(" or ")
            ))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// Serve the namespace described by a bounded, private host file. It
/// never accepts member names or server addresses in an MCP argument.
/// `by` says who makes its calls: the owner's command line, run outside
/// every sandbox, says it's theirs; what a sandboxed program reaches never does.
pub async fn serve(file: &Path, by: super::By) -> anyhow::Result<()> {
    use std::io::Read;
    let mut input = std::fs::File::open(file)?.take((MAX_MESSAGE + 1) as u64);
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_MESSAGE,
        "namespace definition is too large"
    );
    let namespace: Namespace = link_files::saved::parse_json(&bytes)?;
    namespace.check()?;
    crate::settings::read_from(&namespace.dir);
    let named = namespace
        .actions
        .values()
        .flat_map(|a| a.targets.values().map(|t| t.server.clone()))
        .collect();
    let mut tools = route::Tools::new(&namespace.name, namespace.dir.clone(), named);
    tools.asking = namespace.asking.clone();
    tools.by = by;
    tools.desktop = link_sandbox_route::desktop::desktop();
    let server = Arc::new(Server {
        namespace,
        tools,
        peers: Default::default(),
        opening: Default::default(),
    });
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let output = Arc::new(tokio::sync::Mutex::new(tokio::fs::File::from_std(
        link_format::streams::Protocol::file()?,
    )));
    let mut calls = tokio::task::JoinSet::new();
    let room = Arc::new(tokio::sync::Semaphore::new(128));
    while let Some(line) = lines.next_line().await? {
        ensure!(line.len() <= MAX_MESSAGE, "namespace message is too large");
        let message: Value = serde_json::from_str(&line)?;
        if message["method"] == "notifications/cancelled" {
            for session in server.sessions().await {
                session.send(&message).await?;
            }
            continue;
        }
        if message.get("id").is_none() {
            continue;
        }
        let (server, output) = (server.clone(), output.clone());
        let permit = room.clone().acquire_owned().await?;
        calls.spawn(async move {
            let _permit = permit;
            let deadline = Duration::from_millis(server.namespace.timeout_ms);
            let answer = tokio::time::timeout(deadline, server.answer(&message)).await;
            let answer = match answer {
                Ok(Ok(answer)) => answer,
                failed => {
                    for session in server.sessions().await { session.forget(&message["id"]); }
                    let why = match failed { Ok(Err(err)) => format!("{err:#}"), _ => "namespace call timed out; its outcome may be uncertain".into() };
                    json!({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32603, "message": why}})
                }
            };
            let bytes = serde_json::to_vec(&answer)?;
            ensure!(bytes.len() <= MAX_MESSAGE, "namespace answer is too large");
            let mut output = output.lock().await;
            output.write_all(&bytes).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
            Ok::<_, anyhow::Error>(())
        });
        while let Some(done) = calls.try_join_next() {
            done??;
        }
    }
    while let Some(done) = calls.join_next().await {
        done??;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_sandbox_client::namespace::{Action, Target};
    #[test]
    fn question_schema_preserves_nested_validation_at_the_namespace_boundary() {
        let schema = serde_json::to_value(link_interactions::QuestionProposal::schema()).unwrap();
        assert!(!schema.to_string().contains("$ref"));
        let input = json!({"title":"Report", "questions":[{"id":"format", "prompt":"Format?",
            "answer_shape":{"kind":"choice","options":[{"id":"brief","label":"Brief","recommended":true}],
                "min_selected":1,"max_selected":1,"allow_other":true}}]});
        check_arguments(&input, &schema).unwrap();
        assert!(
            serde_json::from_value::<link_interactions::QuestionProposal>(input.clone()).is_ok()
        );
        let mut invalid = input;
        invalid["owner"] = json!("another-owner");
        assert!(check_arguments(&invalid, &schema).is_err());
    }
    #[test]
    fn several_providers_offer_one_object_schema() {
        let variants = [
            json!({"type": "object", "properties": {"id": {"type": "string", "description": "Its id."}, "kind": {"type": "string", "enum": ["run"]}}, "required": ["id", "kind"]}),
            json!({"type": "object", "properties": {"id": {"type": "string"}, "kind": {"type": "string", "enum": ["schedule"]}}, "required": ["id", "kind"]}),
            json!({"type": "object", "properties": {"kind": {"type": "string", "enum": ["other"]}}}),
        ];
        let schema = one_shape(&variants);
        for union in ["anyOf", "oneOf", "allOf"] {
            assert!(schema.get(union).is_none(), "{schema}");
        }
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["kind"]["enum"],
            json!(["run", "schedule", "other"])
        );
        assert_eq!(schema["properties"]["id"]["description"], "Its id.");
        assert_eq!(schema["required"], json!([]));
        assert_eq!(one_shape(&variants[..2])["required"], json!(["id", "kind"]));
    }

    #[test]
    fn a_mutation_never_broadcasts_without_an_explicit_kind() {
        let action = Action {
            select: true,
            broadcast: false,
            unlisted: false,
            targets: [
                (
                    "run".into(),
                    Target {
                        server: "run-one".into(),
                        tool: "stop".into(),
                    },
                ),
                (
                    "schedule".into(),
                    Target {
                        server: "scheduler-one".into(),
                        tool: "cancel".into(),
                    },
                ),
            ]
            .into(),
        };
        assert!(selected(&action, None).is_err());
        assert!(selected(&action, Some("other")).is_err());
        assert_eq!(
            selected(&action, Some("schedule")).unwrap()[0].1.tool,
            "cancel"
        );
        assert_eq!(
            selected(
                &Action {
                    broadcast: true,
                    unlisted: false,
                    ..action
                },
                None
            )
            .unwrap()
            .len(),
            2
        );
    }
}

#[cfg(test)]
mod server_tests {
    use super::super::{Ask, Tool, save};
    use super::*;
    use link_sandbox_client::namespace::{Action, Target};

    #[tokio::test]
    async fn keepalive_answers_without_starting_providers_or_dispatching_actions() {
        let (_root, dir, server) = fixture();
        for id in [1, 2, 3] {
            let answer = server
                .answer(&json!({"jsonrpc":"2.0","id":id,"method":"ping"}))
                .await
                .unwrap();
            assert_eq!(answer, json!({"jsonrpc":"2.0","id":id,"result":{}}));
            assert!(server.peers.read().await.is_empty());
        }
        assert!(!dir.join("run-one").exists());
        assert!(!dir.join("schedule-one").exists());
        assert!(
            server
                .answer(&json!({"id":4,"method":"unknown"}))
                .await
                .is_err()
        );
    }

    /// Two servers of a sandbox whose sandboxes folder is in the test's
    /// own installation folder: its questions are kept in that folder's
    /// interaction store, in the test's too.
    fn fixture() -> (link_test_kit::TempDir, std::path::PathBuf, Server) {
        let root = link_test_kit::TempDir::new("namespace-servers");
        let dir = link_files::layout::Hidden::Sandboxes.at(&root);
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("server");
        let script = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\)[,}].*/\1/p')
  [ -n "$id" ] || continue
  case "$line" in
    *'"method":"initialize"'*) result='{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}'"${INSTRUCTIONS:+,\"instructions\":\"$INSTRUCTIONS\"}"'}' ;;
    *'"method":"tools/list"'*) result='{"tools":[{"name":"list","description":"List","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}},{"name":"stop","description":"Stop","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}]}' ;;
    *) printf 'called\n' >> "$MARKER"; result='{"content":[{"type":"text","text":"called"}]}' ;;
  esac
  printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$id" "$result"
done
"#;
        link_test_kit::write_program(&program, script).unwrap();
        for name in ["run-one", "schedule-one"] {
            save(
                &dir,
                &Tool {
                    name: name.into(),
                    argv: vec![program.display().to_string()],
                    env: [
                        ("PATH".into(), "/usr/bin:/bin".to_string().into()),
                        ("MARKER".into(), dir.join(name).display().to_string().into()),
                        (
                            "INSTRUCTIONS".into(),
                            match name {
                                "run-one" => "Call list before stop.",
                                _ => "",
                            }
                            .to_string()
                            .into(),
                        ),
                    ]
                    .into(),
                    ask: Ask::None,
                    allow: Vec::new(),
                    ask_if: Vec::new(),
                    ask_if_hint: None,
                    shared: false,
                },
            )
            .unwrap();
        }
        let targets = |native: &str| {
            [
                (
                    "run".into(),
                    Target {
                        server: "run-one".into(),
                        tool: native.into(),
                    },
                ),
                (
                    "schedule".into(),
                    Target {
                        server: "schedule-one".into(),
                        tool: native.into(),
                    },
                ),
            ]
            .into()
        };
        let namespace = Namespace {
            asking: None,
            name: "task".into(),
            dir: dir.clone(),
            timeout_ms: 5000,
            actions: [
                (
                    "list".into(),
                    Action {
                        select: false,
                        targets: targets("list"),
                        broadcast: true,
                        unlisted: false,
                    },
                ),
                (
                    "stop".into(),
                    Action {
                        select: false,
                        targets: targets("stop"),
                        broadcast: false,
                        unlisted: false,
                    },
                ),
            ]
            .into(),
        };
        let tools = route::Tools::new(
            "task",
            dir.clone(),
            vec!["run-one".into(), "schedule-one".into()],
        );
        (
            root,
            dir,
            Server {
                namespace,
                tools,
                peers: Default::default(),
                opening: Default::default(),
            },
        )
    }

    /// What each provider tells a model reaches it through the
    /// namespace's own `initialize`, under the kinds that reach it.
    #[tokio::test]
    async fn the_namespace_keeps_its_providers_instructions() {
        let (_root, _dir, server) = fixture();
        let answer = server
            .answer(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
            .await
            .unwrap();
        assert_eq!(
            answer["result"]["instructions"],
            "With kind \"run\", from run-one, whose tools are this server's actions:\nCall list before stop."
        );
    }

    #[tokio::test]
    async fn lists_combine_but_stop_selects_one_original_server() {
        let (_root, dir, server) = fixture();
        let listed = server.listed().await.unwrap();
        assert_eq!(listed["tools"].as_array().unwrap().len(), 2);
        assert_eq!(listed["tools"][1]["name"], "stop");
        // A list only reads, as every original says; a stop never says so.
        assert_eq!(listed["tools"][0]["annotations"]["readOnlyHint"], true);
        assert!(listed["tools"][1].get("annotations").is_none());
        let mut call = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "stop", "arguments": {"id": "one"}}});
        assert!(server.called(&call).await.is_err());
        assert!(!dir.join("run-one").exists());
        assert!(!dir.join("schedule-one").exists());
        call["params"]["arguments"]["kind"] = json!("schedule");
        let answer = server.called(&call).await.unwrap();
        assert_eq!(answer["content"][0]["text"], "called");
        assert!(!dir.join("run-one").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("schedule-one")).unwrap(),
            "called\n"
        );
    }

    #[tokio::test]
    async fn a_namespace_cannot_bypass_a_changed_provider_approval_policy() {
        let (_root, dir, server) = fixture();
        server.listed().await.unwrap();
        let mut tool = load(&dir, "run-one").unwrap();
        tool.ask = Ask::All;
        save(&dir, &tool).unwrap();
        let call = json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "stop", "arguments": {"kind": "run", "id": "one"}}});
        assert!(server.called(&call).await.is_err());
        assert!(!dir.join("run-one").exists());
    }
    #[tokio::test]
    async fn a_single_provider_keeps_the_selector_and_rejects_extra_arguments() {
        let (_root, dir, mut server) = fixture();
        for action in server.namespace.actions.values_mut() {
            action.select = true;
            action.targets.retain(|kind, _| kind == "schedule");
        }
        let listed = server.listed().await.unwrap();
        assert_eq!(
            listed["tools"][1]["inputSchema"]["properties"]["kind"]["enum"],
            json!(["schedule"])
        );
        let mut call = json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"stop","arguments":{"kind":"schedule","id":"one","server":"run-one"}}});
        assert!(server.called(&call).await.is_err());
        assert!(!dir.join("schedule-one").exists());
        call["params"]["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("server");
        server.called(&call).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("schedule-one")).unwrap(),
            "called\n"
        );
    }
    /// A provider not there at the first listing is tried again at the
    /// next, and a call meanwhile says which provider is missing and why.
    #[tokio::test]
    async fn an_unavailable_provider_is_tried_again_and_named() {
        let (_root, dir, mut server) = fixture();
        for action in server.namespace.actions.values_mut() {
            action.targets.get_mut("run").unwrap().server = "late".into();
        }
        let listed = server.listed().await.unwrap();
        assert_eq!(
            listed["tools"][1]["inputSchema"]["properties"]["kind"]["enum"],
            json!(["schedule"])
        );
        let call = json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"stop","arguments":{"kind":"run","id":"one"}}});
        let refused = format!("{:#}", server.called(&call).await.unwrap_err());
        assert!(
            refused.contains("the namespace provider late is unavailable"),
            "{refused}"
        );
        let mut late = load(&dir, "run-one").unwrap();
        late.name = "late".into();
        save(&dir, &late).unwrap();
        let listed = server.listed().await.unwrap();
        assert_eq!(
            listed["tools"][1]["inputSchema"]["properties"]["kind"]["enum"],
            json!(["run", "schedule"])
        );
        server.called(&call).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("run-one")).unwrap(),
            "called\n"
        );
    }

    /// An unlisted action is never offered, yet a call to it by name is
    /// checked against its provider's schema and approval policy as any
    /// other.
    #[tokio::test]
    async fn an_unlisted_action_is_still_checked_and_gated() {
        let (_root, dir, mut server) = fixture();
        let stop = server.namespace.actions.get_mut("stop").unwrap();
        stop.unlisted = true;
        stop.targets.retain(|kind, _| kind == "schedule");
        let listed = server.listed().await.unwrap();
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list"]);
        let call = |arguments: Value| json!({"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"stop","arguments":arguments}});
        // An argument its provider doesn't take is refused.
        assert!(
            server
                .called(&call(json!({"id": "one", "server": "run-one"})))
                .await
                .is_err()
        );
        assert!(!dir.join("schedule-one").exists());
        server.called(&call(json!({"id": "one"}))).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("schedule-one")).unwrap(),
            "called\n"
        );
        // Its provider now asks first: refused, with no one to ask.
        let mut tool = load(&dir, "schedule-one").unwrap();
        tool.ask = Ask::All;
        save(&dir, &tool).unwrap();
        assert!(server.called(&call(json!({"id": "one"}))).await.is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("schedule-one")).unwrap(),
            "called\n"
        );
    }

    #[tokio::test]
    async fn an_unavailable_provider_does_not_hide_another_providers_actions() {
        let (_root, _dir, mut server) = fixture();
        for action in server.namespace.actions.values_mut() {
            action.targets.get_mut("run").unwrap().server = "missing".into();
        }
        let listed = server.listed().await.unwrap();
        assert_eq!(listed["tools"].as_array().unwrap().len(), 2);
        let call = json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"stop","arguments":{"kind":"schedule","id":"one"}}});
        server.called(&call).await.unwrap();
    }
}
