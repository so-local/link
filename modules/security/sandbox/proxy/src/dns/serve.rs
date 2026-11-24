//! The resolver a sandbox asks, over UDP and TCP.

use super::*;

/// The most UDP queries answered at once, past which they're dropped.
pub(super) const MAX_UDP_QUERIES: usize = 256;

/// Answers queries on `socket` (the sandbox's `127.0.0.1:53`, UDP) by
/// `rules`, asking `servers`.
#[expect(
    clippy::disallowed_methods,
    reason = "rests after a failed read rather than spin"
)]
pub async fn serve_udp(socket: tokio::net::UdpSocket, rules: Filter, servers: Vec<SocketAddr>) {
    let socket = std::sync::Arc::new(socket);
    let servers = std::sync::Arc::new(servers);
    // Lookups that wait on someone else take turns ([`MAX_LOOKUPS`]), so
    // slow ones queue here, short of the sandbox's own instant answers.
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_UDP_QUERIES));
    let mut buf = vec![0u8; MAX_MESSAGE];
    loop {
        let Ok((n, from)) = socket.recv_from(&mut buf).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        // More than so many at once are dropped, as a busy server's are.
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let msg = buf[..n].to_vec();
        let (socket, servers) = (socket.clone(), servers.clone());
        tokio::spawn(async move {
            if let Some(answer) = respond(&msg, rules, &servers, true).await {
                let _ = socket.send_to(&answer, from).await;
            }
            drop(slot);
        });
    }
}

/// Answers queries on `listener` (the sandbox's `127.0.0.1:53`, TCP),
/// each message after its length, by `rules`, asking `servers`.
pub async fn serve_tcp(listener: tokio::net::TcpListener, rules: Filter, servers: Vec<SocketAddr>) {
    let servers = std::sync::Arc::new(servers);
    crate::listen::each(listener, move |mut stream: tokio::net::TcpStream| {
        let servers = servers.clone();
        async move {
            loop {
                let next = async {
                    let len = stream.read_u16().await? as usize;
                    anyhow::ensure!(len <= MAX_MESSAGE, "too long");
                    let mut msg = vec![0u8; len];
                    stream.read_exact(&mut msg).await?;
                    Ok::<_, anyhow::Error>(msg)
                };
                let Ok(Ok(msg)) = tokio::time::timeout(Duration::from_secs(10), next).await else {
                    return;
                };
                let Some(answer) = respond(&msg, rules, &servers, false).await else {
                    return;
                };
                let sent = async {
                    stream
                        .write_all(&(answer.len() as u16).to_be_bytes())
                        .await?;
                    stream.write_all(&answer).await
                };
                if sent.await.is_err() {
                    return;
                }
            }
        }
    })
    .await
}
