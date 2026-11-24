//! An agent's own webhooks, end to end, through this module: `lnk agent
//! webhook create` gives the agent on the mini harness two, one signed as
//! Link signs and one as GitHub does (`--sender github`), and its bridge
//! takes their posts on 127.0.0.1, sifted by this folder's `lnk-webhook-connection`.
//! Each signed post reaches the model as a stranger's message from
//! `webhook:<name>`; a stranger's, a replayed and an unsigned post are
//! refused, as is one that is neither JSON nor text; a rolled webhook's
//! previous secret is taken until `roll --done`.

#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use link_harness_conformance::agent::{self, Agent};
use link_secretive::OpaqueSecret;
use link_secretive::inbound::Scheme;

/// The programs the agent runs: the core's (`lnk`, `lnk-agent`,
/// `lnk-mini-harness`, built in `link-core` beside this folder), and this
/// folder's `lnk-webhook-connection`.
fn programs() -> Vec<PathBuf> {
    agent::programs_beside(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new(env!("CARGO_BIN_EXE_lnk-webhook-connection")),
    )
}

/// Runs `lnk agent webhook <args>` for agent `main` in `home`: its
/// output, which must succeed.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn webhook(programs: &[PathBuf], home: &Path, args: &[&str]) -> String {
    let bin = &programs[0];
    let path = programs
        .iter()
        .map(|p| p.display().to_string())
        .chain(["/usr/bin".into(), "/bin".into()])
        .collect::<Vec<_>>()
        .join(":");
    let output = Command::new(bin.join("lnk-agent"))
        .args(["webhook"])
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
        .env("PATH", path)
        .env("LNK_EXE", bin.join("lnk"))
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(home))
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{args:?}: {text}");
    text
}

