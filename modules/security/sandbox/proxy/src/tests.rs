//! The proxy's tests: its parser, its rules, dialing, its relays and its
//! wire, through [`crate::proxy`] as a program's calls reach it.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use link_sandbox_contract::net::{Upstream, forbidden, this_machine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixListener};

use crate::client::*;
use crate::dial::*;
use crate::filter::*;
use crate::http::*;
use crate::listen::*;
use crate::relay::*;
use crate::serve::*;
use link_sandbox_route::connect_inside;
use link_sandbox_route::dial::*;
use link_sandbox_route::io::*;
use link_sandbox_route::resolve::*;

mod dialing;
mod parsing;
mod relaying;
mod rules;
mod watching;

/// This user.
fn me() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

/// An echo server on this machine, allowed only in tests.
async fn echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    addr
}

/// A SOCKS5 server that connects wherever it's asked, and counts.
async fn socks() -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = count.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut greet = [0u8; 3];
                s.read_exact(&mut greet).await.unwrap();
                s.write_all(&[5, 0]).await.unwrap();
                let mut req = [0u8; 10];
                s.read_exact(&mut req).await.unwrap();
                let ip = std::net::Ipv4Addr::new(req[4], req[5], req[6], req[7]);
                let port = u16::from_be_bytes([req[8], req[9]]);
                let mut target = TcpStream::connect((ip, port)).await.unwrap();
                s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut s, &mut target).await;
            });
        }
    });
    (addr, count)
}

async fn through(rules: Filter, request: &str) -> String {
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(proxy(server, rules));
    client.write_all(request.as_bytes()).await.unwrap();
    let mut got = [0u8; 4096];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut got))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&got[..n]).to_string()
}

/// The ClientHello a TLS client sends first, asking for `name`.
fn client_hello(name: &str) -> Vec<u8> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(name.to_string()).unwrap();
    let mut client = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
    let mut hello = Vec::new();
    client.write_tls(&mut hello).unwrap();
    hello
}
