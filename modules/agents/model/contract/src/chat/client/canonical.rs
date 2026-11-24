//! Typed client for an agent's proved model namespace URL.
use super::*;
use tokio::sync::watch;
const LIMIT: usize = 8 << 20;

/// Link's rules apply to its proved routes; native provider calls have their own client.
pub fn client() -> anyhow::Result<link_fetch::Client> {
    static CLIENT: std::sync::LazyLock<Result<link_fetch::Client, String>> =
        std::sync::LazyLock::new(|| {
            link_fetch::Client::with_timeout(std::time::Duration::from_secs(600))
                .map_err(|error| error.to_string())
        });
    CLIENT.as_ref().cloned().map_err(anyhow::Error::msg)
}

/// A model selected within a proved namespace endpoint.
pub struct Target<'a> {
    pub url: &'a str,
    pub name: Option<&'a str>,
}

pub async fn answer(
    client: &link_fetch::Client,
    target: Target<'_>,
    prompt: &Prompt<'_>,
    pieces: &mpsc::Sender<String>,
    mut separate: bool,
    mut cancelled: watch::Receiver<bool>,
    first: Option<std::time::Duration>,
) -> anyhow::Result<Answered> {
    let mut body = serde_json::to_value(prompt)?;
    body["name"] = json!(target.name);
    let target = link_secretive::Secret::from(format!("{}/chat", target.url));
    let request = client
        .secret_url(link_fetch::Method::POST, &target, "model/chat")
        .json(&body)
        .limit(LIMIT as u64);
    let start = Instant::now();
    let mut response = match first {
        Some(first) => tokio::time::timeout(first, request.stream())
            .await
            .map_err(|_| Unreached("no answer in time".into()))?
            .map_err(request_error)?,
        None => request.stream().await.map_err(request_error)?,
    };
    let mut pending = Vec::new();
    let mut total = 0;
    let mut text = String::new();
    let mut began = false;
    loop {
        let reading = async {
            match (first, began) {
                (Some(first), false) => {
                    tokio::time::timeout(first.saturating_sub(start.elapsed()), response.chunk())
                        .await
                        .map_err(|_| Unreached("nothing back in time".into()))?
                }
                _ => response.chunk().await,
            }
            .map_err(anyhow::Error::from)
        };
        let chunk = tokio::select! {chunk=reading=>chunk?,_ = asked_to_end(&mut cancelled)=>return Ok(Answered{text,interrupted:true,..Default::default()})};
        let Some(chunk) = chunk else {
            bail!("model/chat ended without its answer");
        };
        total += chunk.len();
        anyhow::ensure!(total <= LIMIT, "model stream exceeds byte budget");
        pending.extend_from_slice(&chunk);
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let event: super::super::Stream = serde_json::from_slice(&pending[..end])?;
            pending.drain(..=end);
            began = true;
            match event {
                super::super::Stream::Text { text: piece } => {
                    text.push_str(&piece);
                    let piece = if std::mem::take(&mut separate) {
                        format!("\n\n{piece}")
                    } else {
                        piece
                    };
                    let _ = pieces.send(piece).await;
                }
                super::super::Stream::Answer { answer } => return Ok(answer),
                super::super::Stream::Error { kind, message } => {
                    return Err(match kind {
                        Failure::Unreached => Unreached(message).into(),
                        Failure::TooLong => TooLong(message).into(),
                        Failure::Overloaded => Overloaded(message).into(),
                        Failure::Refused => anyhow::anyhow!(message),
                    });
                }
            }
        }
    }
}

/// A model/chat call that wasn't answered, by the contract's [`Failure`]
/// kinds, each saying what the service said (its route is named
/// `model/chat`, never by its URL): a 429, or a 5xx that's busy, is
/// overloaded; a gateway's that couldn't reach the model, unreached; any
/// other refusal, refused, as it was.
fn request_error(error: link_fetch::Error) -> anyhow::Error {
    let said = error.to_string();
    match &error {
        link_fetch::Error::Unreachable { .. } => Unreached(said).into(),
        link_fetch::Error::Later(_) => Overloaded(said).into(),
        link_fetch::Error::Failed(refusal) => match failure(refusal.status) {
            Failure::Overloaded => Overloaded(said).into(),
            // The sandbox couldn't reach or start the model module's bridge.
            Failure::Unreached => Unreached(format!(
                "{said}; lnk module list shows the modules lnk finds"
            ))
            .into(),
            Failure::TooLong | Failure::Refused => error.into(),
        },
        _ => error.into(),
    }
}

