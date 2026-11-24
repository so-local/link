//! A storage credential that refreshes itself, for a bridge that reads
//! and writes a cloud's storage for an agent (the files bridge): an
//! access token, when it ends, where and with what to get the next one
//! (OAuth 2.0's refresh, RFC 6749, section 6), and, optionally, how many
//! bytes its holder may still add. Whatever binds the agent hands it to
//! the bridge, outside every sandbox; the bridge never learns a cloud's
//! own sign-in, only a token of minutes it can't outlive.
//!
//! [`Held`] keeps one: it refreshes the token once two thirds of its
//! life have passed, takes the limit each refresh says, and refuses a
//! write that would pass the room left before anything is sent
//! ([`Held::reserve`]). A grant the cloud no longer takes (revoked, or
//! its binding over) is an error, and every later call is too.

use link_secretive::Duplicate;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::bail;
use serde::{Deserialize, Serialize};

use link_secretive::Secret;

/// What a bridge is handed. Its tokens are never printed.
#[derive(Duplicate, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSecret {
    /// The access token, sent as Bearer.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub token: Secret,
    /// When it ends, in Unix seconds.
    pub expires: u64,
    /// Where the next one is asked: `POST`, form-encoded,
    /// `grant_type=refresh_token&refresh_token=<grant>`.
    pub refresh_url: String,
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub refresh_token: Secret,
    /// The bytes its holder may still add; none, no limit of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headroom_bytes: Option<u64>,
}

/// A refresh, as OAuth asks it (RFC 6749, section 6), the grant in it.
#[derive(Serialize)]
struct RefreshGrant {
    grant_type: &'static str,
    #[serde(with = "link_secretive::bare")]
    refresh_token: Secret,
}

/// The refresh's answer, as OAuth writes it, and the room left.
#[derive(Deserialize)]
struct Refreshed {
    access_token: Secret,
    expires_in: u64,
    #[serde(default)]
    headroom_bytes: Option<u64>,
}

/// A write refused for room: what it needed and what's left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full {
    pub wanted: u64,
    pub left: u64,
}

impl std::fmt::Display for Full {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "there's no room for that: it's {} and {} is left. Delete some files, or make \
             room where they're kept",
            bytes(self.wanted),
            bytes(self.left)
        )
    }
}

impl std::error::Error for Full {}

fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / f64::from(1u32 << 30)),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / f64::from(1u32 << 20)),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} bytes"),
    }
}

struct State {
    credential: StoreSecret,
    /// When the token was got, in Unix seconds.
    got: u64,
    /// Why the grant stopped working, once it did.
    ended: Option<String>,
}

