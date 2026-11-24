//! One client for Link-owned commands, in every harness.
use super::tasks;
use crate::action_registry::{Surface, command_name};
use anyhow::ensure;
use link_format::WithinExt;
use link_model_contract::{api, chat};
use link_sandbox_client::namespace::Endpoints;
use serde_json::{Value, json};

pub fn commands(endpoints: &Endpoints) -> Vec<(String, String)> {
    let mut registry = crate::actions();
    registry.extend(link_model_contract::actions());
    endpoints
        .actions
        .iter()
        .filter_map(|path| {
            registry
                .get(path)
                .filter(|action| action.on(Surface::Command))
                .map(|action| (command_name(path), action.what().to_string()))
        })
        .collect()
}
pub fn parsed(_endpoints: &Endpoints, text: &str) -> Option<(String, String)> {
    let text = text.trim().strip_prefix('/')?;
    let (name, args) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let mut registry = crate::actions();
    registry.extend(link_model_contract::actions());
    registry
        .into_iter()
        .find(|(path, action)| {
            action.on(Surface::Command)
                && action.on(Surface::Route)
                && (command_name(path) == name
                    || action
                        .aliases
                        .iter()
                        .any(|alias| alias.trim_start_matches('/') == name))
        })
        .map(|(path, _)| (path, args.trim().to_string()))
}

/// Runs the command of action `path`, `arguments` after its name, in
/// `thread`: a JSON object as its route takes it, or the task
/// namespace's words ([`super::tasks`]).
pub async fn command(
    endpoints: &Endpoints,
    path: &str,
    arguments: &str,
    thread: &str,
) -> anyhow::Result<Value> {
    match path {
        "task stop" => {
            if let Some(stopping) = tasks::stopping(arguments) {
                return stop(endpoints, stopping?, thread).await;
            }
        }
        "task list" if tasks::agents_only(arguments) => {
            let listed = call(endpoints, path, json!({"thread": thread})).await?;
            return Ok(tasks::agents_of(&listed));
        }
        _ => {}
    }
    let mut input = match (path, tasks::schedule(arguments)) {
        ("cron create", Some(asked)) => asked?,
        _ if arguments.is_empty() => json!({}),
        _ => serde_json::from_str(arguments).within("action arguments are a JSON object")?,
    };
    ensure!(input.is_object(), "action arguments are a JSON object");
    if path.starts_with("task ") || path.starts_with("cron ") || path == "agent send" {
        input
            .as_object_mut()
            .expect("object checked")
            .entry("thread")
            .or_insert_with(|| json!(thread));
    }
    // A chat's context is its own, unless it names another.
    if path == "model context" {
        input
            .as_object_mut()
            .expect("object checked")
            .entry("chat")
            .or_insert_with(|| json!(thread));
    }
    call(endpoints, path, input).await
}
/// `/task:stop`'s words: each thing `stopping` names that `task list`
/// lists in `thread`, stopped by its provider, one `task stop` each.
/// What stopped, and what didn't, and why.
async fn stop(
    endpoints: &Endpoints,
    stopping: tasks::Stopping,
    thread: &str,
) -> anyhow::Result<Value> {
    let listed = call(endpoints, "task list", json!({"thread": thread})).await?;
    let (mut stopped, mut failed) = (Vec::new(), Vec::new());
    for listed in tasks::listed(&listed) {
        if !listed.going || !stopping.stops(listed.work) {
            continue;
        }
        let said = json!({"work": listed.work.word(), "id": listed.id});
        match call(endpoints, "task stop", tasks::stop_request(&listed, thread)).await {
            Ok(_) => stopped.push(said),
            Err(why) => failed.push(json!({"work": listed.work.word(), "id": listed.id,
                "why": format!("{why:#}")})),
        }
    }
    Ok(json!({"stopped": stopped, "failed": failed}))
}

pub async fn call(endpoints: &Endpoints, path: &str, input: Value) -> anyhow::Result<Value> {
    calling(endpoints, path, input, None).await
}

/// The same granted action through a local proxy's existing Unix socket.
#[cfg(unix)]
pub async fn call_via_socket(
    endpoints: &Endpoints,
    path: &str,
    input: Value,
    socket: &std::path::Path,
) -> anyhow::Result<Value> {
    calling(endpoints, path, input, Some(socket)).await
}

