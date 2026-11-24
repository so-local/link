//! Tests of the bridges' binding (`link_sandbox_binding`), as
//! every agent's call relies on it.
#[cfg(test)]
mod tests {
    use link_sandbox_binding::*;
    use std::path::Path;

    /// A call's action holds the most any bridge takes: a store's event
    /// twice over, and a files bridge's object as base64.
    #[test]
    fn a_call_holds_what_every_bridge_takes() {
        const { assert!(2 * link_store_contract::store::EVENT_MAX + (1 << 20) <= CALL_MAX) };
        // The files bridge's largest object, as base64 in its JSON.
        const { assert!((32 << 20) / 3 * 4 + (1 << 20) <= CALL_MAX) };
    }
    use link_test_kit::TempDir;

    fn me() -> Identity {
        Identity {
            agent: "aaaaaaaaaaaa".into(),
            service: String::new(),
            owner: "acct-1".into(),
            teams: Vec::new(),
        }
    }

    fn other() -> Identity {
        Identity {
            agent: "bbbbbbbbbbbb".into(),
            service: String::new(),
            owner: "acct-1".into(),
            teams: Vec::new(),
        }
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// One call to a bridge checking against `keys`, made by `caller`
    /// naming `named`: who the bridge says called, or why it refused.
    fn one_call(
        keys: impl Keys + 'static,
        caller: Binding,
        named: Identity,
    ) -> Result<Identity, String> {
        let (bridge, client) = std::os::unix::net::UnixStream::pair().unwrap();
        let taking = std::thread::spawn(move || {
            take::<String>(&bridge, &keys).map(|(who, action)| {
                assert_eq!(action, "hello");
                who
            })
        });
        let challenge = challenge_on(&client);
        let mut proven = caller.prove(&challenge);
        proven.identity = named;
        send(&client, &proven);
        // Its action, which a refused call never reads.
        send(&client, &"hello");
        taking.join().unwrap().map_err(|err| format!("{err:#}"))
    }

    /// The challenge a bridge sent on `client`.
    fn challenge_on(client: &std::os::unix::net::UnixStream) -> String {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(client), &mut line).unwrap();
        serde_json::from_str::<Challenge>(&line).unwrap().challenge
    }

    /// `value` as a line on `client`, if it's still open.
    fn send(mut client: &std::os::unix::net::UnixStream, value: &impl serde::Serialize) {
        let _ = std::io::Write::write_all(
            &mut client,
            format!("{}\n", serde_json::to_string(value).unwrap()).as_bytes(),
        );
    }

    /// Teams a call names aren't taken: a proof covers only its ID and
    /// owner.
    #[test]
    fn a_call_names_no_teams() {
        let dir = TempDir::new("keyring-teams");
        let keyring = Keyring::at(&dir);
        let mine = keyring
            .bind(&me(), &link_secretive::Secret::from(KEY.to_string()))
            .unwrap();
        let claiming = Identity {
            teams: vec!["t4m9q2x7k1ab".into()],
            ..me()
        };
        assert_eq!(
            one_call(keyring, mine, claiming).unwrap().teams,
            Vec::<String>::new()
        );
    }

    /// A refused call's action is never read: a caller that isn't bound
    /// can't make a bridge hold a large one.
    #[test]
    fn a_refused_calls_action_is_never_read() {
        let dir = TempDir::new("keyring-unread");
        let keyring = Keyring::at(&dir);
        let (bridge, client) = std::os::unix::net::UnixStream::pair().unwrap();
        let taking = std::thread::spawn(move || take::<String>(&bridge, &keyring));
        let challenge = challenge_on(&client);
        let stranger = Binding {
            identity: me(),
            key: KEY.to_string().into(),
        };
        send(&client, &stranger.prove(&challenge));
        // Nothing more is sent: the bridge refuses without waiting for it.
        let refused = taking.join().unwrap().unwrap_err();
        assert!(
            format!("{refused:#}").contains("isn't bound here"),
            "{refused:#}"
        );
    }

