//! One way in: every post from outside that a secret vouches for (a
//! connection's webhook, a payment's, a repository's, an agent's own) is
//! checked here, built once. A receiver says how its sender signs, as a
//! value ([`Scheme`]); it never writes verification.
//!
//! What every receiver gets from [`Inbound::verify`]:
//!
//! - the supplied body slice capped before signature processing
//!   ([`Refused::TooBig`]); callers must also bound HTTP reads before buffering;
//! - closed by default: one without a secret refuses every post
//!   ([`Refused::Unconfigured`]), and nothing builds one that takes
//!   anyone;
//! - a constant-time compare, of the secret or of its HMAC
//!   ([`crate::hmac`]);
//! - a signed time checked against its window where the scheme declares
//!   one ([`Refused::Stale`]);
//! - the delivery's id, for a [`Seen`] store to refuse a replay
//!   ([`Inbound::take`]);
//! - both the current secret and the one before it tried, so any secret
//!   rolls with no restart ([`Secrets`]);
//! - one answer for each refusal ([`Refused::status`]).

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use crate::{OpaqueSecret, hmac};

mod replays;
mod schemes;
pub use replays::{LONGEST_KEEP, Seen, SeenInMemory};
use schemes::message;
pub use schemes::{Id, Part, Place, Scheme, Signature, Time};

/// A receiver's secrets: the current one, and while it rolls, the one
/// before it. Both are tried, so the sender may switch at any moment.
/// An empty one counts as none.
#[derive(Debug, Default)]
pub struct Secrets {
    current: Option<OpaqueSecret>,
    previous: Option<OpaqueSecret>,
}

impl Secrets {
    /// Just `current`.
    pub fn new(current: OpaqueSecret) -> Self {
        Self::default().with_current(current)
    }

    /// `current`, and `previous` until the sender is known to use it.
    pub fn rolling(current: OpaqueSecret, previous: OpaqueSecret) -> Self {
        Self {
            previous: (!previous.is_empty()).then_some(previous),
            ..Self::new(current)
        }
    }

    fn with_current(mut self, current: OpaqueSecret) -> Self {
        self.current = (!current.is_empty()).then_some(current);
        self
    }

    /// Each to try, the current one first; none at all when there's no
    /// current one, so a previous secret alone takes nothing.
    fn each(&self) -> impl Iterator<Item = &OpaqueSecret> {
        let current = self.current.as_ref();
        current
            .into_iter()
            .chain(current.and(self.previous.as_ref()))
    }
}

/// The most signatures read from a header that lists several: a sender
/// lists one for each secret it signs with, two while one rolls.
const MOST_LISTED: usize = 8;

/// A receiver: its name, how its sender signs, the most its body may be,
/// and its secrets.
#[derive(Debug)]
pub struct Inbound {
    pub receiver: Receiver,
    pub scheme: Scheme,
    /// The most bytes a body may have. Required: there's no default, so
    /// every route that takes a post says its cap.
    pub max_body: usize,
    pub secrets: Secrets,
}

/// A receiver's name among every receiver sharing a [`Seen`] store
/// (`stripe`, `hook:<id>`), so one sender's ids never stand for another's:
/// never empty, and never a NUL, which ends it in what the store keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receiver(String);

impl Receiver {
    /// `name`, unless it's empty or holds a NUL.
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        (!name.is_empty() && !name.contains('\0')).then_some(Self(name))
    }
}

/// Why a post was refused. Each has one answer ([`Refused::status`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The receiver has no secret: it takes nothing.
    Unconfigured,
    /// Its body is over the receiver's cap.
    TooBig,
    /// Its time or id is missing or isn't what the scheme reads.
    Malformed,
    /// It carries no signature, or one that isn't hex.
    Unsigned,
    /// Its signature, or its secret, isn't one of the receiver's.
    BadSignature,
    /// It was signed outside its window.
    Stale,
    /// Its id was taken already, inside its window.
    Replayed,
    /// The [`Seen`] store couldn't say whether its id is new.
    Unavailable,
}

impl Refused {
    /// The one answer to each, in one table, the same at every receiver:
    /// 503 where the sender should try again (no secret yet; a store
    /// down), 413 for too big, 400 for malformed, 401 for the rest.
    pub fn status(self) -> u16 {
        match self {
            Refused::Unconfigured | Refused::Unavailable => 503,
            Refused::TooBig => 413,
            Refused::Malformed => 400,
            Refused::Unsigned | Refused::BadSignature | Refused::Stale | Refused::Replayed => 401,
        }
    }
}

