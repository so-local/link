//! What a call's errors say: its route without a secret ([`Route`]), a
//! refusal ([`Refusal`]), the one [`Error`], and text made [`printable`].

use super::*;

/// Text from elsewhere as it may be printed: control characters, and
/// those that show nothing but change how text around them shows (bidi,
/// zero-width), are written as escapes, so a service can't send escape
/// sequences to the terminal or make a line read otherwise than it is.
pub fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        // Debug escapes exactly the characters that don't print as
        // themselves, and quotes and backslashes, which do.
        let hidden = c.is_control()
            || (c.escape_debug().next() == Some('\\') && !matches!(c, '"' | '\'' | '\\'));
        if hidden {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// A call as its errors name it: its method, and its URL without a user,
/// a password, a query or a fragment, which may hold a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route(pub(super) String);

impl Route {
    pub(super) fn new(method: &Method, url: &str) -> Self {
        let shown = match reqwest::Url::parse(url) {
            Ok(mut url) => {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            }
            // Not a URL: what comes before anything that could hold one.
            Err(_) => url.split(['?', '#', '@']).next().unwrap_or_default().into(),
        };
        Self(format!("{method} {}", printable(&shown)))
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An answer that wasn't a success: its status and what it said.
#[derive(Debug)]
pub struct Refusal {
    pub route: Route,
    pub status: u16,
    pub(super) body: Vec<u8>,
    pub(super) after: Option<Duration>,
}

impl Refusal {
    /// What it said, as `T` (an API's error type), closed as an answer
    /// is ([`Request::answer`]).
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, Error> {
        parse(&self.route, &self.body)
    }

    /// How long it asks to wait before another try (`Retry-After`, a 429's
    /// or a 503's), when it says.
    pub fn retry_after(&self) -> Option<Duration> {
        self.after
    }

    /// What it said, as it may be shown: at most 512 bytes, without a
    /// relay's `link relay: ` prefix, in [`printable`] text.
    pub fn reason(&self) -> String {
        let shown = &self.body[..self.body.len().min(MAX_REASON)];
        let text = String::from_utf8_lossy(shown);
        printable(text.trim().trim_start_matches("link relay: "))
    }
}

/// `<route> refused (<status>): <what it said>`.
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (route, status, reason) = (&self.route, self.status, self.reason());
        if reason.is_empty() {
            write!(f, "{route} refused ({status})")
        } else {
            write!(f, "{route} refused ({status}): {reason}")
        }
    }
}

/// How a call went wrong. None names a credential, a query or a value
/// an answer held.
#[derive(Debug)]
pub enum Error {
    /// The client couldn't be made (no TLS).
    Setup(String),
    /// The URL isn't one its client sends to, `only` (`https://`, or
    /// `http://` on this machine): nothing was sent.
    Insecure { route: Route, only: &'static str },
    /// The request's body didn't serialize: nothing was sent.
    Unsendable { route: Route, why: String },
    /// No answer: no connection, TLS refused, or none in time. `sent`
    /// unless no connection was made, so nothing of it could have arrived.
    Unreachable {
        route: Route,
        why: String,
        sent: bool,
    },
    /// A redirect (3xx), which Link never follows.
    Redirected { route: Route, status: u16 },
    /// 401: the credential isn't taken (signed out, revoked, mistyped),
    /// with what it said.
    SignedOut(Refusal),
    /// 429: too many calls; try again later
    /// ([`Refusal::retry_after`]), with what it said.
    Later(Refusal),
    /// A 5xx: the service failed, not the call.
    Failed(Refusal),
    /// Any other refusal (a 4xx), with what it said.
    Refused(Refusal),
    /// An answer longer than `limit` bytes.
    TooLarge { route: Route, limit: u64 },
    /// An answer that isn't its type: not JSON, a field of the wrong
    /// type, or one the type doesn't take.
    Unexpected { route: Route, why: String },
}

impl Error {
    /// The refusal, when the service refused (a 4xx or a 5xx): its
    /// status and what it said.
    pub fn refusal(&self) -> Option<&Refusal> {
        match self {
            Error::Failed(refusal)
            | Error::Refused(refusal)
            | Error::SignedOut(refusal)
            | Error::Later(refusal) => Some(refusal),
            _ => None,
        }
    }

    /// The status the answer had (`404`), when there was one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Redirected { status, .. } => Some(*status),
            _ => self.refusal().map(|refusal| refusal.status),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Setup(why) => write!(f, "an HTTP client couldn't be made: {why}"),
            Error::Insecure { route, only } => {
                write!(f, "{route}: Link sends this only to {only}")
            }
            Error::Unsendable { route, why } => {
                write!(f, "{route}: the request isn't sendable: {why}")
            }
            Error::Unreachable { route, why, .. } => write!(f, "reaching {route}: {why}"),
            Error::Redirected { route, status } => write!(
                f,
                "{route} answered with a redirect ({status}), which Link never follows"
            ),
            Error::Later(refusal) => {
                write!(f, "{refusal}")?;
                match refusal.after {
                    Some(after) => write!(f, "; try again in {}s", after.as_secs()),
                    None if refusal.reason().is_empty() => f.write_str(": try again later"),
                    None => Ok(()),
                }
            }
            Error::Failed(refusal) | Error::Refused(refusal) | Error::SignedOut(refusal) => {
                write!(f, "{refusal}")
            }
            Error::TooLarge { route, limit } => {
                write!(f, "{route} answered more than {limit} bytes")
            }
            Error::Unexpected { route, why } => {
                write!(f, "{route} answered what Link doesn't take: {why}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_shows_no_secret() {
        let route = Route::new(
            &Method::GET,
            "https://ada:hunter2@relay.example/b/x?token=s3cret#frag",
        );
        assert_eq!(route.to_string(), "GET https://relay.example/b/x");
        let route = Route::new(&Method::POST, "not a url?token=s3cret");
        assert_eq!(route.to_string(), "POST not a url");
    }

    #[test]
    fn printable_escapes_what_hides() {
        assert_eq!(printable("ok \"x\""), "ok \"x\"");
        assert_eq!(printable("a\u{1b}[31mb"), "a\\u{1b}[31mb");
        assert_eq!(printable("a\u{202e}b"), "a\\u{202e}b");
    }
}
