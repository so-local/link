//! A controlled public destination behind a real SOCKS route. Egress
//! policy tests need no reachable internet service or external resolver.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

// The proxy resolves names locally before asking a SOCKS exit to dial an IP.
// These addresses of IANA's own block are handled only by this loopback stand-in.
pub const RESOLVE: &str = "example.com=192.0.32.10,nowhere.invalid=192.0.32.11";

pub struct Public {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<Vec<String>>>,
}

impl Public {
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            while !done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => requests.push(answer(stream)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accepting the fixture's SOCKS connection: {e}"),
                }
            }
            requests
        });
        Self {
            address,
            stop,
            server: Some(server),
        }
    }

    pub fn url(&self) -> String {
        format!("socks5://{}", self.address)
    }

    pub fn finish(mut self) -> Vec<String> {
        self.stop.store(true, Ordering::Release);
        self.server.take().unwrap().join().unwrap()
    }
}

impl Drop for Public {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

fn answer(mut stream: TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut greeting = [0; 3];
    stream.read_exact(&mut greeting).unwrap();
    assert_eq!(greeting, [5, 1, 0]);
    stream.write_all(&[5, 0]).unwrap();
    let mut head = [0; 4];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(head, [5, 1, 0, 1]);
    let mut address = [0; 4];
    stream.read_exact(&mut address).unwrap();
    let mut port = [0; 2];
    stream.read_exact(&mut port).unwrap();
    let (host, result) = match address {
        [192, 0, 32, 10] => ("example.com", 0),
        [192, 0, 32, 11] => ("nowhere.invalid", 4),
        other => panic!("unexpected egress to {other:?}"),
    };
    stream
        .write_all(&[5, result, 0, 1, 0, 0, 0, 0, 0, 0])
        .unwrap();
    format!("{host}:{}", u16::from_be_bytes(port))
}
