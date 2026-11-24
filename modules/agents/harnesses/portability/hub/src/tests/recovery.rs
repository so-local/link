//! Saved native bindings survive refused resumes and failed persistence.

use super::*;

#[derive(Clone, Copy)]
enum Recovery {
    Resume,
    Load,
    Unsupported,
}

fn refused(asked: Asked, recovery: Recovery) -> Pipes {
    let (hub, native) = tokio::io::duplex(1 << 16);
    let (read, write) = tokio::io::split(native);
    tokio::spawn(async move {
        let resumed = asked.clone();
        let loaded = asked.clone();
        let _ = sdk::Agent
            .builder()
            .on_receive_request(
                async move |_asked: InitializeRequest, responder, _cx| {
                    let caps = match recovery {
                        Recovery::Load => AgentCapabilities::new().load_session(true),
                        Recovery::Resume => AgentCapabilities::new().session_capabilities(
                            SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
                        ),
                        Recovery::Unsupported => AgentCapabilities::new(),
                    };
                    responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(caps),
                    )
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: ResumeSessionRequest, responder, _cx| {
                    resumed
                        .lock()
                        .unwrap()
                        .push(format!("resume {}", request.session_id.0));
                    responder.respond_with_result(Err(
                        sdk::Error::invalid_params().data("missing native session")
                    ))
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: LoadSessionRequest, responder, _cx| {
                    loaded
                        .lock()
                        .unwrap()
                        .push(format!("load {}", request.session_id.0));
                    responder.respond_with_result(Err(
                        sdk::Error::invalid_params().data("missing native session")
                    ))
                },
                sdk::on_receive_request!(),
            )
            .on_receive_request(
                async move |_request: NewSessionRequest, responder, _cx| {
                    asked.lock().unwrap().push("new".into());
                    responder.respond(NewSessionResponse::new(SessionId::new("replacement")))
                },
                sdk::on_receive_request!(),
            )
            .connect_to(harness::lines::<sdk::Agent>(
                Box::new(read),
                Box::new(write),
                None,
            ))
            .await;
    });
    let (read, write) = tokio::io::split(hub);
    Pipes {
        read: Box::new(read),
        write: Box::new(write),
        child: None,
    }
}

#[tokio::test]
async fn a_refused_resume_preserves_the_binding_and_never_starts_an_empty_replacement() {
    for recovery in [Recovery::Resume, Recovery::Load, Recovery::Unsupported] {
        let dir = temp("hub-resume-refused");
        let path = dir.join("sessions.json");
        sessions::Sessions::at(path.clone())
            .keep("link:main", "original")
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let asked = Asked::default();
        let hub = Hub::new(Config {
            start: Box::new({
                let asked = asked.clone();
                move || Ok(refused(asked.clone(), recovery))
            }),
            cwd: "/agent".into(),
            key: Some(link_secretive::Secret::from(KEY.to_string())),
            agent: Some(AGENT.into()),
            sessions: path.clone(),
            state: None,
            tools: Tools::default(),
        });
        let at = served(hub).await;
        let (heard, result) = say(&at, "link:main", "continue", Said::default()).await;
        let error = result.unwrap_err().to_string();
        assert!(error.contains("binding is preserved"), "{error}");
        assert!(!heard.taken && heard.pieces.is_empty(), "{heard:?}");
        let operations: &[&str] = match recovery {
            Recovery::Load => &["load original"],
            Recovery::Resume => &["resume original"],
            Recovery::Unsupported => &[],
        };
        assert_eq!(*asked.lock().unwrap(), operations);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        // A runtime that accepts the saved session can still resume it exactly.
        let at = served(super::hub(&dir, &asked, 10).0).await;
        say(&at, "link:main", "back", Said::default())
            .await
            .1
            .unwrap();
        assert_eq!(asked.lock().unwrap().last().unwrap(), "original: back");
    }
}

#[test]
fn invalid_binding_metadata_is_refused_without_overwriting_it() {
    for invalid in [
        "{",
        "[]",
        r#"{"link:main":null}"#,
        r#"{"link:main":""}"#,
        r#"{"link:main":"same","link:other":"same"}"#,
        r#"{"":"native"}"#,
    ] {
        let dir = temp("hub-invalid-bindings");
        let path = dir.join("sessions.json");
        std::fs::write(&path, invalid).unwrap();
        let sessions = sessions::Sessions::at(path.clone());
        assert!(sessions.of("link:main").is_err(), "{invalid}");
        assert!(sessions.threads().is_err(), "{invalid}");
        assert!(sessions.thread_of("native").is_err(), "{invalid}");
        assert!(sessions.keep("link:main", "new").is_err(), "{invalid}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), invalid);
    }
}

#[test]
fn an_unreadable_binding_file_is_not_an_empty_map() {
    let dir = temp("hub-unreadable-bindings");
    let path = dir.join("sessions.json");
    std::fs::create_dir(&path).unwrap();
    let sessions = sessions::Sessions::at(path);
    assert!(sessions.of("link:main").is_err());
    assert!(sessions.keep("link:main", "new").is_err());
}

#[test]
fn a_failed_write_does_not_publish_a_new_binding_in_memory() {
    let dir = temp("hub-binding-write");
    let path = dir.join("sessions.json");
    let sessions = sessions::Sessions::at(path.clone());
    sessions.keep("link:main", "original").unwrap();
    std::fs::remove_file(&path).unwrap();
    // Rename onto a directory fails for root and ordinary users alike.
    std::fs::create_dir(&path).unwrap();
    assert!(sessions.keep("link:main", "replacement").is_err());
    assert!(sessions.keep("link:other", "another").is_err());
    assert_eq!(
        sessions.of("link:main").unwrap().as_deref(),
        Some("original")
    );
    assert!(sessions.of("link:other").unwrap().is_none());
    assert_eq!(sessions.threads().unwrap(), ["link:main"]);
}

#[test]
fn a_native_session_cannot_leak_between_threads() {
    let dir = temp("hub-binding-alias");
    let path = dir.join("sessions.json");
    let sessions = sessions::Sessions::at(path.clone());
    sessions.keep("link:main", "native").unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(sessions.keep("link:other", "native").is_err());
    assert_eq!(
        sessions.thread_of("native").unwrap().as_deref(),
        Some("link:main")
    );
    assert!(sessions.of("link:other").unwrap().is_none());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[tokio::test]
async fn corrupt_bindings_refuse_a_prompt_before_the_runtime_creates_a_session() {
    let dir = temp("hub-corrupt-prompt");
    let path = dir.join("sessions.json");
    std::fs::write(&path, "{").unwrap();
    let asked = Asked::default();
    let (hub, starts) = super::hub(&dir, &asked, 1);
    assert!(!hub.healthy().await);
    let at = served(hub).await;
    let (heard, result) = say(&at, "link:main", "continue", Said::default()).await;
    assert!(result.is_err());
    assert!(!heard.taken);
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{");
}
