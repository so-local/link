//! A connection's messages pushed to a machine that packs agents, instead of
//! polled by a bridge that runs as long as its agent: what a cloud running
//! an agent only while it answers needs from a connection module, so the
//! cloud itself knows no connection by name.
//!
//! ```text
//! lnk-<connection> hook
//!         reads [`Hooking`] as one JSON line on stdin (the connection's settings,
//!         its token included, the address to push to, and the secret each
//!         push carries), points the connection's pushes there, and prints
//!         [`Hooked`]: the settings `sift` is handed, none of them secret.
//!         Run outside every sandbox, where the token is.
//! lnk-<connection> unhook
//!         reads [`Unhooking`] and turns the pushes off, so a bridge polls,
//!         or holds its socket, again.
//! lnk-<connection> sift
//!         reads [`Sifting`] JSON lines until stdin closes, and writes one
//!         [`Verdict`] line for each, in order: a push whose secret or
//!         signature is wrong is dropped, a stranger's message is dropped,
//!         a check the connection makes of the address is answered, and the
//!         owner's message is taken, with its thread and the id the connection
//!         gives it ([`Verdict::Take`]). A [`Say`] is answered in the push's
//!         own response where the connection allows it, else taken (whoever
//!         runs it then sends it with `send`). It holds no token and calls
//!         nothing: it only reads.
//! lnk-<connection> take
//!         reads [`Taking`] (the [`Bridge`] and one pushed update) and
//!         answers it as `bridge` answers each message it polls: the
//!         update's message to the harness's endpoint, the answer into the
//!         chat. Ends once the answer is sent.
//! lnk-<connection> hold
//!         a connection that only pushes on a connection held open
//!         ([`Describe::held`]): reads [`HoldLine`]s until stdin closes,
//!         holding each bot's connection, and writes a [`Held`] line for each
//!         message one gets and each time one comes up or goes down. One
//!         process holds many bots. It answers nothing and sends nothing.
//! ```
//!
//! Each is optional: a connection that pushes over plain HTTPS gives `hook`,
//! `unhook`, `sift` and `take` ([`Describe::pushed`]); one that pushes
//! only on a held connection gives `hold`, `sift` and `take`.

use link_secretive::Duplicate;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use super::Bridge;

/// The most a pushed update is, as text: a sifted line is read whole.
pub const UPDATE_MAX: usize = 1 << 20;

/// What `hook` reads on stdin. Its token is in `settings`, so never on a
/// command line.
#[derive(Duplicate, Default, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Hooking {
    /// The connection's section of [`crate::harness::Settings`], its token
    /// included.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
    /// Where the connection pushes to: an HTTPS address that names nothing
    /// of whose it is.
    pub url: String,
    /// What each push carries to show it's the connection's (Telegram's
    /// `secret_token`); empty where the connection signs its pushes with a
    /// secret of its own instead (Slack's signing secret, in `settings`).
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub secret: link_secretive::Secret,
}

/// What `hook` prints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hooked {
    /// What `sift` is handed as [`Sifting::settings`]: who may talk to the
    /// agent and who the bot is, and no secret.
    pub settings: serde_json::Value,
    /// Steps a person takes on the connection's own pages before the pushes
    /// arrive, when the module couldn't make the change itself; none: it's
    /// done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

/// What `unhook` reads on stdin.
#[derive(Duplicate, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unhooking {
    /// The connection's section of [`crate::harness::Settings`], its token
    /// included.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub settings: link_secretive::Secret<link_secretive::Json>,
}