/// A post's headers, looked up by name in any case.
pub trait Headers {
    fn header(&self, name: &str) -> Option<&str>;
}

/// Names in lowercase, as a connection's `Sifting` holds them.
impl Headers for BTreeMap<String, String> {
    fn header(&self, name: &str) -> Option<&str> {
        self.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

#[cfg(feature = "http")]
impl Headers for http::HeaderMap {
    fn header(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(|v| v.to_str().ok())
    }
}

/// A post that passed [`Inbound::verify`]: its delivery's id, if the
/// scheme reads one; what a [`Seen`] store keeps it as, the id after the
/// receiver's name and the scheme's, each before a NUL; and until when
/// (seconds since 1970) it's kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub id: Option<String>,
    pub seen: Option<String>,
    pub until: u64,
}

impl Inbound {
    /// Whether `body`, with `headers`, is from this receiver's sender,
    /// `now` seconds since 1970. Checks all but a replay: [`Inbound::take`]
    /// does that too, or the caller hands [`Verified::seen`] to a [`Seen`]
    /// store of its own, one that outlives a restart for a scheme that
    /// signs no time.
    pub fn verify(
        &self,
        headers: &dyn Headers,
        body: &[u8],
        now: u64,
    ) -> Result<Verified, Refused> {
        if body.len() > self.max_body {
            return Err(Refused::TooBig);
        }
        if self.secrets.current.is_none() {
            return Err(Refused::Unconfigured);
        }
        match self.scheme {
            Scheme::Header { name } => {
                let carried = headers.header(name).ok_or(Refused::Unsigned)?;
                let ok = self
                    .secrets
                    .each()
                    .any(|secret| crate::equal(carried.as_bytes(), &secret.0.0));
                match ok {
                    true => Ok(Verified {
                        id: None,
                        seen: None,
                        until: now,
                    }),
                    false => Err(Refused::BadSignature),
                }
            }
            Scheme::Hmac {
                signature,
                over,
                time,
                id,
            } => {
                let value = |at: &Place| read(at, headers, body);
                let signed_at = match time {
                    Some(time) => {
                        let text = value(&time.at)
                            .into_iter()
                            .next()
                            .ok_or(Refused::Malformed)?;
                        let at = text.parse::<u64>().map_err(|_| Refused::Malformed)?;
                        Some((text, at, time.window))
                    }
                    None => None,
                };
                let delivery = match id {
                    Some(id) => {
                        let text = value(&id.at).into_iter().next();
                        Some(text.filter(|t| !t.is_empty()).ok_or(Refused::Malformed)?)
                    }
                    None => None,
                };
                // Only what could be a tag, and few: each is compared, not
                // computed, so a long list costs nothing but its reading.
                let tags: Vec<Vec<u8>> = value(&signature.at)
                    .into_iter()
                    .filter_map(|t| hmac::unhex(t.strip_prefix(signature.prefix)?))
                    .filter(|tag| tag.len() == 32)
                    .take(if signature.many { MOST_LISTED } else { 1 })
                    .collect();
                if tags.is_empty() {
                    return Err(Refused::Unsigned);
                }
                if let Some((_, at, window)) = signed_at
                    && at.abs_diff(now) > window
                {
                    return Err(Refused::Stale);
                }
                let message = message(
                    over,
                    signed_at.as_ref().map_or("", |s| s.0.as_str()),
                    delivery.as_deref().unwrap_or(""),
                    body,
                );
                let ok = self.secrets.each().any(|secret| {
                    let signed = secret.sign(&message);
                    tags.iter().any(|tag| crate::equal(&signed, tag))
                });
                if !ok {
                    return Err(Refused::BadSignature);
                }
                let until = match (signed_at, id) {
                    (Some((_, at, window)), _) => at.saturating_add(window),
                    (None, Some(id)) => now.saturating_add(id.keep),
                    (None, None) => now,
                };
                let seen = delivery
                    .as_ref()
                    .map(|id| format!("{}\0{}\0{id}", self.receiver.0, self.scheme.label()));
                Ok(Verified {
                    id: delivery,
                    seen,
                    until,
                })
            }
        }
    }

