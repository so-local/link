//! Native lifecycle and capture behavior, independent of owner decisions.

use super::*;
use futures_util::poll;
use std::collections::BTreeMap;

#[tokio::test]
async fn explicit_opens_save_native_sessions_before_any_prompt() {
    let dir = temp("hub-explicit-open");
    let asked = Asked::default();
    let (hub, _) = super::hub(&dir, &asked, 1);
    let at = served(hub).await;
    let (first, second, listed) = asking(&at, async |cx| {
        let first = cx
            .send_request(NewSessionRequest::new("/ignored"))
            .block_task()
            .await?;
        let second = cx
            .send_request(NewSessionRequest::new("/ignored"))
            .block_task()
            .await?;
        let listed = cx
            .send_request(ListSessionsRequest::new())
            .block_task()
            .await?;
        Ok((first.session_id, second.session_id, listed.sessions))
    })
    .await
    .unwrap();
    assert_ne!(first, second);
    let bindings = hub_sessions(&dir);
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[&*first.0], "s1");
    assert_eq!(bindings[&*second.0], "s2");
    assert_eq!(*asked.lock().unwrap(), ["new", "new"]);
    assert_eq!(listed.len(), 2);

    // The same logical sessions are available after a host restart.
    let at = served(super::hub(&dir, &asked, 10).0).await;
    asking(&at, async |cx| {
        cx.send_request(ResumeSessionRequest::new(first, "/ignored"))
            .block_task()
            .await?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(asked.lock().unwrap().last().unwrap(), "resume s1");
}

#[tokio::test]
async fn repeating_an_addressed_open_keeps_one_native_binding() {
    let dir = temp("hub-repeated-open");
    let asked = Asked::default();
    let at = served(super::hub(&dir, &asked, 1).0).await;
    asking(&at, async |cx| {
        for _ in 0..2 {
            let meta = Said {
                thread: Some("link:main".into()),
                ..Default::default()
            }
            .into_meta(Meta::new());
            let opened = cx
                .send_request(NewSessionRequest::new("/agent").meta(meta))
                .block_task()
                .await?;
            assert_eq!(&*opened.session_id.0, "link:main");
        }
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(*asked.lock().unwrap(), ["new"]);
    assert_eq!(
        hub_sessions(&dir),
        BTreeMap::from([("link:main".into(), "s1".into())])
    );
}

#[tokio::test]
async fn invalid_explicit_threads_are_refused_before_native_session_creation() {
    let dir = temp("hub-invalid-open");
    let asked = Asked::default();
    let at = served(super::hub(&dir, &asked, 1).0).await;
    let result = asking(&at, async |cx| {
        let meta = Said {
            thread: Some("".into()),
            ..Default::default()
        }
        .into_meta(Meta::new());
        cx.send_request(NewSessionRequest::new("/agent").meta(meta))
            .block_task()
            .await
            .map(drop)
    })
    .await;
    assert!(result.is_err());
    assert!(asked.lock().unwrap().is_empty());
    assert!(!dir.join("sessions.json").exists());
}

#[tokio::test]
async fn cancelling_a_saved_session_does_not_start_a_runtime() {
    let dir = temp("hub-cancel-stopped");
    let (hub, starts) = super::hub(&dir, &Asked::default(), 1);
    hub.sessions.keep("link:main", "native").unwrap();
    hub.cancel("link:main");
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(hub_sessions(&dir)["link:main"], "native");
}

#[test]
fn a_restored_group_can_swap_bindings_without_a_partial_mapping() {
    let dir = temp("hub-binding-swap");
    let sessions = sessions::Sessions::at(dir.join("sessions.json"));
    sessions
        .keep_all(BTreeMap::from([
            ("link:one".into(), "native-one".into()),
            ("link:two".into(), "native-two".into()),
        ]))
        .unwrap();
    let restored = BTreeMap::from([
        ("link:one".into(), "native-two".into()),
        ("link:two".into(), "native-one".into()),
    ]);
    sessions.keep_all(restored.clone()).unwrap();
    assert_eq!(hub_sessions(&dir), restored);
    assert_eq!(
        sessions.of("link:one").unwrap().as_deref(),
        Some("native-two")
    );
    assert_eq!(
        sessions.of("link:two").unwrap().as_deref(),
        Some("native-one")
    );
}

#[test]
fn an_invalid_restored_group_publishes_no_bindings() {
    let dir = temp("hub-invalid-group");
    let path = dir.join("sessions.json");
    let sessions = sessions::Sessions::at(path.clone());
    sessions.keep("link:main", "original").unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(
        sessions
            .keep_all(BTreeMap::from([
                ("link:one".into(), "same".into()),
                ("link:two".into(), "same".into()),
            ]))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(sessions.threads().unwrap(), ["link:main"]);
}

struct BlockingStore {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Store for BlockingStore {
    fn kinds(&self) -> asp::AgentState {
        asp::AgentState {
            export: vec![asp::Kind::Memory],
            ..Default::default()
        }
    }

    fn export(&self, _kinds: &[asp::Kind]) -> anyhow::Result<asp::State> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        self.release.lock().unwrap().recv()?;
        Ok(asp::State::default())
    }

    fn import(&self, _state: asp::State) -> anyhow::Result<asp::methods::Imported> {
        anyhow::bail!("this fixture does not import")
    }
}

#[tokio::test]
async fn a_cancelled_export_caller_cannot_release_a_running_store_capture() {
    let dir = temp("hub-capture-caller");
    let asked = Asked::default();
    let (entered, began) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let (hub, _) = super::hub(&dir, &asked, 1);
    let mut hub = Arc::into_inner(hub).unwrap();
    hub.state = Some(Arc::new(BlockingStore {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    }));
    let hub = Arc::new(hub);
    let capturing = tokio::spawn({
        let hub = hub.clone();
        async move { hub.export(vec![asp::Kind::Memory]).await }
    });
    began.await.unwrap();
    capturing.abort();
    assert!(capturing.await.unwrap_err().is_cancelled());
    let mut next = Box::pin(hub.activity.enter("link:main"));
    assert!(poll!(next.as_mut()).is_pending());
    release.send(()).unwrap();
    next.await;
}

#[tokio::test]
async fn replay_routing_is_restored_after_a_failed_load() {
    let dir = temp("hub-replay-route");
    let asked = Asked::default();
    let hub = Hub::new(Config {
        start: Box::new(move || Ok(fake_options(asked.clone(), 1, true, None, true))),
        cwd: "/agent".into(),
        key: Some(link_secretive::Secret::from(KEY.to_string())),
        agent: Some(AGENT.into()),
        sessions: dir.join("sessions.json"),
        state: None,
        tools: Tools::default(),
    })
    .with_calls(Duration::from_secs(10));
    let up = hub.up().await.unwrap();
    hub.sessions.keep("link:main", "s1").unwrap();
    let (original, mut original_updates) = tokio::sync::mpsc::unbounded_channel();
    up.route(
        "s1",
        harness::Route {
            to: harness::To::Kept(original),
            session: SessionId::new("link:main"),
        },
    );
    let at = served(hub.clone()).await;
    let result = asking(&at, async |cx| {
        cx.send_request(LoadSessionRequest::new(
            SessionId::new("link:main"),
            "/agent",
        ))
        .block_task()
        .await
    })
    .await;
    assert!(result.is_err());
    up.cx
        .send_request(PromptRequest::new(
            SessionId::new("s1"),
            vec![ContentBlock::Text(TextContent::new("still routed"))],
        ))
        .block_task()
        .await
        .unwrap();
    let update = tokio::time::timeout(hub.harness.calls, original_updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(update, SessionUpdate::AgentMessageChunk(_)));
}

#[tokio::test]
async fn cancellation_reaches_the_existing_opened_session_alone() {
    let dir = temp("hub-cancel-open");
    let asked = Asked::default();
    let (hub, starts) = super::hub(&dir, &asked, 1);
    let at = served(hub.clone()).await;
    asking(&at, async |cx| {
        cx.send_request(ResumeSessionRequest::new(
            SessionId::new("link:main"),
            "/agent",
        ))
        .block_task()
        .await?;
        Ok(())
    })
    .await
    .unwrap();
    hub.sessions.keep("link:other", "not-opened").unwrap();
    hub.cancel("link:other");
    hub.cancel("link:main");
    link_test_kit::wait::eventually_async(
        "native cancellation received",
        hub.harness.calls,
        || asked.lock().unwrap().iter().any(|a| a == "cancel s1"),
    )
    .await;
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(*asked.lock().unwrap(), ["new", "cancel s1"]);
}

struct Restored(BTreeMap<String, String>);

impl Restored {
    fn state(&self) -> asp::State {
        asp::State {
            sessions: Some(
                self.0
                    .keys()
                    .map(|id| {
                        asp::forms::Session::of_messages(SessionId::new(id.clone()), None, &[])
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }
}

impl Store for Restored {
    fn kinds(&self) -> asp::AgentState {
        asp::AgentState {
            import: vec![asp::Kind::Sessions],
            ..Default::default()
        }
    }

    fn export(&self, _kinds: &[asp::Kind]) -> anyhow::Result<asp::State> {
        anyhow::bail!("this fixture does not export")
    }

    fn import(&self, _state: asp::State) -> anyhow::Result<asp::methods::Imported> {
        Ok(asp::methods::Imported {
            sessions: self
                .0
                .iter()
                .map(|(old, new)| (SessionId::new(old.clone()), SessionId::new(new.clone())))
                .collect(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn restored_bindings_force_native_resume_in_the_existing_runtime() {
    let dir = temp("hub-restored-open");
    let asked = Asked::default();
    let (hub, starts) = super::hub(&dir, &asked, 1);
    let restored = Arc::new(Restored(BTreeMap::from([
        ("link:one".into(), "s2".into()),
        ("link:two".into(), "s1".into()),
    ])));
    let mut hub = Arc::into_inner(hub).unwrap();
    hub.state = Some(restored.clone());
    let hub = Arc::new(hub);
    let at = served(hub.clone()).await;
    asking(&at, async |cx| {
        for thread in ["link:one", "link:two"] {
            cx.send_request(ResumeSessionRequest::new(SessionId::new(thread), "/agent"))
                .block_task()
                .await?;
        }
        cx.send_request(asp::methods::Import {
            state: restored.state(),
        })
        .block_task()
        .await?;
        for thread in ["link:one", "link:two"] {
            cx.send_request(ResumeSessionRequest::new(SessionId::new(thread), "/agent"))
                .block_task()
                .await?;
        }
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(hub_sessions(&dir), restored.0);
    assert_eq!(
        *asked.lock().unwrap(),
        ["new", "new", "resume s2", "resume s1"]
    );
}

#[tokio::test]
async fn an_invalid_import_leaves_all_saved_bindings_unchanged() {
    let dir = temp("hub-restored-alias");
    let asked = Asked::default();
    let (hub, _) = super::hub(&dir, &asked, 1);
    let restored = Arc::new(Restored(BTreeMap::from([
        ("link:one".into(), "same".into()),
        ("link:two".into(), "same".into()),
    ])));
    let mut hub = Arc::into_inner(hub).unwrap();
    hub.state = Some(restored.clone());
    hub.sessions.keep("link:main", "original").unwrap();
    let original = std::fs::read(dir.join("sessions.json")).unwrap();
    let result = hub.import(restored.state()).await;
    assert!(result.is_err());
    assert_eq!(std::fs::read(dir.join("sessions.json")).unwrap(), original);
    assert_eq!(hub.sessions.threads().unwrap(), ["link:main"]);
    assert_eq!(
        hub.sessions.of("link:main").unwrap().as_deref(),
        Some("original")
    );
}
