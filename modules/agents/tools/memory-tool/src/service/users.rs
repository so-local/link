//! Whose processes a session is taken from: the service's own user, the
//! one `--peer` names, and the group its socket is shared with.

use super::*;

/// The group `socket` is shared with for `--peer`, whose user is in it
/// (`--socket-group`): the socket its group's (mode 660), in a folder no one
/// but its owner and that group reaches. The kernel's word on who
/// connects ([`taken_from`]) is still what takes a session; this lets
/// the peer connect at all.
pub(super) fn shared_group(socket: &Path, group: libc::gid_t) -> Result<libc::gid_t> {
    use std::os::unix::fs::PermissionsExt;
    let dir = socket.parent().context("a socket in no folder")?;
    let mode = std::fs::metadata(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .permissions()
        .mode();
    ensure!(
        mode & 0o007 == 0,
        "{} is reached by anyone (mode {:o}): its socket is shared only in a folder no one else enters",
        dir.display(),
        mode & 0o7777
    );
    Ok(group)
}

/// The ID of group `name` (`--socket-group`), or `name` itself when it's a
/// number, as the system's groups say: one that isn't there, an error.
pub fn group_id(name: &str) -> Result<libc::gid_t> {
    if let Ok(id) = name.parse() {
        return Ok(id);
    }
    let shown = link_format::clean(name);
    let c = std::ffi::CString::new(name).with_context(|| format!("no group {shown} here"))?;
    // SAFETY: a group is plain data, all zeros a valid one.
    let mut entry: libc::group = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::group = std::ptr::null_mut();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: getgrnam_r fills `entry` and points `found` at it, its
    // strings in `buffer`, whose length is given; only the ID is read.
    let failed = unsafe {
        libc::getgrnam_r(
            c.as_ptr(),
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    ensure!(failed == 0 && !found.is_null(), "no group {shown} here");
    Ok(entry.gr_gid)
}

/// The ID of user `name` (`--peer`), or `name` itself when it's a
/// number, as the system's users say: one that isn't there, an error.
pub fn user_id(name: &str) -> Result<libc::uid_t> {
    if let Ok(id) = name.parse() {
        return Ok(id);
    }
    let shown = link_format::clean(name);
    let c = std::ffi::CString::new(name).with_context(|| format!("no user {shown} here"))?;
    // SAFETY: a passwd is plain data, all zeros a valid one.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: getpwnam_r fills `entry` and points `found` at it, its
    // strings in `buffer`, whose length is given; only the ID is read.
    let failed = unsafe {
        libc::getpwnam_r(
            c.as_ptr(),
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    ensure!(failed == 0 && !found.is_null(), "no user {shown} here");
    Ok(entry.pw_uid)
}

/// Whether the process at the other end of `stream` runs as the user this
/// one does, as the system says; no when it can't say.
pub fn same_user(stream: &UnixStream) -> bool {
    // SAFETY: getuid takes nothing and can't fail.
    taken_from(peer_user(stream), unsafe { libc::getuid() }, None)
}

/// Whether a session is taken from a peer running as `peer` (none when
/// the system can't say) by a service running as `me`, which takes
/// `also`'s sessions too.
pub(super) fn taken_from(
    peer: Option<libc::uid_t>,
    me: libc::uid_t,
    also: Option<libc::uid_t>,
) -> bool {
    peer.is_some_and(|peer| peer == me || Some(peer) == also)
}

/// The user the process at the other end of `stream` runs as.
#[cfg(target_os = "linux")]
pub(super) fn peer_user(stream: &UnixStream) -> Option<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED fills one ucred, whose size is given, of a
    // socket this process holds open.
    let got = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (got == 0).then_some(cred.uid)
}

/// The user the process at the other end of `stream` runs as.
#[cfg(not(target_os = "linux"))]
pub(super) fn peer_user(stream: &UnixStream) -> Option<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: getpeereid fills two IDs of a socket this process holds open.
    let got = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (got == 0).then_some(uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session is taken only from the service's own user: never
    /// another's, even root's, nor one the system can't name.
    #[test]
    fn a_session_is_taken_only_from_its_own_user() {
        assert!(taken_from(Some(1000), 1000, None));
        assert!(!taken_from(Some(1001), 1000, None));
        assert!(!taken_from(Some(0), 1000, None));
        assert!(!taken_from(None, 1000, None));
        // And the one other user it's told to take: no one else.
        assert!(taken_from(Some(1001), 1000, Some(1001)));
        assert!(!taken_from(Some(1002), 1000, Some(1001)));
        assert!(!taken_from(None, 1000, Some(1001)));
        // Both ends of a pair this process holds are its own.
        let (a, _b) = UnixStream::pair().unwrap();
        assert!(same_user(&a));
    }
}
