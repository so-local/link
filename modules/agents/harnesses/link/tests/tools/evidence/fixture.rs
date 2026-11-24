use super::super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Semaphore;

pub(super) struct Control {
    pub holding: AtomicBool,
    pub listing: AtomicBool,
    pub mode: AtomicUsize,
    pub effects: AtomicUsize,
    pub release_list: Semaphore,
    pub release_second: Semaphore,
    second_entered: Semaphore,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            holding: AtomicBool::new(false),
            listing: AtomicBool::new(false),
            mode: AtomicUsize::new(0),
            effects: AtomicUsize::new(0),
            release_list: Semaphore::new(0),
            release_second: Semaphore::new(0),
            second_entered: Semaphore::new(0),
        }
    }
}

pub(super) struct Mcp {
    pub url: String,
    pub control: Arc<Control>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Mcp {
    pub async fn new() -> Self {
        link_fetch::install_crypto_provider();
        let control = Arc::new(Control::default());
        let app = Router::new()
            .route("/mcp", post(reply))
            .with_state(control.clone());
        let held = link_test_kit::Held::loopback();
        let port = held.port();
        let listener = held.listener();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            url: format!("http://127.0.0.1:{port}/mcp"),
            control,
            task,
        }
    }
}

async fn reply(
    State(control): State<Arc<Control>>,
    Json(message): Json<Value>,
) -> axum::response::Response {
    let method = message["method"].as_str().unwrap_or_default();
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
        }
        "tools/list" => {
            if control.holding.load(Ordering::SeqCst) {
                control.listing.store(true, Ordering::SeqCst);
                control.release_list.acquire().await.unwrap().forget();
            }
            json!({"tools":[{"name":"search","description":"Fixture search.","inputSchema":{"type":"object","properties":{"query":{"type":"string"}}},"annotations":{"readOnlyHint":true}}]})
        }
        "tools/call" => {
            control.effects.fetch_add(1, Ordering::SeqCst);
            if control.holding.load(Ordering::SeqCst) {
                if message["params"]["arguments"]["query"] == "Albuquerque" {
                    control.second_entered.acquire().await.unwrap().forget();
                } else {
                    control.second_entered.add_permits(1);
                    control.release_second.acquire().await.unwrap().forget();
                }
            }
            json!({"content":[{"type":"text","text":"fixture answer"}],"structuredContent":{"exit":7,"fact":"independent fixture"}})
        }
        _ => return axum::http::StatusCode::ACCEPTED.into_response(),
    };
    let mode = if method == "tools/call" {
        control.mode.load(Ordering::SeqCst)
    } else {
        0
    };
    if mode == 4 {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let id = if mode == 1 || mode == 2 {
        json!(message["id"].as_u64().unwrap() + 1)
    } else {
        message["id"].clone()
    };
    let answer = if mode == 3 {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32001,"message":"fixture refusal","data":{"reason":"known"}}})
    } else {
        json!({"jsonrpc":"2.0","id":id,"result":result})
    };
    if mode == 2 {
        return axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(format!("data: {answer}\n\n")))
            .unwrap();
    }
    ([("mcp-session-id", "fixture")], Json(answer)).into_response()
}