/// The contract's kind of a 5xx from the model service: busy, a gateway
/// that couldn't reach it, or a refusal.
fn failure(status: u16) -> Failure {
    match status {
        503 => Failure::Overloaded,
        502 | 504 => Failure::Unreached,
        _ => Failure::Refused,
    }
}
pub async fn get<T: serde::de::DeserializeOwned>(
    client: &link_fetch::Client,
    url: &str,
    action: &str,
) -> anyhow::Result<T> {
    let target = link_secretive::Secret::from(format!("{url}/{action}"));
    Ok(client
        .secret_url(link_fetch::Method::GET, &target, "model action")
        .limit(LIMIT as u64)
        .answer()
        .await?)
}
pub async fn post<T: serde::de::DeserializeOwned>(
    client: &link_fetch::Client,
    url: &str,
    action: &str,
    input: &impl serde::Serialize,
) -> anyhow::Result<T> {
    let target = link_secretive::Secret::from(format!("{url}/{action}"));
    Ok(client
        .secret_url(link_fetch::Method::POST, &target, "model action")
        .json(input)
        .limit(LIMIT as u64)
        .answer()
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn proved_reads_refuse_unknown_fields_and_redirects_without_disclosing_the_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/capability/model", listener.local_addr().unwrap());
        let replies = [
            (
                "200 OK",
                r#"{"ready":true,"loaded":[],"host_path":"private"}"#,
            ),
            ("302 Found", ""),
            ("200 OK", r#"["plain-model"]"#),
        ];
        let serving = tokio::spawn(async move {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                link_sandbox_route::http::read_head(&mut stream)
                    .await
                    .unwrap();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\nLocation: /followed\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = client().unwrap();
        let error = get::<crate::api::Status>(&client, &url, "status")
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("capability"));
        let error = get::<Vec<String>>(&client, &url, "list").await.unwrap_err();
        assert!(error.to_string().contains("redirect"));
        assert!(!error.to_string().contains("capability"));
        assert_eq!(
            get::<Vec<String>>(&client, &url, "list").await.unwrap(),
            ["plain-model"]
        );
        serving.await.unwrap();
    }

    /// Each refusal of a model/chat call keeps what the service said,
    /// sorted into the contract's kinds the fallback acts on, and never
    /// names the route's URL.
    #[tokio::test]
    async fn a_refused_chat_says_why_by_its_kind() {
        let listener = tokio::net::TcpListener::from_std({
            let listener = link_test_kit::net::Held::loopback().listener();
            listener.set_nonblocking(true).unwrap();
            listener
        })
        .unwrap();
        let url = format!(
            "http://{}/capability/model/chat",
            listener.local_addr().unwrap()
        );
        let bridge = "Link's sandbox: the model-action-chat route's bridge couldn't be reached";
        let replies = [
            ("429 Too Many Requests", "Retry-After: 7\r\n", "slow down"),
            ("503 Service Unavailable", "", "busy"),
            ("502 Bad Gateway", "", bridge),
            ("500 Internal Server Error", "", "it broke"),
            (
                "403 Forbidden",
                "",
                "model action failed or its input was refused",
            ),
        ];
        let serving = tokio::spawn(async move {
            for (status, header, body) in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                // The whole request is read before it's answered.
                let (head, mut asked) = link_sandbox_route::http::read_head(&mut stream)
                    .await
                    .unwrap();
                let length = link_sandbox_route::http::RequestHead::parse(&head)
                    .unwrap()
                    .length()
                    .unwrap() as usize;
                let read = asked.len();
                asked.resize(length, 0);
                tokio::io::AsyncReadExt::read_exact(&mut stream, &mut asked[read..])
                    .await
                    .unwrap();
                let response = format!(
                    "HTTP/1.1 {status}\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = client().unwrap();
        let target = link_secretive::Secret::from(url);
        let mut errors = Vec::new();
        for _ in replies {
            let asked = client
                .secret_url(link_fetch::Method::POST, &target, "model/chat")
                .json(&json!({}))
                .stream()
                .await;
            let Err(error) = asked else {
                panic!("a refusal is an error");
            };
            errors.push(request_error(error));
        }
        serving.await.unwrap();
        for error in &errors {
            assert!(!error.to_string().contains("capability"), "{error}");
        }
        assert!(errors[0].is::<Overloaded>(), "{}", errors[0]);
        assert!(errors[0].to_string().contains("slow down"), "{}", errors[0]);
        assert!(errors[0].to_string().contains("7s"), "{}", errors[0]);
        assert!(errors[1].is::<Overloaded>(), "{}", errors[1]);
        assert!(errors[1].to_string().contains("busy"), "{}", errors[1]);
        assert!(errors[2].is::<Unreached>(), "{}", errors[2]);
        assert!(errors[2].to_string().contains(bridge), "{}", errors[2]);
        assert!(
            errors[2].to_string().contains("lnk module list"),
            "{}",
            errors[2]
        );
        for (error, said) in [(&errors[3], "it broke"), (&errors[4], "input was refused")] {
            assert!(
                !error.is::<Overloaded>() && !error.is::<Unreached>(),
                "{error}"
            );
            assert!(error.to_string().contains(said), "{error}");
        }
    }
}
