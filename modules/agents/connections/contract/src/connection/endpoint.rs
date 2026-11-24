use super::*;

/// The longest wait for a harness's answer, token to token.
pub(super) const ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// The longest wait to connect to the harness's endpoint.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a message says beside its text: who it's from, with its hops,
/// the tools its conversation is narrowed to, the model it runs on, and
/// a subagent's parent conversation.
#[derive(Clone, Copy, Default)]
pub(super) struct Sent<'a> {
    pub(super) receipts: bool,
    pub(super) delivery: Option<&'a link_interactions_contract::TransportEventId>,
    pub(super) occurrence: Option<&'a link_interactions_contract::ScheduledOccurrence>,
    pub(super) parent: Option<&'a str>,
    pub(super) from: Option<Speaker<'a>>,
    pub(super) tools: Option<&'a [String]>,
    pub(super) model: Option<&'a str>,
    /// The most its message stands ([`crate::acp::Said::standing`]): a
    /// firing's, which its schedule or subagent recorded.
    pub(super) standing: Option<link_agent_identity::access::Standing>,
}

/// The body of each request on an endpoint's connection.
type Body = http_body_util::Full<hyper::body::Bytes>;

/// A WebSocket to the harness's ACP ([`crate::acp::ACP_PATH`]), on a
/// connection of its own to this machine's loopback (never through a
/// proxy): proved first, with [`Bridge::proof`], and opened with the
/// agent's headers and key on that same connection, so whatever else
/// took the port (while the harness started, or restarted) gets
/// neither. A connection closed before it answers is tried again, as a
/// refused one is: a harness starting again lets its port's connections
/// go until it's up.
pub(super) async fn upgraded(bridge: &Bridge) -> anyhow::Result<crate::acp::Socket> {
    use tokio_tungstenite::tungstenite::{handshake::client::generate_key, protocol::Role};
    let (mut sender, endpoint) = reached(bridge, true).await?;
    let request = headed(hyper::Request::get(crate::acp::ACP_PATH), bridge, &endpoint)
        .header(hyper::header::CONNECTION, "upgrade")
        .header(hyper::header::UPGRADE, "websocket")
        .header(hyper::header::SEC_WEBSOCKET_VERSION, "13")
        .header(hyper::header::SEC_WEBSOCKET_KEY, generate_key())
        .body(Body::default())?;
    sender
        .ready()
        .await
        .map_err(|err| anyhow::anyhow!("reaching the agent: {err}"))?;
    let mut response = tokio::time::timeout(CONNECT_TIMEOUT, sender.send_request(request))
        .await
        .map_err(|_| anyhow::anyhow!("the agent didn't answer"))?
        .map_err(|err| anyhow::anyhow!("reaching the agent: {err}"))?;
    let status = response.status();
    if status != hyper::StatusCode::SWITCHING_PROTOCOLS {
        let body = read_all(&mut response).await.unwrap_or_default();
        let why = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_default();
        anyhow::bail!("the agent answered {status} {}", link_format::clean(&why));
    }
    let upgraded = hyper::upgrade::on(response)
        .await
        .map_err(|err| anyhow::anyhow!("reaching the agent: {err}"))?;
    Ok(tokio_tungstenite::WebSocketStream::from_raw_socket(
        hyper_util::rt::TokioIo::new(upgraded),
        Role::Client,
        None,
    )
    .await)
}

