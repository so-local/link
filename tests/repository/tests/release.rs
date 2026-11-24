//! Releases are signed by Link's signer: every place that trusts it trusts
//! the same line, only `main` can sign, and no committed file holds a
//! staging or dev build's key.

use std::{path::Path, process::Command};

use link_core::release::LINK_SIGNER;

fn manifest_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn repo(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `scripts/release/link-signers` is the one source: `install.sh`, which runs
/// before any lnk exists, embeds the same line.
#[test]
fn links_signer_is_the_same_everywhere() {
    let signers = LINK_SIGNER.trim();
    assert_eq!(repo("scripts/release/link-signers").trim(), signers);
    assert!(repo("core/cli/install.sh").contains(&format!("\nRELEASE_SIGNERS='{signers}'\n")));
}

/// A release is never a staging or dev build: no workflow of this
/// repository builds one, has the staging key to sign one or names a dev
/// build's signer, and the staging key is neither Link's (the library's
/// own test) nor a test's. No committed file holds a dev build's key:
/// every release signer in the workspace is Link's, the staging key or a
/// test's, and every private key a test's.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn no_release_is_a_staging_or_dev_build() {
    let workflows = manifest_path("../../.github/workflows");
    let mut read = 0;
    for entry in std::fs::read_dir(&workflows).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let name = entry.file_name();
        assert!(
            !text.contains("LNK_BUILD: staging")
                && !text.contains("LNK_BUILD: \"staging\"")
                && !text.contains("LNK_BUILD=staging")
                && !text.contains("LNK_STAGING_SIGNING")
                && !text.contains("staging-signers"),
            "{name:?} builds a staging build"
        );
        assert!(
            !text.contains("LNK_BUILD: dev")
                && !text.contains("LNK_BUILD: \"dev\"")
                && !text.contains("LNK_BUILD=dev")
                && !text.contains("LNK_DEV_SIGNER")
                && !text.contains("--dev-key"),
            "{name:?} builds a dev build"
        );
        read += 1;
    }
    assert!(read >= 1, "the workflows, in {}", workflows.display());
    let staging = repo("scripts/release/staging-signers");
    for test_key in ["test-release-key.pub", "test-staging-key.pub"] {
        let public = repo(&format!("packages/test-kit/fixtures/{test_key}"));
        let key = public.split_whitespace().nth(1).unwrap();
        assert!(!staging.contains(key), "staging-signers holds {test_key}");
    }

    // Every whole Ed25519 key after `ssh-ed25519 ` on a release signer's
    // line (`namespaces="link-release"`, quoted or escaped).
    let signers = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.contains("namespaces=") && l.contains("link-release"))
            .flat_map(|l| l.split("ssh-ed25519 ").skip(1))
            .map(|k| {
                k.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || "+/=".contains(*c))
                    .collect::<String>()
            })
            .filter(|k| k.len() == 68)
            .collect()
    };
    let mut known = signers(LINK_SIGNER);
    known.extend(signers(&staging));
    for test_key in ["test-release-key.pub", "test-staging-key.pub"] {
        let public = repo(&format!("packages/test-kit/fixtures/{test_key}"));
        known.push(public.split_whitespace().nth(1).unwrap().to_string());
    }
    // The library's own test signer.
    known.push("AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW".into());
    let files = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(manifest_path("../.."))
        .output()
        .unwrap();
    assert!(files.status.success());
    let mut read = 0;
    for path in String::from_utf8(files.stdout).unwrap().split('\0') {
        let Ok(text) = std::fs::read_to_string(manifest_path("../..").join(path)) else {
            continue;
        };
        read += 1;
        for key in signers(&text) {
            assert!(
                known.contains(&key),
                "{path} holds a release signer that's not Link's, staging or a test's: {key}"
            );
        }
        // An OpenSSH private key's body starts with "openssh-key-v1".
        assert!(
            !text.contains(concat!("b3BlbnNzaC1", "rZXktdjE"))
                || path.starts_with("packages/test-kit/fixtures/test-"),
            "{path} holds a private key"
        );
    }
    assert!(read > 100, "the workspace's files");
}
