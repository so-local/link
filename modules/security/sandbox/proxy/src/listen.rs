//! Listening, and connecting to a sandbox's socket as itself.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};

/// The listener on `127.0.0.1:<listen>` this proxy's starter handed it
/// open ([`link_sandbox_client::HeldPort`]), when [`HELD_LISTENER_ENV`]
/// names one: taken before anything else runs, closed on exec from then
/// on, so no program this proxy starts holds it. Refused, never bound in
/// its place, unless it's a listening TCP socket on exactly that address.
///
/// [`HELD_LISTENER_ENV`]: link_sandbox_client::HELD_LISTENER_ENV
pub fn handed(listen: Option<u16>) -> anyhow::Result<Option<std::net::TcpListener>> {
    let Some(said) = std::env::var_os(link_sandbox_client::HELD_LISTENER_ENV) else {
        return Ok(None);
    };
    let said = said
        .to_str()
        .context("the listener handed has no descriptor")?;
    let port = listen.context("a listener was handed without --listen")?;
    take_listener(said, port).map(Some)
}

/// The listener on descriptor `said`, this process's from now on, if
/// it's a listening TCP socket on `127.0.0.1:<port>`.
fn take_listener(said: &str, port: u16) -> anyhow::Result<std::net::TcpListener> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let fd: libc::c_int = said
        .parse()
        .ok()
        .filter(|fd| *fd > libc::STDERR_FILENO)
        .with_context(|| format!("the listener handed isn't on a descriptor of its own: {said}"))?;
    // SAFETY: fcntl and fstat of a descriptor number, open or not;
    // nothing is closed or owned yet.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        anyhow::ensure!(flags >= 0, "no listener was handed on descriptor {fd}");
        // Closed on exec before anything else: no program this starts
        // gets it.
        anyhow::ensure!(
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == 0,
            "keeping the listener handed on descriptor {fd}: {}",
            std::io::Error::last_os_error()
        );
        let mut stat: libc::stat = std::mem::zeroed();
        anyhow::ensure!(
            libc::fstat(fd, &mut stat) == 0 && stat.st_mode & libc::S_IFMT == libc::S_IFSOCK,
            "descriptor {fd}, handed as a listener, isn't a socket"
        );
    }
    let mut kind: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: getsockopt into an int of the size given.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&raw mut kind).cast(),
            &mut size,
        )
    };
    anyhow::ensure!(
        got == 0 && kind == libc::SOCK_STREAM,
        "descriptor {fd}, handed as a listener, isn't a listening TCP socket"
    );
    // Read through a listener that never closes it: nothing is owned
    // until it's taken.
    // SAFETY: a socket's descriptor, open, never closed by this.
    let borrowed = std::mem::ManuallyDrop::new(unsafe { std::net::TcpListener::from_raw_fd(fd) });
    let at = borrowed.local_addr()?;
    let wanted = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
    anyhow::ensure!(
        port != 0 && at == wanted,
        "the listener handed is on {at}, not {wanted}"
    );
    // Whether it listens is asked by listening: macOS answers no
    // `SO_ACCEPTCONN` question (ENOPROTOOPT), and `listen` on a socket
    // already listening only sets its backlog on every system, while a
    // connected one refuses it.
    // SAFETY: listen on a socket's descriptor.
    anyhow::ensure!(
        unsafe { libc::listen(fd, libc::SOMAXCONN) } == 0,
        "descriptor {fd}, handed as a listener, isn't a listening TCP socket: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: the descriptor handed to this process alone, a listening
    // socket, owned from here and closed once.
    let listener = std::net::TcpListener::from(unsafe { OwnedFd::from_raw_fd(fd) });
    Ok(listener)
}

/// The most connections one listener serves at once: past it, the next
/// waits for one to end, so a program opening connections without end
/// can't use up the proxy's file descriptors.
pub(crate) const MAX_CONNECTIONS: usize = 512;

/// How long a listener waits after a failed accept (out of file
/// descriptors, a connection reset before it's taken) before the next.
const ACCEPT_RETRY: std::time::Duration = std::time::Duration::from_millis(100);

/// A listener of TCP or unix connections; a TCP connection accepted
/// sends each write at once, as [`connect_tcp`]'s.
pub trait Listener: Send + 'static {
    type Stream: Send + 'static;
    fn next(&self) -> impl std::future::Future<Output = std::io::Result<Self::Stream>> + Send;
}

impl Listener for TcpListener {
    type Stream = TcpStream;
    async fn next(&self) -> std::io::Result<TcpStream> {
        let (stream, _) = self.accept().await?;
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }
}

