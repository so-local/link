use super::*;

/// The groups an allowed user talked to the agent in, kept in the
/// connection's folder ([`Bridge::state`]), which the harness never reaches:
/// `send` sends there, beside the owner's own chat, and nowhere else a
/// harness names.
const GROUPS: &str = "groups.json";

/// How many groups are kept, the latest.
const GROUPS_KEPT: usize = 1000;

/// The groups kept, oldest first, as `groups.json` keeps them, naming
/// its schema.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Groups {
    groups: Vec<String>,
}

link_files::specification!(Groups, include_str!("groups.schema.toml"));

/// Keeps `chat` (a group, as its thread names it after the connection:
/// `-1001234567890`, `g-120363012345678901`) as one an allowed user
/// talked to the agent in, which `send` may send to. Nothing for a
/// bridge without a folder.
pub fn remember_group(state: &std::path::Path, chat: &str) -> anyhow::Result<()> {
    static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if !state.is_absolute() {
        return Ok(());
    }
    let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut groups = groups(state);
    if groups.iter().any(|group| group == chat) {
        return Ok(());
    }
    groups.push(chat.to_string());
    let extra = groups.len().saturating_sub(GROUPS_KEPT);
    groups.drain(..extra);
    let path = state.join(GROUPS);
    let aside = path.with_extension(format!("{}.tmp", std::process::id()));
    link_files::write_private(&aside, &link_files::saved::print_json(&Groups { groups })?)?;
    std::fs::rename(&aside, &path)
        .map_err(|err| anyhow::anyhow!("writing {}: {err}", path.display()))
}

/// Whether an allowed user talked to the agent in group `chat`
/// ([`remember_group`]).
pub fn knows_group(state: &std::path::Path, chat: &str) -> bool {
    state.is_absolute() && groups(state).iter().any(|group| group == chat)
}

fn groups(state: &std::path::Path) -> Vec<String> {
    std::fs::read(state.join(GROUPS))
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Groups>(&bytes).ok())
        .unwrap_or_default()
        .groups
}

/// Where a linked connection's bridge takes what `send` sends (`send.sock`
/// in its folder, which only the user reaches): it holds the connection's
/// one connection (a linked device), so `lnk-<connection> send` hands it
/// each message ([`send_through`]). Each is a bridge's call
/// ([`crate::bridge`]): the agent's identity, the message
/// (`{"chat", "text"}`) and its proof with the agent's key, answered
/// with one line, `{}` or the error envelope.
pub fn sends_at(state: &std::path::Path) -> std::path::PathBuf {
    state.join("send.sock")
}

/// A message `send` hands a running bridge.
#[derive(Serialize, Deserialize)]
struct Handed {
    chat: String,
    text: String,
}

/// A running bridge's answer to a message handed it, once it went.
#[derive(Serialize, Deserialize)]
struct Went {}

/// How long a call to `send.sock` may take to arrive, its message, once
/// it's proved.
const SEND_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// What a connection's bridge takes at `send.sock` at once and waits for
/// ([`crate::bridge::serve`]): its messages go one at a time, the rest
/// waiting their turn. Only its own agent calls it.
pub const SENDS: crate::bridge::Limits = crate::bridge::Limits {
    name: "the connection's bridge",
    calls: 8,
    per_agent: 8,
    per_owner: 8,
    action_max: 1 << 20,
    action_wait: SEND_WAIT,
    answer_wait: std::time::Duration::from_secs(30),
};

/// Takes the messages `send` hands the bridge whose folder is `state`
/// ([`sends_at`]), for good, on the server every bridge runs on
/// ([`crate::bridge::serve_with`], within [`SENDS`]): each sent by `send`
/// (its chat, as its thread names it after the connection, and its text),
/// one at a time, and only those proved as an agent `keys` holds, any
/// other refused: the bridge's own, with the key the machine's keyring
/// holds for it still ([`Bridge::sends_keys`]), so once the agent is
/// stopped, its old key sends nothing.
pub async fn take_sends<F, Fut>(
    state: &std::path::Path,
    keys: impl crate::bridge::Keys + 'static,
    send: F,
) -> anyhow::Result<()>
where
    F: Fn(String, String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let path = sends_at(state);
    let listener = crate::socket::listen_async(&path, crate::socket::Given::default())?;
    let turn = tokio::sync::Mutex::new(());
    let never = std::sync::atomic::AtomicBool::new(false);
    let keys: std::sync::Arc<dyn crate::bridge::Keys> = std::sync::Arc::new(keys);
    crate::bridge::serve_with(listener, keys, &SENDS, &never, |_, handed: Handed| {
        let (turn, send) = (&turn, &send);
        async move {
            let _turn = turn.lock().await;
            send(handed.chat, handed.text).await.map(|()| Went {})
        }
    })
    .await;
    Ok(())
}