    /// [`Inbound::verify`], then its id handed to `seen`: refused if it was
    /// taken inside its window. A scheme that signs no time takes nothing
    /// with a store that forgets at a restart ([`Seen::durable`]): its
    /// posts could be replayed after every one.
    pub async fn take(
        &self,
        headers: &dyn Headers,
        body: &[u8],
        now: u64,
        seen: &dyn Seen,
    ) -> Result<Verified, Refused> {
        if self.scheme.untimed() && !seen.durable() {
            return Err(Refused::Unconfigured);
        }
        let verified = self.verify(headers, body, now)?;
        if let Some(key) = &verified.seen {
            match seen.first(key, verified.until, now).await {
                Ok(true) => {}
                Ok(false) => return Err(Refused::Replayed),
                Err(_) => return Err(Refused::Unavailable),
            }
        }
        Ok(verified)
    }
}

/// Every value at `at`: one for a header or a JSON field, each listed for
/// a header's fields.
fn read(at: &Place, headers: &dyn Headers, body: &[u8]) -> Vec<String> {
    match *at {
        Place::Header(name) => headers
            .header(name)
            .map(str::to_string)
            .into_iter()
            .collect(),
        Place::Field { header, key } => headers
            .header(header)
            .into_iter()
            .flat_map(|line| line.split(','))
            .filter_map(|field| {
                let (k, v) = field.trim().split_once('=')?;
                (k == key).then(|| v.to_string())
            })
            .collect(),
        Place::Json(field) => serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get(field)?.as_str().map(str::to_string))
            .into_iter()
            .collect(),
        Place::BodyHash => vec![hmac::hex(
            ring::digest::digest(&ring::digest::SHA256, body).as_ref(),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Secret;

    const NOW: u64 = 1_760_000_000;

    fn secret(text: &str) -> OpaqueSecret {
        OpaqueSecret::from_text(Secret::new(text.to_string()))
    }

    fn inbound(scheme: Scheme, secrets: Secrets) -> Inbound {
        Inbound {
            receiver: Receiver::new("test").unwrap(),
            scheme,
            max_body: 64,
            secrets,
        }
    }

    /// A post signed as `scheme` says, with `with`, at `time`, its id `id`.
    fn post(
        scheme: Scheme,
        with: &str,
        time: u64,
        id: &str,
    ) -> (BTreeMap<String, String>, Vec<u8>) {
        let body = match scheme {
            Scheme::Hmac {
                id:
                    Some(Id {
                        at: Place::Json(field),
                        ..
                    }),
                ..
            } => format!(r#"{{"{field}":"{id}"}}"#),
            // Each delivery its own body, as a sender's are.
            _ => format!(r#"{{"text":"hi","delivery":"{id}"}}"#),
        };
        let headers = match scheme {
            Scheme::Header { name } => BTreeMap::from([(name.to_string(), with.to_string())]),
            hmac => hmac.sign(&secret(with), time, id, body.as_bytes()).unwrap(),
        };
        (headers, body.into_bytes())
    }

    /// A store that outlives a restart, as a test stands one in: memory
    /// that says it's durable.
    #[derive(Default)]
    struct Durable(SeenInMemory);

    impl Seen for Durable {
        fn first<'a>(
            &'a self,
            id: &'a str,
            until: u64,
            now: u64,
        ) -> Pin<Box<dyn Future<Output = std::io::Result<bool>> + Send + 'a>> {
            self.0.first(id, until, now)
        }

        fn durable(&self) -> bool {
            true
        }
    }

    async fn take(
        inbound: &Inbound,
        seen: &dyn Seen,
        post: &(BTreeMap<String, String>, Vec<u8>),
        now: u64,
    ) -> Result<Verified, Refused> {
        inbound.take(&post.0, &post.1, now, seen).await
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        // Every future here is ready at once: SeenInMemory's.
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(out) => out,
            std::task::Poll::Pending => unreachable!("a memory store is never pending"),
        }
    }

    /// One table of cases, against every scheme: what each must take, and
    /// what each must refuse, with which answer.
    #[test]
    fn every_scheme_takes_its_own_and_refuses_the_rest() {
        let schemes = [
            ("link", Scheme::LINK),
            ("telegram", Scheme::TELEGRAM),
            ("slack", Scheme::SLACK),
            ("stripe", Scheme::STRIPE),
            ("github", Scheme::GITHUB),
        ];
        for (name, scheme) in schemes {
            let timed = matches!(scheme, Scheme::Hmac { time: Some(_), .. });
            let identified = matches!(scheme, Scheme::Hmac { id: Some(_), .. });
            let seen = Durable::default();
            let at = |result: Result<Verified, Refused>| result.map(|_| ());
            let receiver = inbound(scheme, Secrets::new(secret("s3cret")));

            // A good post, taken.
            let good = post(scheme, "s3cret", NOW, "d-1");
            assert_eq!(
                at(block_on(take(&receiver, &seen, &good, NOW))),
                Ok(()),
                "{name}"
            );

            // The same post again: a replay, where the scheme has an id.
            let again = block_on(take(&receiver, &seen, &good, NOW + 1));
            assert_eq!(
                at(again),
                if identified {
                    Err(Refused::Replayed)
                } else {
                    Ok(())
                },
                "{name}"
            );

            // A wrong secret.
            let wrong = post(scheme, "other", NOW, "d-2");
            assert_eq!(
                at(block_on(take(&receiver, &seen, &wrong, NOW))),
                Err(Refused::BadSignature),
                "{name}"
            );

            // A stale time, either side of the window.
            if timed {
                let old = post(scheme, "s3cret", NOW - 301, "d-3");
                assert_eq!(
                    at(block_on(take(&receiver, &seen, &old, NOW))),
                    Err(Refused::Stale),
                    "{name}"
                );
                let ahead = post(scheme, "s3cret", NOW + 301, "d-4");
                assert_eq!(
                    at(block_on(take(&receiver, &seen, &ahead, NOW))),
                    Err(Refused::Stale),
                    "{name}"
                );
                let edge = post(scheme, "s3cret", NOW - 300, "d-5");
                assert_eq!(
                    at(block_on(take(&receiver, &seen, &edge, NOW))),
                    Ok(()),
                    "{name}"
                );
            }

            // A body over the cap, before anything else is read.
            let mut big = post(scheme, "s3cret", NOW, "d-6");
            big.1 = vec![b' '; 65];
            assert_eq!(
                at(block_on(take(&receiver, &seen, &big, NOW))),
                Err(Refused::TooBig),
                "{name}"
            );

            // No signature at all.
            let bare = (BTreeMap::new(), good.1.clone());
            let refused = at(block_on(take(&receiver, &seen, &bare, NOW)));
            let id_in_headers = matches!(
                scheme,
                Scheme::Hmac {
                    id: Some(Id {
                        at: Place::Header(_) | Place::Field { .. },
                        ..
                    }),
                    ..
                }
            );
            let want = if timed || id_in_headers {
                Refused::Malformed
            } else {
                Refused::Unsigned
            };
            assert_eq!(refused, Err(want), "{name}");

            // An unconfigured receiver: nothing at all.
            let none = inbound(scheme, Secrets::default());
            let fresh = post(scheme, "s3cret", NOW, "d-7");
            assert_eq!(
                at(block_on(take(&none, &seen, &fresh, NOW))),
                Err(Refused::Unconfigured),
                "{name}"
            );
            let empty = inbound(scheme, Secrets::new(secret("")));
            assert_eq!(
                at(block_on(take(&empty, &seen, &fresh, NOW))),
                Err(Refused::Unconfigured),
                "{name}"
            );

            // While it rolls: the previous secret taken, beside the new.
            let rolling = inbound(scheme, Secrets::rolling(secret("n3w"), secret("s3cret")));
            let old_secret = post(scheme, "s3cret", NOW, "d-8");
            let new_secret = post(scheme, "n3w", NOW, "d-9");
            assert_eq!(
                at(block_on(take(&rolling, &seen, &old_secret, NOW))),
                Ok(()),
                "{name}"
            );
            assert_eq!(
                at(block_on(take(&rolling, &seen, &new_secret, NOW))),
                Ok(()),
                "{name}"
            );
            let stranger = post(scheme, "other", NOW, "d-10");
            assert_eq!(
                at(block_on(take(&rolling, &seen, &stranger, NOW))),
                Err(Refused::BadSignature),
                "{name}"
            );
        }
    }

    /// An id is something its sender signed: one anyone could change
    /// would let a captured post be taken again under a new id.
    #[test]
    fn every_schemes_id_is_signed() {
        for scheme in [Scheme::LINK, Scheme::SLACK, Scheme::STRIPE, Scheme::GITHUB] {
            if let Scheme::Hmac {
                over, id: Some(id), ..
            } = scheme
            {
                let signed = match id.at {
                    Place::Json(_) | Place::BodyHash => over.contains(&Part::Body),
                    Place::Header(_) | Place::Field { .. } => over.contains(&Part::Id),
                };
                assert!(signed, "{scheme:?}");
            }
        }
    }

    #[test]
    fn a_github_body_sent_again_under_a_new_delivery_is_a_replay() {
        let receiver = inbound(Scheme::GITHUB, Secrets::new(secret("s3cret")));
        let seen = Durable::default();
        let (mut headers, body) = post(Scheme::GITHUB, "s3cret", NOW, "d-1");
        headers.insert("x-github-delivery".into(), "d-1".into());
        assert!(block_on(receiver.take(&headers, &body, NOW, &seen)).is_ok());
        headers.insert("x-github-delivery".into(), "d-2".into());
        let again = block_on(receiver.take(&headers, &body, NOW, &seen));
        assert_eq!(again, Err(Refused::Replayed));
        // Another event is another body, and taken.
        let other = br#"{"action":"completed"}"#.to_vec();
        let signed = Scheme::GITHUB
            .sign(&secret("s3cret"), NOW, "", &other)
            .unwrap();
        assert!(block_on(receiver.take(&signed, &other, NOW, &seen)).is_ok());
    }

    #[test]
    fn no_scheme_keeps_an_id_longer_than_the_longest() {
        for scheme in [
            Scheme::LINK,
            Scheme::TELEGRAM,
            Scheme::SLACK,
            Scheme::STRIPE,
            Scheme::GITHUB,
        ] {
            if let Scheme::Hmac { time, id, .. } = scheme {
                assert!(time.is_none_or(|t| t.window <= LONGEST_KEEP), "{scheme:?}");
                assert!(id.is_none_or(|i| i.keep <= LONGEST_KEEP), "{scheme:?}");
            }
        }
    }

    #[test]
    fn each_refusal_has_one_answer() {
        let table = [
            (Refused::Unconfigured, 503),
            (Refused::Unavailable, 503),
            (Refused::TooBig, 413),
            (Refused::Malformed, 400),
            (Refused::Unsigned, 401),
            (Refused::BadSignature, 401),
            (Refused::Stale, 401),
            (Refused::Replayed, 401),
        ];
        for (refused, status) in table {
            assert_eq!(refused.status(), status, "{refused:?}");
        }
    }

    #[test]
    fn link_signs_as_its_spec_says() {
        let headers = Scheme::LINK
            .sign(&secret("s3cret"), NOW, "d-1", b"{}")
            .unwrap();
        let tag = hmac::hex(&hmac::sign(
            b"s3cret",
            format!("link webhook\0{NOW}\0d-1\0{{}}").as_bytes(),
        ));
        assert_eq!(headers["link-signature"], format!("v1={tag}"));
        assert_eq!(headers["link-time"], NOW.to_string());
        assert_eq!(headers["link-id"], "d-1");
    }

    #[test]
    fn stripe_takes_any_listed_signature() {
        let receiver = inbound(Scheme::STRIPE, Secrets::new(secret("whsec")));
        let (mut headers, body) = post(Scheme::STRIPE, "whsec", NOW, "evt_1");
        let line = headers["stripe-signature"].clone();
        let (time, good) = line.split_once(',').unwrap();
        headers.insert("stripe-signature".into(), format!("{time},v1=00ff,{good}"));
        assert!(receiver.verify(&headers, &body, NOW).is_ok());
        // One listed of a scheme that lists one is all that's read.
        let slack = inbound(Scheme::SLACK, Secrets::new(secret("s")));
        let (headers, body) = post(Scheme::SLACK, "s", NOW, "");
        assert!(slack.verify(&headers, &body, NOW).is_ok());
    }

    #[test]
    fn a_previous_secret_alone_takes_nothing() {
        let secrets = Secrets {
            current: None,
            previous: Some(secret("s3cret")),
        };
        let receiver = inbound(Scheme::LINK, secrets);
        let (headers, body) = post(Scheme::LINK, "s3cret", NOW, "d-1");
        assert_eq!(
            receiver.verify(&headers, &body, NOW),
            Err(Refused::Unconfigured)
        );
    }

    #[test]
    fn an_id_is_kept_for_its_window_and_then_let_go() {
        let receiver = inbound(Scheme::LINK, Secrets::new(secret("s3cret")));
        let (headers, body) = post(Scheme::LINK, "s3cret", NOW, "d-1");
        let verified = receiver.verify(&headers, &body, NOW).unwrap();
        assert_eq!(
            verified,
            Verified {
                id: Some("d-1".into()),
                seen: Some("test\0link-signature\0d-1".into()),
                until: NOW + 300
            }
        );
        let github = inbound(Scheme::GITHUB, Secrets::new(secret("s3cret")));
        let (headers, body) = post(Scheme::GITHUB, "s3cret", NOW, "g-1");
        assert_eq!(
            github.verify(&headers, &body, NOW).unwrap().until,
            NOW + LONGEST_KEEP
        );

        let seen = SeenInMemory::default();
        assert!(block_on(seen.first("d-1", NOW + 300, NOW)).unwrap());
        assert!(!block_on(seen.first("d-1", NOW + 300, NOW + 300)).unwrap());
        assert!(block_on(seen.first("d-1", NOW + 600, NOW + 301)).unwrap());
    }

    #[test]
    fn a_store_that_cant_say_is_a_retry() {
        struct Down;
        impl Seen for Down {
            fn first<'a>(
                &'a self,
                _: &'a str,
                _: u64,
                _: u64,
            ) -> Pin<Box<dyn Future<Output = std::io::Result<bool>> + Send + 'a>> {
                Box::pin(std::future::ready(Err(std::io::Error::other("down"))))
            }

            fn durable(&self) -> bool {
                true
            }
        }
        let receiver = inbound(Scheme::LINK, Secrets::new(secret("s3cret")));
        let (headers, body) = post(Scheme::LINK, "s3cret", NOW, "d-1");
        let refused = block_on(receiver.take(&headers, &body, NOW, &Down));
        assert_eq!(refused, Err(Refused::Unavailable));
    }

