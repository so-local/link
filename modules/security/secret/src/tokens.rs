//! A caller's own tokens for a host, made outside the sandbox by the
//! trusted bridge: traded for an assertion, never cached, so every call
//! rechecks the assertion's current authority at its issuer; or signed by
//! its machine, as the module that keeps the caller answers
//! `secret.token`, kept in this process until a minute before it ends.

use link_api::types::{AccessToken, AgentTokenRequest};
use link_secret_contract::route::{Assertion, TOKEN, Token, TokenAsk};
use link_secretive::{Duplicate, Secret};

pub(crate) fn exchange(assertion: &Assertion, host: &str, grants: &str) -> anyhow::Result<Secret> {
    let request: AgentTokenRequest = serde_json::from_value(serde_json::json!({
        "resource": host, "grants": grants,
    }))?;
    let answer: AccessToken = link_fetch::blocking::Client::new()?
        .post(&assertion.url)
        .bearer(&assertion.proof)
        .json(&request)
        .answer()?;
    anyhow::ensure!(
        answer.expires_at > link_api::jwt::now(),
        "the issuer returned an expired token"
    );
    Ok(answer.access_token)
}

/// How long before its end a kept machine-signed token is made again: a
/// call made with one never meets its end on the way.
const MARGIN: u64 = 60;

/// Each workload's machine-signed tokens, by workload, host and grants,
/// with when each ends.
type Tokens = std::collections::HashMap<(String, String, String), (Secret, u64)>;

static TOKENS: std::sync::Mutex<Option<Tokens>> = std::sync::Mutex::new(None);

/// The own access token of the workload whose ID its binding names, for
/// `host`, granted `grants`, as the module that keeps it answers
/// [`TOKEN`]: kept in this process, never in a file and never handed into
/// the sandbox, until a minute before it ends.
pub(crate) fn installation_signed(
    workload: &str,
    host: &str,
    grants: &str,
) -> anyhow::Result<Secret> {
    let key = (workload.to_string(), host.to_string(), grants.to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    {
        let mut tokens = TOKENS.lock().unwrap_or_else(|e| e.into_inner());
        let tokens = tokens.get_or_insert_with(Tokens::new);
        tokens.retain(|_, (_, ends)| *ends > now + MARGIN);
        if let Some((token, _)) = tokens.get(&key) {
            return Ok(token.duplicate());
        }
    }
    // Made with the lock let go, so one workload's wait holds up no
    // other's; two calls of one at once may each make one, the later kept.
    let made = made(
        &TOKEN.ask(&TokenAsk {
            workload: workload.to_string(),
            host: host.to_string(),
            grants: grants.to_string(),
        }),
        host,
    )?;
    let token = made.token.duplicate();
    let mut tokens = TOKENS.lock().unwrap_or_else(|e| e.into_inner());
    let tokens = tokens.get_or_insert_with(Tokens::new);
    tokens.insert(key, (made.token, made.expires_at.unwrap_or(now)));
    Ok(token)
}

/// The token of the first of `answers` that made one, else an error with
/// each one's why.
fn made(
    answers: &[link_core::answers::Answer<Option<Token>>],
    host: &str,
) -> anyhow::Result<Token> {
    let mut why = Vec::new();
    for answer in answers {
        match &answer.reply {
            Ok(Some(token)) => return Ok(token.duplicate()),
            Ok(None) => {}
            Err(err) => why.push(format!("{}: {err:#}", answer.module)),
        }
    }
    anyhow::bail!(
        "no module here made its caller a token for {}{}",
        link_format::clean(host),
        match why.is_empty() {
            true => String::new(),
            false => format!(": {}", link_format::clean(&why.join("; "))),
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_secretive::Duplicate;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Serving(Option<tokio::task::JoinHandle<Vec<serde_json::Value>>>);
    impl Drop for Serving {
        fn drop(&mut self) {
            if let Some(worker) = self.0.take() {
                worker.abort();
            }
        }
    }

    #[tokio::test]
    async fn every_assertion_trade_rechecks_authority_without_exposing_or_caching_it() {
        let held = link_test_kit::net::Held::loopback();
        let address = held.address();
        let listener = held.listener();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let mut worker = Serving(Some(tokio::spawn(async move {
            let mut bodies = Vec::new();
            for status in ["200 OK", "401 Unauthorized"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break request.len();
                    }
                };
                let head = String::from_utf8(request).unwrap();
                assert!(
                    head.to_ascii_lowercase()
                        .contains("authorization: bearer binding-proof\r\n")
                );
                let length: usize = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await.unwrap();
                assert!(end > 0);
                bodies.push(serde_json::from_slice::<serde_json::Value>(&body).unwrap());
                let body = serde_json::json!({"access_token": "eyJ.caller.token", "expires_at": link_api::jwt::now() + 30}).to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            bodies
        })));
        let assertion = Assertion {
            url: format!("http://{address}/token"),
            proof: "binding-proof".to_string().into(),
        };
        assert!(!format!("{assertion:?}").contains("binding-proof"));
        let proof = assertion.duplicate();
        let first = tokio::task::spawn_blocking(move || {
            exchange(&proof, "dispatch.test", "team.member@team:t4m9q2x7k1ab")
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(first.expose(), "eyJ.caller.token");
        let second = tokio::task::spawn_blocking(move || {
            exchange(&assertion, "dispatch.test", "team.member@team:t4m9q2x7k1ab")
        })
        .await
        .unwrap();
        assert!(
            second.is_err(),
            "the ended binding cannot reuse its previous token"
        );
        assert_eq!(
            worker.0.take().unwrap().await.unwrap(),
            vec![
                serde_json::json!({"resource": "dispatch.test", "grants": "team.member@team:t4m9q2x7k1ab"});
                2
            ]
        );
    }
}
