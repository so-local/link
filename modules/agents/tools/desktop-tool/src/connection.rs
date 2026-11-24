//! The connection to the display's X server: made only once the kernel
//! says who serves it, every wait with a deadline.

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};
use x11rb::utils::RawFdContainer;

use crate::auth;
use crate::limits::limits;

/// A connection to the X server at `socket`, given `cookie` only once
/// the kernel says who serves it (`SO_PEERCRED`): your user, and, when
/// this tool started it (`started`), a process it started. Any other is
/// refused before the cookie is sent: an X server another user started
/// there, taking every client (`-ac`), would be handed it, and a root
/// one (a machine's own Xorg at that number) would get the agent's
/// screenshots and input. The Xvfb this tool starts runs as your user.
pub(crate) fn connect(
    socket: &Path,
    cookie: &[u8],
    started: Option<u32>,
) -> Result<RustConnection<Timed>> {
    let stream = UnixStream::connect(socket)?;
    let peer = peer(&stream).context("asking who serves the display")?;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    ensure!(
        served_by(peer.uid, me),
        "its X server is another user's (uid {}): the desktop's cookie goes only to yours",
        peer.uid
    );
    if let Some(started) = started {
        match peer.pid {
            Some(pid) => ensure!(
                descends(pid, started),
                "its X server (process {pid}) isn't the one this tool started"
            ),
            // Linux names every peer it can see: one it can't is no
            // process this tool started.
            None => ensure!(
                !cfg!(target_os = "linux"),
                "its X server is in no process this tool can see, so not the one it started"
            ),
        }
    }
    let (stream, _) = DefaultStream::from_unix_stream(stream)?;
    let stream = Timed {
        stream,
        wait: Duration::from_millis(limits().reply_ms),
    };
    let conn = RustConnection::connect_to_stream_with_auth_info(
        stream,
        0,
        auth::NAME.to_vec(),
        cookie.to_vec(),
    )?;
    Ok(conn)
}

/// Whether an X server run by `uid` may be given the cookie of user
/// `me`'s desktop: only `me`'s own, root's never, unless `me` is root.
fn served_by(uid: u32, me: u32) -> bool {
    uid == me
}

/// Who is at the other end of a connected socket, as the kernel says:
/// its user, and its process where the system says.
struct Peer {
    uid: u32,
    pid: Option<u32>,
}

fn peer(socket: &UnixStream) -> std::io::Result<Peer> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut size = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the descriptor is live and the kernel fills one ucred,
        // whose size is given.
        let got = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut credentials).cast(),
                &raw mut size,
            )
        };
        if got != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if size as usize != size_of::<libc::ucred>() {
            return Err(std::io::Error::other("incomplete peer credentials"));
        }
        Ok(Peer {
            uid: credentials.uid,
            pid: u32::try_from(credentials.pid).ok().filter(|pid| *pid > 0),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: the kernel fills two IDs of a socket this process holds.
        if unsafe { libc::getpeereid(socket.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Peer { uid, pid: None })
    }
}

/// Whether process `pid` is `ancestor` or started by it, through its
/// parents; true where the system doesn't say (no `/proc`), since the
/// user was checked already.
fn descends(pid: u32, ancestor: u32) -> bool {
    let mut at = pid;
    // A chain of parents no deeper than a sandbox's few.
    for _ in 0..limits().parents_max {
        if at == ancestor {
            return true;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{at}/stat")) else {
            return !Path::new("/proc/self/stat").exists();
        };
        // `pid (name) state ppid ...`: the name may hold anything but
        // ends at the last `)`.
        let parent = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse::<u32>().ok());
        match parent {
            Some(parent) if parent > 0 => at = parent,
            _ => return false,
        }
    }
    false
}

/// A connection's stream whose every wait has a deadline: an X server
/// that hangs fails the call rather than holding the tool for good.
pub(crate) struct Timed {
    stream: DefaultStream,
    wait: Duration,
}

impl Stream for Timed {
    fn poll(&self, mode: PollMode) -> std::io::Result<()> {
        let mut events = 0;
        if mode.readable() {
            events |= libc::POLLIN;
        }
        if mode.writable() {
            events |= libc::POLLOUT;
        }
        let mut fd = libc::pollfd {
            fd: self.stream.as_raw_fd(),
            events,
            revents: 0,
        };
        let wait = libc::c_int::try_from(self.wait.as_millis()).unwrap_or(libc::c_int::MAX);
        loop {
            // SAFETY: one live pollfd, its count given.
            match unsafe { libc::poll(&mut fd, 1, wait) } {
                0 => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!(
                            "the X server didn't answer within {}s",
                            self.wait.as_secs_f32()
                        ),
                    ));
                }
                n if n > 0 => return Ok(()),
                _ => {
                    let err = std::io::Error::last_os_error();
                    if err.kind() != std::io::ErrorKind::Interrupted {
                        return Err(err);
                    }
                }
            }
        }
    }

    fn read(&self, buf: &mut [u8], fds: &mut Vec<RawFdContainer>) -> std::io::Result<usize> {
        self.stream.read(buf, fds)
    }

    fn write(&self, buf: &[u8], fds: &mut Vec<RawFdContainer>) -> std::io::Result<usize> {
        self.stream.write(buf, fds)
    }

    fn write_vectored(
        &self,
        bufs: &[std::io::IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> std::io::Result<usize> {
        self.stream.write_vectored(bufs, fds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel names who serves a socket: this process, of this user,
    /// at the other end of a pair; and a process descends from itself,
    /// never from one that isn't its parent.
    #[test]
    fn the_peer_is_whom_the_kernel_names() {
        let (left, _right) = UnixStream::pair().unwrap();
        let peer = peer(&left).unwrap();
        // SAFETY: geteuid has no preconditions.
        assert_eq!(peer.uid, unsafe { libc::geteuid() });
        let me = std::process::id();
        if cfg!(target_os = "linux") {
            assert_eq!(peer.pid, Some(me));
        }
        assert!(descends(me, me));
        if me != 1 && Path::new("/proc/self/stat").exists() {
            assert!(!descends(1, me));
        }
    }

    /// The cookie goes only to an X server of your user: never another
    /// user's, and never root's, as a machine's own Xorg is.
    #[test]
    fn only_your_users_server_takes_the_cookie() {
        assert!(served_by(1000, 1000));
        assert!(!served_by(0, 1000));
        assert!(!served_by(1001, 1000));
        assert!(served_by(0, 0));
    }

    /// A server at the display that isn't the one this tool started, nor
    /// started by it, is refused before the cookie is sent, though it's
    /// your user's.
    #[test]
    fn a_server_this_tool_did_not_start_is_refused() {
        let dir = link_test_kit::TempDir::new("desktop-not-started");
        let socket = dir.join("X0");
        let _listening = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // No process is this one's parent by that number.
        let err = connect(&socket, b"cookie", Some(u32::MAX))
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("isn't the one this tool started"), "{err}");
    }

    /// A server that never answers fails the wait rather than holding the
    /// tool: every poll has its deadline.
    #[test]
    fn a_silent_server_times_out() {
        let (left, _right) = UnixStream::pair().unwrap();
        let (stream, _) = DefaultStream::from_unix_stream(left).unwrap();
        let timed = Timed {
            stream,
            wait: Duration::from_millis(1),
        };
        let err = timed.poll(PollMode::Readable).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(timed.poll(PollMode::Writable).is_ok());
    }
}
