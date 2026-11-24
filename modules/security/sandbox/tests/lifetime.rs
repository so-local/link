//! A proxy closes its run's children before releasing its listener.

use std::io::{BufRead, Read};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FORK: &str = r#"
import os, signal, socket, sys, time
s = socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=3)
r, w = os.pipe()
if os.fork() == 0:
    os.close(r)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    s.sendall((str(os.getpid()) + '\n').encode())
    os.write(w, b'y')
    os.close(w)
    while True: time.sleep(1)
os.close(w)
os.read(r, 1)
s.close()
if sys.argv[2] == 'wait': time.sleep(20)
"#;

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_programs_remaining_children_end_with_its_proxy_run() {
    for mode in ["exit", "wait"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut proxy = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
            .args([
                "proxy",
                "--listen",
                "0",
                "--upstream",
                "direct",
                "--",
                "/usr/bin/python3",
                "-c",
                FORK,
                &port.to_string(),
                mode,
            ])
            .env("LNK_SERVICES", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut connection = loop {
            if let Ok((connection, _)) = listener.accept() {
                break connection;
            }
            if Instant::now() >= until {
                let _ = proxy.kill();
                let _ = proxy.wait();
                panic!("the program didn't connect");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        connection
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut pid = String::new();
        std::io::BufReader::new(&mut connection)
            .read_line(&mut pid)
            .unwrap();
        let remaining: i32 = pid.trim().parse().unwrap();
        assert!(remaining > 1);
        if mode == "wait" {
            // SAFETY: the one proxy this test started, never a group.
            unsafe { libc::kill(proxy.id() as i32, libc::SIGTERM) };
        }
        let until = Instant::now() + Duration::from_secs(5);
        while proxy.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                let _ = proxy.kill();
                let _ = proxy.wait();
                unsafe { libc::kill(remaining, libc::SIGKILL) };
                panic!("the proxy didn't end");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut byte = [0];
        let ended = connection.read(&mut byte);
        // Clean up the pre-fix failure too; a regression leaves no child.
        unsafe { libc::kill(remaining, libc::SIGKILL) };
        assert!(
            matches!(ended, Ok(0)),
            "{mode}: the child outlived its run: {ended:?}"
        );
    }
}