    /// A change made to a good post's headers and body.
    type Edit = dyn Fn(&mut BTreeMap<String, String>, &mut Vec<u8>);

    /// What isn't a post of the scheme's at all: each refused for what it
    /// lacks, before any secret is tried.
    #[test]
    fn malformed_posts_are_refused_by_what_they_lack() {
        let refused = |scheme: Scheme, edit: &Edit| {
            let receiver = inbound(scheme, Secrets::new(secret("s3cret")));
            let (mut headers, mut body) = post(scheme, "s3cret", NOW, "d-1");
            edit(&mut headers, &mut body);
            block_on(receiver.take(&headers, &body, NOW, &Durable::default())).map(|_| ())
        };
        let set = |name: &'static str, value: &'static str| {
            move |headers: &mut BTreeMap<String, String>, _: &mut Vec<u8>| {
                headers.insert(name.into(), value.into());
            }
        };
        let table: [(&str, Scheme, Box<Edit>, Refused); 8] = [
            (
                "a signature that isn't hex",
                Scheme::LINK,
                Box::new(set("link-signature", "v1=zz")),
                Refused::Unsigned,
            ),
            (
                "another scheme's prefix",
                Scheme::GITHUB,
                Box::new(|h: &mut BTreeMap<String, String>, _: &mut Vec<u8>| {
                    let tag = h["x-hub-signature-256"].replace("sha256=", "sha1=");
                    h.insert("x-hub-signature-256".into(), tag);
                }),
                Refused::Unsigned,
            ),
            (
                "a tag that's too short",
                Scheme::SLACK,
                Box::new(set("x-slack-signature", "v0=00ff")),
                Refused::Unsigned,
            ),
            (
                "a time that isn't a number",
                Scheme::SLACK,
                Box::new(set("x-slack-request-timestamp", "soon")),
                Refused::Malformed,
            ),
            (
                "Stripe's header without its time",
                Scheme::STRIPE,
                Box::new(|h: &mut BTreeMap<String, String>, _: &mut Vec<u8>| {
                    let line = h["stripe-signature"]
                        .split(',')
                        .filter(|f| !f.starts_with("t="))
                        .collect::<Vec<_>>()
                        .join(",");
                    h.insert("stripe-signature".into(), line);
                }),
                Refused::Malformed,
            ),
            (
                "Stripe's empty signatures",
                Scheme::STRIPE,
                Box::new(|h: &mut BTreeMap<String, String>, _: &mut Vec<u8>| {
                    h.insert("stripe-signature".into(), format!("t={NOW},v1=,v1="));
                }),
                Refused::Unsigned,
            ),
            (
                "a JSON body without its id",
                Scheme::STRIPE,
                Box::new(|_: &mut BTreeMap<String, String>, b: &mut Vec<u8>| {
                    *b = br#"{"type":"x"}"#.to_vec();
                }),
                Refused::Malformed,
            ),
            (
                "an empty id",
                Scheme::LINK,
                Box::new(set("link-id", "")),
                Refused::Malformed,
            ),
        ];
        for (case, scheme, edit, want) in table {
            assert_eq!(refused(scheme, &*edit), Err(want), "{case}");
        }
    }

