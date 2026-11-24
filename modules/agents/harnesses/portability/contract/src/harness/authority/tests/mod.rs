use super::*;
use crate::bridge::Identity;
use link_test_kit::TempDir;

struct Fixture {
    dir: TempDir,
    host: Ledger,
    peer: Binding,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let keys = Keyring::at(&dir.join("keys"));
        let identity = Identity {
            agent: "aaaaaaaaaaaa".into(),
            service: String::new(),
            owner: "me@box".into(),
            teams: Vec::new(),
        };
        let peer = keys
            .bind(&identity, &link_secretive::Secret::new("a".repeat(64)))
            .unwrap();
        let host = Ledger::at(
            &dir.join("host"),
            Agent {
                id: identity.agent,
                owner: identity.owner,
            },
            keys,
        )
        .unwrap();
        Self { dir, host, peer }
    }

    fn attempt(&self, thread: &str) -> Attempt {
        let session = self.host.session(&self.peer, thread).unwrap();
        self.host
            .admit(&self.peer, &session, native("opaque-binding"))
            .unwrap()
    }

    fn reload(&self) -> Ledger {
        Ledger::at(
            &self.dir.join("host"),
            self.host.agent.clone(),
            self.host.keys.clone(),
        )
        .unwrap()
    }
}

fn native(id: &str) -> NativeBinding {
    NativeBinding {
        adapter: "example".into(),
        id: id.into(),
    }
}

#[test]
fn session_mapping_survives_restart_and_native_binding_changes() {
    let f = Fixture::new("authority-session");
    let first = f.attempt("link:main");
    let saved = first.session.clone();
    assert_eq!(f.reload().session(&f.peer, "link:main").unwrap(), saved);
    f.host.retire(&f.peer, &first).unwrap();
    let second = f
        .reload()
        .admit(&f.peer, &saved, native("different-native-session"))
        .unwrap();
    assert_eq!(second.session, saved);
    assert_ne!(second.id, first.id);
    assert_ne!(second.generation, first.generation);
    assert_ne!(second.binding, first.binding);
    // Ambiguous routing is not guessed by this profile.
    for thread in ["telegram:42", "link:../other", "link:", "main"] {
        assert!(f.host.session(&f.peer, thread).is_err());
    }
    assert!(
        Ledger::at(
            &f.dir.join("unrouted"),
            Agent {
                id: "main".into(),
                owner: "me@box".into()
            },
            f.host.keys.clone()
        )
        .is_err()
    );
}

#[test]
fn lost_admission_ack_returns_one_committed_generation() {
    let f = Fixture::new("authority-admit");
    let first = f.attempt("link:main");
    assert_eq!(
        f.reload()
            .admit(&f.peer, &first.session, first.binding.clone())
            .unwrap(),
        first
    );
    let other = f.host.session(&f.peer, "link:other").unwrap();
    assert!(f.host.admit(&f.peer, &other, native("other")).is_err());
    assert!(
        f.host
            .admit(&f.peer, &first.session, native("other"))
            .is_err()
    );
}

#[test]
fn foreign_revoked_and_stale_bindings_cannot_admit_or_retire() {
    let f = Fixture::new("authority-binding");
    let attempt = f.attempt("link:main");
    let foreign = f
        .host
        .keys
        .bind(
            &Identity {
                agent: "bbbbbbbbbbbb".into(),
                service: String::new(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::new("b".repeat(64)),
        )
        .unwrap();
    assert!(f.host.session(&foreign, "link:main").is_err());
    let mut stale = attempt.clone();
    stale.generation = "different".into();
    assert!(f.host.retire(&f.peer, &stale).is_err());
    f.host.keys.unbind(
        &f.peer.identity.agent,
        &link_secret_contract::keeper::RouteRevoker,
    );
    assert!(f.host.retire(&f.peer, &attempt).is_err());
    assert!(f.host.session(&f.peer, "link:main").is_err());
}

#[test]
fn restart_barrier_preserves_sessions_but_never_revives_an_old_attempt() {
    let f = Fixture::new("authority-recovery");
    let attempt = f.attempt("link:main");
    f.reload().recover().unwrap();
    assert!(f.host.retire(&f.peer, &attempt).is_err());
    let next = f
        .reload()
        .admit(&f.peer, &attempt.session, native("replacement"))
        .unwrap();
    assert_eq!(next.session, attempt.session);
    assert_ne!(next.generation, attempt.generation);
}

#[test]
fn private_session_metadata_refuses_corruption_unknown_versions_and_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("authority-state");
    f.attempt("link:main");
    let path = f.dir.join("host").join(FILE);
    let original = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&original).contains(f.peer.key.expose()));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for replacement in [
        b"{".to_vec(),
        {
            let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
            value["schema"] = serde_json::json!(2);
            serde_json::to_vec(&value).unwrap()
        },
        vec![b'x'; MAX as usize + 1],
    ] {
        std::fs::write(&path, replacement).unwrap();
        assert!(f.reload().session(&f.peer, "link:main").is_err());
    }
    let outside = f.dir.join("outside");
    std::fs::write(&outside, &original).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(f.reload().session(&f.peer, "link:main").is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), original);
}
