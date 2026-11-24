//! Store contract fakes.
use std::path::Path;
/// The service [`bound`] gives an agent to act as: a test's alone.
pub const TEST_SERVICE: &str = "s7e5t0s1v2c3";

/// The key [`bound`] gives an agent: a test's alone.
pub const TEST_KEY: &str = "7e57000000000000000000000000000000000000000000000000000000000000";

/// Binds agent `agent` (its ID), owned by `me@box` and acting as
/// [`TEST_SERVICE`], in the keyring in `dir` ([`crate::bridge::Keyring`]),
/// with [`TEST_KEY`]: what it proves
/// its bridge calls with in a test.
pub fn bound(dir: &Path, agent: &str) -> crate::bridge::Binding {
    crate::bridge::Keyring::at(dir)
        .bind(
            &crate::bridge::Identity {
                agent: agent.into(),
                service: TEST_SERVICE.into(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            &link_secretive::Secret::from(TEST_KEY.to_string()),
        )
        .expect("binding a test's agent")
}

/// `driver` behind a store bridge of its own ([`crate::store::serve_bridge`]),
/// for tests: listening at `dir/store.sock` for good, taking calls proved
/// as an agent of the keyring in `dir/keys`; and how agent `agent` (its
/// ID, owned by `me@box`, bound there with [`TEST_KEY`]) calls it.
pub fn store_bridge(
    dir: &Path,
    agent: &str,
    driver: crate::store::Driver,
) -> crate::store::Bridged {
    let keys = dir.join("keys");
    let binding = bound(&keys, agent);
    let socket = dir.join("store.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("listening for a test");
    std::thread::spawn(move || {
        crate::store::serve_bridge(
            listener,
            std::sync::Arc::new(crate::bridge::Keyring::at(&keys)),
            std::sync::Arc::new(move |_: &crate::bridge::Identity| Ok(driver.clone())),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    });
    crate::store::Bridged { socket, binding }
}