    #[test]
    fn stripe_reads_a_few_tags_and_only_what_could_be_one() {
        let receiver = inbound(Scheme::STRIPE, Secrets::new(secret("whsec")));
        let (mut headers, body) = post(Scheme::STRIPE, "whsec", NOW, "evt_1");
        let line = headers["stripe-signature"].clone();
        let (time, good) = line.split_once(',').unwrap();
        let wrong = format!("v1={}", "00".repeat(32));
        // Eight that are wrong first: the right one, ninth, isn't read.
        let many = [wrong.as_str(); MOST_LISTED].join(",");
        headers.insert("stripe-signature".into(), format!("{time},{many},{good}"));
        assert_eq!(
            receiver.verify(&headers, &body, NOW),
            Err(Refused::BadSignature)
        );
        // Short ones aren't tags, and aren't counted.
        let short = vec!["v1=00"; 100].join(",");
        headers.insert("stripe-signature".into(), format!("{time},{short},{good}"));
        assert!(receiver.verify(&headers, &body, NOW).is_ok());
    }

    #[test]
    fn a_scheme_without_a_time_needs_a_store_that_outlives_a_restart() {
        let receiver = inbound(Scheme::GITHUB, Secrets::new(secret("s3cret")));
        let (headers, body) = post(Scheme::GITHUB, "s3cret", NOW, "g-1");
        let memory = SeenInMemory::default();
        let refused = block_on(receiver.take(&headers, &body, NOW, &memory));
        assert_eq!(refused, Err(Refused::Unconfigured));
        let durable = Durable::default();
        assert!(block_on(receiver.take(&headers, &body, NOW, &durable)).is_ok());
        // A timed scheme is fine in memory: its window is what's kept.
        let link = inbound(Scheme::LINK, Secrets::new(secret("s3cret")));
        let (headers, body) = post(Scheme::LINK, "s3cret", NOW, "d-1");
        assert!(block_on(link.take(&headers, &body, NOW, &memory)).is_ok());
    }

