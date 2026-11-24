//! The resolver's sockets, made in the sandbox's network and handed out (Linux).

/// The sandbox's resolver's sockets (UDP and TCP, its `127.0.0.1:53`):
/// bubblewrap's sandbox, named on `info` (its `--info-fd`), is joined by
/// `lnk-sandbox dns-socket` ([`listen_in`]), which listens there
/// and hands them back. Only its owner, this user, can join it.
#[cfg(target_os = "linux")]
pub fn sockets(
    info: std::fs::File,
) -> anyhow::Result<(std::net::UdpSocket, std::net::TcpListener)> {
    use anyhow::Context;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;
    // `{"child-pid": 123, ...}`, written once its namespaces are made,
    // in several writes. It is read whole, to its closing brace or its
    // end, before `info` is closed: a write to a closed pipe would kill
    // bubblewrap (SIGPIPE) before its sandbox starts.
    let text = link_sandbox_contract::bubblewrap::read_info(info)?;
    let pid = link_sandbox_contract::bubblewrap::child_pid(&text)
        .context("bubblewrap said nothing of its sandbox")?;
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    let fd = theirs.as_raw_fd();
    let mut helper = if link_sandbox_contract::box_workers::controller()?.is_some() {
        link_sandbox_contract::box_workers::privileged(link_sandbox_contract::box_workers::program())
    } else {
        // Handed no secret: it joins the sandbox's namespaces, and needs
        // only its pid and the socket to send on.
        link_secretive::process::command(std::env::current_exe()?)
    };
    helper.args([
        "dns-socket",
        "--pid",
        &pid.to_string(),
        "--fd",
        &fd.to_string(),
    ]);
    // SAFETY: fcntl is async-signal-safe: its end, open across its exec.
    unsafe {
        helper.pre_exec(move || {
            libc::fcntl(fd, libc::F_SETFD, 0);
            Ok(())
        });
    }
    let status = helper.status().context("running sandbox dns-socket")?;
    drop(theirs);
    anyhow::ensure!(status.success(), "joining the sandbox's network failed");
    let fds = receive_fds(ours.as_raw_fd(), 2)?;
    anyhow::ensure!(
        fds.len() == 2,
        "the sandbox's resolver's sockets didn't come"
    );
    // SAFETY: just received, owned by nothing else.
    let (udp, tcp) = unsafe {
        (
            std::net::UdpSocket::from_raw_fd(fds[0]),
            std::net::TcpListener::from_raw_fd(fds[1]),
        )
    };
    udp.set_nonblocking(true)?;
    tcp.set_nonblocking(true)?;
    Ok((udp, tcp))
}

/// `sandbox dns-socket`: joins the network of bubblewrap's sandbox `pid`
/// as its owner (this user owns the user namespace bubblewrap made, so
/// it may), listens on its `127.0.0.1:53`, UDP and TCP, and sends both
/// sockets over `fd`. Nothing else: it runs no program there, and exits.
#[cfg(target_os = "linux")]
pub fn listen_in(pid: u32, fd: i32) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::os::fd::AsRawFd;
    const NS_GET_USERNS: libc::c_ulong = 0xb701;
    let net =
        std::fs::File::open(format!("/proc/{pid}/ns/net")).context("the sandbox's network")?;
    // SAFETY: an ioctl on our descriptor, returning a new one.
    let user = unsafe { libc::ioctl(net.as_raw_fd(), NS_GET_USERNS as _) };
    anyhow::ensure!(user >= 0, "its owner: {}", std::io::Error::last_os_error());
    // SAFETY: this process has one thread, as joining a user namespace needs.
    unsafe {
        // An ordinary box's launcher creates the network in the machine's
        // user namespace. Linux refuses re-entering the namespace already held.
        use std::os::unix::fs::MetadataExt;
        let current = std::fs::metadata("/proc/self/ns/user")?;
        let target = std::fs::metadata(format!("/proc/self/fd/{user}"))?;
        if (current.dev(), current.ino()) != (target.dev(), target.ino()) {
            anyhow::ensure!(
                libc::setns(user, libc::CLONE_NEWUSER) == 0,
                "joining its user namespace: {}",
                std::io::Error::last_os_error()
            );
        }
        anyhow::ensure!(
            libc::setns(net.as_raw_fd(), libc::CLONE_NEWNET) == 0,
            "joining its network: {}",
            std::io::Error::last_os_error()
        );
    }
    let udp = std::net::UdpSocket::bind("127.0.0.1:53").context("listening on 127.0.0.1:53")?;
    let tcp = std::net::TcpListener::bind("127.0.0.1:53").context("listening on 127.0.0.1:53")?;
    send_fds(fd, &[udp.as_raw_fd(), tcp.as_raw_fd()])
}

/// Sends `fds` over the unix socket `to`.
#[cfg(target_os = "linux")]
pub(super) fn send_fds(to: i32, fds: &[i32]) -> anyhow::Result<()> {
    let size = std::mem::size_of_val(fds);
    // SAFETY: CMSG_SPACE is arithmetic.
    let space = unsafe { libc::CMSG_SPACE(size as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: a zeroed msghdr, filled below with pointers that outlive it.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // SAFETY: the control buffer has room for one header and `fds`.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&msg);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(size as u32) as _;
        std::ptr::copy_nonoverlapping(
            fds.as_ptr(),
            libc::CMSG_DATA(header).cast::<i32>(),
            fds.len(),
        );
        anyhow::ensure!(
            libc::sendmsg(to, &msg, 0) == 1,
            "handing the sockets over: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

/// Up to `most` file descriptors sent over the unix socket `from`.
#[cfg(target_os = "linux")]
pub(super) fn receive_fds(from: i32, most: usize) -> anyhow::Result<Vec<i32>> {
    let size = most * std::mem::size_of::<i32>();
    // SAFETY: CMSG_SPACE is arithmetic.
    let space = unsafe { libc::CMSG_SPACE(size as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: a zeroed msghdr, filled with pointers that outlive it.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // SAFETY: recvmsg fills `msg`'s buffers, which it was given.
    let n = unsafe { libc::recvmsg(from, &mut msg, libc::MSG_CMSG_CLOEXEC) };
    anyhow::ensure!(
        n >= 0,
        "receiving the sockets: {}",
        std::io::Error::last_os_error()
    );
    let mut fds = Vec::new();
    // SAFETY: walking the control messages the kernel wrote.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&msg);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(header).cast::<i32>();
                let count = ((*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize)
                    / std::mem::size_of::<i32>();
                for i in 0..count {
                    fds.push(*data.add(i));
                }
            }
            header = libc::CMSG_NXTHDR(&msg, header);
        }
    }
    Ok(fds)
}
