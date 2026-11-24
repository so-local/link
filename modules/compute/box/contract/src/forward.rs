//! A Unix socket on another machine, given by `ssh -R`: what that machine
//! runs first, and the forward's own spec. `lnk box forward --socket` uses
//! it, and so does any module forwarding a socket to a machine over SSH.

use anyhow::ensure;

/// What a machine runs over SSH before `ssh -R` gives it a Unix socket at
/// `socket` (a path in its runtime folder):
/// the socket's folder made, only its user's; a socket a connection
/// before left there removed (sshd binds over none, and removes none);
/// and its runtime folder printed, which `ssh -R` needs whole. One line with
/// no `'`, `!` or `\`, so any login shell passes it to `sh` as it is.
pub fn socket_forward_prepare(socket: &str) -> anyhow::Result<String> {
    check_socket(socket)?;
    let folder = socket.rsplit_once('/').map(|(folder, _)| folder);
    let runtime = &link_files::layout::LAYOUT.runtime_root;
    let mut script = format!(
        "if test -n \"$XDG_RUNTIME_DIR\"; then lnk_runtime=\"$XDG_RUNTIME_DIR/{}\"; \
         elif test -d \"{}/$(id -u)\"; then lnk_runtime=\"{}/$(id -u)/{}\"; \
         else lnk_runtime=\"{}/{}$(id -u)\"; fi; \
         if test -L \"$lnk_runtime\"; then exit 1; fi; \
         umask 077 && mkdir -p \"$lnk_runtime\" && chmod 700 \"$lnk_runtime\" && ",
        runtime.folder,
        runtime.linux.display(),
        runtime.linux.display(),
        runtime.folder,
        runtime.fallback.display(),
        runtime.fallback_prefix,
    );
    if let Some(folder) = folder {
        script += &format!(
            "mkdir -p \"$lnk_runtime/{folder}\" && chmod 700 \"$lnk_runtime/{folder}\" && "
        );
    }
    script += &format!("rm -f \"$lnk_runtime/{socket}\" && echo \"$lnk_runtime\"");
    Ok(format!("exec /bin/sh -c '{script}'"))
}

/// `ssh -R`'s forward to `socket` in the runtime folder `home` printed (by
/// [`socket_forward_prepare`]), from port `to` on this machine's loopback.
pub fn socket_forward(home: &str, socket: &str, to: &str) -> anyhow::Result<String> {
    check_socket(socket)?;
    let home = home.trim_end_matches(['\n', '\r']).trim_end_matches('/');
    ensure!(
        home.starts_with('/')
            && home
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-+@".contains(&b)),
        "the machine's home folder ({:?}) can't hold the socket",
        link_format::clean(home)
    );
    Ok(format!("{home}/{socket}:127.0.0.1:{to}"))
}

/// An error unless `socket` is a relative runtime path that a forward
/// and a shell take as it is.
fn check_socket(socket: &str) -> anyhow::Result<()> {
    ensure!(
        !socket.is_empty()
            && !socket.starts_with('/')
            && !socket.ends_with('/')
            && socket
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && socket
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)),
        "{:?} isn't a socket's path in a home folder",
        link_format::clean(socket)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket's path in the runtime folder, as a module names one.
    const SOCKET: &str = "proxy/proxy.sock";

    #[test]
    fn a_socket_forward_is_prepared_then_named_whole() {
        let prepare = socket_forward_prepare(SOCKET).unwrap();
        let script = prepare
            .strip_prefix("exec /bin/sh -c '")
            .and_then(|s| s.strip_suffix('\''))
            .unwrap();
        assert!(!script.contains(['\'', '!', '\n', '\\']), "{script}");
        assert!(
            script.contains("chmod 700 \"$lnk_runtime/proxy\""),
            "{script}"
        );
        assert!(
            script.contains("rm -f \"$lnk_runtime/proxy/proxy.sock\""),
            "{script}"
        );
        // Run: the folder made, a stale socket gone, the runtime root printed.
        let home = link_test_kit::TempDir::new("forward");
        let runtime =
            link_files::layout::Root::Runtime.default_in(&link_test_kit::env::runtime(&home));
        let socket = runtime.join(SOCKET);
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        std::fs::write(&socket, "").unwrap();
        let out = link_test_kit::env::command("/bin/sh", &home)
            .args(["-c", &prepare])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(!socket.exists());
        let printed = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            socket_forward(&printed, SOCKET, "{port}").unwrap(),
            format!("{}:127.0.0.1:{{port}}", socket.display())
        );
        assert!(socket_forward("relative\n", SOCKET, "1").is_err());
        assert!(socket_forward("/home/a:b\n", SOCKET, "1").is_err());
        for bad in ["/abs.sock", "../x.sock", "a/./b", "a b", "a'b", ""] {
            assert!(socket_forward_prepare(bad).is_err(), "{bad:?}");
        }
    }
}
