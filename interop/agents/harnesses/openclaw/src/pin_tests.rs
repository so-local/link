//! Where its installer comes from: a pinned source, and its checksum.

use super::*;

/// The installer comes from a fixed commit, and its checksum is a
/// SHA-256.
#[test]
fn installer_is_pinned() {
    let commit = INSTALLER.url.split('/').nth(5).unwrap();
    assert!(commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(INSTALLER.sha256.len(), 64);
    assert_eq!(INSTALLER.companions.len(), 1);
    for companion in INSTALLER.companions {
        assert_eq!(companion.url.split('/').nth(5), Some(commit));
        assert_eq!(companion.sha256.len(), 64);
    }
}

/// Its hosts are hosts, and take in its installer, and GitHub's
/// releases (where a download goes on from github.com) for Signal.
#[test]
fn its_installs_hosts_are_listed() {
    for host in HOSTS {
        link_sandbox_contract::net::check_host(host).unwrap();
    }
    let installer_hosts = std::iter::once(INSTALLER.url)
        .chain(INSTALLER.companions.iter().map(|file| file.url))
        .map(|url| url.split('/').nth(2).unwrap());
    for host in installer_hosts.chain(["github.com", "release-assets.githubusercontent.com"]) {
        let hosts: Vec<String> = HOSTS.iter().map(|h| h.to_string()).collect();
        assert!(
            link_sandbox_contract::net::host_allowed(host, &hosts),
            "{host}"
        );
    }
}
