//! A field flag a schema step retired, as the entry refuses it.

use super::{Binding, flag_of};

impl Binding {
    /// What a flag at `path`, which the shape lacks, is told where a step
    /// retired its field: the field it is now, or why it's gone.
    pub(crate) fn retired(&self, path: &[String]) -> Option<RetiredFlag> {
        use crate::schema::Fate;
        let dotted = crate::schema::path::join(path.iter().map(String::as_str));
        let kind = &self.format.kind;
        let under = |p: &str| dotted == p || dotted.starts_with(&format!("{p}."));
        // The first step that changed it, then its fate through every
        // later one, the whole path followed.
        let retired = self
            .format
            .retired()
            .into_iter()
            .filter(|r| under(&r.path))
            .min_by_key(|r| r.since)?;
        let since = retired.since;
        let flag = flag_of(path);
        Some(match self.format.fate(since - 1, &dotted) {
            Fate::Renamed(now) => {
                let now: Vec<String> = crate::schema::path::keys(&now)?
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                let now = format!("--{}", flag_of(&now));
                RetiredFlag {
                    message: format!("--{flag} is gone since {kind} schema {since}. Use {now}."),
                    now: Some(now),
                }
            }
            Fate::Removed(why) => RetiredFlag {
                message: format!("--{flag} is gone since {kind} schema {since}. {why}"),
                now: None,
            },
            Fate::Refused(why) => RetiredFlag {
                message: format!("--{flag} is refused since {kind} schema {since}. {why}"),
                now: None,
            },
        })
    }
}

/// A field flag a schema step retired: what it's told, and the flag that
/// replaces it, where one does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetiredFlag {
    pub message: String,
    pub now: Option<String>,
}

impl std::fmt::Display for RetiredFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RetiredFlag {}
