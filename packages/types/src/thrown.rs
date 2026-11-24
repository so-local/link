//! A thrown error: its public part, [`Error`], and its private part,
//! [`ErrorPrivate`], filled where the problem is found and passed on
//! unchanged. Where it's caught, at a transport's edge, its name gives its
//! status, its public part becomes the answer, and its private part goes
//! to the logs, never across the wire.

use link_actions::value::Words;

use crate::{Error, ErrorName, ErrorPrivate};

/// An error thrown by its name, complete.
#[derive(Clone, Debug, PartialEq)]
pub struct Thrown {
    pub public: Error,
    pub private: ErrorPrivate,
}

impl Thrown {
    /// The error `name`, with its message for a person. An unavailable
    /// error says when to try again, so it's thrown by [`Self::unavailable`].
    pub fn new(name: ErrorName, message: impl Into<String>) -> Self {
        debug_assert!(
            name != ErrorName::Unavailable,
            "an unavailable error says when to try again: Thrown::unavailable"
        );
        Self::named(name, message)
    }

    /// An unavailable error, tried again after `retry_after`.
    pub fn unavailable(message: impl Into<String>, retry_after: crate::Duration) -> Self {
        let mut thrown = Self::named(ErrorName::Unavailable, message);
        thrown.public.retry_after = Some(retry_after.0);
        thrown
    }

    fn named(name: ErrorName, message: impl Into<String>) -> Self {
        Self {
            public: Error {
                name,
                message: message.into(),
                reason: None,
                retry_after: None,
            },
            private: ErrorPrivate::default(),
        }
    }

    /// Why, as a word the caller can act on, sent with it.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.public.reason = Some(reason.into());
        self
    }

    /// What caused it, kept private.
    pub fn cause(mut self, cause: impl std::fmt::Display) -> Self {
        self.private.cause = Some(cause.to_string());
        self
    }

    /// A value involved, by name, kept private.
    pub fn value(mut self, name: impl Into<String>, value: impl std::fmt::Display) -> Self {
        self.private.values.insert(name.into(), value.to_string());
        self
    }

    /// Its name, as every transport says it.
    pub fn name(&self) -> &'static str {
        self.public.name.word()
    }

    /// Its HTTP status, which its name gives.
    pub fn status(&self) -> u16 {
        link_actions::transport::transport()
            .status(self.name())
            .expect("transport.toml gives every named error its status")
    }
}

impl std::fmt::Display for Thrown {
    /// Its public part alone, as a person may see it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name(), self.public.message)
    }
}

impl std::error::Error for Thrown {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conflict carries its status, sends its public part alone, and
    /// keeps its cause and values private.
    #[test]
    fn a_thrown_error_sends_only_its_public_part() {
        let thrown = Thrown::new(ErrorName::Conflict, "The file changed since you opened it")
            .reason("stale-revision")
            .cause("revision 4 is not the stored revision 5")
            .value("path", "instructions/main.md");
        assert_eq!((thrown.name(), thrown.status()), ("conflict", 409));
        let sent = serde_json::to_value(&thrown.public).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "name": "conflict",
                "message": "The file changed since you opened it",
                "reason": "stale-revision",
            })
        );
        assert!(!thrown.to_string().contains("revision 4"));
        assert_eq!(thrown.private.values["path"], "instructions/main.md");
    }

    /// Every name has its status, and an unavailable error says when to
    /// try again.
    #[test]
    fn every_name_has_its_status() {
        for (name, _) in ErrorName::WORDS {
            let thrown = Thrown::named(*name, "x");
            assert!((400..600).contains(&thrown.status()), "{}", thrown.name());
        }
        let busy = Thrown::unavailable("No slot is free", crate::Duration::seconds(30));
        assert_eq!(busy.status(), 503);
        assert_eq!(
            serde_json::to_value(&busy.public).unwrap()["retry-after"],
            "30s"
        );
    }
}
