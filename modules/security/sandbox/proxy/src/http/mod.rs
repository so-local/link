//! What the proxy answers itself, and how it reads the requests it
//! carries: on the one HTTP parser, [`link_sandbox_route::http`].

mod request;

pub use link_sandbox_route::http::*;

pub use request::{Browser, from_a_browser, one_request, proxy_authorized};
use tokio::io::AsyncWriteExt;

pub fn refusal(err: &anyhow::Error) -> &'static str {
    if err.to_string().contains("refused:") {
        "403 Forbidden"
    } else {
        "502 Bad Gateway"
    }
}

/// A 407: a request without the run's proxy secret, and how to send it.
pub async fn unauthorized<S: tokio::io::AsyncWrite + Unpin>(client: &mut S) {
    let _ = client
        .write_all(
            b"HTTP/1.1 407 Proxy Authentication Required\r\n\
              Proxy-Authenticate: Basic realm=\"lnk sandbox\"\r\n\
              Content-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
}

/// A 429: over the wire's calls a minute, and when to try again.
pub async fn too_many<S: tokio::io::AsyncWrite + Unpin>(client: &mut S, wait: u64) {
    let _ = client
        .write_all(
            format!(
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: {wait}\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await;
}