/// One line `sift` reads: a push, or a [`Say`].
#[derive(Duplicate, Default, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Sifting {
    /// What `hook` printed ([`Hooked::settings`]): no token.
    pub settings: serde_json::Value,
    /// The secret the push should carry, or sign with: [`Hooking::secret`],
    /// or the connection's signing secret. Empty for a held connection's
    /// update, which has none.
    #[serde(default)]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub secret: link_secretive::Secret,
    /// While the secret rolls, the one before it, taken beside it until
    /// the sender uses the new one; else empty, and not sent.
    #[serde(default, skip_serializing_if = "link_secretive::Secret::is_empty")]
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub previous: link_secretive::Secret,
    /// The push's headers, their names in lowercase.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// The push's body, as it came: what's signed.
    #[serde(default)]
    pub body: String,
    /// A message to send the owner instead, for whoever runs `sift` to
    /// answer the push with: no push to check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say: Option<Say>,
    /// The chats on this connection the agent is shared with, by the
    /// connection's own ids ([`Bridge::shared`], its spec's `[access]
    /// message`), as they are when the push comes, never kept from
    /// `hook`: a message there from anyone the connection says is in it is
    /// taken, with [`Verdict::Take`]'s `from`. Empty, and not sent: its
    /// owner alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
}

impl Sifting {
    /// Its secrets as a receiver checks a post with them
    /// ([`link_secretive::inbound`]): the current one, and the previous one
    /// while it rolls.
    pub fn secrets(&self) -> link_secretive::inbound::Secrets {
        let opaque = |secret: &link_secretive::Secret| {
            link_secretive::OpaqueSecret::from_text(secret.duplicate())
        };
        link_secretive::inbound::Secrets::rolling(opaque(&self.secret), opaque(&self.previous))
    }
}

/// A message to the owner, in answer to their push.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Say {
    /// The chat, as its thread names it after the connection
    /// (`telegram:42`: `42`).
    pub chat: String,
    pub text: String,
}

/// What `sift` makes of one line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Nothing for the agent: answer the push `status` and nothing more
    /// (401 for a wrong secret, 200 for a stranger's message, so the
    /// connection doesn't push it again).
    Drop { status: u16 },
    /// Answer the push with this, and wake nothing: the connection's check of
    /// the address, or a [`Say`] the connection takes in a push's response.
    Answer {
        status: u16,
        /// The response's `Content-Type`.
        content_type: String,
        body: String,
    },
    /// A message for the agent: its thread ([`super::acp::Said::thread`]), its
    /// chat, the id the connection gives the push, the same on every retry of
    /// it, and who it's from when it isn't the owner: someone in a chat
    /// the agent is shared with (`member:<connection>:<user id>`,
    /// [`crate::access::person`]). For a [`Say`], it can't be answered in
    /// the push's response: send it with `send`.
    Take {
        thread: String,
        chat: String,
        dedupe: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<crate::access::ChatMember>,
    },
}

impl Verdict {
    /// Dropped, with `status`.
    pub fn drop(status: u16) -> Self {
        Self::Drop { status }
    }

    /// Answered `200` with `body` as JSON.
    pub fn json(body: &serde_json::Value) -> Self {
        Self::Answer {
            status: 200,
            content_type: "application/json".into(),
            body: body.to_string(),
        }
    }
}

/// What `take` reads on stdin, one JSON line.
#[derive(Duplicate, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taking {
    /// The bridge, as `bridge` is handed it: the endpoint, its key and
    /// the connection's settings, its token included.
    #[duplicate]
    pub bridge: Bridge,
    /// The update pushed, as `sift` took it: the push's body as JSON, or
    /// what `hold` gave.
    pub update: serde_json::Value,
}

/// What `hold` reads, a JSON line each.
#[derive(Duplicate, PartialEq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum HoldLine {
    /// Hold this bot's connection, as `key` (whatever the holder calls it),
    /// in place of whatever `key` held.
    Hold {
        key: String,
        /// The connection's settings, its token included.
        #[serde(with = "link_secretive::bare")]
        #[duplicate]
        settings: link_secretive::Secret<link_secretive::Json>,
    },
    /// Let go of `key`'s connection.
    Drop(String),
}

/// What `hold` writes, a JSON line each: a message one of its
/// connections got, or a connection up or down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub key: String,
    /// A message, for `sift` and then `take`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<HoldState>,
}

