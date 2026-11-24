//! How a sender vouches for a post ([`Scheme`]): where its signature, time
//! and id are, what it signs over, and each sender's own.

use super::*;

/// How a sender vouches for a post.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// The secret itself, echoed in header `name` (Telegram's).
    Header { name: &'static str },
    /// An HMAC-SHA256 of the parts `over`, in order, under the secret.
    Hmac {
        signature: Signature,
        over: &'static [Part],
        /// When the sender signed, if it says: refused outside its window.
        time: Option<Time>,
        /// The delivery's id, the same on every retry of it, if the sender
        /// gives one: required then, and kept by a [`Seen`] store.
        id: Option<Id>,
    },
}

/// Where a signature is, and how it's written: lowercase or uppercase hex,
/// after `prefix`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    pub at: Place,
    pub prefix: &'static str,
    /// Whether the sender may list several (Stripe, signing with two
    /// secrets while one rolls): any one that matches is enough.
    pub many: bool,
}

/// What an HMAC is made over, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// These bytes, as they are: a label, a separator.
    Literal(&'static [u8]),
    /// The [`Time`], as the sender wrote it.
    Time,
    /// The [`Id`], as the sender wrote it.
    Id,
    /// The body, as it came.
    Body,
}

/// Where a sender writes one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A header, whole.
    Header(&'static str),
    /// `key=value` among the comma-separated fields of a header (Stripe's
    /// `t=…,v1=…`).
    Field {
        header: &'static str,
        key: &'static str,
    },
    /// A top-level string field of a JSON body.
    Json(&'static str),
    /// The body's SHA-256, in hex: an id for a sender that signs only its
    /// body, which no one can change without changing what's signed.
    BodyHash,
}

/// A signed time: seconds since 1970, taken within `window` seconds of
/// now, either side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    pub at: Place,
    pub window: u64,
}

/// A delivery's id. Kept for its time's window when the scheme signs a
/// time, else for `keep` seconds, so a replay inside it is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Id {
    pub at: Place,
    pub keep: u64,
}

impl Scheme {
    /// Link's own: an HMAC over `link webhook`, the time, the id and the
    /// body, each after a NUL; `link-time`, `link-id`, and
    /// `link-signature: v1=<hex>`; a 300 s window, and the id required.
    /// Our boxes and anyone posting to an agent sign this way.
    pub const LINK: Scheme = Scheme::Hmac {
        signature: Signature {
            at: Place::Header("link-signature"),
            prefix: "v1=",
            many: false,
        },
        over: &[
            Part::Literal(b"link webhook\0"),
            Part::Time,
            Part::Literal(b"\0"),
            Part::Id,
            Part::Literal(b"\0"),
            Part::Body,
        ],
        time: Some(Time {
            at: Place::Header("link-time"),
            window: 300,
        }),
        id: Some(Id {
            at: Place::Header("link-id"),
            keep: 300,
        }),
    };

    /// Telegram's: the webhook's `secret_token`, echoed.
    pub const TELEGRAM: Scheme = Scheme::Header {
        name: "x-telegram-bot-api-secret-token",
    };

    /// Slack's: `x-slack-signature: v0=<hex>` over
    /// `v0:<x-slack-request-timestamp>:<body>`, a 300 s window. Its
    /// `event_id` is in the body, and the connection dedupes by it.
    pub const SLACK: Scheme = Scheme::Hmac {
        signature: Signature {
            at: Place::Header("x-slack-signature"),
            prefix: "v0=",
            many: false,
        },
        over: &[
            Part::Literal(b"v0:"),
            Part::Time,
            Part::Literal(b":"),
            Part::Body,
        ],
        time: Some(Time {
            at: Place::Header("x-slack-request-timestamp"),
            window: 300,
        }),
        id: None,
    };