/// A connection to the harness's endpoint, on this machine's loopback
/// (never through a proxy): with [`Bridge::proof`], one that first
/// proved it's the harness ([`prove`]), so the key and the message go on
/// that same connection and whatever else took the port (while the
/// harness started, or restarted) gets neither. A connection closed
/// before it answers is tried again, as a refused one is: a harness
/// starting again lets its port's connections go until it's up. With
/// `upgrades`, it may be upgraded to another protocol.
#[expect(
    clippy::disallowed_methods,
    reason = "retries while what holds the port starts"
)]
async fn reached(
    bridge: &Bridge,
    upgrades: bool,
) -> anyhow::Result<(hyper::client::conn::http1::SendRequest<Body>, Endpoint)> {
    let endpoint = endpoint_parts(&bridge.endpoint)?;
    let (authority, port) = (&endpoint.authority, endpoint.reach.to_string());
    let started = std::time::Instant::now();
    let sender = loop {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, connect_starting(&endpoint.reach))
            .await
            .map_err(|_| anyhow::anyhow!("reaching the agent: no answer on {port}"))?
            .map_err(|err| anyhow::anyhow!("reaching the agent: {err}"))?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Body>(hyper_util::rt::TokioIo::new(stream))
                .await
                .map_err(|err| anyhow::anyhow!("reaching the agent: {err}"))?;
        // Driven while the message is answered; it ends when `sender` and
        // the answer are dropped.
        tokio::spawn(async move {
            let _ = match upgrades {
                true => connection.with_upgrades().await,
                false => connection.await,
            };
        });
        if !bridge.proof {
            break sender;
        }
        match prove(&mut sender, authority, bridge, &port).await? {
            Asked::Proved => break sender,
            // What held the port let the connection go unanswered: a
            // sandbox's proxy with no harness behind it yet, or a harness
            // ending as another starts. The next connection may reach it.
            Asked::Closed if started.elapsed() < STARTING => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Asked::Closed => anyhow::bail!(
                "what answers on the agent's {port} couldn't prove it's its harness: each \
                 connection closed unanswered, and nothing was sent to it"
            ),
        }
    };
    Ok((sender, endpoint))
}

/// `request` with what every request to the endpoint carries: its host,
/// the agent's name and ID, and its key.
fn headed(
    mut request: hyper::http::request::Builder,
    bridge: &Bridge,
    endpoint: &Endpoint,
) -> hyper::http::request::Builder {
    request = request.header(hyper::header::HOST, &endpoint.authority);
    if !bridge.agent.is_empty() {
        request = request.header(AGENT_HEADER, &bridge.agent);
    }
    let identity = bridge.identity();
    if !identity.agent.is_empty() {
        request = request.header(AGENT_ID_HEADER, &identity.agent);
    }
    if !bridge.key.is_empty() {
        let bearer = bridge.key.expose_map(|key| format!("Bearer {key}"));
        request = request.header(hyper::header::AUTHORIZATION, bearer.expose().as_str());
    }
    request
}

/// What a challenge on a connection came to, when it wasn't refused.
enum Asked {
    /// The harness proved it holds the key.
    Proved,
    /// The connection closed before any answer: nothing was proved, and
    /// nothing but the challenge was sent.
    Closed,
}

