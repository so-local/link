//! Kernel-authenticated local service peers. No claimed UID crosses this boundary.

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

/// The effective user at the other end of a connected Unix socket.
pub fn uid(socket: &UnixStream) -> std::io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut size = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the descriptor is live and the kernel receives bounded writable storage.
        if unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut credentials).cast(),
                &raw mut size,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if size as usize != size_of::<libc::ucred>() {
            return Err(std::io::Error::other("incomplete peer credentials"));
        }
        Ok(credentials.uid)
    }
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: the kernel receives live storage for two integers.
        if unsafe { libc::getpeereid(socket.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(uid)
    }
}

/// Root or the service owner may change its host policy.
pub fn owner(peer: u32, service: u32) -> bool {
    peer == 0 || peer == service
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_do_not_replace_kernel_peer_identity() {
        let (left, right) = UnixStream::pair().unwrap();
        // SAFETY: a scalar getter.
        let me = unsafe { libc::geteuid() };
        assert_eq!(uid(&left).unwrap(), me);
        assert_eq!(uid(&right).unwrap(), me);
        assert!(owner(0, me));
        assert!(owner(me, me));
        assert!(!owner(me.saturating_add(1), me));
    }
}
