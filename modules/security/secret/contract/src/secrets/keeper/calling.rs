//! A call to the keeper: its socket, connected to and checked, one JSON
//! line each way.

use super::*;

/// The keeper's socket, where there's a keeper here.
pub(super) fn socket() -> Option<PathBuf> {
    if let Some(socket) = link_secretive::test_var(TEST_KEEPER) {
        return Some(socket.into());
    }
    (cfg!(target_os = "linux") && user_exists(KEEPER_USER)).then(|| PathBuf::from(SOCKET))
}

/// Whether this machine has a user named `name`.
fn user_exists(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: a NUL-ended name; the entry it points to isn't kept.
    !unsafe { libc::getpwnam(name.as_ptr()) }.is_null()
}

/// `request`, sent to the keeper at `socket`, and its reply: only from a
/// keeper running as the user whose folder the socket is in, a folder
/// no one else writes.
pub(super) fn call(socket: &Path, request: &Request) -> anyhow::Result<Reply> {
    let not_answering = || {
        format!(
            "your secrets' keeper isn't answering at {}: sudo systemctl start {KEEPER_USER} \
             starts it",
            socket.display()
        )
    };
    // A keeper that accepted the call and closed it unasked is running,
    // and busy with this user's other calls.
    let closed = |err: std::io::Error| match err.kind() {
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
            anyhow::Error::new(err).context(format!(
                "your secrets' keeper at {} closed the call unanswered: it's busy with your \
                 other calls",
                socket.display()
            ))
        }
        _ => anyhow::Error::new(err).context(not_answering()),
    };
    let mut stream = connect(socket).with_context(not_answering)?;
    stream.set_read_timeout(Some(WAIT))?;
    stream.set_write_timeout(Some(WAIT))?;
    let asked = Asked {
        protocol: PROTOCOL.current,
        request,
    };
    let mut line = Secret::new(serde_json::to_vec(&asked)?);
    line.expose_mut().push(b'\n');
    stream.write_all(line.expose()).map_err(closed)?;
    let reply = read_line(&mut stream, REPLY_MAX).map_err(|err| {
        match err.downcast::<std::io::Error>() {
            Ok(io) => closed(io),
            Err(err) => err.context(not_answering()),
        }
    })?;
    let answered: Answered<Reply> = serde_json::from_slice(reply.expose())
        .context("your secrets' keeper answered what isn't a reply")?;
    PROTOCOL.accept("your secrets' keeper", answered.protocol)?;
    Ok(answered.reply)
}

/// A connection to the socket at `path`, answered by its folder's owner.
pub(super) fn connect(path: &Path) -> anyhow::Result<UnixStream> {
    use std::os::unix::fs::MetadataExt;
    let folder = path.parent().context("a socket in no folder")?;
    let meta = std::fs::metadata(folder)?;
    if meta.mode() & 0o022 != 0 {
        bail!("{} is written by others than its owner", folder.display());
    }
    let stream = UnixStream::connect(path)?;
    let peer = peer_uid(&stream)?;
    if peer != meta.uid() {
        bail!(
            "what answers at {} isn't {}'s owner's",
            path.display(),
            folder.display()
        );
    }
    Ok(stream)
}

/// The user at `stream`'s other end.
pub(super) fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` are this call's to write, of their sizes.
        let done = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if done != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(cred.uid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: `uid` and `gid` are this call's to write.
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(uid)
    }
}

/// One line of `stream`, at most `max` bytes, its ending taken off: an
/// error past that, or at its end with no line.
pub(super) fn read_line(stream: &mut impl Read, max: u64) -> anyhow::Result<Secret<Vec<u8>>> {
    let mut line = Secret::new(Vec::new());
    BufReader::new(stream.take(max + 1)).read_until(b'\n', line.expose_mut())?;
    if line.expose().last() != Some(&b'\n') {
        bail!("no whole line within {max} bytes");
    }
    line.expose_mut().pop();
    Ok(line)
}