/// Hands `text` for chat `chat` to the running bridge whose folder is
/// `state` ([`take_sends`]), as `agent`'s, proved, once it went or failed.
pub async fn send_through(
    state: &std::path::Path,
    agent: &crate::bridge::Binding,
    chat: &str,
    text: &str,
) -> anyhow::Result<()> {
    let path = sends_at(state);
    let mut stream = tokio::net::UnixStream::connect(&path)
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "nothing carries the connection ({}: {err}): its bridge isn't running",
                path.display()
            )
        })?;
    let handed = Handed {
        chat: chat.to_string(),
        text: text.to_string(),
    };
    crate::bridge::call_async(&mut stream, agent, &handed).await?;
    let went = crate::bridge::read_answer_async::<Went>(&mut stream, 1 << 20).await;
    match went {
        Ok(Went {}) => Ok(()),
        Err(err) => match err.downcast::<crate::bridge::Failure>() {
            Ok(failure) => anyhow::bail!("{failure}"),
            Err(_) => anyhow::bail!("the bridge stopped before it said whether it sent"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lnk-connection-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A group is known once an allowed user talked to the agent in it,
    /// each once, the latest kept; a bridge without a folder knows none.
    #[test]
    fn the_groups_the_owner_talks_in_are_kept() {
        let state = folder("groups");
        assert!(!knows_group(&state, "-100123"));
        remember_group(&state, "-100123").unwrap();
        remember_group(&state, "-100123").unwrap();
        remember_group(&state, "g-120363").unwrap();
        assert!(knows_group(&state, "-100123") && knows_group(&state, "g-120363"));
        assert!(!knows_group(&state, "-100124"));
        assert_eq!(groups(&state).len(), 2);
        let kept = std::fs::read_to_string(state.join(GROUPS)).unwrap();
        assert_eq!(kept, r#"{"groups":["-100123","g-120363"],"schema":1}"#);
        for n in 0..GROUPS_KEPT {
            remember_group(&state, &format!("-{n}")).unwrap();
        }
        assert!(!knows_group(&state, "-100123"));
        assert!(knows_group(&state, &format!("-{}", GROUPS_KEPT - 1)));
        remember_group(std::path::Path::new(""), "-1").unwrap();
        assert!(!knows_group(std::path::Path::new(""), "-1"));
        let _ = std::fs::remove_dir_all(state);
    }

    /// `send` hands a running bridge its message, and is told whether it
    /// went; with no bridge running, it says so.
    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    async fn send_hands_the_running_bridge_its_message() {
        let state = folder("sends");
        let agent = crate::bridge::Binding {
            identity: crate::bridge::Identity {
                agent: "abcdefghijk1".into(),
                service: String::new(),
                owner: "me@box".into(),
                teams: Vec::new(),
            },
            key: "0123456789abcdef0123456789abcdef".to_string().into(),
        };
        assert!(
            send_through(&state, &agent, "15551234567", "Hi")
                .await
                .unwrap_err()
                .to_string()
                .contains("isn't running")
        );
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = sent.clone();
        let taking = state.clone();
        let bridges = agent.duplicate();
        let keyring = crate::bridge::Keyring::at(&state.join("keys"));
        keyring.bind(&agent.identity, &agent.key).unwrap();
        let bound = keyring.clone();
        tokio::spawn(async move {
            take_sends(
                &taking,
                crate::bridge::Both(bridges, bound),
                |chat, text| {
                    let kept = kept.clone();
                    async move {
                        anyhow::ensure!(chat != "99", "not sending to 99");
                        kept.lock().unwrap().push((chat, text));
                        Ok(())
                    }
                },
            )
            .await
        });
        while !sends_at(&state).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // A call proved, its message never sent: let go in time, the next
        // taken.
        let mut stalled = tokio::net::UnixStream::connect(sends_at(&state))
            .await
            .unwrap();
        let challenge: crate::bridge::Challenge =
            crate::bridge::read_line_async(&mut stalled, 4096, "challenge")
                .await
                .unwrap();
        crate::bridge::write_line_async(&mut stalled, &agent.prove(&challenge.challenge))
            .await
            .unwrap();
        tokio::time::timeout(
            SEND_WAIT + std::time::Duration::from_secs(5),
            send_through(&state, &agent, "15551234567", "Good\nmorning"),
        )
        .await
        .expect("a stalled call held the rest")
        .unwrap();
        let refused = send_through(&state, &agent, "99", "Hi").await.unwrap_err();
        assert!(refused.to_string().contains("not sending to 99"));
        // Another agent, or this one without its key, sends nothing.
        let other = crate::bridge::Binding {
            identity: crate::bridge::Identity {
                agent: "zzzzzzzzzzzz".into(),
                ..agent.identity.clone()
            },
            ..agent.duplicate()
        };
        let guessed = crate::bridge::Binding {
            key: "ffffffffffffffffffffffffffffffff".to_string().into(),
            ..agent.duplicate()
        };
        for stranger in [other, guessed] {
            let refused = send_through(&state, &stranger, "15551234567", "Hi")
                .await
                .unwrap_err();
            assert!(refused.to_string().contains("isn't"), "{refused}");
        }
        // Stopped: its key out of the keyring, the bridge still running,
        // takes nothing more of it.
        keyring.unbind_key(&agent, &link_secret_contract::keeper::RouteRevoker);
        let stopped = send_through(&state, &agent, "15551234567", "Late")
            .await
            .unwrap_err();
        assert!(stopped.to_string().contains("isn't bound"), "{stopped}");
        assert_eq!(
            *sent.lock().unwrap(),
            [("15551234567".to_string(), "Good\nmorning".to_string())]
        );
        let _ = std::fs::remove_dir_all(state);
    }

    /// `send.sock` takes a message only while the machine's keyring holds
    /// the key its bridge started with: once the agent stops (its key
    /// unbound), or starts again with a new one, a bridge still running
    /// takes nothing proved with the old.
    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    async fn send_sock_checks_the_keyring() {
        let state = folder("sends-keyring");
        let keyring = crate::bridge::Keyring::at(&state.join("keys"));
        let identity = crate::bridge::Identity {
            agent: "abcdefghijk1".into(),
            service: String::new(),
            owner: "me@box".into(),
            teams: Vec::new(),
        };
        let agent = keyring
            .bind(
                &identity,
                &link_secretive::Secret::from("0123456789abcdef0123456789abcdef".to_string()),
            )
            .unwrap();
        let taking = state.clone();
        let keys = crate::bridge::Both(agent.duplicate(), keyring.clone());
        tokio::spawn(async move { take_sends(&taking, keys, |_, _| async { Ok(()) }).await });
        while !sends_at(&state).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        send_through(&state, &agent, "1", "Hi").await.unwrap();
        // Started again, with a new key: the old proves nothing here.
        keyring
            .bind(
                &identity,
                &link_secretive::Secret::from("fedcba9876543210fedcba9876543210".to_string()),
            )
            .unwrap();
        let refused = send_through(&state, &agent, "1", "Hi").await.unwrap_err();
        assert!(refused.to_string().contains("isn't bound"), "{refused}");
        // Stopped: unbound.
        keyring.unbind(&identity.agent, &link_secret_contract::keeper::RouteRevoker);
        let refused = send_through(&state, &agent, "1", "Hi").await.unwrap_err();
        assert!(refused.to_string().contains("isn't bound"), "{refused}");
        let _ = std::fs::remove_dir_all(state);
    }

    /// `send.sock` passes the test every bridge passes.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn send_sock_is_a_bridge() {
        let state = folder("sends-conformance");
        let keyring = crate::bridge::Keyring::at(&state.join("keys"));
        let identity = crate::bridge::Identity {
            agent: "abcdefghijk1".into(),
            service: String::new(),
            owner: "me@box".into(),
            teams: Vec::new(),
        };
        let agent = keyring
            .bind(
                &identity,
                &link_secretive::Secret::from("0123456789abcdef0123456789abcdef".to_string()),
            )
            .unwrap();
        let taking = state.clone();
        let keys = crate::bridge::Both(agent.duplicate(), keyring);
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sending = ran.clone();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(take_sends(&taking, keys, |_, _| {
                    sending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async { Ok(()) }
                }))
        });
        while !sends_at(&state).exists() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        link_sandbox_bridge::conformance::check(&link_sandbox_bridge::conformance::Subject {
            socket: sends_at(&state),
            agents: vec![agent],
            action: serde_json::json!({"chat": "1", "text": "Hi"}),
            ran: std::sync::Arc::new(move || ran.load(std::sync::atomic::Ordering::SeqCst)),
            big: None,
            limits: SENDS,
        });
        let _ = std::fs::remove_dir_all(state);
    }
}
