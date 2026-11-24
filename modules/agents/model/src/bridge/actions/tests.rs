use super::*;
use axum::{
    Json, Router,
    routing::{get, post},
};
use link_sandbox_route::Unsaid;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn runtime() -> (u16, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app=Router::new()
        .route("/v1/models",get(||async{Json(json!({"data":[{"id":"qwen3:8b","root":"/home/alice/secret.gguf"},{"id":"/home/alice/secret.gguf"},{"id":"another"}]}))}))
        .route("/api/ps",get(||async{Json(json!({"models":[{"name":"qwen3:8b","path":"/home/alice/secret.gguf"}]}))}))
        .route("/api/show",post(||async{Json(json!({"details":{"family":"qwen3"},"modelfile":"/home/alice/secret.gguf"}))}))
        .route("/v1/chat/completions",post(move|Json(body):Json<Value>|{counted.fetch_add(1,Ordering::SeqCst);async move {
            assert_eq!(body["model"],"qwen3:8b");
            Json(json!({"model":"/home/alice/private.gguf","choices":[{"message":{"content":"hello","tool_calls":[{"id":"c1","function":{"name":"file__read","arguments":"{\"file_path\":\"a\"}"}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":3}}))
        }}))
        .route("/v1/embeddings",post(||async{Json(json!({"data":[{"embedding":[0.5,1.0]}],"host":"/home/alice"}))}))
        .route("/tokenize",post(||async{Json(json!({"tokens":[1,2,3],"host":"/home/alice"}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (port, task, calls)
}
fn grant(port: u16) -> Api {
    Api {
        transcribe: None,
        window: None,
        answer: None,
        backend: Backend::Local { port },
        name: "qwen3:8b".into(),
        allow: Some(vec!["qwen3:8b".into()]),
        fallback: false,
    }
}
async fn ask(grant: &Api, method: &str, action: &str, body: Value, settings: &Path) -> String {
    let (mut client, server) = tokio::io::duplex(1024);
    let body = if body.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&body).unwrap()
    };
    let head = format!(
        "{method} /_link/model/{action} HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let serving = serve(
        Unsaid(server),
        head.as_bytes(),
        body,
        grant,
        settings,
        "agent-one",
        "s1s2s3s4s5s6",
        "direct",
    );
    let reading = async {
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).await.unwrap();
        String::from_utf8(bytes).unwrap()
    };
    let (_, answer) = tokio::join!(serving, reading);
    answer
}
fn body(answer: &str) -> Value {
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    serde_json::from_str(answer.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn namespace_actions_project_only_their_own_answer_types() {
    let (port, task, _) = runtime().await;
    let grant = grant(port);
    let dir = link_test_kit::TempDir::new("typed-model");
    assert_eq!(
        body(&ask(&grant, "GET", "list", Value::Null, &dir).await),
        json!(["qwen3:8b"])
    );
    assert_eq!(
        body(&ask(&grant, "GET", "show", Value::Null, &dir).await)["endpoint"],
        json!({"ready":true,"loaded":["qwen3:8b"]})
    );
    assert_eq!(
        body(&ask(&grant, "POST", "embed", json!({"texts":["hello"]}), &dir).await),
        json!({"vectors":[[0.5,1.0]]})
    );
    assert_eq!(
        body(&ask(&grant, "POST", "count", json!({"text":"hello"}), &dir).await),
        json!({"tokens":3})
    );
    let chat=ask(&grant,"POST","chat",json!({"system":"system","messages":[{"role":"user","text":"hello","calls":[],"answers":[],"thinking":[],"parts":[]}],"answer":512,"window":8192}),&dir).await;
    assert!(chat.starts_with("HTTP/1.1 200"), "{chat}");
    let events: Vec<chat::Stream> = chat
        .split_once("\r\n\r\n")
        .unwrap()
        .1
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let chat::Stream::Answer { answer } = events.last().unwrap() else {
        panic!("no completed answer")
    };
    assert_eq!(answer.text, "hello");
    assert_eq!(answer.calls[0].name, "file__read");
    assert_eq!(answer.tokens.input, 12);
    assert_eq!(
        body(&ask(&grant, "GET", "usage", Value::Null, &dir).await),
        json!({"requests":1,"unmetered":0,"tokens":{"input":12,"output":3,"cached":0}})
    );
    assert!(!chat.contains("alice"));
    task.abort();
}
#[tokio::test]
async fn each_chat_call_is_kept_part_by_part() {
    let (port, task, _) = runtime().await;
    let grant = grant(port);
    let dir = link_test_kit::TempDir::new("model-context");
    let message =
        json!({"role":"user","text":"hello","calls":[],"answers":[],"thinking":[],"parts":[]});
    let tool = json!({"name":"file__read","description":"Reads a file","schema":{"type":"object"}});
    let asked = json!({"system":"Be brief.","messages":[message],"tools":[tool],"answer":512,"window":8192,"chat":"main"});
    let chat = ask(&grant, "POST", "chat", asked, &dir).await;
    assert!(chat.starts_with("HTTP/1.1 200"), "{chat}");
    // Without a name, a chat is its opening message's.
    let unnamed = json!({"system":"Be brief.","messages":[message],"answer":512,"window":8192});
    let chat = ask(&grant, "POST", "chat", unnamed, &dir).await;
    assert!(chat.starts_with("HTTP/1.1 200"), "{chat}");

    let context: api::Context = serde_json::from_value(body(
        &ask(&grant, "POST", "context", json!({"chat":"main"}), &dir).await,
    ))
    .unwrap();
    assert_eq!(context.chats.len(), 2);
    let report = context.chat.expect("main's report");
    assert_eq!(report.chat, "main");
    assert_eq!(report.latest.window, Some(8192));
    let paths: Vec<&str> = report
        .latest
        .parts
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["tools/file__read", "instructions", "conversation/user"]
    );
    let tokens: u64 = report.latest.parts.iter().map(|s| s.tokens).sum();
    assert_eq!(tokens, 12, "the measured prompt, split");
    assert!(
        report
            .latest
            .parts
            .iter()
            .all(|s| s.how == link_model_contract::context::How::Apportioned)
    );

    // The latest chat without a body; no words of the prompt in it.
    let latest = ask(&grant, "POST", "context", Value::Null, &dir).await;
    let latest: api::Context = serde_json::from_value(body(&latest)).unwrap();
    assert!(latest.chat.expect("latest").chat.starts_with("opening-"));
    let kept = std::fs::read_dir(super::super::context::dir(&dir, "agent-one").unwrap()).unwrap();
    for file in kept.filter_map(Result::ok) {
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(
            !text.contains("Be brief") && !text.contains("hello"),
            "{text}"
        );
    }
    task.abort();
}
#[tokio::test]
async fn a_composed_system_prompt_is_counted_by_its_spans() {
    let (port, task, _) = runtime().await;
    let grant = grant(port);
    let dir = link_test_kit::TempDir::new("model-spans");
    let system = "You are Scout.\n\nYour notes: the harbour opens at six.";
    let message =
        json!({"role":"user","text":"hello","calls":[],"answers":[],"thinking":[],"parts":[]});
    let asked = json!({
        "system": system, "messages": [message], "answer": 512, "window": 8192, "chat": "main",
        "spans": [{"name": "persona", "start": 0, "end": 14},
                  {"name": "notes", "start": 16, "end": system.len()}]
    });
    let chat = ask(&grant, "POST", "chat", asked, &dir).await;
    assert!(chat.starts_with("HTTP/1.1 200"), "{chat}");
    let context: api::Context = serde_json::from_value(body(
        &ask(&grant, "POST", "context", Value::Null, &dir).await,
    ))
    .unwrap();
    let report = context.chat.expect("main's report");
    let paths: Vec<&str> = report
        .latest
        .parts
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "instructions/persona",
            "instructions",
            "instructions/notes",
            "conversation/user"
        ]
    );
    task.abort();
}
#[tokio::test]
async fn a_call_unanswered_is_never_kept() {
    // No runtime on the port: the chat fails, and nothing is kept.
    let held = link_test_kit::HeldNumber::port();
    let grant = grant(held.port_number());
    let dir = link_test_kit::TempDir::new("model-unanswered");
    let message =
        json!({"role":"user","text":"hello","calls":[],"answers":[],"thinking":[],"parts":[]});
    let asked = json!({"system":"s","messages":[message],"answer":512,"window":8192,"chat":"main"});
    let chat = ask(&grant, "POST", "chat", asked, &dir).await;
    assert!(chat.contains("\"error\""), "{chat}");
    // It says why, in Link's words, and what shows what runs.
    let said = format!("nothing answers on port {}", held.port_number());
    assert!(chat.contains(&said), "{chat}");
    assert!(chat.contains("lnk model runtime list"), "{chat}");
    assert!(chat.contains("\"kind\":\"unreached\""), "{chat}");
    let context: api::Context = serde_json::from_value(body(
        &ask(&grant, "POST", "context", Value::Null, &dir).await,
    ))
    .unwrap();
    assert!(context.chats.is_empty() && context.chat.is_none());
}
#[tokio::test]
async fn request_cannot_change_the_backend_or_escape_the_model_allow_list() {
    let (port, task, calls) = runtime().await;
    let grant = grant(port);
    let dir = link_test_kit::TempDir::new("model-deny");
    for input in [
        json!({"text":"hello","url":"http://elsewhere"}),
        json!({"text":"hello","name":"another"}),
    ] {
        let answer = ask(&grant, "POST", "count", input, &dir).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    }
    let answer = ask(&grant, "GET", "list?surprise=1", Value::Null, &dir).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    task.abort();
}
/// A refusal says why when the reason is the caller's own request,
/// and a provider's refusal its status and message, its key taken out.
#[tokio::test]
async fn a_refusal_says_why_without_the_credential() {
    let (port, task, calls) = runtime().await;
    let mut grant = grant(port);
    grant.answer = Some(1024);
    let dir = link_test_kit::TempDir::new("model-said");
    let chat = json!({"system":"system","messages":[],"answer":4096,"window":8192});
    let answer = ask(&grant, "POST", "chat", chat, &dir).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert!(
        answer.contains("the request's answer, 4096 tokens, is more than the 1024 granted"),
        "{answer}"
    );
    let answer = ask(
        &grant,
        "POST",
        "count",
        json!({"text":"hello","name":"another"}),
        &dir,
    )
    .await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    task.abort();
    let said = refused_by_provider(
        reqwest::StatusCode::FORBIDDEN,
        br#"{"error":{"message":"key sk-or-secret1 may not use this model"}}"#,
        Some("sk-or-secret1"),
    );
    assert_eq!(
        said,
        "the model provider answered 403 Forbidden: key <credential> may not use this model"
    );
    assert_eq!(
        refused_by_provider(reqwest::StatusCode::BAD_GATEWAY, b"<html>", None),
        "the model provider answered 502 Bad Gateway"
    );
}
#[test]
fn routes_are_generated_only_for_the_declared_program_surface() {
    let bridge = link_sandbox_route::Bridge {
        socket: "/tmp/model.sock".into(),
        start: Vec::new(),
    };
    let mut routes =
        link_model_contract::route::actions::routes(bridge.clone(), grant(11434)).unwrap();
    assert_eq!(routes.len(), 8);
    assert!(routes.iter().all(|route| route.needs_url_secret));
    assert!(!routes.iter().any(|route| route.prefix.ends_with("/set")));
    let mut fallback = grant(11434);
    fallback.fallback = true;
    routes.extend(link_model_contract::route::actions::routes(bridge, fallback).unwrap());
    link_sandbox_route::check_routes(&routes).unwrap();
    assert!(serde_json::from_str::<chat::Chat>(r#"{"provider":"openai"}"#).is_err());
}

#[tokio::test]
async fn leaving_a_chat_cancels_the_provider_before_it_answers() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (started, waiting) = tokio::sync::oneshot::channel();
    let provider = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (head, rest) = link_sandbox_route::http::read_head(&mut socket)
            .await
            .unwrap();
        let length = RequestHead::parse(&head).unwrap().length().unwrap() as usize;
        let mut missing = vec![0; length.saturating_sub(rest.len())];
        socket.read_exact(&mut missing).await.unwrap();
        started.send(()).unwrap();
        let mut byte = [0; 1];
        assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
    });
    let dir = link_test_kit::TempDir::new("cancel-model-action");
    let config = dir.to_path_buf();
    let (client, server) = tokio::io::duplex(4096);
    let serving = tokio::spawn(async move {
        let body=serde_json::to_vec(&json!({"system":"system","messages":[{"role":"user","text":"hello"}],"answer":512,"window":8192})).unwrap();
        let head = format!(
            "POST /_link/model/chat HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        serve(
            Unsaid(server),
            head.as_bytes(),
            body,
            &grant(port),
            &config,
            "agent-one",
            "s1s2s3s4s5s6",
            "direct",
        )
        .await
    });
    waiting.await.unwrap();
    drop(client);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), serving)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), provider)
        .await
        .unwrap()
        .unwrap();
}