/// Asks what answers on `sender`'s connection to prove it holds the
/// bridge's key, for its agent: a random challenge, answered with
/// [`crate::harness::endpoint_proof`]. An error, before anything else is
/// sent, when an answer comes without it.
async fn prove(
    sender: &mut hyper::client::conn::http1::SendRequest<Body>,
    authority: &str,
    bridge: &Bridge,
    port: &str,
) -> anyhow::Result<Asked> {
    let key = &bridge.key;
    let challenge = link_files::random_token()?;
    let mut request = hyper::Request::get(crate::harness::PROOF_PATH)
        .header(hyper::header::HOST, authority)
        .header(crate::harness::CHALLENGE_HEADER, &challenge);
    if !bridge.agent.is_empty() {
        request = request.header(AGENT_HEADER, &bridge.agent);
    }
    let identity = bridge.identity();
    if !identity.agent.is_empty() {
        request = request.header(AGENT_ID_HEADER, &identity.agent);
    }
    let request = request.body(Body::default())?;
    let asked = async {
        let Ok(mut response) = sender.send_request(request).await else {
            return anyhow::Ok(None);
        };
        let proof = response
            .headers()
            .get(crate::harness::PROOF_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        // Read whole, so the connection is ready for the message.
        read_all(&mut response).await?;
        anyhow::Ok(Some(proof.filter(|_| response.status().is_success())))
    };
    // An agent asleep is woken by its door, which holds this connection
    // until its harness answers or `WAKING` passes, then closes it: its
    // answer is waited for as long, never taken for an impostor's sooner.
    let proof = match tokio::time::timeout(crate::constants::WAKING, asked).await {
        Ok(Ok(None)) => return Ok(Asked::Closed),
        answered => answered.ok().and_then(Result::ok).flatten().flatten(),
    };
    anyhow::ensure!(
        proof.is_some_and(|proof| crate::harness::proof_matches(key, &challenge, &proof)),
        "what answers on the agent's {port} isn't its harness (it couldn't prove it \
         holds the agent's key): nothing was sent to it"
    );
    Ok(Asked::Proved)
}

/// A connection to the agent's endpoint, tried again while it's
/// refused: an agent woken from idle is starting, its port let go by the
/// bridge a moment before its harness takes it. The caller bounds how
/// long ([`CONNECT_TIMEOUT`]).
#[expect(
    clippy::disallowed_methods,
    reason = "retries while what holds the port starts"
)]
async fn connect_starting(reach: &Reach) -> std::io::Result<Box<dyn Stream>> {
    use tokio::io::AsyncWriteExt;
    let started = std::time::Instant::now();
    loop {
        let connected: std::io::Result<Box<dyn Stream>> = match reach {
            Reach::Port(port) => tokio::net::TcpStream::connect(("127.0.0.1", *port))
                .await
                .map(|s| Box::new(s) as Box<dyn Stream>),
            Reach::Socket { path, port } => match connect_socket(path).await {
                // A sandbox's `in.sock` takes the port inside first.
                Ok(mut socket) => match port {
                    Some(port) => socket.write_u16(*port).await.map(|()| socket),
                    None => Ok(socket),
                }
                .map(|s| Box::new(s) as Box<dyn Stream>),
                Err(err) => Err(err),
            },
        };
        match connected {
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) && started.elapsed() < STARTING =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            connected => return connected,
        }
    }
}