/// A credential held by a bridge.
pub struct Held {
    state: Mutex<State>,
    client: link_fetch::Client,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Held {
    pub fn new(credential: StoreSecret) -> anyhow::Result<Self> {
        anyhow::ensure!(
            refreshes_safely(&credential.refresh_url),
            "a credential refreshes over https"
        );
        Ok(Self {
            state: Mutex::new(State {
                credential,
                got: unix_now(),
                ended: None,
            }),
            // OAuth's refresh (RFC 6749, section 5.1), whose answer may
            // hold fields of the authorization server's own: read open.
            client: link_fetch::Client::builder()
                .timeout(Duration::from_secs(20))
                .outside()
                .build()?,
            now: Box::new(unix_now),
        })
    }

    /// The same, on another clock (tests).
    pub fn with_clock(mut self, now: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        let got = now();
        self.state.lock().unwrap_or_else(|e| e.into_inner()).got = got;
        self.now = Box::new(now);
        self
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The token to send now: refreshed first once two thirds of its life
    /// have passed.
    pub async fn token(&self) -> anyhow::Result<Secret> {
        let due = {
            let state = self.state();
            if let Some(why) = &state.ended {
                bail!("{why}");
            }
            let life = state.credential.expires.saturating_sub(state.got);
            (self.now)() >= state.got + life * 2 / 3
        };
        if due {
            self.refresh().await?;
        }
        Ok(self.state().credential.token.duplicate())
    }

    /// Asks for the next token now, and the room left with it.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let (url, grant) = {
            let state = self.state();
            if let Some(why) = &state.ended {
                bail!("{why}");
            }
            (
                state.credential.refresh_url.clone(),
                state.credential.refresh_token.duplicate(),
            )
        };
        let answer = self
            .client
            .post(&url)
            .form(&RefreshGrant {
                grant_type: "refresh_token",
                refresh_token: grant,
            })
            .answer::<Refreshed>()
            .await;
        let refreshed = match answer {
            Ok(refreshed) => refreshed,
            Err(err) if matches!(err.status(), Some(400 | 401)) => {
                let said = err.refusal().map(|r| r.reason()).unwrap_or_default();
                let why = format!(
                    "the storage token's grant is no longer taken ({})",
                    said.chars().take(200).collect::<String>()
                );
                self.state().ended = Some(why.clone());
                bail!("{why}");
            }
            Err(err) => {
                return Err(anyhow::Error::from(err).context("refreshing the storage token"));
            }
        };
        let now = (self.now)();
        let mut state = self.state();
        state.credential.token = refreshed.access_token;
        state.credential.expires = now + refreshed.expires_in;
        state.got = now;
        if state.credential.headroom_bytes.is_some() || refreshed.headroom_bytes.is_some() {
            state.credential.headroom_bytes = Some(refreshed.headroom_bytes.unwrap_or(0));
        }
        Ok(())
    }

    /// Takes `bytes` of the room left before a write that adds them: an
    /// error ([`Full`]) when they'd pass it, and nothing is taken. With no
    /// limit, every write is taken.
    pub fn reserve(&self, bytes: u64) -> Result<(), Full> {
        let mut state = self.state();
        match &mut state.credential.headroom_bytes {
            None => Ok(()),
            Some(left) if bytes > *left => Err(Full {
                wanted: bytes,
                left: *left,
            }),
            Some(left) => {
                *left -= bytes;
                Ok(())
            }
        }
    }

    /// Gives back `bytes` taken for a write that failed, or freed by a
    /// delete.
    pub fn release(&self, bytes: u64) {
        if let Some(left) = &mut self.state().credential.headroom_bytes {
            *left = left.saturating_add(bytes);
        }
    }

    /// The room left, when the credential has a limit.
    pub fn headroom(&self) -> Option<u64> {
        self.state().credential.headroom_bytes
    }
}