/// What `add` or `roll` printed: the address, and the secret.
fn made(printed: &str) -> (String, String) {
    let line = |label: &str| {
        printed
            .lines()
            .find_map(|l| l.trim().strip_prefix(label))
            .unwrap_or_else(|| panic!("no {label} in {printed}"))
            .trim()
            .to_string()
    };
    (line("Address:"), line("Secret:"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Posts `body` as `content_type` to `address`, signed with `secret` as
/// `scheme` says with delivery id `id`, or unsigned with none: the status.
async fn post(
    address: &str,
    signed: Option<(Scheme, &str)>,
    id: &str,
    content_type: &str,
    body: &str,
) -> u16 {
    link_fetch::install_crypto_provider();
    let mut request = reqwest::Client::new()
        .post(address)
        .body(body.to_string())
        .header("content-type", content_type);
    if let Some((scheme, secret)) = signed {
        let secret = OpaqueSecret::from_text(secret.to_string().into());
        for (name, value) in scheme.sign(&secret, now(), id, body.as_bytes()).unwrap() {
            request = request.header(name, value);
        }
    }
    request.send().await.unwrap().status().as_u16()
}

/// Waits until the model was asked a message whose last line holds
/// `text`; the whole message.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn asked(agent: &mut Agent, text: &str) -> String {
    let start = Instant::now();
    loop {
        let found = agent.asked.lock().unwrap().iter().find_map(|a| {
            let messages = a["messages"].as_array()?;
            let last = messages.last()?["content"].as_str()?;
            last.contains(text).then(|| last.to_string())
        });
        if let Some(found) = found {
            return found;
        }
        agent.check(start, &format!("the model wasn't asked {text:?}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Waits until something listens at `address`'s port on 127.0.0.1.
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn listening(agent: &mut Agent, address: &str) {
    let port: u16 = address
        .trim_start_matches("http://127.0.0.1:")
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let start = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        agent.check(start, "the bridge doesn't listen for its webhooks");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_posts_reach_the_agent_and_no_others() {
    let programs = programs();
    let mut hooks = None;
    let mut agent = agent::start(&programs, "webhook", false, "", &[], |home, _| {
        let deploy = made(&webhook(&programs, home, &["create", "deploy"]));
        let repo = made(&webhook(
            &programs,
            home,
            &["create", "repo", "--sender", "github"],
        ));
        hooks = Some((deploy, repo));
    })
    .await;
    let ((deploy, secret), (repo, repo_secret)) = hooks.unwrap();
    assert!(deploy.starts_with("http://127.0.0.1:"), "{deploy}");
    listening(&mut agent, &deploy).await;
    let json = "application/json";

    // Signed as Link signs: a stranger's message to the agent.
    let link = Some((Scheme::LINK, secret.as_str()));
    let body = r#"{"ref":"main"}"#;
    assert_eq!(post(&deploy, link, "d-1", json, body).await, 202);
    let message = asked(&mut agent, body).await;
    assert!(
        message.starts_with("(from webhook:deploy, not your owner:)"),
        "{message}"
    );
    // Signed as GitHub signs, at the other.
    let github = Some((Scheme::GITHUB, repo_secret.as_str()));
    let pushed = r#"{"pusher":"dana"}"#;
    assert_eq!(post(&repo, github, "g-1", json, pushed).await, 202);
    let message = asked(&mut agent, pushed).await;
    assert!(
        message.starts_with("(from webhook:repo, not your owner:)"),
        "{message}"
    );
    // Text too.
    let text = "text/plain; charset=utf-8";
    assert_eq!(post(&deploy, link, "d-2", text, "deployed").await, 202);
    asked(&mut agent, "> deployed").await;

    // A stranger's: another secret, or one hook's at another.
    let stranger = "0".repeat(64);
    let strangers = Some((Scheme::LINK, stranger.as_str()));
    assert_eq!(post(&deploy, strangers, "d-3", json, body).await, 401);
    assert_eq!(post(&repo, link, "d-4", json, body).await, 401);
    // Replayed: the same delivery again, and GitHub's body under another.
    assert_eq!(post(&deploy, link, "d-1", json, body).await, 401);
    assert_eq!(post(&repo, github, "g-2", json, pushed).await, 401);
    // Unsigned: without even the time Link's scheme signs, malformed.
    assert_eq!(post(&deploy, None, "d-5", json, body).await, 400);
    assert_eq!(post(&repo, None, "g-3", json, r#"{"n":3}"#).await, 401);
    // Signed, but neither JSON nor text: GitHub's form, which its
    // settings offer.
    let form = "application/x-www-form-urlencoded";
    assert_eq!(
        post(&repo, github, "g-4", form, "payload=%7B%7D").await,
        415
    );

    // Rolled: the new secret and the previous one taken, until `--done`.
    let (address, rolled) = made(&webhook(&programs, &agent.home, &["rotate", "deploy"]));
    assert_eq!(address, deploy, "the address stays");
    let new = Some((Scheme::LINK, rolled.as_str()));
    assert_eq!(post(&deploy, new, "d-6", text, "new").await, 202);
    assert_eq!(post(&deploy, link, "d-7", text, "old").await, 202);
    asked(&mut agent, "> new").await;
    asked(&mut agent, "> old").await;
    webhook(&programs, &agent.home, &["rotate", "deploy", "--done"]);
    assert_eq!(post(&deploy, link, "d-8", text, "older").await, 401);
    assert_eq!(post(&deploy, new, "d-9", text, "after").await, 202);
    asked(&mut agent, "> after").await;

    // Nothing refused reached the model; the model never saw a secret.
    let seen = serde_json::to_string(&*agent.asked.lock().unwrap()).unwrap();
    for refused in ["payload", "older", r#"{\"n\":3}"#] {
        assert!(!seen.contains(refused), "{refused} reached the model");
    }
    for secret in [&secret, &rolled, &repo_secret] {
        assert!(
            !seen.contains(secret.as_str()),
            "a secret reached the model"
        );
    }
    agent.stop();
}
