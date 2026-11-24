//! The endpoint's key: a connection without it is refused, a harness
//! with none lets no one in, and a connection's challenge is answered with
//! a proof of it.

use crate::common;
use crate::turns::{client, keep_config, world};

use link_harness::config::Config;
use link_harness_contract::harness::Model;

#[tokio::test]
async fn it_refuses_a_caller_without_the_key() {
    let world = world("key").await;
    for auth in [None, Some("Bearer wrong"), Some("Bearer the-ke")] {
        assert_eq!(
            common::opens(&world.endpoint, auth, &[]).await,
            401,
            "{auth:?}"
        );
    }
    let asking = common::Asking {
        key: "the-key",
        ..Default::default()
    };
    // A thread that would be a path is refused.
    let refused = common::ask_at(&world.endpoint, "../x", "hi", asking).await;
    assert!(refused.unwrap_err().contains("isn't a thread"));
    assert!(world.asked.lock().unwrap().is_empty());

    // With the key: a whole answer.
    let answer = common::ask_at(&world.endpoint, "", "hi", asking).await;
    assert_eq!(answer.unwrap(), "Answer 1");
    assert_eq!(
        common::conversation_events(&world.root.join("files/chats/default")).len(),
        2
    );
}

/// With no key set, it lets no one in: 503, never an open endpoint.
#[tokio::test]
async fn it_refuses_everyone_while_it_has_no_key() {
    let world = world("no-key").await;
    let (home, files) = (world.root.join("home"), world.root.join("files"));
    keep_config(
        &home,
        &files,
        Config {
            model: Model {
                provider: "local".into(),
                name: Some("fake".into()),
                ..Model::default()
            },
            endpoint_key: None,
            spec: None,
            ..Config::default()
        },
    );
    for auth in [None, Some("Bearer the-key"), Some("Bearer ")] {
        assert_eq!(
            common::opens(&world.endpoint, auth, &[]).await,
            503,
            "{auth:?}"
        );
    }
    assert!(world.asked.lock().unwrap().is_empty());
}

/// It answers a connection's challenge with its key, and only a challenge:
/// what it sends proves it holds the key without giving it away.
#[tokio::test]
async fn it_proves_it_holds_the_key() {
    use link_harness_contract::harness::{
        CHALLENGE_HEADER, PROOF_HEADER, PROOF_PATH, proof_matches,
    };
    let world = world("proof").await;
    let origin = world
        .endpoint
        .trim_end_matches(link_connection_contract::acp::ACP_PATH);
    let challenge = link_files::random_token().unwrap();
    let response = client()
        .get(format!("{origin}{PROOF_PATH}"))
        .header(CHALLENGE_HEADER, &challenge)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let proof = response.headers()[PROOF_HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert!(proof_matches(
        &link_secretive::Secret::from("the-key".to_string()),
        &challenge,
        &proof
    ));
    assert!(!response.text().await.unwrap().contains("the-key"));
    // No challenge, or one that isn't: nothing.
    for challenge in [None, Some("the-key"), Some("../x")] {
        let mut request = client().get(format!("{origin}{PROOF_PATH}"));
        if let Some(challenge) = challenge {
            request = request.header(CHALLENGE_HEADER, challenge);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 400, "{challenge:?}");
        assert!(response.headers().get(PROOF_HEADER).is_none());
    }
    assert!(world.asked.lock().unwrap().is_empty());
}