    /// A caller that doesn't prove who it is within [`PROOF_WAIT`], a
    /// byte at a time, is refused then, by `take` and `take_async` alike.
    #[test]
    fn a_proof_not_given_in_time_is_refused() {
        use std::io::Write as _;
        #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
        fn trickle(mut client: std::os::unix::net::UnixStream) {
            let _ = challenge_on(&client);
            std::thread::spawn(move || {
                while client.write_all(b" ").is_ok() {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });
        }
        let dir = TempDir::new("keyring-late");
        let keyring = Keyring::at(&dir);
        let started = std::time::Instant::now();
        // Both at once: each waits out the proof's time.
        let (bridge, client) = std::os::unix::net::UnixStream::pair().unwrap();
        let taking = {
            let keyring = keyring.clone();
            std::thread::spawn(move || take::<String>(&bridge, &keyring))
        };
        trickle(client);
        let (bridge, client) = std::os::unix::net::UnixStream::pair().unwrap();
        bridge.set_nonblocking(true).unwrap();
        let taking_async = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut bridge = tokio::net::UnixStream::from_std(bridge).unwrap();
                    take_async::<String>(&mut bridge, &keyring).await
                })
        });
        trickle(client);
        for taking in [taking, taking_async] {
            let refused = taking.join().unwrap().unwrap_err();
            assert!(
                format!("{refused:#}").contains("no proof of who's calling within"),
                "{refused:#}"
            );
        }
        assert!(started.elapsed() < PROOF_WAIT + std::time::Duration::from_secs(3));
    }

    /// An agent with no owner can't be bound, and says why.
    #[test]
    fn an_agent_without_an_owner_isnt_bound() {
        let dir = TempDir::new("keyring-owner");
        let nobody = Identity {
            owner: String::new(),
            ..me()
        };
        let err = Keyring::at(&dir)
            .bind(&nobody, &link_secretive::Secret::from(KEY.to_string()))
            .unwrap_err();
        assert!(format!("{err:#}").contains("has no owner"), "{err:#}");
    }

    /// A call proved with its agent's key is taken as that agent's; one
    /// naming another agent, proved with this one's key, isn't.
    #[test]
    fn a_key_proves_only_its_own_agent() {
        let dir = TempDir::new("keyring");
        let keyring = Keyring::at(&dir);
        let mine = keyring
            .bind(&me(), &link_secretive::Secret::from(KEY.to_string()))
            .unwrap();
        keyring
            .bind(
                &other(),
                &link_secretive::Secret::from(KEY.replace('0', "1").to_string()),
            )
            .unwrap();
        assert_eq!(
            one_call(keyring.clone(), mine.duplicate(), me()).unwrap(),
            me()
        );
        let refused = one_call(keyring.clone(), mine.duplicate(), other()).unwrap_err();
        assert!(refused.contains("isn't agent bbbbbbbbbbbb's"), "{refused}");
        // Another owner's agent with the same ID isn't this one.
        let stranger = Identity {
            owner: "acct-2".into(),
            ..me()
        };
        let refused = one_call(keyring, mine, stranger).unwrap_err();
        assert!(refused.contains("isn't bound here"), "{refused}");
    }

    /// One bridge for two agents tells them apart by their proofs alone.
    #[test]
    fn one_bridge_keeps_two_agents_apart() {
        let dir = TempDir::new("keyring-two");
        let keyring = Keyring::at(&dir);
        let a = keyring
            .bind(&me(), &link_secretive::Secret::from(KEY.to_string()))
            .unwrap();
        let b = keyring
            .bind(
                &other(),
                &link_secretive::Secret::from(KEY.replace('0', "2").to_string()),
            )
            .unwrap();
        assert_eq!(
            one_call(keyring.clone(), a.duplicate(), me()).unwrap(),
            me()
        );
        assert_eq!(
            one_call(keyring.clone(), b.duplicate(), other()).unwrap(),
            other()
        );
        assert!(one_call(keyring.clone(), a, other()).is_err());
        assert!(one_call(keyring, b, me()).is_err());
    }

    /// A bridge for one agent takes only that agent's calls.
    #[test]
    fn a_binding_takes_only_its_agent() {
        let mine = Binding {
            identity: me(),
            key: KEY.to_string().into(),
        };
        assert_eq!(
            one_call(mine.duplicate(), mine.duplicate(), me()).unwrap(),
            me()
        );
        let theirs = Binding {
            identity: other(),
            key: KEY.to_string().into(),
        };
        assert!(one_call(mine, theirs.duplicate(), other()).is_err());
    }

    /// A bridge proof is never an endpoint proof, so an endpoint, which
    /// signs any challenge it's sent, can't be asked to make one.
    #[test]
    fn a_bridge_proof_is_no_endpoint_proof() {
        let challenge = "a7".repeat(32);
        assert_ne!(
            proof(
                &link_secretive::Secret::from(KEY.to_string()),
                &challenge,
                &me()
            ),
            link_harness_contract::harness::endpoint_proof(
                &link_secretive::Secret::from(KEY.to_string()),
                &challenge
            )
        );
        assert!(proof_matches(
            &link_secretive::Secret::from(KEY.to_string()),
            &challenge,
            &me(),
            &proof(
                &link_secretive::Secret::from(KEY.to_string()),
                &challenge,
                &me()
            )
        ));
        assert!(!proof_matches(
            &link_secretive::Secret::from("".to_string()),
            &challenge,
            &me(),
            &proof(
                &link_secretive::Secret::from("".to_string()),
                &challenge,
                &me()
            )
        ));
        assert!(!proof_matches(
            &link_secretive::Secret::from(KEY.to_string()),
            &challenge,
            &other(),
            &proof(
                &link_secretive::Secret::from(KEY.to_string()),
                &challenge,
                &me()
            )
        ));
    }

    /// The keyring is the user's alone, a file per agent, and refuses an
    /// ID that would be a path.
    #[test]
    fn the_keyring_is_private_and_takes_only_ids() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("keyring-private");
        let keyring = Keyring::at(&dir.join("keys"));
        keyring
            .bind(&me(), &link_secretive::Secret::from(KEY.to_string()))
            .unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(keyring.dir()), 0o700);
        assert_eq!(mode(&keyring.dir().join("aaaaaaaaaaaa.json")), 0o600);
        let sneaky = Identity {
            agent: "../x".into(),
            ..me()
        };
        assert!(
            keyring
                .bind(&sneaky, &link_secretive::Secret::from(KEY.to_string()))
                .is_err()
        );
        assert!(keyring.bound("../x").is_none());
        assert!(
            keyring
                .bind(&me(), &link_secretive::Secret::from("short".to_string()))
                .is_err()
        );
        // Its key is never in its Debug.
        assert!(!format!("{:?}", keyring.bound(&me().agent).unwrap()).contains(KEY));
    }

    /// A passable keyring's folder is `0711`; an agent bound shared is
    /// `0640` of the group it names, and of the next one when it's bound
    /// again; one bound plainly is `0600`; the lock is `0600`.
    #[test]
    fn a_passable_keyring_shares_each_file_with_its_group_alone() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TempDir::new("keyring-passable");
        let keyring = Keyring::passable(&dir.join("keys"));
        let secret = link_secretive::Secret::from(KEY.to_string());
        // Groups this user may give a file: its own, and any when root.
        // SAFETY: getegid can't fail.
        let mine = unsafe { libc::getegid() };
        let (first, second) = match unsafe { libc::geteuid() } {
            0 => (61_001, 61_002),
            _ => (mine, mine),
        };
        keyring.bind_shared(&me(), &secret, first).unwrap();
        keyring.bind(&other(), &secret).unwrap();
        let meta = |p: &Path| std::fs::metadata(p).unwrap();
        let mode = |p: &Path| meta(p).permissions().mode() & 0o7777;
        let file = keyring.dir().join("aaaaaaaaaaaa.json");
        assert_eq!(mode(keyring.dir()), 0o711);
        assert_eq!((mode(&file), meta(&file).gid()), (0o640, first));
        assert_eq!(mode(&keyring.dir().join("bbbbbbbbbbbb.json")), 0o600);
        assert_eq!(mode(&keyring.dir().join(".lock")), 0o600);
        assert_eq!(keyring.bound(&me().agent).unwrap().key, secret);
        // Bound again, for another group: that one's alone.
        keyring.bind_shared(&me(), &secret, second).unwrap();
        assert_eq!((mode(&file), meta(&file).gid()), (0o640, second));
        // Its folder kept `0711`, and one shut again opened.
        std::fs::set_permissions(keyring.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
        keyring.bind_shared(&me(), &secret, second).unwrap();
        assert_eq!(mode(keyring.dir()), 0o711);
        // A plain keyring is never passable.
        let plain = Keyring::at(&dir.join("plain"));
        plain.bind(&me(), &secret).unwrap();
        assert_eq!(mode(plain.dir()), 0o700);
        // Unbound, its file's gone.
        keyring.unbind(&me().agent, &link_secret_contract::keeper::RouteRevoker);
        assert!(!file.exists());
        // Another user's folder is refused.
        if unsafe { libc::geteuid() } == 0 {
            let theirs = dir.join("theirs");
            std::fs::create_dir(&theirs).unwrap();
            std::os::unix::fs::chown(&theirs, Some(61_003), None).unwrap();
            assert!(Keyring::passable(&theirs).bind(&me(), &secret).is_err());
        }
    }

    /// As other users, with a passable keyring: a user in an agent's
    /// group opens its file and lists nothing; one in another group, or
    /// in the group an agent that's left a slot had, opens nothing. As
    /// root alone (it runs as users of its own choosing).
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_shared_key_is_read_by_its_group_alone() {
        use std::os::unix::fs::PermissionsExt;
        // Only root reads as other users.
        if !link_test_kit::gate::root() {
            return;
        }
        let dir = TempDir::short("keyring-groups");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o711)).unwrap();
        let keyring = Keyring::passable(&dir.join("keys"));
        let secret = link_secretive::Secret::from(KEY.to_string());
        let (helper, theirs) = (61_011, 61_012);
        // `program` with `args`, as a user of its own in group `gid`
        // alone: whether it succeeded.
        let as_group = |gid: u32, program: &str, path: &Path| -> bool {
            std::process::Command::new("setpriv")
                .args([
                    format!("--reuid={}", 62_000 + gid % 1000),
                    format!("--regid={gid}"),
                    "--clear-groups".into(),
                    "--".into(),
                    program.into(),
                ])
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        keyring.bind_shared(&me(), &secret, helper).unwrap();
        keyring.bind_shared(&other(), &secret, theirs).unwrap();
        let mine = keyring.dir().join("aaaaaaaaaaaa.json");
        let yours = keyring.dir().join("bbbbbbbbbbbb.json");
        assert!(as_group(helper, "cat", &mine), "its helper reads its key");
        assert!(!as_group(helper, "cat", &yours), "and no other's");
        assert!(!as_group(theirs, "cat", &mine));
        assert!(!as_group(helper, "ls", keyring.dir()), "nor lists them");
        // The agent leaves its slot, and another's bound there, for the
        // same helper's group: the first's file is gone, and the second's
        // is the helper's.
        keyring.unbind(&me().agent, &link_secret_contract::keeper::RouteRevoker);
        keyring.bind_shared(&other(), &secret, helper).unwrap();
        assert!(!as_group(helper, "cat", &mine), "the one gone: nothing");
        assert!(as_group(helper, "cat", &yours));
        assert!(!as_group(theirs, "cat", &yours), "its old group: nothing");
    }
}
