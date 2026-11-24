use super::*;
use link_interactions::{runtime::WorkAdmission, sources, *};
use std::time::Duration;

struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn source_observation_survives_client_disconnect_and_settles_the_original_call() {
    let root = link_test_kit::TempDir::new("observed-source-call");
    let store = Store::at(&*root);
    let binding = store
        .register_work(WorkAdmission {
            owner_route: Some("link:main".into()),
            kind: WorkKind::Conversation,
            owner: sources::owner(),
            agent: "main".into(),
            routing_agent: "main".into(),
            session: "link:main".into(),
            attempt: "one".into(),
            parent_attempt: None,
            checkpoint: Some("original".into()),
        })
        .unwrap();
    let application = store
        .policy_call(
            &binding.state.work,
            &SourceId::new("sandbox").unwrap(),
            "fake/write",
            "call",
            b"write",
        )
        .unwrap();
    let (client, server) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(client);
    let session = Arc::new(Session {
        tool: "fake".into(),
        started: Started {
            argv: vec![],
            env: BTreeMap::new(),
            shared: false,
        },
        stdin: tokio::sync::Mutex::new(Box::new(write)),
        pending: Mutex::default(),
        hints: Mutex::default(),
        used: Mutex::new(std::time::Instant::now()),
        child: Mutex::new(None),
    });
    let _reader = Task(tokio::spawn(read_server(Arc::downgrade(&session), read)));
    let (ready, mut accepted) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let _server = Task(tokio::spawn(async move {
        let mut server = tokio::io::BufReader::new(server);
        let mut line = String::new();
        server.read_line(&mut line).await.unwrap();
        assert!(line.contains("write"));
        ready.send(()).unwrap();
        released.await.unwrap();
        server
            .get_mut()
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n")
            .await
            .unwrap();
    }));
    let source = session.clone();
    let caller = tokio::spawn(async move {
        source
            .observed_request(
                &json!({"id":7,"method":"tools/call","params":{"name":"write"}}),
                vec![],
                Some(application),
            )
            .await
    });
    link_test_kit::wait::eventually_async(
        "source accepted the exact call",
        Duration::from_secs(10),
        || accepted.try_recv().ok(),
    )
    .await;
    caller.abort();
    let _ = caller.await;
    owner::command(
        &store,
        &sources::owner(),
        "main",
        "link:main",
        "/stop",
        TransportEventId::new("stop").unwrap(),
    )
    .unwrap();
    store
        .finish_work(binding.token.expose(), Some("progress".into()))
        .unwrap();
    assert_eq!(
        store.works("main").unwrap()[0].state.observed,
        control::Execution::Unknown
    );
    release.send(()).unwrap();
    link_test_kit::wait::eventually_async(
        "source confirmed the original outcome",
        Duration::from_secs(10),
        || store.works("main").unwrap()[0].state.observed == control::Execution::Stopped,
    )
    .await;
    assert!(
        store.works("main").unwrap()[0]
            .state
            .in_flight_effects
            .is_empty()
    );
}