impl Listener for UnixListener {
    type Stream = UnixStream;
    async fn next(&self) -> std::io::Result<UnixStream> {
        Ok(self.accept().await?.0)
    }
}

/// `listener`'s next connection. A failed accept is said and waited out
/// briefly, never the listener's end: running out of file descriptors
/// for a moment mustn't leave a sandbox without its network.
#[expect(
    clippy::disallowed_methods,
    reason = "rests after a failed accept rather than spin"
)]
pub async fn accept<L: Listener>(listener: &L) -> L::Stream {
    loop {
        match listener.next().await {
            Ok(stream) => return stream,
            Err(err) => {
                link_format::aside(
                    link_format::Report::warning(link_format::sentence!(
                        "Couldn't accept a connection."
                    ))
                    .why(link_format::Prose::from(
                        link_format::Sentence::adopt(&err.to_string()),
                    )),
                );
                tokio::time::sleep(ACCEPT_RETRY).await;
            }
        }
    }
}

/// Serves each of `listener`'s connections with `serve`, in a task of its
/// own, [`MAX_CONNECTIONS`] at once, for good.
pub async fn each<L, F, Fut>(listener: L, serve: F)
where
    L: Listener,
    F: Fn(L::Stream) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let serving = serve(accept(&listener).await);
        tokio::spawn(async move {
            serving.await;
            drop(slot);
        });
    }
}

/// Who a proxy's socket folder is shared with: its sockets' mode, and
/// whose `in.sock` the proxy takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sharing {
    /// This user alone: the program runs as the proxy's user, its folder
    /// `0700` and its sockets `0600`.
    Alone,
    /// The proxy runs as a user other than its agent's: the folder `0770`
    /// (and set-group-ID, so what's made in it takes its group), its
    /// sockets `0660`, of `group`, which only the agent's user and the
    /// proxy's share; `in.sock` taken only from `agent`, the agent's user.
    Group { agent: u32, group: u32 },
}

impl Sharing {
    /// From `--agent-user` and `--group`: both, or neither.
    pub fn of(agent: Option<u32>, group: Option<u32>) -> anyhow::Result<Sharing> {
        match (agent, group) {
            (None, None) => Ok(Sharing::Alone),
            (Some(agent), Some(group)) => Ok(Sharing::Group { agent, group }),
            _ => anyhow::bail!("--agent-user and --group go together"),
        }
    }

    /// How the proxy's own sockets in the folder are made.
    pub fn given(self) -> link_sandbox_bridge::socket::Given {
        match self {
            Sharing::Alone => link_sandbox_bridge::socket::Given::default(),
            Sharing::Group { group, .. } => link_sandbox_bridge::socket::Given {
                owner: None,
                group: Some(group),
                mode: 0o660,
            },
        }
    }

    /// The user whose `in.sock` the proxy connects to: its agent's.
    pub fn agent(self) -> u32 {
        match self {
            // SAFETY: geteuid has no preconditions.
            Sharing::Alone => unsafe { libc::geteuid() },
            Sharing::Group { agent, .. } => agent,
        }
    }

    /// An error unless folder `dir` is as it's shared: of its group, and
    /// closed to everyone else. A folder of this user's alone is made so
    /// by its caller.
    pub fn check(self, dir: &Path) -> anyhow::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let Sharing::Group { group, .. } = self else {
            return Ok(());
        };
        let meta =
            std::fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
        anyhow::ensure!(
            meta.is_dir() && meta.gid() == group && meta.mode() & 0o7777 == SHARED_DIR,
            "{} isn't a folder its proxy and its agent share: group {group}, mode {SHARED_DIR:o}",
            dir.display()
        );
        Ok(())
    }
}

/// A proxy's socket folder, and whom it's shared with.
#[derive(Clone, Copy, Debug)]
pub struct Folder<'a> {
    pub dir: &'a Path,
    pub sharing: Sharing,
}

impl Folder<'_> {
    /// Its sandbox's `in.sock`.
    pub fn inbound(self) -> link_sandbox_route::Inbound {
        link_sandbox_route::Inbound {
            path: self.dir.join("in.sock"),
            agent: self.sharing.agent(),
        }
    }
}

/// A socket folder's mode when a proxy and its agent share it by a group
/// ([`Sharing::Group`]): theirs alone, and set-group-ID, so `in.sock`,
/// made inside the sandbox, where the group can't be named, takes it.
pub const SHARED_DIR: u32 = 0o2770;

