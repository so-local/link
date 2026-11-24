//! The agent reached and looked at: its sandbox's socket, whether its
//! proxy runs, it answers, it's busy, or its port has a connection.

use super::*;

/// Copies both ways between `client` and `harness` until each side ends.
pub(super) fn pipe_tcp(client: TcpStream, harness: TcpStream) {
    let (Ok(mut client_in), Ok(mut harness_in)) = (client.try_clone(), harness.try_clone()) else {
        return;
    };
    let (mut client_out, mut harness_out) = (client, harness);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_in, &mut harness_out);
        let _ = harness_out.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut harness_in, &mut client_out);
    let _ = client_out.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
}

/// Marks the agent keyed `key` asleep, or not.
pub(super) fn set_mark(key: &str, asleep: bool) {
    let mark = mark_of(key);
    match asleep {
        // Only where the bridge's folder is: never made again once gone.
        true if crate::bridges::dir().is_dir() => {
            if let Some(dir) = mark.parent() {
                let _ = link_files::create_private_dir(dir);
            }
            let _ = std::fs::write(&mark, b"");
        }
        true => {}
        false => {
            let _ = std::fs::remove_file(&mark);
        }
    }
}

/// Forgets that the agent keyed `key` is asleep: it's stopped, or started
/// anew.
pub fn forget(key: &str) {
    set_mark(key, false);
}

/// A connection to the socket at `path`, in a folder the sandbox writes,
/// as the proxy reaches one there: never through a link, and only to a
/// socket of this user's, so a harness can't point the bridge at another
/// socket of the user's.
fn connect_inside(path: &Path) -> std::io::Result<UnixStream> {
    link_files::nofollow::connect(path)
}