    /// Stripe's: `stripe-signature: t=<time>,v1=<hex>[,v1=<hex>]` over
    /// `<t>.<body>`, a 300 s window (Stripe's own libraries'), the event's
    /// `id` in the body.
    pub const STRIPE: Scheme = Scheme::Hmac {
        signature: Signature {
            at: Place::Field {
                header: "stripe-signature",
                key: "v1",
            },
            prefix: "",
            many: true,
        },
        over: &[Part::Time, Part::Literal(b"."), Part::Body],
        time: Some(Time {
            at: Place::Field {
                header: "stripe-signature",
                key: "t",
            },
            window: 300,
        }),
        id: Some(Id {
            at: Place::Json("id"),
            keep: 300,
        }),
    };

    /// GitHub's: `x-hub-signature-256: sha256=<hex>` over the body alone.
    /// Its `x-github-delivery` isn't signed, so a captured body could be
    /// sent again under any delivery: its id is the body's hash, which a
    /// redelivery repeats and no other event has. It signs no time, so the
    /// id is kept 30 days, as long as GitHub redelivers by hand.
    pub const GITHUB: Scheme = Scheme::Hmac {
        signature: Signature {
            at: Place::Header("x-hub-signature-256"),
            prefix: "sha256=",
            many: false,
        },
        over: &[Part::Body],
        time: None,
        id: Some(Id {
            at: Place::BodyHash,
            keep: LONGEST_KEEP,
        }),
    };

    /// The headers that sign `body` at `time` with `id` as this scheme
    /// says: what a sender (Link's own, or a test standing in for one)
    /// adds to its post. `time` and `id` go where the scheme reads them,
    /// unless that's the body, which the caller writes. None for
    /// [`Scheme::Header`], which signs nothing: its header is the secret.
    pub fn sign(
        &self,
        secret: &OpaqueSecret,
        time: u64,
        id: &str,
        body: &[u8],
    ) -> Option<BTreeMap<String, String>> {
        let Scheme::Hmac {
            signature,
            over,
            time: timed,
            id: identified,
        } = self
        else {
            return None;
        };
        let time = time.to_string();
        let mut headers = BTreeMap::new();
        let mut put = |at: &Place, value: &str| match *at {
            Place::Header(name) => {
                headers.insert(name.to_string(), value.to_string());
            }
            Place::Field { header, key } => {
                let line: &mut String = headers.entry(header.to_string()).or_default();
                if !line.is_empty() {
                    line.push(',');
                }
                line.push_str(&format!("{key}={value}"));
            }
            Place::Json(_) | Place::BodyHash => {}
        };
        if let Some(timed) = timed {
            put(&timed.at, &time);
        }
        if let Some(identified) = identified {
            put(&identified.at, id);
        }
        let message = message(over, &time, id, body);
        let tag = hmac::hex(&secret.sign(&message));
        put(&signature.at, &format!("{}{tag}", signature.prefix));
        Some(headers)
    }
}

/// The bytes `over` names, in order.
pub(super) fn message(over: &[Part], time: &str, id: &str, body: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();
    for part in over {
        message.extend_from_slice(match part {
            Part::Literal(bytes) => bytes,
            Part::Time => time.as_bytes(),
            Part::Id => id.as_bytes(),
            Part::Body => body,
        });
    }
    message
}

impl Scheme {
    /// The header its signature is in: what tells one scheme's ids from
    /// another's in a [`Seen`] store.
    pub(super) fn label(&self) -> &'static str {
        match self {
            Scheme::Header { name } => name,
            Scheme::Hmac { signature, .. } => match signature.at {
                Place::Header(name) | Place::Field { header: name, .. } | Place::Json(name) => name,
                Place::BodyHash => "body",
            },
        }
    }

    /// Whether it signs no time, so only its id stops a replay: it needs a
    /// [`Seen`] store that outlives a restart.
    pub(super) fn untimed(&self) -> bool {
        matches!(
            self,
            Scheme::Hmac {
                time: None,
                id: Some(_),
                ..
            }
        )
    }
}
