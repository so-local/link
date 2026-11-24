//! A webhook's questions: a post of `{"question": "..."}` asks the owner
//! on their chats, the agent never woken, and the poster fetches the
//! answer with a `GET` signed as its posts are
//! through the shared interaction coordinator.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use link_interactions::{sources, *};
use link_secretive::inbound::{Inbound, Receiver, Refused, Secrets};
use serde::{Deserialize, Serialize};

use super::Hook;

/// How many questions a webhook asks a day, unless it says
/// (`--questions-per-day`).
pub const PER_DAY: u32 = 20;

/// The longest question a webhook may ask, in characters.
const LONGEST: usize = 600;

/// A webhook's question, as it's posted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Posted {
    question: String,
}

/// What the poster is answered: its question's id, its status, and until
/// when it waits.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub id: String,
    /// `waiting`, `allowed`, `refused` or `expired`.
    pub status: String,
    pub expires: u64,
}

/// Where an agent's webhooks ask its owner: the agent, the store, how
/// long a question waits, and where each webhook's count today is kept.
pub struct Questions {
    pub agent: String,
    /// The service the agent acts as, its bare ID: a webhook asks as its
    /// agent, since a webhook has no service of its own at launch.
    pub service: String,
    pub store: Store,
    pub wait: Duration,
    pub counts: PathBuf,
}

/// Whether `body` is a question, rather than a message for the agent.
pub fn is_question(body: &str) -> bool {
    serde_json::from_str::<Posted>(body).is_ok()
}

impl Questions {
    /// Agent `agent`'s, waiting `wait`, counted in its settings' folder.
    pub fn of(agent: &crate::settings::Settings) -> Option<Self> {
        let wait = agent.spec().ok()?.spec.connections?.ask_wait?.get();
        Some(Self {
            agent: agent.name.clone(),
            service: agent.service.clone()?,
            store: Store::machine().ok()?,
            wait,
            counts: crate::settings::config_of(&agent.name).join("questions"),
        })
    }

    fn requester(&self, hook: &Hook) -> anyhow::Result<Requester> {
        let mut requester =
            sources::requester("webhook", &format!("service:{}", self.service), hook.name())?;
        requester.routing_agent = Some(self.agent.clone());
        Ok(requester)
    }

    /// Asks the owner `body`'s question for `hook`: its status, or the
    /// HTTP status it's refused with.
    pub fn ask(&self, hook: &Hook, body: &str) -> Result<Status, u16> {
        let posted: Posted = serde_json::from_str(body).map_err(|_| 400u16)?;
        let text = posted.question.trim();
        // A question is a line or so of text, with no link to follow: a
        // stolen secret can't put one in front of the owner.
        if text.is_empty() || text.chars().count() > LONGEST || holds_a_link(text) {
            return Err(422);
        }
        if !self.count(hook) {
            return Err(429);
        }
        let create = || -> anyhow::Result<Snapshot> {
            let draft = sources::confirmation(
                text.into(),
                text.into(),
                link_files::random_token()?,
                self.wait,
            );
            let id = self.store.create_bound(
                self.requester(hook)?,
                sources::owner(),
                DomainId::new("external.consent").map_err(anyhow::Error::msg)?,
                draft,
                approval::Effect::ReplyOnly,
                None,
                now(),
            )?;
            self.store.snapshot(&id, now())
        };
        let waiting = create().map_err(|err| {
            link_format::daemon::warn!("asking the owner a webhook's question: {err:#}");
            503u16
        })?;
        Ok(Status {
            id: waiting.question.id.to_string(),
            status: "waiting".into(),
            expires: waiting.question.expires_at,
        })
    }

    /// Question `id`'s status, if it's `hook`'s.
    pub fn status(&self, hook: &Hook, id: &str) -> Option<Status> {
        let id = RequestId::new(id).ok()?;
        let waiting = self.store.snapshot(&id, now()).ok()?;
        if waiting.question.requester != self.requester(hook).ok()? {
            return None;
        }
        let status = match sources::confirmed(&waiting) {
            Some(true) => "allowed",
            Some(false) => "refused",
            None if now() >= waiting.question.expires_at => "expired",
            None => "waiting",
        };
        Some(Status {
            id: waiting.question.id.to_string(),
            status: status.into(),
            expires: waiting.question.expires_at,
        })
    }

