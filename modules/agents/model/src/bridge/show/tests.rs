use super::*;

use std::sync::{Arc, Mutex};

use axum::{Json, Router, routing::get, routing::post};

/// What a runtime says that never reaches a sandbox.
const PRIVATE: [&str; 5] = [
    "/home/sam/models",
    "FROM /home/sam",
    "{{ .Prompt }}",
    "LLAMA 3 COMMUNITY LICENSE",
    "temperature",
];

/// A runtime on a port of its own answering `routes`.
async fn runtime(routes: Router) -> u16 {
    let listener = link_test_kit::Held::loopback().listener();
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(
            listener,
            routes.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    port
}

#[tokio::test]
async fn metadata_reuses_the_connection_and_reads_changed_values() {
    use axum::extract::ConnectInfo;
    use std::collections::BTreeSet;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicU64, Ordering};
    let window = Arc::new(AtomicU64::new(8192));
    let peers = Arc::new(Mutex::new(BTreeSet::new()));
    let port = runtime(
        Router::new()
            .route(
                "/api/show",
                post({
                    let window = window.clone();
                    let peers = peers.clone();
                    move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                        peers.lock().unwrap().insert(peer);
                        let context = window.load(Ordering::SeqCst);
                        async move { Json(json!({"details": {"family": "test"}, "model_info": {"test.context_length": context}})) }
                    }
                }),
            )
            .route(
                "/api/ps",
                get({
                    let peers = peers.clone();
                    move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                        peers.lock().unwrap().insert(peer);
                        async { Json(json!({"models": []})) }
                    }
                }),
            ),
    )
    .await;
    assert_eq!(ask(port, "test").await.unwrap().trained, Some(8192));
    window.store(16384, Ordering::SeqCst);
    assert_eq!(ask(port, "test").await.unwrap().trained, Some(16384));
    assert_eq!(
        peers.lock().unwrap().len(),
        1,
        "fresh metadata uses the same HTTP connection"
    );
}

/// `shown` as it's sent, and nothing private in it.
fn sent(shown: &Shown) -> String {
    let text = serde_json::to_string(shown).unwrap();
    for private in PRIVATE {
        assert!(!text.contains(private), "{private} reached it: {text}");
    }
    text
}

#[tokio::test]
async fn ollama_is_answered_in_links_shape() {
    let port = runtime(
        Router::new()
            .route(
                "/api/show",
                post(|| async {
                    Json(json!({
                        "modelfile": "FROM /home/sam/models/llama3.gguf",
                        "template": "{{ .Prompt }}",
                        "license": "LLAMA 3 COMMUNITY LICENSE",
                        "parameters": "num_ctx 16384\ntemperature 0.2",
                        "details": {"family": "llama", "parameter_size": "8.0B",
                                    "quantization_level": "Q4_K_M"},
                        "model_info": {"llama.context_length": 131072,
                                       "general.basename": "/home/sam/models"},
                        "capabilities": ["completion", "tools", "insert"]
                    }))
                }),
            )
            .route("/api/ps", get(|| async { Json(json!({"models": []})) })),
    )
    .await;
    let shown = ask(port, "llama3:8b").await.unwrap();
    sent(&shown);
    assert_eq!(
        shown,
        Shown {
            facts: None,
            name: "llama3:8b".into(),
            family: Some("llama".into()),
            parameters: Some("8.0B".into()),
            quantization: Some("Q4_K_M".into()),
            context: Some(16384),
            trained: Some(131072),
            capabilities: vec![Capability::Completion, Capability::Tools],
            endpoint: None,
        }
    );
}