/// Whether folder `dir`, a sandbox's own, is shared with its proxy by a
/// group ([`Sharing::Group`]): this user's, its mode [`SHARED_DIR`].
/// Never through a link.
pub fn shared(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    std::fs::symlink_metadata(dir)
        .is_ok_and(|m| m.is_dir() && m.uid() == me && m.mode() & 0o7777 == SHARED_DIR)
}

/// Inside a sandbox: listens at `path`, in the folder it shares with its
/// proxy, the socket made `mode` once bound (its group the folder's).
/// Bound at its path, never moved there as [`listen_unix`]'s are: Landlock,
/// which keeps a sandbox to the programs it lists, refuses a move from
/// one folder to another. Until its mode is set it's as the sandbox's
/// umask makes it, which no one else can connect to; the folder is the
/// sandbox's own, where nothing but it and its proxy reaches.
pub fn listen_inside(path: &Path, mode: u32) -> anyhow::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    link_sandbox_bridge::socket::check_path(path)?;
    let _ = std::fs::remove_file(path);
    let listener =
        UnixListener::bind(path).with_context(|| format!("listening on {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("giving {} its mode", path.display()))?;
    Ok(listener)
}

/// Listens at `path`, a socket in a sandbox's folder, made as `given`
/// says before it's there: a socket left at `path` is replaced.
pub fn listen_unix(
    path: &Path,
    given: link_sandbox_bridge::socket::Given,
) -> anyhow::Result<UnixListener> {
    link_sandbox_bridge::socket::listen_async(path, given)
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, IntoRawFd};

    use super::take_listener;

    /// `listener`'s descriptor as a starter hands it: open across exec.
    fn handed(listener: std::net::TcpListener) -> String {
        let fd = listener.into_raw_fd();
        // SAFETY: fcntl on a descriptor this test holds.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        fd.to_string()
    }

    #[test]
    fn the_listener_handed_is_served_as_it_is_and_kept_from_programs() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let taken = take_listener(&handed(listener), port).unwrap();
        // SAFETY: fcntl on the descriptor `taken` owns.
        let flags = unsafe { libc::fcntl(taken.as_raw_fd(), libc::F_GETFD) };
        assert!(flags & libc::FD_CLOEXEC != 0, "{flags}");
        assert_eq!(taken.local_addr().unwrap().port(), port);
        // The same socket: a connection to the port reaches it.
        let _client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        taken.accept().unwrap();
    }

    /// macOS answers no question of whether a socket listens, so it's
    /// made to: a TCP socket bound to the port, not yet listening, is
    /// served once it's taken, as one already listening is.
    #[test]
    fn a_bound_socket_handed_is_made_to_listen() {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let port = socket.local_addr().unwrap().port();
        let fd = socket.into_raw_fd();
        // SAFETY: fcntl on a descriptor this test holds.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        let taken = take_listener(&fd.to_string(), port).unwrap();
        taken.set_nonblocking(false).unwrap();
        let _client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        taken.accept().unwrap();
    }

    #[test]
    fn a_listener_on_another_address_is_refused() {
        let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = other.local_addr().unwrap().port();
        let err = take_listener(&handed(other), port.wrapping_add(1).max(1)).unwrap_err();
        assert!(err.to_string().contains("not 127.0.0.1:"), "{err}");
        // Every interface, not the loopback alone.
        let anywhere = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = anywhere.local_addr().unwrap().port();
        let err = take_listener(&handed(anywhere), port).unwrap_err();
        assert!(err.to_string().contains("0.0.0.0"), "{err}");
    }

    #[test]
    fn what_isnt_a_listening_tcp_socket_is_refused() {
        // Each kept open by the test: refused before it's taken.
        let file = std::fs::File::open("/dev/null").unwrap();
        let err = take_listener(&file.as_raw_fd().to_string(), 1).unwrap_err();
        assert!(err.to_string().contains("isn't a socket"), "{err}");
        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = udp.local_addr().unwrap().port();
        let err = take_listener(&udp.as_raw_fd().to_string(), port).unwrap_err();
        assert!(err.to_string().contains("listening TCP"), "{err}");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let port = stream.local_addr().unwrap().port();
        let err = take_listener(&stream.as_raw_fd().to_string(), port).unwrap_err();
        assert!(err.to_string().contains("listening TCP"), "{err}");
        // Standard input, output and error are never a listener's.
        for said in ["", "x", "-1", "0", "2"] {
            assert!(take_listener(said, 1).is_err(), "{said}");
        }
    }
}