    /// Counts one more of `hook`'s questions today: false, past its cap.
    fn count(&self, hook: &Hook) -> bool {
        let day = now() / 86_400;
        let path = self.counts.join(format!("{}.json", hook.name()));
        let Ok(_held) = link_files::lock_file(&path.with_extension("lock")) else {
            return false;
        };
        let Counts(mut counts) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| link_files::saved::parse_json(&bytes).ok())
            .unwrap_or_default();
        counts.retain(|d, _| *d == day);
        let today = counts.entry(day).or_default();
        if *today >= hook.questions_per_day {
            return false;
        }
        *today += 1;
        link_files::saved::print_json(&Counts(counts))
            .ok()
            .is_some_and(|text| link_files::write_private(&path, &text).is_ok())
    }
}

/// A hook's questions asked each day, as its file keeps them, naming its
/// schema.
#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
struct Counts(BTreeMap<u64, u32>);

link_files::specification!(Counts, include_str!("counts.schema.toml"));

/// Whether a `GET`'s `headers` are signed with `hook`'s secret, as Link's
/// scheme signs an empty body. A webhook GitHub signs takes none: GitHub
/// only posts.
pub fn signed(hook: &Hook, headers: &BTreeMap<String, String>) -> Result<(), u16> {
    let settings: link_connection_contract::webhook::Settings =
        serde_json::from_value(hook.settings.clone()).map_err(|_| 404u16)?;
    if !settings.from.is_link() {
        return Err(404);
    }
    let receiver = Receiver::new(format!("webhook:{}", settings.name))
        .ok_or(Refused::Unconfigured.status())?;
    let opaque = |secret: &link_secretive::Secret| {
        link_secretive::OpaqueSecret::from_text(secret.duplicate())
    };
    let inbound = Inbound {
        receiver,
        scheme: settings.from.scheme(),
        max_body: 0,
        secrets: Secrets::rolling(
            opaque(&hook.secret),
            opaque(
                hook.previous
                    .as_ref()
                    .unwrap_or(&link_secretive::Secret::default()),
            ),
        ),
    };
    inbound
        .verify(headers, b"", now())
        .map(|_| ())
        .map_err(|refused| refused.status())
}

