//! How the sandbox installs a system package with apt, where sudo needs
//! no password: with apt's own retries and per-connection timeout
//! ([`link_machine::apt`]), so a slow mirror that ends a download midway
//! is fetched again inside the one install rather than failing it, on a
//! computer as on a box, whose apt keeps the same settings in a file.

/// The arguments to `sudo` that install `package` with apt without
/// asking (`sudo -n`), with apt's retries and timeout.
pub(crate) fn install(package: &str) -> anyhow::Result<Vec<String>> {
    let mut args = vec!["-n".to_string()];
    args.extend(link_machine::apt::apt_get(&[
        "install", "-y", "-q", package,
    ])?);
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The install asks sudo for no password and gives apt its retries
    /// and a timeout for each transport, before the command.
    #[test]
    fn the_install_gives_apt_its_retries_and_timeouts() {
        let args = install("bubblewrap").unwrap();
        assert_eq!(args[..2], ["-n", "apt-get"]);
        assert_eq!(
            args[args.len() - 4..],
            ["install", "-y", "-q", "bubblewrap"]
        );
        for setting in [
            "Acquire::Retries=",
            "Acquire::http::Timeout=",
            "Acquire::https::Timeout=",
        ] {
            assert!(args.iter().any(|a| a.starts_with(setting)), "{args:?}");
        }
    }
}