/// A held connection's state.
///
/// A reader takes a state it doesn't know (a newer writer's) as
/// [`HoldState::Down`], never up: a connection it can't tell is up is
/// handled as one that's down. A reader that predates
/// [`HoldState::Refused`] skips a line that says it, so a writer says it
/// only to readers of this version or later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HoldState {
    Up,
    Down,
    /// Down, and the connection refused the bot's token: it won't come up
    /// until the token changes.
    Refused,
}

impl<'de> Deserialize<'de> for HoldState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(
            match serde_json::Value::deserialize(deserializer)?.as_str() {
                Some("up") => HoldState::Up,
                Some("refused") => HoldState::Refused,
                _ => HoldState::Down,
            },
        )
    }
}

type Boxed<T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send>>;

/// `hold`: given the lines read, and where to write what it holds.
pub type Hold =
    fn(tokio::sync::mpsc::Receiver<HoldLine>, tokio::sync::mpsc::Sender<Held>) -> Boxed<()>;

/// How a connection's pushes are made and read: [`super::About::pushed`].
#[derive(Clone, Copy, Debug)]
pub struct Pushed {
    /// `hook`; none for a connection whose pushes only come on a held
    /// connection.
    pub hook: Option<fn(Hooking) -> Boxed<Hooked>>,
    /// `unhook`.
    pub unhook: Option<fn(Unhooking) -> Boxed<()>>,
    /// `sift`, for one line: it reads, and calls nothing.
    pub sift: fn(&Sifting) -> Verdict,
    /// `take`.
    pub take: fn(Taking) -> Boxed<()>,
    /// `hold`, given the lines read and where to write: none for a connection
    /// that pushes over HTTPS.
    pub hold: Option<Hold>,
}

/// The most a [`Sifting`] line is read: an update at its most, escaped,
/// with its headers.
const LINE_MAX: usize = 2 * UPDATE_MAX + 64 * 1024;

/// The most of a line past [`LINE_MAX`] that is skipped to find its end:
/// past it, `sift` ends rather than read on.
const SKIP_MAX: usize = 16 * UPDATE_MAX;

/// `sift`, over `input` until it ends: a verdict line on `output` for
/// each line, flushed, so whoever runs it reads each at once. A line it
/// can't read is dropped with 400. A line too long is read no further
/// than its most, the rest skipped without being kept, and one whose end
/// isn't found within [`SKIP_MAX`] more ends `sift` with an error.
pub fn sift_lines(
    sift: fn(&Sifting) -> Verdict,
    mut input: impl std::io::BufRead,
    mut output: impl std::io::Write,
) -> anyhow::Result<()> {
    use std::io::{BufRead as _, Read as _};
    loop {
        let mut line = Vec::new();
        // A line past its most is read no further than that, then refused.
        let read = (&mut input)
            .take(LINE_MAX as u64)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        let verdict = match serde_json::from_slice::<Sifting>(&line) {
            Ok(sifting) if sifting.body.len() <= UPDATE_MAX => sift(&sifting),
            _ => {
                if line.last() != Some(&b'\n') {
                    skip_line(&mut input)?;
                }
                Verdict::drop(400)
            }
        };
        // As its declaration writes it.
        output
            .write_all(&link_actions::answer::json_line::<crate::declared::Verdict>(&verdict)?)?;
        output.flush()?;
    }
}

/// Skips `input` to the end of the line, keeping none of it, and no
/// further than [`SKIP_MAX`]: an error past that.
fn skip_line(input: &mut impl std::io::BufRead) -> anyhow::Result<()> {
    let mut skipped = 0;
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        let (used, ended) = match buf.iter().position(|b| *b == b'\n') {
            Some(at) => (at + 1, true),
            None => (buf.len(), false),
        };
        input.consume(used);
        if ended {
            return Ok(());
        }
        skipped += used;
        anyhow::ensure!(
            skipped <= SKIP_MAX,
            "a line longer than {} bytes: sift reads no further",
            LINE_MAX + SKIP_MAX
        );
    }
}

/// The value of header `name` (lowercase) in `headers`.
pub fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers.get(name).map(String::as_str)
}

