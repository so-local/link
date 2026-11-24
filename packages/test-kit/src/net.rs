//! Ports a test serves on, and what it reads of a connection another
//! process holds.

/// A listener on this machine's loopback, port of the system's picking,
/// that the test holds from the moment the port is chosen until the
/// server it starts serves on it: the server is given the listener, never
/// a port found free and bound later, which another program can take in
/// between.
#[derive(Debug)]
pub struct Held(std::net::TcpListener);

impl Held {
    /// A new one, bound to `127.0.0.1:0`.
    pub fn loopback() -> Held {
        Held(
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap_or_else(|err| panic!("binding a loopback port: {err}")),
        )
    }

    /// The port it holds.
    pub fn port(&self) -> u16 {
        self.address().port()
    }

    /// The address it holds, `127.0.0.1:<port>`.
    pub fn address(&self) -> std::net::SocketAddr {
        self.0.local_addr().unwrap()
    }

    /// The listener, for a server in the test's own process.
    pub fn listener(self) -> std::net::TcpListener {
        self.0
    }

    /// Hands the listener to `command`'s child: open there across its
    /// exec, its descriptor's number in the variable `variable` names,
    /// closed on exec in this process and every other child. The child
    /// serves on it as it is. Called after the command's environment is
    /// set, as clearing that drops the variable.
    #[cfg(unix)]
    pub fn hand_to(self, command: &mut std::process::Command, variable: &str) {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let listener = self.0;
        command.env(variable, listener.as_raw_fd().to_string());
        // SAFETY: in the child, between fork and exec: only fcntl, on a
        // descriptor the closure owns, so it's open while `command` is.
        unsafe {
            command.pre_exec(move || {
                let fd = listener.as_raw_fd();
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(windows)]
compile_error!("link-test-kit: no Windows implementation of net::Held::hand_to, which EO1 adds");

/// Whether the connection a process of group `group` holds, from local
/// port `port` to `peer`, has `TCP_NODELAY`: on macOS as `lsof` shows its
/// flags, on Linux read from the socket itself (`pidfd_getfd`, as the
/// harness's parent may). None where that can't be read.
#[cfg(target_os = "macos")]
pub fn nodelay_of(group: u32, port: u16, peer: u16) -> Option<bool> {
    let output = link_secretive::process::command("/usr/sbin/lsof")
        .args(["-a", "-n", "-P", "-T", "f", "-g", &group.to_string()])
        .arg(format!("-iTCP:{peer}"))
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let listed = String::from_utf8_lossy(&output.stdout);
    let line = listed.lines().find(|line| {
        line.contains(&format!(":{port}->")) && line.contains(&format!(":{peer} ("))
    })?;
    // The connection's TCP flags: by name, or as bits (`TF_NODELAY`, 0x4).
    let flags = line.split_once("TF=")?.1.split([' ', ')']).next()?;
    Some(flags.split(',').any(|flag| {
        flag == "NODELAY"
            || flag
                .strip_prefix("UNKNOWN=0x")
                .and_then(|bits| u64::from_str_radix(bits, 16).ok())
                .is_some_and(|bits| bits & 0x4 != 0)
    }))
}

#[cfg(target_os = "linux")]
pub fn nodelay_of(group: u32, port: u16, peer: u16) -> Option<bool> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // The socket's inode, from the kernel's list of connections.
    let inode = ["/proc/net/tcp", "/proc/net/tcp6"]
        .iter()
        .find_map(|list| {
            let list = std::fs::read_to_string(list).ok()?;
            list.lines().skip(1).find_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let port_of = |at: usize| {
                    let (_, port) = fields.get(at)?.rsplit_once(':')?;
                    u16::from_str_radix(port, 16).ok()
                };
                (port_of(1)? == port && port_of(2)? == peer)
                    .then(|| fields.get(9).map(|i| i.to_string()))?
            })
        })?;
    let socket = format!("socket:[{inode}]");
    // The process of the group holding it, and its descriptor there.
    let (pid, fd) = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let pgrp: u32 = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(2)?
                .parse()
                .ok()?;
            if pgrp != group {
                return None;
            }
            std::fs::read_dir(entry.path().join("fd"))
                .ok()?
                .flatten()
                .find_map(|fd| {
                    let to = std::fs::read_link(fd.path()).ok()?;
                    let fd: i32 = fd.file_name().to_str()?.parse().ok()?;
                    (to.to_str() == Some(socket.as_str())).then_some((pid, fd))
                })
        })?;
    // SAFETY: plain syscalls; each descriptor they return is owned here.
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let pidfd = (pidfd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(pidfd as i32) })?;
    let copy = unsafe {
        libc::syscall(
            libc::SYS_pidfd_getfd,
            std::os::fd::AsRawFd::as_raw_fd(&pidfd),
            fd,
            0,
        )
    };
    let copy = (copy >= 0).then(|| unsafe { OwnedFd::from_raw_fd(copy as i32) })?;
    let mut nodelay: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `nodelay` and `len` outlive the call, `len` its size.
    let got = unsafe {
        libc::getsockopt(
            std::os::fd::AsRawFd::as_raw_fd(&copy),
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            (&mut nodelay as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    (got == 0).then_some(nodelay != 0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn nodelay_of(_group: u32, _port: u16, _peer: u16) -> Option<bool> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The port is never free while held, and a child handed it has its
    /// descriptor open.
    #[test]
    fn a_held_port_is_never_free_and_its_child_serves_on_it() {
        let held = Held::loopback();
        let address = held.address();
        let taken = std::net::TcpListener::bind(address).unwrap_err();
        assert_eq!(taken.kind(), std::io::ErrorKind::AddrInUse);
        let mut command = crate::env::command("sh", &crate::TempDir::new("held"));
        command.args(["-c", "readlink /proc/self/fd/$HELD"]);
        held.hand_to(&mut command, "HELD");
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let socket = String::from_utf8_lossy(&output.stdout);
        assert!(socket.starts_with("socket:["), "{socket}");
    }

    /// The flag read from the socket that accepted, here in this test's
    /// own process group, set and not; none where this system won't say
    /// (no `pidfd_getfd`, or no right to it).
    #[test]
    fn nodelay_read_from_the_accepted_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        // SAFETY: a plain getter.
        let group = unsafe { libc::getpgrp() } as u32;
        for nodelay in [true, false] {
            let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (accepted, _) = listener.accept().unwrap();
            accepted.set_nodelay(nodelay).unwrap();
            let (port, peer) = (
                accepted.local_addr().unwrap().port(),
                client.local_addr().unwrap().port(),
            );
            let read = nodelay_of(group, port, peer);
            assert!(read.is_none_or(|read| read == nodelay), "{read:?}");
            // Another peer: no such connection.
            assert_eq!(nodelay_of(group, port, 1), None);
        }
    }
}

/// A number a program the test starts takes itself, such as a block of
/// ports or an X display, held by this test across every test program
/// on the machine until it's dropped. Seeing it free doesn't keep it:
/// another test program can see the same, and take it before the
/// program this test starts does. So each first locks a file of the
/// number's own (`flock`, let go by the kernel however the test ends),
/// and takes the number only if `free` then says nothing else holds it.
#[cfg(unix)]
#[derive(Debug)]
pub struct HeldNumber {
    number: u32,
    _lock: std::fs::File,
}

#[cfg(unix)]
impl HeldNumber {
    /// The first of `candidates` this test can hold: `kind` names the
    /// numbers, such as `ports` or `X`, so kinds never share a lock.
    /// Fails the test when every one is held or taken.
    pub fn first(
        kind: &str,
        candidates: impl IntoIterator<Item = u32>,
        free: impl Fn(u32) -> bool,
    ) -> HeldNumber {
        candidates
            .into_iter()
            .find_map(|number| Self::hold(kind, number).filter(|_| free(number)))
            .unwrap_or_else(|| panic!("every {kind} number is held or taken"))
    }

    /// `number` of `kind`, unless another test holds it: for a caller
    /// with a search of its own. Whether anything else has taken it is
    /// the caller's to see, after it holds it.
    pub fn hold(kind: &str, number: u32) -> Option<HeldNumber> {
        use std::os::fd::AsRawFd;
        let path = std::path::PathBuf::from(format!("/tmp/lnk-test-{kind}{number}.lock"));
        // Opened read only when it's there: another user's may be.
        let file = std::fs::File::open(&path)
            .or_else(|_| std::fs::File::create_new(&path))
            .or_else(|_| std::fs::File::open(&path))
            .ok()?;
        // SAFETY: flock on a descriptor this file owns.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        (locked == 0).then_some(HeldNumber {
            number,
            _lock: file,
        })
    }

    /// A loopback port a program the test starts binds itself, such as an
    /// sshd or a harness given its port: free on `127.0.0.1` when taken,
    /// and no other test's while held. From 12000 to 15999, below the
    /// ports the system hands a listener bound to 0 (Linux's from 32768),
    /// below Link's own (its defaults from 18789, its agents' blocks from
    /// 20000) and apart from the sandbox tests' (16500 to 18499). The
    /// program binding it again after a run before meets `TIME_WAIT`
    /// unless it sets `SO_REUSEADDR`, as Rust's listeners do. One port
    /// alone: a program that binds the ones after it too, such as a
    /// harness serving its tools on the next, is given a block held whole
    /// ([`HeldNumber::first`] over blocks' bases).
    pub fn port() -> HeldNumber {
        const FROM: u32 = 12_000;
        const COUNT: u32 = 4_000;
        // Spread by this process, so runs at once seldom try the same first.
        let start = std::process::id() % COUNT;
        let ports = (0..COUNT).map(|k| FROM + (start + k) % COUNT);
        Self::first("loopback-port", ports, |port| {
            port_probe(port as u16).is_ok()
        })
    }

    /// The number it holds.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// The port it holds, of [`HeldNumber::port`].
    pub fn port_number(&self) -> u16 {
        self.number as u16
    }
}

