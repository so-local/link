//! Next's graph, at run time: the Nexts a command declares it can give
//! (its `next` and `fail` in its module's `actions.toml`), and the check
//! that a debug build gives no other. A command with no declarations is
//! never checked: its module hasn't declared them yet.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{Command, Next};

/// The Nexts one command declares, each as `cmd!` writes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Declared {
    /// What it gives when it succeeds.
    #[serde(default)]
    pub next: Vec<String>,
    /// What it gives when it fails; `retry` is itself again.
    #[serde(default)]
    pub fail: Vec<String>,
}

/// The word that declares the same command again.
pub const RETRY: &str = "retry";

impl Declared {
    pub fn new(next: Vec<String>, fail: Vec<String>) -> Self {
        Declared { next, fail }
    }

    pub fn is_empty(&self) -> bool {
        self.next.is_empty() && self.fail.is_empty()
    }
}

static DECLARED: OnceLock<Declared> = OnceLock::new();

/// What the command this process runs declares: set once, where its
/// command line is read.
pub fn declare(declared: Declared) {
    if !declared.is_empty() {
        let _ = DECLARED.set(declared);
    }
}

/// Whether `given` is one of `templates`: the same command, its words up
/// to the first flag, placeholder or value, or `retry` for `typed`.
pub fn declares(templates: &[String], given: &Command, typed: &Command) -> bool {
    if given == typed {
        return templates.iter().any(|t| t == RETRY);
    }
    templates
        .iter()
        .filter(|t| *t != RETRY)
        .any(|t| fits(&Command::fill(t, Vec::new()), given))
}

/// Whether `given` is `template`'s command: the same words up to its
/// first flag, placeholder or value, and where `given` names more, a
/// placeholder in `template` for it.
fn fits(template: &Command, given: &Command) -> bool {
    use crate::Word;
    let path = template.path();
    let words = given.words();
    let same = path.len() <= words.len()
        && path.iter().zip(words).all(|(p, w)| match w {
            Word::Fixed(f) | Word::Value(f) => f == p,
            Word::Placeholder(_) => false,
        });
    let more = match words.get(path.len()) {
        Some(Word::Fixed(f) | Word::Value(f)) => !f.starts_with('-'),
        Some(Word::Placeholder(_)) => true,
        None => false,
    };
    let placeholder = matches!(
        template.words().get(path.len()),
        Some(Word::Placeholder(_) | Word::Value(_))
    );
    same && (!more || placeholder)
}

/// In a debug build, a panic for a Next `typed` gives that its command
/// doesn't declare, when it declares any: Next's graph must hold every
/// Next a person can be given.
pub(crate) fn check(next: &Next, failed: bool, typed: &Command) {
    let Some(declared) = DECLARED.get().filter(|_| cfg!(debug_assertions)) else {
        return;
    };
    let templates = if failed {
        &declared.fail
    } else {
        &declared.next
    };
    for suggestion in &next.0 {
        let given = &suggestion.command;
        if given.is_lnk() && !declares(templates, given, typed) {
            panic!(
                "{} gives {} {}, not declared in its module's actions.toml {}",
                typed.text(),
                if failed { "on failure" } else { "on success" },
                given.text(),
                if failed { "fail" } else { "next" },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd;

    #[test]
    fn a_next_is_declared_by_its_command_or_retry() {
        let typed = Command::typed(["lnk".into(), "agent".into(), "start".into()]);
        let declared = vec!["lnk agent connect <connection>".to_string(), RETRY.into()];
        assert!(declares(
            &declared,
            &cmd!("lnk agent connect <connection>"),
            &typed
        ));
        assert!(declares(
            &declared,
            &cmd!("lnk agent connect telegram"),
            &typed
        ));
        assert!(declares(&declared, &typed, &typed));
        assert!(!declares(&declared, &cmd!("lnk agent show"), &typed));
        assert!(!declares(&declared[..1], &typed, &typed));
    }

    #[test]
    fn typed_flags_keep_the_same_command_path_as_rendered_flags() {
        let declared = vec!["lnk box list".to_owned()];
        let typed = Command::typed(
            ["lnk", "box", "list", "--cloud", "aws", "--refresh"].map(str::to_owned),
        );
        assert!(declares(
            &declared,
            &typed.clone().and("--tag <tag>"),
            &typed
        ));
        assert!(declares(
            &declared,
            &cmd!("lnk box list --tag <tag>"),
            &typed
        ));
        assert!(!declares(&declared, &cmd!("lnk box list other"), &typed));
        assert!(!declares(&declared, &cmd!("lnk box delete other"), &typed));
        assert!(
            !declares(&declared, &typed, &typed),
            "an exact retry still needs retry"
        );
    }
}