async fn calling(
    endpoints: &Endpoints,
    path: &str,
    input: Value,
    socket: Option<&std::path::Path>,
) -> anyhow::Result<Value> {
    ensure!(
        endpoints.actions.iter().any(|name| name == path),
        "action is outside the grant"
    );
    let (namespace, name) = path
        .split_once(' ')
        .within("action needs namespace and name")?;
    let url = endpoints
        .urls
        .get(namespace)
        .within("namespace has no granted endpoint")?;
    let action = if namespace == "model" {
        link_model_contract::actions().get(path).cloned()
    } else {
        crate::actions().get(path).cloned()
    }
    .within("unregistered action")?;
    let limits = action.limits.within("action has no budget")?;
    let client =
        link_fetch::Client::builder().timeout(std::time::Duration::from_millis(limits.time_ms));
    #[cfg(unix)]
    let client = match socket {
        Some(socket) => client.unix_socket(socket),
        None => client,
    };
    let client = client.build()?;
    if namespace == "model" {
        return model(&client, url, name, input).await;
    }
    let read = matches!(path, "agent show" | "memory show" | "tool list");
    let query = if read {
        let input = input.as_object().within("read arguments are an object")?;
        if path == "agent show"
            && input.len() == 1
            && input.get("instructions") == Some(&Value::Bool(true))
        {
            "?instructions=true"
        } else {
            ensure!(input.is_empty(), "unknown read argument");
            ""
        }
    } else {
        ""
    };
    let target = link_secretive::Secret::from(format!("{url}/{name}{query}"));
    let request = if read {
        client.secret_url(link_fetch::Method::GET, &target, path)
    } else {
        client
            .secret_url(link_fetch::Method::POST, &target, path)
            .json(&input)
    };
    Ok(request.limit(limits.bytes as u64).answer().await?)
}
async fn model(
    client: &link_fetch::Client,
    url: &str,
    name: &str,
    input: Value,
) -> anyhow::Result<Value> {
    Ok(match name {
        "show" => {
            let input = input.as_object().within("show arguments are an object")?;
            ensure!(
                input.keys().all(|key| key == "name"),
                "unknown show argument"
            );
            let query = match input.get("name") {
                None => String::new(),
                Some(name) => {
                    let name = name.as_str().within("name is a string")?;
                    ensure!(
                        link_model_contract::show::plain_name(name),
                        "invalid model name"
                    );
                    format!("?name={name}")
                }
            };
            serde_json::to_value(
                chat::client::canonical::get::<link_model_contract::show::Shown>(
                    client,
                    url,
                    &format!("show{query}"),
                )
                .await?,
            )?
        }
        "list" => {
            ensure!(input == json!({}), "list takes no arguments");
            serde_json::to_value(
                chat::client::canonical::get::<Vec<String>>(client, url, name).await?,
            )?
        }
        "status" => {
            ensure!(input == json!({}), "status takes no arguments");
            serde_json::to_value(
                chat::client::canonical::get::<api::Status>(client, url, name).await?,
            )?
        }
        "usage" => {
            ensure!(input == json!({}), "usage takes no arguments");
            serde_json::to_value(
                chat::client::canonical::get::<api::Usage>(client, url, name).await?,
            )?
        }
        "context" => serde_json::to_value(
            chat::client::canonical::post::<api::Context>(
                client,
                url,
                name,
                &serde_json::from_value::<api::ContextOf>(input)?,
            )
            .await?,
        )?,
        "embed" => serde_json::to_value(
            chat::client::canonical::post::<api::Embedded>(
                client,
                url,
                name,
                &serde_json::from_value::<api::Embed>(input)?,
            )
            .await?,
        )?,
        "transcribe" => serde_json::to_value(
            chat::client::canonical::post::<api::Heard>(
                client,
                url,
                name,
                &serde_json::from_value::<api::Transcribe>(input)?,
            )
            .await?,
        )?,
        "count" => serde_json::to_value(
            chat::client::canonical::post::<api::Counted>(
                client,
                url,
                name,
                &serde_json::from_value::<api::Count>(input)?,
            )
            .await?,
        )?,
        "chat" => {
            let input: chat::Chat = serde_json::from_value(input)?;
            input.check()?;
            let (pieces, mut events) = tokio::sync::mpsc::channel(32);
            let (_cancel, cancelled) = tokio::sync::watch::channel(false);
            let prompt = input.prompt();
            let answer = chat::client::canonical::answer(
                client,
                chat::client::canonical::Target {
                    url,
                    name: input.name.as_deref(),
                },
                &prompt,
                &pieces,
                false,
                cancelled,
                None,
            );
            tokio::pin!(answer);
            let answer = loop {
                tokio::select! {answer=&mut answer=>break answer?,Some(_)=events.recv()=>{}}
            };
            serde_json::to_value(chat::Stream::Answer { answer })?
        }
        _ => anyhow::bail!("unknown model action"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// A task namespace's route on loopback: `list` answers a running
    /// command, a subagent and a schedule; every other action answers
    /// `{}`. What it was asked, by path and body.
    async fn route() -> (Endpoints, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Seen::default();
        let keep = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let keep = keep.clone();
                tokio::spawn(async move {
                    let mut asked = Vec::new();
                    let mut buf = [0u8; 4096];
                    let (head, body) = loop {
                        let n = socket.read(&mut buf).await.unwrap();
                        asked.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&asked).to_string();
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if body.len() >= length {
                                break (head.to_string(), body.to_string());
                            }
                        }
                    };
                    let path = head.split_whitespace().nth(1).unwrap().to_string();
                    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    keep.lock().unwrap().push((path.clone(), body));
                    let answer = match path.as_str() {
                        "/task/list" => json!({"structuredContent": {"results": {
                            "run": {"structuredContent": {"commands": [
                                {"id": "r1", "state": "running"}, {"id": "r2", "state": "exited"}]}},
                            "schedule": {"structuredContent": {"jobs": [
                                {"id": "a1", "kind": "task"}, {"id": "s1", "kind": "schedule"}]}}}}}),
                        _ => json!({}),
                    }
                    .to_string();
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    socket.write_all(reply.as_bytes()).await.unwrap();
                });
            }
        });
        let endpoints = Endpoints {
            urls: [
                ("task".to_string(), format!("http://127.0.0.1:{port}/task")),
                ("cron".to_string(), format!("http://127.0.0.1:{port}/cron")),
            ]
            .into(),
            actions: ["task list", "task stop", "cron create"]
                .map(String::from)
                .to_vec(),
        };
        (endpoints, seen)
    }

    /// The task namespace's words reach its providers as the requests
    /// they take: one stop each for what's listed and going, the list's
    /// subagents, and an interval's cron.
    #[tokio::test]
    async fn task_words_become_their_providers_requests() {
        let (endpoints, seen) = route().await;
        let stopped = command(&endpoints, "task stop", "--all", "telegram:42")
            .await
            .unwrap();
        assert_eq!(
            stopped,
            json!({"stopped": [{"work": "tasks", "id": "r1"}, {"work": "agents", "id": "a1"},
                {"work": "schedules", "id": "s1"}], "failed": []})
        );
        let asked = std::mem::take(&mut *seen.lock().unwrap());
        assert_eq!(
            asked[0],
            ("/task/list".into(), json!({"thread": "telegram:42"}))
        );
        assert_eq!(
            asked[1..]
                .iter()
                .map(|(p, b)| (p.as_str(), b["kind"].as_str(), b["id"].as_str()))
                .collect::<Vec<_>>(),
            [
                ("/task/stop", Some("run"), Some("r1")),
                ("/task/stop", Some("schedule"), Some("a1")),
                ("/task/stop", Some("schedule"), Some("s1")),
            ]
        );
        let agents = command(&endpoints, "task stop", "agents", "telegram:42")
            .await
            .unwrap();
        assert_eq!(agents["stopped"], json!([{"work": "agents", "id": "a1"}]));
        seen.lock().unwrap().clear();
        let listed = command(&endpoints, "task list", "--agents", "telegram:42")
            .await
            .unwrap();
        assert_eq!(
            listed,
            json!({"agents": [{"id": "a1", "thread": "task-a1"}]})
        );
        seen.lock().unwrap().clear();
        command(
            &endpoints,
            "cron create",
            "10m check the build",
            "telegram:42",
        )
        .await
        .unwrap();
        assert_eq!(
            seen.lock().unwrap()[0],
            (
                "/cron/create".into(),
                json!({"prompt": "check the build", "cron": "*/10 * * * *",
                "thread": "telegram:42"})
            )
        );
        assert!(
            command(&endpoints, "cron create", "7m nope", "telegram:42")
                .await
                .is_err()
        );
        assert!(
            command(&endpoints, "task stop", "everything", "telegram:42")
                .await
                .is_err()
        );
    }
}