    #[test]
    fn a_receivers_name_is_never_empty_or_a_nul() {
        assert!(Receiver::new("hook:1").is_some());
        assert!(Receiver::new("").is_none());
        assert!(Receiver::new("hook\0link-signature").is_none());
    }

    #[test]
    fn one_store_keeps_each_receivers_ids_apart() {
        let seen = Durable::default();
        let mut one = inbound(Scheme::LINK, Secrets::new(secret("one")));
        one.receiver = Receiver::new("hook:1").unwrap();
        let mut two = inbound(Scheme::LINK, Secrets::new(secret("two")));
        two.receiver = Receiver::new("hook:2").unwrap();
        // The same receiver's name, another scheme: the scheme's label alone
        // keeps them apart.
        let mut github = inbound(Scheme::GITHUB, Secrets::new(secret("one")));
        github.receiver = Receiver::new("hook:1").unwrap();
        // Each sender's own post, with the same id: none burns another's.
        let (headers, body) = post(Scheme::LINK, "one", NOW, "d-1");
        assert!(block_on(one.take(&headers, &body, NOW, &seen)).is_ok());
        let (headers, body) = post(Scheme::LINK, "two", NOW, "d-1");
        assert!(block_on(two.take(&headers, &body, NOW, &seen)).is_ok());
        let (headers, body) = post(Scheme::GITHUB, "one", NOW, "d-1");
        assert!(block_on(github.take(&headers, &body, NOW, &seen)).is_ok());
        // And each one's replay is still its own.
        let (headers, body) = post(Scheme::LINK, "one", NOW, "d-1");
        let again = block_on(one.take(&headers, &body, NOW, &seen));
        assert_eq!(again, Err(Refused::Replayed));
    }
}