#[tokio::test]
async fn llama_cpp_lm_studio_and_openai_are_answered_in_links_shape() {
    let llama = runtime(Router::new().route(
        "/props",
        get(|| async {
            Json(json!({"model_path": "/home/sam/models/x.gguf",
                "chat_template": "{{ .Prompt }}",
                "default_generation_settings": {"n_ctx": 8192, "temperature": 0.8}}))
        }),
    ))
    .await;
    let shown = ask(llama, "x").await.unwrap();
    sent(&shown);
    assert_eq!(shown.context, Some(8192));

    let lm = runtime(Router::new().route(
        "/api/v0/models/qwen3-8b",
        get(|| async {
            Json(json!({"id": "qwen3-8b", "type": "vlm", "arch": "qwen3",
                "quantization": "Q4_K_M", "path": "/home/sam/models/qwen",
                "loaded_context_length": 4096, "max_context_length": 32768,
                "license": "LLAMA 3 COMMUNITY LICENSE"}))
        }),
    ))
    .await;
    let shown = ask(lm, "qwen3-8b").await.unwrap();
    sent(&shown);
    assert_eq!(
        (shown.family.as_deref(), shown.context, shown.trained),
        (Some("qwen3"), Some(4096), Some(32768))
    );
    assert_eq!(shown.capabilities, [Capability::Vision]);

    // vLLM serves the list alone.
    let vllm = runtime(Router::new().route(
        "/v1/models",
        get(|| async {
            Json(
                json!({"object": "list", "data": [{"id": "meta/llama", "object": "model",
                "max_model_len": 65536, "root": "/home/sam/models",
                "template": "{{ .Prompt }}"}]}),
            )
        }),
    ))
    .await;
    let shown = ask(vllm, "meta/llama").await.unwrap();
    sent(&shown);
    assert_eq!(shown.context, Some(65536));
    // Its list, as a sandbox gets it: plain ids alone.
    let listed = list(vllm).await.unwrap();
    assert_eq!(
        listed,
        json!({"object": "list", "data": [{"id": "meta/llama", "object": "model"}]})
    );
}

/// Ollama's loaded window wins over its settings'; a 200 with no
/// details is no answer; a name with `/` never asks LM Studio's route.
#[tokio::test]
async fn ollama_says_its_loaded_window_and_an_empty_answer_is_none() {
    let port = runtime(
        Router::new()
            .route(
                "/api/show",
                post(|| async {
                    Json(json!({"details": {"family": "qwen3"},
                        "parameters": "num_ctx 16384"}))
                }),
            )
            .route(
                "/api/ps",
                get(|| async {
                    Json(json!({"models": [{"name": "qwen3:8b", "context_length": 40960}]}))
                }),
            ),
    )
    .await;
    assert_eq!(ask(port, "qwen3:8b").await.unwrap().context, Some(40960));
    assert_eq!(ollama("x", &json!({"details": {}}), None), None);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let lm = runtime(Router::new().fallback(move |uri: axum::http::Uri| {
        seen.lock().unwrap().push(uri.path().to_string());
        async { axum::http::StatusCode::NOT_FOUND }
    }))
    .await;
    assert!(ask(lm, "a/b").await.is_err());
    let asked = asked.lock().unwrap().clone();
    assert!(
        !asked.iter().any(|p| p.starts_with("/api/v0/")),
        "{asked:?}"
    );
}

/// llama.cpp's window is its one model's: another name gets none.
#[tokio::test]
async fn llama_cpp_answers_only_for_its_model() {
    let props = json!({"model_path": "/home/sam/models/x.gguf",
        "default_generation_settings": {"n_ctx": 8192}});
    assert_eq!(llama_cpp("x", &props).and_then(|s| s.context), Some(8192));
    assert_eq!(llama_cpp("y", &props), None);
    let aliased = json!({"model_alias": "mine", "n_ctx": 4096});
    assert_eq!(
        llama_cpp("mine", &aliased).and_then(|s| s.context),
        Some(4096)
    );
}

/// An answer past [`ANSWER_MAX`], or later than [`SHOW_WITHIN`], is
/// refused.
#[tokio::test]
async fn an_oversized_or_slow_answer_is_refused() {
    let big = runtime(Router::new().route(
        "/props",
        get(|| async {
            let filler = "x".repeat(ANSWER_MAX);
            Json(json!({"model_alias": "x", "n_ctx": 8192, "filler": filler}))
        }),
    ))
    .await;
    assert!(ask(big, "x").await.is_err());
    let slow =
        runtime(Router::new().route("/props", get(std::future::pending::<Json<Value>>))).await;
    let asking = tokio::spawn(async move { ask(slow, "x").await });
    link_test_kit::wait::eventually_async("the metadata deadline", SHOW_WITHIN * 2, || {
        asking.is_finished()
    })
    .await;
    let error = asking.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("in time"), "{error}");
}

/// An answer none maps, or none at all, is refused; so is a name that
/// isn't one.
#[tokio::test]
async fn an_answer_link_cant_map_is_refused() {
    let odd = runtime(
        Router::new()
            .route(
                "/props",
                get(|| async { Json(json!({"model_path": "/home/sam"})) }),
            )
            .route(
                "/v1/models/{*name}",
                get(|| async { Json(json!({"id": "another", "object": "model"})) }),
            ),
    )
    .await;
    let refused = ask(odd, "x").await.unwrap_err();
    assert!(
        refused.to_string().contains("says nothing of x"),
        "{refused}"
    );
    assert!(ask(odd, "../x").await.is_err());
}