/// Whether `url` is HTTPS, or HTTP to this machine alone, which never
/// leaves it.
fn refreshes_safely(url: &str) -> bool {
    link_fetch::may_send_to(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A refresh endpoint on this machine: each request's body, and its
    /// answers in turn.
    fn endpoint(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1/storage/token",
            listener.local_addr().unwrap().port()
        );
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                seen.lock().unwrap().push(String::from_utf8(sent).unwrap());
                let mut stream = reader.into_inner();
                // A body `Location: <url>` is a redirect there.
                let (extra, body) = match body.strip_prefix("Location: ") {
                    Some(url) => (format!("location: {url}\r\n"), String::new()),
                    None => (String::new(), body),
                };
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\n{extra}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (url, asked)
    }

    fn credential(url: &str, headroom: Option<u64>) -> StoreSecret {
        StoreSecret {
            token: "first".to_string().into(),
            expires: 1_000 + 600,
            refresh_url: url.into(),
            refresh_token: "grant-ab+cd".to_string().into(),
            headroom_bytes: headroom,
        }
    }

    #[tokio::test]
    async fn a_token_is_refreshed_at_two_thirds_of_its_life() {
        let (url, asked) = endpoint(vec![
            (200, r#"{"access_token": "second", "token_type": "Bearer", "expires_in": 600, "headroom_bytes": 50}"#.into()),
            (200, r#"{"access_token": "third", "token_type": "Bearer", "expires_in": 600, "headroom_bytes": 70}"#.into()),
        ]);
        let now = Arc::new(AtomicU64::new(1_000));
        let clock = now.clone();
        let held = Held::new(credential(&url, Some(100)))
            .unwrap()
            .with_clock(move || clock.load(Ordering::SeqCst));
        assert_eq!(
            held.token().await.unwrap(),
            link_secretive::Secret::from("first".to_string())
        );
        now.store(1_399, Ordering::SeqCst);
        assert_eq!(
            held.token().await.unwrap(),
            link_secretive::Secret::from("first".to_string())
        );
        now.store(1_400, Ordering::SeqCst);
        assert_eq!(
            held.token().await.unwrap(),
            link_secretive::Secret::from("second".to_string())
        );
        assert_eq!(held.headroom(), Some(50));
        // Three tokens' lives and more, each refreshed in time.
        now.store(1_800, Ordering::SeqCst);
        assert_eq!(
            held.token().await.unwrap(),
            link_secretive::Secret::from("third".to_string())
        );
        assert_eq!(held.headroom(), Some(70));
        let asked = asked.lock().unwrap();
        assert_eq!(
            asked[0],
            "grant_type=refresh_token&refresh_token=grant-ab%2Bcd"
        );
    }

    #[tokio::test]
    async fn a_write_past_the_room_left_is_refused_before_it_is_sent() {
        let (url, _) = endpoint(Vec::new());
        let held = Held::new(credential(&url, Some(100))).unwrap();
        held.reserve(60).unwrap();
        let full = held.reserve(50).unwrap_err();
        assert_eq!(
            full,
            Full {
                wanted: 50,
                left: 40
            }
        );
        assert!(
            full.to_string().contains("50 bytes and 40 bytes is left"),
            "{full}"
        );
        held.release(60);
        held.reserve(100).unwrap();
        // With no limit, every write is taken.
        let unlimited = Held::new(credential(&url, None)).unwrap();
        unlimited.reserve(u64::MAX).unwrap();
    }

    #[tokio::test]
    async fn a_grant_no_longer_taken_ends_the_credential() {
        let (url, _) = endpoint(vec![(400, r#"{"error": "invalid_grant"}"#.into())]);
        let held = Held::new(credential(&url, Some(100))).unwrap();
        let err = held.refresh().await.unwrap_err().to_string();
        assert!(err.contains("invalid_grant"), "{err}");
        assert!(held.token().await.is_err(), "and every call after");
        // Never printed.
        let shown = format!("{:?}", credential(&url, None));
        assert!(
            !shown.contains("first") && !shown.contains("grant-"),
            "{shown}"
        );
    }

    /// A refresh answered with a redirect never sends the grant on.
    #[tokio::test]
    async fn a_redirect_is_never_followed() {
        let (elsewhere, asked_elsewhere) = endpoint(vec![(
            200,
            r#"{"access_token": "stolen", "expires_in": 600}"#.into(),
        )]);
        for status in [301, 302, 307, 308] {
            let (url, _) = endpoint(vec![(status, format!("Location: {elsewhere}"))]);
            let held = Held::new(credential(&url, None)).unwrap();
            assert!(held.refresh().await.is_err(), "{status}");
        }
        assert!(asked_elsewhere.lock().unwrap().is_empty());
    }

    #[test]
    fn a_credential_refreshes_over_https_only() {
        let mut c = credential("http://example.com/token", None);
        assert!(Held::new(credential(&c.refresh_url, None)).is_err());
        for bad in [
            "http://127.0.0.1.example.com/token",
            "http://localhost.example.com/token",
            "http://127.0.0.1@example.com/token",
            "ftp://127.0.0.1/token",
            "not a url",
        ] {
            c.refresh_url = bad.into();
            assert!(
                Held::new(credential(&c.refresh_url, None)).is_err(),
                "{bad}"
            );
        }
        c.refresh_url = "http://[::1]:8080/token".into();
        assert!(Held::new(credential(&c.refresh_url, None)).is_ok());
        c.refresh_url = "https://cloud.example/v1/storage/token".into();
        assert!(Held::new(c).is_ok());
    }
}
