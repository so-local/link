//! Where its installer comes from: a pinned source, and its checksum.

use super::*;

/// The installer comes from the pinned commit, and its checksum is a
/// SHA-256.
#[test]
fn installer_is_pinned_to_the_release() {
    assert!(INSTALLER.url.contains(&format!("/{COMMIT}/")));
    assert_eq!(INSTALLER.sha256.len(), 64);
}

/// Its hosts are hosts, and take in its installer, and GitHub's
/// releases (where a download goes on from github.com) for Signal.
#[test]
fn its_installs_hosts_are_listed() {
    for host in HOSTS {
        link_sandbox_contract::net::check_host(host).unwrap();
    }
    let installer = INSTALLER.url.split('/').nth(2).unwrap();
    for host in [
        installer,
        "github.com",
        "release-assets.githubusercontent.com",
    ] {
        let hosts: Vec<String> = HOSTS.iter().map(|h| h.to_string()).collect();
        assert!(
            link_sandbox_contract::net::host_allowed(host, &hosts),
            "{host}"
        );
    }
}