/// Whether `a` and `b` are the same, compared in constant time for their
/// length; never two empty ones.
pub fn same_secret(a: &str, b: &str) -> bool {
    !b.is_empty() && link_secretive::equal(a.as_bytes(), b.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always_take(sifting: &Sifting) -> Verdict {
        match &sifting.say {
            Some(say) => Verdict::json(&serde_json::json!({"text": say.text})),
            None => Verdict::Take {
                thread: "x:1".into(),
                chat: "1".into(),
                dedupe: sifting.body.clone(),
                from: None,
            },
        }
    }

    /// One verdict a line, in order, and a line that isn't one dropped
    /// with 400 without stopping the rest.
    #[test]
    fn sift_answers_each_line_in_order() {
        let input = [
            serde_json::to_string(&Sifting {
                body: "a".into(),
                ..Default::default()
            })
            .unwrap(),
            "not json".into(),
            serde_json::to_string(&Sifting {
                say: Some(Say {
                    chat: "1".into(),
                    text: "hi".into(),
                }),
                ..Default::default()
            })
            .unwrap(),
        ]
        .join("\n")
            + "\n";
        let mut out = Vec::new();
        sift_lines(always_take, input.as_bytes(), &mut out).unwrap();
        let verdicts: Vec<Verdict> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(verdicts.len(), 3);
        assert!(matches!(&verdicts[0], Verdict::Take { dedupe, .. } if dedupe == "a"));
        assert_eq!(verdicts[1], Verdict::drop(400));
        assert!(matches!(&verdicts[2], Verdict::Answer { status: 200, .. }));
    }

    /// What `sift` and `hold` write is their declarations' shape: each
    /// verdict and each held line, written as its declared type, is the
    /// same JSON, a value it hasn't left out.
    #[test]
    fn verdicts_and_held_lines_are_their_declared_shapes() {
        fn declared<T>(value: &impl Serialize)
        where
            T: serde::de::DeserializeOwned + Serialize + link_actions::value::Validate,
        {
            let line = link_actions::answer::json_line::<T>(value).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&line).unwrap(),
                serde_json::to_value(value).unwrap()
            );
        }
        for verdict in [
            Verdict::drop(401),
            Verdict::json(&serde_json::json!({"ok": true})),
            Verdict::Take {
                thread: "telegram:42".into(),
                chat: "42".into(),
                dedupe: "7".into(),
                from: Some(crate::access::ChatMember::new("telegram", "43").unwrap()),
            },
            Verdict::Take {
                thread: "telegram:42".into(),
                chat: "42".into(),
                dedupe: "7".into(),
                from: None,
            },
        ] {
            declared::<crate::declared::Verdict>(&verdict);
        }
        for held in [
            Held {
                key: "a".into(),
                update: Some(serde_json::json!({"id": 1})),
                state: None,
            },
            Held {
                key: "a".into(),
                update: None,
                state: Some(HoldState::Refused),
            },
        ] {
            declared::<crate::declared::HeldLine>(&held);
        }
    }

    /// A body past its most is dropped, and the line after it still read.
    #[test]
    fn a_body_too_long_is_dropped() {
        let long = serde_json::to_string(&Sifting {
            body: "x".repeat(UPDATE_MAX + 1),
            ..Default::default()
        })
        .unwrap();
        let next = serde_json::to_string(&Sifting {
            body: "b".into(),
            ..Default::default()
        })
        .unwrap();
        let mut out = Vec::new();
        sift_lines(
            always_take,
            format!("{long}\n{next}\n").as_bytes(),
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<Verdict>(lines[0]).unwrap(),
            Verdict::drop(400)
        );
    }

    /// The rest of a line too long is skipped without being kept, and
    /// one with no end in sight ends `sift` before it's read through.
    #[test]
    fn a_line_with_no_end_is_read_no_further() {
        use std::io::Read as _;
        /// Counts what's read through it.
        struct Counted<R> {
            inner: R,
            read: std::rc::Rc<std::cell::Cell<usize>>,
        }
        impl<R: std::io::Read> std::io::Read for Counted<R> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.inner.read(buf)?;
                self.read.set(self.read.get() + n);
                Ok(n)
            }
        }
        let read = std::rc::Rc::new(std::cell::Cell::new(0));
        // 64 MiB with no newline, then a line that would be answered.
        let endless = std::io::repeat(b'x')
            .take(64 * UPDATE_MAX as u64)
            .chain(&b"\n{\"body\":\"b\"}\n"[..]);
        let input = std::io::BufReader::new(Counted {
            inner: endless,
            read: read.clone(),
        });
        let mut out = Vec::new();
        assert!(sift_lines(always_take, input, &mut out).is_err());
        assert!(
            read.get() <= LINE_MAX + SKIP_MAX + 64 * 1024,
            "read {} bytes of one line",
            read.get()
        );
        assert!(out.is_empty(), "no verdict for it");
        // Within its most, the rest is skipped and the next line answered.
        let long = format!(
            "{}\n{}\n",
            "x".repeat(LINE_MAX + 3 * UPDATE_MAX),
            serde_json::to_string(&Sifting {
                body: "b".into(),
                ..Default::default()
            })
            .unwrap()
        );
        let mut out = Vec::new();
        sift_lines(always_take, long.as_bytes(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<Verdict>(lines[0]).unwrap(),
            Verdict::drop(400)
        );
        assert!(lines[1].contains("\"take\""));
    }

    /// The verdicts' and lines' JSON, as the contract writes them.
    #[test]
    fn the_lines_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&Verdict::drop(401)).unwrap(),
            r#"{"drop":{"status":401}}"#
        );
        assert_eq!(
            serde_json::from_str::<HoldLine>(r#"{"drop":"k1"}"#).unwrap(),
            HoldLine::Drop("k1".into())
        );
        let hold: HoldLine =
            serde_json::from_str(r#"{"hold":{"key":"k1","settings":{"bot_token":"t"}}}"#).unwrap();
        assert!(matches!(hold, HoldLine::Hold { ref key, .. } if key == "k1"));
        assert!(!format!("{hold:?}").contains("bot_token"));
        assert_eq!(
            serde_json::to_string(&Held {
                key: "k1".into(),
                update: None,
                state: Some(HoldState::Up)
            })
            .unwrap(),
            r#"{"key":"k1","state":"up"}"#
        );
    }

    /// Each state as it's written and read; one a reader doesn't know,
    /// or that isn't a state at all, is down, never up.
    #[test]
    fn a_state_not_known_is_down() {
        for (state, json) in [
            (HoldState::Up, "\"up\""),
            (HoldState::Down, "\"down\""),
            (HoldState::Refused, "\"refused\""),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            assert_eq!(serde_json::from_str::<HoldState>(json).unwrap(), state);
        }
        for unknown in [
            r#"{"key":"k1","state":"sleepy"}"#,
            r#"{"key":"k1","state":"UP"}"#,
            r#"{"key":"k1","state":1}"#,
            r#"{"key":"k1","state":{"up":true}}"#,
        ] {
            let held: Held = serde_json::from_str(unknown).unwrap();
            assert_eq!(held.state, Some(HoldState::Down), "{unknown}");
        }
        let held: Held = serde_json::from_str(r#"{"key":"k1"}"#).unwrap();
        assert_eq!(held.state, None);
    }

    /// The chats shared are sent only when there are some, and read back as sent.
    #[test]
    fn a_siftings_shared_chats_are_sent_only_when_some() {
        let owners = serde_json::to_value(Sifting {
            body: "a".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(owners.get("shared").is_none(), "{owners}");
        let line = serde_json::to_string(&Sifting {
            body: "a".into(),
            shared: vec!["C0TEAM".into()],
            ..Default::default()
        })
        .unwrap();
        let read: Sifting = serde_json::from_str(&line).unwrap();
        assert_eq!(read.shared, ["C0TEAM"]);
    }

    #[test]
    fn secrets_compare_whole() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abd", "abc"));
        assert!(!same_secret("ab", "abc"));
        assert!(!same_secret("", ""));
    }
}