/// A connection to the endpoint's socket at `path`, as that socket
/// itself: never through a link put in its place, only to a socket owned
/// by its folder's owner, and only when what listens on it runs as that
/// owner (its peer's credentials). Whoever writes the socket's folder (a
/// sandbox's own, say) could otherwise point a connection, and the key
/// and message after it, at any socket the caller may reach. Linux opens
/// the path without following it and connects through the open file, so
/// nothing swapped in between is reached.
async fn connect_socket(path: &std::path::Path) -> std::io::Result<tokio::net::UnixStream> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let not_its = || {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} isn't the endpoint's socket", path.display()),
        )
    };
    let folder = path.parent().ok_or_else(not_its)?;
    let owner = std::fs::metadata(folder)?.uid();
    #[cfg(target_os = "linux")]
    let stream = {
        use std::os::fd::{AsRawFd, FromRawFd};
        let c_path =
            std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| not_its())?;
        // SAFETY: a NUL-ended path; the descriptor it gives is owned below.
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` was just opened, and is this file's alone.
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        if !meta.file_type().is_socket() || meta.uid() != owner {
            return Err(not_its());
        }
        tokio::net::UnixStream::connect(format!("/proc/self/fd/{}", file.as_raw_fd())).await?
    };
    #[cfg(not(target_os = "linux"))]
    let stream = {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.file_type().is_socket() || meta.uid() != owner {
            return Err(not_its());
        }
        tokio::net::UnixStream::connect(path).await?
    };
    if stream.peer_cred()?.uid() != owner {
        return Err(not_its());
    }
    Ok(stream)
}

/// A connection to an endpoint, over TCP or a Unix socket.
trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for S {}

/// How long a refused connection to an agent, or one closed unanswered,
/// is tried again: one woken from idle, or started again, starts within
/// it.
const STARTING: std::time::Duration = std::time::Duration::from_secs(5);

/// The longest answer read whole (an error's, or one not streamed), and
/// the longest streamed answer, or line of one.
pub(super) const MAX_WHOLE: usize = 16 << 20;

/// The rest of `response`'s body, at most [`MAX_WHOLE`] bytes.
pub(super) async fn read_all(
    response: &mut hyper::Response<hyper::body::Incoming>,
) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(response).await? {
        anyhow::ensure!(
            body.len() + chunk.len() <= MAX_WHOLE,
            "the agent's answer is over {} MB",
            MAX_WHOLE >> 20
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The next piece of `response`'s body, waiting at most
/// [`ANSWER_TIMEOUT`] for it; none at its end.
pub(super) async fn next_chunk(
    response: &mut hyper::Response<hyper::body::Incoming>,
) -> anyhow::Result<Option<hyper::body::Bytes>> {
    use http_body_util::BodyExt;
    loop {
        let frame = tokio::time::timeout(ANSWER_TIMEOUT, response.body_mut().frame())
            .await
            .map_err(|_| anyhow::anyhow!("the agent stopped answering"))?;
        match frame {
            None => return Ok(None),
            Some(frame) => {
                if let Ok(data) = frame?.into_data() {
                    return Ok(Some(data));
                }
            }
        }
    }
}

/// An endpoint reached over a Unix socket in place of a port on this
/// machine's loopback: `unix:<path>`, the socket at `<path>` speaking
/// HTTP itself, or `unix:<path>:<port>`, a sandbox's `in.sock` (in its
/// proxy's folder), told `<port>`, the endpoint's port inside, first
/// (two bytes), as its proxy would. Requests go to the same paths
/// (`/v1/chat/completions`), and no port on this machine's loopback is
/// used. The socket is reached as itself, never through a link, and
/// only when it and what listens on it are its folder's owner's
/// (`connect_socket`).
pub const UNIX_ENDPOINT: &str = "unix:";

/// Where an endpoint is reached.
enum Reach {
    /// This machine's loopback, at this port.
    Port(u16),
    /// A Unix socket, told the port inside first when there is one.
    Socket {
        path: std::path::PathBuf,
        port: Option<u16>,
    },
}

impl std::fmt::Display for Reach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reach::Port(port) => write!(f, "port {port}"),
            Reach::Socket { path, port: None } => write!(f, "socket {}", path.display()),
            Reach::Socket {
                path,
                port: Some(port),
            } => write!(f, "port {port}, through {}", path.display()),
        }
    }
}

/// An endpoint, from its address: where it's reached, and its
/// `host:port`.
struct Endpoint {
    reach: Reach,
    authority: String,
}

/// An endpoint's parts, from its address
/// ([`crate::harness::endpoint_port`], or [`UNIX_ENDPOINT`]'s).
fn endpoint_parts(url: &str) -> anyhow::Result<Endpoint> {
    if let Some(rest) = url.strip_prefix(UNIX_ENDPOINT) {
        let (path, port) = match rest.rsplit_once(':') {
            Some((path, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                let port: u16 = port
                    .parse()
                    .map_err(|_| anyhow::anyhow!("{url:?} has no port"))?;
                anyhow::ensure!(port >= 1024, "{url:?} isn't an unprivileged port");
                (path, Some(port))
            }
            _ => (rest, None),
        };
        anyhow::ensure!(
            path.starts_with('/') && !path.chars().any(char::is_control),
            "{url:?} isn't a socket's absolute path (unix:/<path>[:<port>])"
        );
        let authority = port.map_or("localhost".to_string(), |port| format!("127.0.0.1:{port}"));
        return Ok(Endpoint {
            reach: Reach::Socket {
                path: path.into(),
                port,
            },
            authority,
        });
    }
    let port = crate::harness::endpoint_port(url)?;
    let rest = url.strip_prefix("http://").unwrap_or(url);
    let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
    Ok(Endpoint {
        reach: Reach::Port(port),
        authority: authority.to_string(),
    })
}

/// Waits, up to a minute, until the harness's endpoint answers HTTP (a
/// sandbox's port may take connections before the harness does), until
/// it has once in this process: a connection hands over what came while the
/// bridge was off as soon as it starts, often before the harness
/// answers, and a bridge asks for each thread at once.
#[expect(
    clippy::disallowed_methods,
    reason = "polls until the endpoint answers"
)]
pub(super) async fn started(bridge: &Bridge) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    static UP: AtomicBool = AtomicBool::new(false);
    if UP.load(Ordering::Relaxed) {
        return;
    }
    let Ok(endpoint) = endpoint_parts(&bridge.endpoint) else {
        return;
    };
    let answers = || async {
        let mut stream = connect_starting(&endpoint.reach).await?;
        stream.write_all(b"HEAD / HTTP/1.0\r\n\r\n").await?;
        let mut byte = [0u8; 1];
        stream.read(&mut byte).await
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        let answered = tokio::time::timeout(Duration::from_secs(2), answers()).await;
        if matches!(answered, Ok(Ok(1))) {
            UP.store(true, Ordering::Relaxed);
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What one connection to a stand-in endpoint carried, request by
    /// request: each one's head and body.
    type Carried = std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>;

    /// How a stand-in endpoint answers the challenge.
    #[derive(Clone, Copy)]
    enum Answer {
        /// 200 to everything, with no proof: a program that took the port.
        Anything,
        /// A proof made with another key.
        OtherKey,
        /// The proof, with the key: the harness.
        Harness,
    }

    /// A stand-in endpoint on a port of its own, answering `answer`, and
    /// what each connection to it carried. Its ACP's WebSocket opens.
    async fn stand_in(answer: Answer) -> (u16, Carried) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let carried: Carried = Default::default();
        let all = carried.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(stand_in_carries(stream, answer, all.clone()));
            }
        });
        (port, carried)
    }

    /// [`stand_in`] on a Unix socket at `path`, as a sandbox's `in.sock`
    /// is when `port` (two bytes, which it checks) comes first.
    async fn stand_in_socket(answer: Answer, path: &std::path::Path, port: Option<u16>) -> Carried {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::UnixListener::bind(path).unwrap();
        let carried: Carried = Default::default();
        let all = carried.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                if let Some(port) = port
                    && stream.read_u16().await.ok() != Some(port)
                {
                    continue;
                }
                tokio::spawn(stand_in_carries(stream, answer, all.clone()));
            }
        });
        carried
    }

    /// What a stand-in endpoint does with one connection: answers each
    /// request, kept in `carried`.
    async fn stand_in_carries<S>(stream: S, answer: Answer, carried: Carried)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
        let n = {
            let mut carried = carried.lock().unwrap();
            carried.push(Vec::new());
            carried.len() - 1
        };
        let mut stream = tokio::io::BufReader::new(stream);
        loop {
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let header = |name: &str| {
                head.lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                    })
                    .unwrap_or_default()
            };
            let length: usize = header("content-length").parse().unwrap_or(0);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await.unwrap();
            let request = format!("{head}{}", String::from_utf8_lossy(&body));
            carried.lock().unwrap()[n].push(request);
            let answer = if head.starts_with("GET /_link/proof") {
                let challenge = header("link-challenge");
                let proof = match answer {
                    Answer::Anything => String::new(),
                    Answer::OtherKey => {
                        let proof = crate::harness::endpoint_proof(
                            &link_secretive::Secret::from("other".to_string()),
                            &challenge,
                        );
                        format!("Link-Proof: {proof}\r\n")
                    }
                    Answer::Harness => {
                        let proof = crate::harness::endpoint_proof(
                            &link_secretive::Secret::from("the-key".to_string()),
                            &challenge,
                        );
                        format!("Link-Proof: {proof}\r\n")
                    }
                };
                format!("HTTP/1.1 200 OK\r\n{proof}Content-Length: 0\r\n\r\n")
            } else {
                // ACP's WebSocket, opened: what it then carries is the
                // ACP client's to test.
                let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(
                    header("sec-websocket-key").as_bytes(),
                );
                let _ = stream
                    .get_mut()
                    .write_all(
                        format!(
                            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                             Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await;
                return;
            };
            if stream.get_mut().write_all(answer.as_bytes()).await.is_err() {
                return;
            }
        }
    }

    /// [`stand_in`] that first closes `closed` connections unanswered,
    /// as a sandbox's proxy does while its harness starts again.
    async fn stand_in_after(closed: usize, answer: Answer) -> (u16, Carried) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let carried: Carried = Default::default();
        let all = carried.clone();
        tokio::spawn(async move {
            for _ in 0..closed {
                drop(listener.accept().await);
            }
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(stand_in_carries(stream, answer, all.clone()));
            }
        });
        (port, carried)
    }

    fn bridge_to(port: u16, proof: bool) -> Bridge {
        Bridge {
            agent: "main".into(),
            endpoint: format!("http://127.0.0.1:{port}/acp"),
            key: "the-key".to_string().into(),
            proof,
            ..Bridge::default()
        }
    }

    /// Whatever else took the agent's port, answering anything, or with a
    /// proof made with another key, gets neither the key nor the message.
    #[tokio::test]
    async fn a_stand_in_that_cant_prove_gets_neither_the_key_nor_the_message() {
        for answer in [Answer::Anything, Answer::OtherKey] {
            let (port, carried) = stand_in(answer).await;
            let (progress, _) = tokio::sync::watch::channel(String::new());
            let err = ask(
                &bridge_to(port, true),
                "telegram:42",
                "my secret plans",
                &progress,
            )
            .await
            .unwrap_err();
            assert!(format!("{err:#}").contains("isn't its harness"), "{err:#}");
            let carried = format!("{:?}", carried.lock().unwrap());
            assert!(carried.contains("/_link/proof"), "{carried}");
            for never in ["the-key", "my secret plans", "uthorization", "telegram:42"] {
                assert!(!carried.contains(never), "{never} reached it: {carried}");
            }
        }
    }

    /// The harness proves it holds the key, and the key and its ACP's
    /// WebSocket go on the connection that proved it. Without `proof` (an
    /// adapter that can't), the WebSocket opens as it always did.
    #[tokio::test]
    async fn the_harness_proves_it_and_gets_the_message_on_that_connection() {
        let (port, carried) = stand_in(Answer::Harness).await;
        upgraded(&bridge_to(port, true)).await.unwrap();
        let carried = carried.lock().unwrap().clone();
        let proved: Vec<&Vec<String>> = carried
            .iter()
            .filter(|c| c.iter().any(|r| r.starts_with("GET /_link/proof")))
            .collect();
        assert_eq!(proved.len(), 1, "{carried:?}");
        assert_eq!(proved[0].len(), 2, "{carried:?}");
        let opened = &proved[0][1];
        assert!(opened.starts_with("GET /acp"), "{opened}");
        assert!(opened.contains("authorization: Bearer the-key"), "{opened}");
        // Each names its agent, the proof too.
        for request in proved[0] {
            assert!(request.contains("link-agent: main"), "{request}");
        }

        let (port, carried) = stand_in(Answer::Anything).await;
        upgraded(&bridge_to(port, false)).await.unwrap();
        let carried = format!("{:?}", carried.lock().unwrap());
        assert!(!carried.contains("/_link/proof"), "{carried}");
        assert!(carried.contains("Bearer the-key"), "{carried}");
    }

    /// A harness starting again closes connections unanswered until it's
    /// up; the message waits for it, and goes on the connection that
    /// proved it, never "isn't its harness".
    #[tokio::test]
    async fn a_connection_closed_unanswered_is_tried_again() {
        let (port, carried) = stand_in_after(2, Answer::Harness).await;
        upgraded(&bridge_to(port, true)).await.unwrap();
        // Another test's probe of a port it once held can reach this one.
        let carried = carried.lock().unwrap().clone();
        let proved: Vec<&Vec<String>> = carried
            .iter()
            .filter(|c| c.iter().any(|r| r.starts_with("GET /_link/proof")))
            .collect();
        assert_eq!(proved.len(), 1, "{carried:?}");
        assert!(proved[0][1].contains("Bearer the-key"), "{carried:?}");
    }

    /// An endpoint over a Unix socket (`unix:`): the proof and the ACP's
    /// WebSocket go on it, to a socket that speaks HTTP itself, and to a
    /// sandbox's `in.sock`, told the port inside first. What can't prove
    /// it's the harness there gets neither the key nor the message.
    #[tokio::test]
    async fn an_endpoint_on_a_unix_socket_is_asked_after_its_proof() {
        let dir = link_test_kit::TempDir::new("unix-endpoint");
        let (progress, _) = tokio::sync::watch::channel(String::new());
        for (name, port) in [("harness.sock", None), ("in.sock", Some(18800))] {
            let path = dir.join(name);
            let carried = stand_in_socket(Answer::Harness, &path, port).await;
            let endpoint = match port {
                None => format!("unix:{}", path.display()),
                Some(port) => format!("unix:{}:{port}", path.display()),
            };
            let bridge = Bridge {
                endpoint,
                ..bridge_to(0, true)
            };
            upgraded(&bridge).await.unwrap();
            let carried = carried.lock().unwrap().clone();
            let proved: Vec<&Vec<String>> = carried
                .iter()
                .filter(|c| c.iter().any(|r| r.starts_with("GET /_link/proof")))
                .collect();
            assert_eq!(proved.len(), 1, "{carried:?}");
            let opened = &proved[0][1];
            assert!(opened.starts_with("GET /acp"), "{opened}");
            assert!(opened.contains("authorization: Bearer the-key"), "{opened}");
            let host = match port {
                None => "host: localhost".to_string(),
                Some(port) => format!("host: 127.0.0.1:{port}"),
            };
            assert!(opened.contains(&host), "{opened}");
        }

        let path = dir.join("other.sock");
        let carried = stand_in_socket(Answer::OtherKey, &path, None).await;
        let bridge = Bridge {
            endpoint: format!("unix:{}", path.display()),
            ..bridge_to(0, true)
        };
        let err = ask(&bridge, "telegram:42", "my secret plans", &progress)
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("isn't its harness"), "{err:#}");
        let carried = format!("{:?}", carried.lock().unwrap());
        for never in ["the-key", "my secret plans"] {
            assert!(!carried.contains(never), "{never} reached it: {carried}");
        }

        for bad in [
            "unix:",
            "unix:relative.sock",
            "unix:/a\nb.sock",
            "unix:/x/in.sock:80",
            "unix:/x/in.sock:99999",
        ] {
            assert!(endpoint_parts(bad).is_err(), "{bad:?}");
        }
    }

    /// An endpoint's socket is reached as itself, and only when what
    /// listens on it runs as its owner, who owns its folder too: a link
    /// put in its place (by whoever writes that folder), a socket of
    /// another user's there, or one whose listener is someone else, is
    /// refused before anything is sent, so nothing else is handed the
    /// key or the message.
    #[tokio::test]
    async fn an_endpoint_socket_swapped_for_another_gets_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("unix-swapped");
        let ask_at = |path: &std::path::Path| Bridge {
            endpoint: format!("unix:{}:18800", path.display()),
            ..bridge_to(0, true)
        };
        // Another socket on the machine, a harness that would prove.
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let target = elsewhere.join("in.sock");
        let carried = stand_in_socket(Answer::Harness, &target, Some(18800)).await;
        let folder = dir.join("unit");
        std::fs::create_dir(&folder).unwrap();
        let swapped = folder.join("in.sock");
        std::os::unix::fs::symlink(&target, &swapped).unwrap();
        let err = upgraded(&ask_at(&swapped)).await.unwrap_err();
        assert!(format!("{err:#}").contains("isn't"), "{err:#}");
        assert!(
            carried.lock().unwrap().is_empty(),
            "a link is never followed"
        );
        // The real one, there, is asked.
        std::fs::remove_file(&swapped).unwrap();
        let carried = stand_in_socket(Answer::Harness, &swapped, Some(18800)).await;
        upgraded(&ask_at(&swapped)).await.unwrap();
        assert!(!carried.lock().unwrap().is_empty());

        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } != 0 {
            return;
        }
        // Its folder another user's, the socket not: refused.
        let theirs = dir.join("theirs");
        std::fs::create_dir(&theirs).unwrap();
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = theirs.join("in.sock");
        let carried = stand_in_socket(Answer::Harness, &path, Some(18800)).await;
        std::os::unix::fs::chown(&theirs, Some(65534), Some(65534)).unwrap();
        assert!(upgraded(&ask_at(&path)).await.is_err());
        // Both that user's, but what listens isn't: refused too.
        std::os::unix::fs::chown(&path, Some(65534), Some(65534)).unwrap();
        let err = upgraded(&ask_at(&path)).await.unwrap_err();
        assert!(format!("{err:#}").contains("isn't"), "{err:#}");
        assert!(carried.lock().unwrap().is_empty(), "nothing reached it");
    }
}