/// Whether `text` holds what a chat would show as a link: a scheme
/// (`://`), `www.`, or a name with a dot before two letters
/// (`evil.example`), which chats link without a scheme; the full-width
/// and ideographic forms of a colon, slash or dot read as theirs.
fn holds_a_link(text: &str) -> bool {
    let plain: Vec<char> = text
        .chars()
        .map(|c| match c {
            '\u{FF0E}' | '\u{3002}' | '\u{FF61}' => '.',
            '\u{FF1A}' => ':',
            '\u{FF0F}' => '/',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    let joined: String = plain.iter().collect();
    if joined.contains("://") || joined.contains("www.") {
        return true;
    }
    plain.windows(4).any(|w| {
        w[0].is_alphanumeric() && w[1] == '.' && w[2].is_alphabetic() && w[3].is_alphabetic()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(per_day: u32) -> Hook {
        Hook {
            connection: "webhook-connection".into(),
            settings: serde_json::json!({"name": "claude-code", "from": "link"}),
            secret: link_secretive::Secret::new("s3cret".into()),
            previous: None,
            from: "webhook:claude-code".into(),
            thread: "webhook:claude-code".into(),
            questions_per_day: per_day,
        }
    }

    fn questions(dir: &std::path::Path) -> Questions {
        Questions {
            agent: "main".into(),
            service: "s1s2s3s4s5s6".into(),
            store: Store::at(dir.join("questions")),
            wait: Duration::from_secs(600),
            counts: dir.join("counts"),
        }
    }

    /// A question waits for the owner as the webhook's, and its status
    /// follows their answer; another webhook's id says nothing.
    #[test]
    fn a_webhooks_question_waits_for_the_owner() {
        let dir = link_test_kit::TempDir::new("hook-questions");
        let questions = questions(&dir);
        let hook = hook(20);
        assert!(is_question(r#"{"question": "Deploy now?"}"#));
        assert!(!is_question(r#"{"question": "Deploy now?", "to": "x"}"#));
        assert!(!is_question("Deploy now?"));
        let asked = questions
            .ask(&hook, r#"{"question": "Deploy now?"}"#)
            .unwrap();
        assert_eq!(asked.status, "waiting");
        let id = RequestId::new(asked.id.clone()).unwrap();
        let waiting = questions.store.snapshot(&id, now()).unwrap();
        assert_eq!(waiting.question.title, "Deploy now?");
        assert!(link_connection_contract::questions::asked_of(
            &questions.store,
            &waiting.question.requester,
            "main"
        ));
        let e = questions
            .store
            .plan_delivery(
                &id,
                "telegram:42",
                "telegram",
                &render::text_capabilities(),
                None,
                now(),
            )
            .unwrap();
        assert_ne!(e.response_code, asked.id);
        questions
            .store
            .submit(
                &Ingress {
                    actor: sources::owner(),
                    route: "telegram:42".into(),
                    surface: "telegram".into(),
                    input_kind: InputKind::Typed,
                },
                ResponseCandidate {
                    request: id,
                    revision: waiting.question.revision,
                    delivery: e.delivery.id,
                    value: reply::parse(&waiting.question, "no", false, false, false).unwrap(),
                    transport_event: TransportEventId::new("answer").unwrap(),
                },
                now(),
            )
            .unwrap();
        assert_eq!(
            questions.status(&hook, &asked.id).unwrap().status,
            "refused"
        );
        let other = Hook {
            from: "webhook:ci".into(),
            ..self::hook(20)
        };
        assert_eq!(questions.status(&other, &asked.id), None);
    }

    /// A link, an empty or a long question is refused; past its cap a
    /// day, a webhook asks no more.
    #[test]
    fn a_question_is_short_plain_text_within_its_cap() {
        let dir = link_test_kit::TempDir::new("hook-questions-refused");
        let questions = questions(&dir);
        let hook = hook(2);
        for refused in [
            r#"{"question": "Sign in at https://evil.example"}"#.to_string(),
            r#"{"question": "see www.example.com"}"#.into(),
            r#"{"question": "Re-verify at link-verify.example/login"}"#.into(),
            r#"{"question": "Sign in at https\uff1a//evil.example"}"#.into(),
            r#"{"question": "Visit evil\u3002com now"}"#.into(),
            r#"{"question": "  "}"#.into(),
            format!(r#"{{"question": "{}"}}"#, "x".repeat(601)),
        ] {
            assert_eq!(questions.ask(&hook, &refused), Err(422), "{refused}");
        }
        assert!(!holds_a_link(
            "Merge v1.2 into main, e.g. now? 3.14 is fine."
        ));
        let ask = || questions.ask(&hook, r#"{"question": "Merge it?"}"#);
        assert!(ask().is_ok() && ask().is_ok());
        assert_eq!(ask(), Err(429));
    }

    /// A status is fetched only signed with the webhook's secret, as its
    /// posts are.
    #[test]
    fn a_status_is_fetched_signed() {
        let hook = hook(20);
        let secret = link_secretive::OpaqueSecret::from_text(hook.secret.duplicate());
        let headers = link_secretive::inbound::Scheme::LINK
            .sign(&secret, now(), "get-1", b"")
            .unwrap();
        assert_eq!(signed(&hook, &headers), Ok(()));
        let wrong =
            link_secretive::OpaqueSecret::from_text(link_secretive::Secret::new("other".into()));
        let forged = link_secretive::inbound::Scheme::LINK
            .sign(&wrong, now(), "get-2", b"")
            .unwrap();
        assert!(signed(&hook, &forged).is_err());
        assert!(signed(&hook, &BTreeMap::new()).is_err());
        let github = Hook {
            settings: serde_json::json!({"name": "claude-code", "from": "github"}),
            ..self::hook(20)
        };
        assert_eq!(signed(&github, &headers), Err(404));
    }
}