#[cfg(unix)]
fn port_probe(port: u16) -> std::io::Result<tokio::net::TcpSocket> {
    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(([127, 0, 0, 1], port).into())?;
    // Do not listen: a concurrent fork can retain this descriptor until
    // exec, after the parent's probe has gone. A reusable bound socket
    // still tests the port without obstructing its eventual listener.
    Ok(socket)
}

#[cfg(all(test, unix))]
mod held_number {
    use super::HeldNumber;

    /// A concurrent fork can inherit the probe until exec. Its continued
    /// lifetime must not stop the program receiving the port from binding.
    #[test]
    fn an_inherited_probe_does_not_delay_the_ports_listener() {
        let held = HeldNumber::port();
        let _inherited_probe = super::port_probe(held.port_number()).unwrap();
        let _listener = std::net::TcpListener::bind(("127.0.0.1", held.port_number())).unwrap();
        assert_eq!(
            super::port_probe(held.port_number())
                .err()
                .map(|e| e.kind()),
            Some(std::io::ErrorKind::AddrInUse),
            "a listening port must still be refused"
        );
    }

    /// A number one test holds is passed over by every other until it's
    /// dropped, even one that sees it free.
    #[test]
    fn a_held_number_is_passed_over_until_it_is_dropped() {
        let kind = format!("kit-{}-", std::process::id());
        let _files = Files(kind.clone());
        let first = HeldNumber::first(&kind, 0..4, |_| true);
        assert_eq!(first.number(), 0);
        let second = HeldNumber::first(&kind, 0..4, |_| true);
        assert_eq!(second.number(), 1);
        // Taken by something else: passed over though its lock is free.
        let third = HeldNumber::first(&kind, 0..4, |n| n != 2);
        assert_eq!(third.number(), 3);
        drop(first);
        // Free again once nothing has its file open: a child another test
        // forks holds a copy of every descriptor until it execs, so the
        // lock can outlive the drop a moment, never end before it.
        let again = crate::wait::eventually(
            "the dropped number to be free again",
            std::time::Duration::from_secs(5),
            || HeldNumber::hold(&kind, 0),
        );
        assert_eq!(again.number(), 0);
        assert!(
            HeldNumber::hold(&kind, 1).is_none(),
            "a held number was taken"
        );
    }

    /// A held port is below the system's own, free when taken, and
    /// another test's is another.
    #[test]
    fn a_held_port_is_one_of_the_tests_own_range_and_another_is_another() {
        let (first, second) = (HeldNumber::port(), HeldNumber::port());
        assert_ne!(first.number(), second.number());
        for held in [first, second] {
            assert!((12_000..16_000).contains(&held.number()));
            std::net::TcpListener::bind(("127.0.0.1", held.port_number())).unwrap();
        }
    }

    /// The test's lock files, removed however it ends.
    struct Files(String);

    impl Drop for Files {
        fn drop(&mut self) {
            for n in 0..4 {
                let _ = std::fs::remove_file(format!("/tmp/lnk-test-{}{n}.lock", self.0));
            }
        }
    }
}
