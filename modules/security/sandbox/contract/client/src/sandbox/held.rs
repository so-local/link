//! A port of this machine's loopback held for a proxy from the moment
//! it's chosen until the proxy serves on it.
//!
//! A port picked by binding and then letting go is free again for any
//! program of the machine until the proxy binds it: one that takes it
//! first leaves the proxy unable to start. Held, the port is chosen by
//! binding it, and the open listener itself is handed to the proxy
//! (`lnk sandbox proxy --listen <port>`), which serves on it as it is
//! ([`HELD_LISTENER_ENV`]). The port is never free in between.

use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// The variable that names the descriptor of the listener a proxy's
/// starter hands it ([`HeldPort::hand_to`]): `lnk sandbox proxy --listen
/// <port>` serves on that listener, after checking it's a TCP listener on
/// `127.0.0.1:<port>`, and never binds the port itself. The proxy's
/// program is never given it.
pub const HELD_LISTENER_ENV: &str = "LNK_PROXY_LISTENER_FD";

/// A port of this machine's loopback, chosen by binding it, held open
/// until it's handed to the proxy that serves on it.
#[derive(Debug)]
pub struct HeldPort(std::net::TcpListener);

impl HeldPort {
    /// A free port of this machine's loopback, of the system's picking,
    /// held.
    pub fn new() -> std::io::Result<HeldPort> {
        std::net::TcpListener::bind("127.0.0.1:0").map(HeldPort)
    }

    /// The port held.
    pub fn port(&self) -> std::io::Result<u16> {
        Ok(self.0.local_addr()?.port())
    }

    /// Hands the listener to `command`'s child, a `lnk sandbox proxy
    /// --listen` on this port: open in that child alone across its exec,
    /// on the descriptor [`HELD_LISTENER_ENV`] names, closed on exec in
    /// this process and every other child as before. `command` holds it
    /// until it's dropped. Called after its environment is set: clearing
    /// that would drop the variable.
    pub fn hand_to(self, command: &mut Command) {
        let listener = self.0;
        let fd = listener.as_raw_fd();
        command.env(HELD_LISTENER_ENV, fd.to_string());
        // SAFETY: in the child, after it forks and before it runs the
        // program: only fcntl, on a descriptor the closure owns, so it's
        // open until `command` is dropped.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(listener.as_raw_fd(), libc::F_GETFD);
                if flags < 0
                    || libc::fcntl(
                        listener.as_raw_fd(),
                        libc::F_SETFD,
                        flags & !libc::FD_CLOEXEC,
                    ) < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_held_port_is_never_free_for_another_program() {
        let held = HeldPort::new().unwrap();
        let port = held.port().unwrap();
        assert_ne!(port, 0);
        let taken = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap_err();
        assert_eq!(taken.kind(), std::io::ErrorKind::AddrInUse);
        // Handed, the command holds it until it's dropped.
        let mut command = Command::new("/bin/true");
        held.hand_to(&mut command);
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());
        let fd = command
            .get_envs()
            .find(|(name, _)| *name == HELD_LISTENER_ENV)
            .and_then(|(_, value)| value?.to_str()?.parse::<i32>().ok())
            .unwrap();
        // Closed on exec here: only the child it's handed to gets it.
        // SAFETY: fcntl on a descriptor `command` holds open.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0, "{flags}");
        drop(command);
        std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    }
}