/// The sandbox's `in.sock`, of an agent awake. One whose proxy runs is
/// taken for awake (its door opened as it started), but the sandbox makes
/// `in.sock` after its proxy's `out.sock`: not there yet, or there and
/// refusing (a socket is bound before it listens), it's starting, and
/// it's waited for until it answers, as a woken one is.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a starting agent until it answers"
)]
pub(super) fn reach_inside(sleep: &Sleep) -> anyhow::Result<UnixStream> {
    use std::io::ErrorKind::{ConnectionRefused, NotFound};
    let path = sleep.net.join("in.sock");
    let deadline = Instant::now() + WAKING;
    loop {
        match connect_inside(&path) {
            Err(err) if matches!(err.kind(), NotFound | ConnectionRefused) => {}
            reached => return Ok(reached?),
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "it didn't answer within {} s of starting",
            WAKING.as_secs()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether its proxy runs: its `out.sock` takes a connection.
pub(super) fn proxy_runs(net: &Path) -> bool {
    connect_inside(&net.join("out.sock")).is_ok()
}

/// Whether the agent answers: its proxy runs, and its harness answers on
/// its port inside, through `in.sock`; or, handed its port, on it.
pub(super) fn answers(sleep: &Sleep) -> bool {
    if sleep.handoff {
        let Ok(mut harness) = TcpStream::connect(("127.0.0.1", sleep.port)) else {
            return false;
        };
        let _ = harness.set_read_timeout(Some(Duration::from_secs(2)));
        let probe = format!(
            "HEAD / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            sleep.port
        );
        let mut byte = [0u8; 1];
        return harness.write_all(probe.as_bytes()).is_ok()
            && matches!(harness.read(&mut byte), Ok(1));
    }
    if !proxy_runs(&sleep.net) {
        return false;
    }
    let Ok(mut inside) = connect_inside(&sleep.net.join("in.sock")) else {
        return false;
    };
    let _ = inside.set_read_timeout(Some(Duration::from_secs(2)));
    let probe = format!(
        "HEAD / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        sleep.port
    );
    if inside.write_all(&sleep.port.to_be_bytes()).is_err()
        || inside.write_all(probe.as_bytes()).is_err()
    {
        return false;
    }
    let mut byte = [0u8; 1];
    matches!(inside.read(&mut byte), Ok(1))
}

/// Whether it has something to do without a connection: a turn running
/// (`turn.json` in a conversation's folder) or a background command
/// (its record's lock held by its supervisor).
pub(super) fn busy(sleep: &Sleep) -> bool {
    let turn = std::fs::read_dir(&sleep.conversations)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().join("turn.json").exists());
    let running = std::fs::read_dir(&sleep.run)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|x| x == "lock"))
        .any(|entry| {
            std::fs::File::open(entry.path())
                .is_ok_and(|file| matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
        });
    turn || running
}

/// Whether a connection to `port` on this machine is open: its harness
/// is talking to someone.
pub(super) fn connected(port: u16) -> bool {
    if cfg!(target_os = "linux") {
        // Each socket's local address and state (`01`: established).
        return ["/proc/net/tcp", "/proc/net/tcp6"].iter().any(|table| {
            std::fs::read_to_string(table)
                .unwrap_or_default()
                .lines()
                .skip(1)
                .any(|line| {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    let local = fields.get(1).and_then(|a| a.rsplit_once(':'));
                    let at = local.and_then(|(_, p)| u16::from_str_radix(p, 16).ok());
                    at == Some(port) && fields.get(3) == Some(&"01")
                })
        });
    }
    link_secretive::process::command("lsof")
        .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED"])
        .output()
        .is_ok_and(|output| !output.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge reaches the sandbox's own socket, never another of the
    /// user's that a link in the sandbox's folder points at.
    #[test]
    fn a_link_in_the_sandboxs_folder_is_never_followed() {
        let dir = link_test_kit::TempDir::short("idle-inside");
        let elsewhere = dir.join("elsewhere.sock");
        let _theirs = std::os::unix::net::UnixListener::bind(&elsewhere).unwrap();
        let net = dir.join("net");
        std::fs::create_dir_all(&net).unwrap();
        std::os::unix::fs::symlink(&elsewhere, net.join("in.sock")).unwrap();
        assert!(connect_inside(&net.join("in.sock")).is_err());
        assert!(!proxy_runs(&net), "out.sock isn't there");
        std::fs::remove_file(net.join("in.sock")).unwrap();
        let _ours = std::os::unix::net::UnixListener::bind(net.join("in.sock")).unwrap();
        assert!(connect_inside(&net.join("in.sock")).is_ok());
        // Not a socket: refused.
        std::fs::write(net.join("out.sock"), "").unwrap();
        assert!(!proxy_runs(&net));
    }

    /// A connection to an agent whose proxy runs but whose sandbox hasn't
    /// made `in.sock` yet (it's starting), or has bound it and isn't
    /// listening yet, waits for it, never dropped.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn an_agent_starting_is_waited_for() {
        let dir = link_test_kit::TempDir::short("idle-starting");
        let net = dir.join("net");
        std::fs::create_dir_all(&net).unwrap();
        let sleep = Sleep {
            port: 1,
            net: net.clone(),
            after_ms: 0,
            run: dir.join("run"),
            conversations: dir.join("chats"),
            job: Job::Terminal(dir.join("control")),
            handoff: false,
            shared: None,
        };
        // Not there for 300 ms, then bound and refusing for 300 ms, then
        // listening.
        let made = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let bound = bound(&net.join("in.sock"));
            std::thread::sleep(Duration::from_millis(300));
            listen(bound)
        });
        let started = Instant::now();
        assert!(reach_inside(&sleep).is_ok());
        assert!(started.elapsed() >= Duration::from_millis(600));
        let _listener = made.join().unwrap();
    }

    /// A socket bound at `path`, not yet listening: a connection to it is
    /// refused.
    fn bound(path: &Path) -> std::os::fd::OwnedFd {
        use std::os::fd::FromRawFd;
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: socket has no preconditions; its descriptor is owned
        // here alone.
        let fd = unsafe {
            std::os::fd::OwnedFd::from_raw_fd(libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0))
        };
        // SAFETY: an all-zero sockaddr_un is a valid one.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (to, from) in addr.sun_path.iter_mut().zip(path.as_os_str().as_bytes()) {
            *to = *from as libc::c_char;
        }
        // SAFETY: a socket just made and an address of its size.
        let bound = unsafe {
            libc::bind(
                std::os::fd::AsRawFd::as_raw_fd(&fd),
                std::ptr::addr_of!(addr).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
        };
        assert_eq!(bound, 0, "{}", std::io::Error::last_os_error());
        let refused = UnixStream::connect(path).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::ConnectionRefused);
        fd
    }

    /// `bound`, listening.
    fn listen(bound: std::os::fd::OwnedFd) -> std::os::unix::net::UnixListener {
        // SAFETY: a bound socket of ours.
        let listening = unsafe { libc::listen(std::os::fd::AsRawFd::as_raw_fd(&bound), 8) };
        assert_eq!(listening, 0, "{}", std::io::Error::last_os_error());
        std::os::unix::net::UnixListener::from(bound)
    }
}
