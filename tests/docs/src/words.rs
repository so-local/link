//! One word per meaning in the docs' prose: `words.toml` lists the
//! phrases that never name a Link concept, from `AGENTS.md`'s Words, and
//! what to say instead. A page that used one when the rule came in is
//! held in `large-files.toml`'s `[held]`.
//!
//! IAM's pages also never say The IAM Model's retired words, each in its
//! retired sense (`words.toml`'s `[[retired]]`): the folders holding
//! them are each workspace's own list, `iam-pages.toml` beside its
//! `large-files.toml`, so no workspace names another's folders.

use std::path::Path;

use serde::Deserialize;

use crate::structure::held;
use crate::{Check, Finding, Rule, Scope, Tree};

pub const RULES: &[Rule] = &[
    Rule {
        id: "words",
        scope: Scope::Site,
        fix: "a phrase words.toml lists names no Link concept: say what it says instead",
        check: Check::Each(words),
    },
    Rule {
        id: "title-pronouns",
        scope: Scope::Site,
        fix: "a page's title stands alone, so it names its subject, never a pronoun \
              words.toml lists: say what the sandbox can reach, not what it can",
        check: Check::Each(title_pronouns),
    },
    Rule {
        id: "retired-words",
        scope: Scope::Every,
        fix: "an IAM page says a word The IAM Model retired, in its retired sense: \
              say what words.toml's [[retired]] says instead",
        check: Check::Each(retired),
    },
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct File {
    word: Vec<Word>,
    title_pronouns: Vec<String>,
    retired: Vec<Word>,
}

/// What to say, the phrases never said for it, and why.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Word {
    say: String,
    not: Vec<String>,
    why: String,
}

/// `words.toml`, read once.
fn file() -> &'static File {
    static FILE: std::sync::OnceLock<File> = std::sync::OnceLock::new();
    FILE.get_or_init(|| toml::from_str(include_str!("../words.toml")).expect("words.toml"))
}

fn listed() -> &'static [Word] {
    &file().word
}

/// A page's title names its subject: no pronoun `words.toml` lists, in
/// any case, outside code.
fn title_pronouns(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !crate::site::is_page(tree, path) || crate::generated_reference(text) {
        return Vec::new();
    }
    let Some(title) = crate::md::body(text)
        .lines()
        .find_map(|line| line.strip_prefix("# "))
    else {
        return Vec::new();
    };
    let plain: String = title.split('`').step_by(2).collect::<Vec<_>>().join(" ");
    let said: Vec<&str> = file()
        .title_pronouns
        .iter()
        .map(String::as_str)
        .filter(|pronoun| says(&plain, pronoun))
        .collect();
    if said.is_empty() {
        return Vec::new();
    }
    vec![crate::fail(format!(
        "{}: \"{title}\" says {}",
        tree.shown(path),
        said.join(", ")
    ))]
}

/// Whether `prose` says `phrase` as whole words: in its case when it has
/// a capital letter, in any case when it doesn't. A phrase inside a name
/// isn't said: one joined to a letter by an underscore, a dot or a slash
/// is part of a file's name or a path (`config_toml.rs`, a quoted
/// product's `config.yaml` or `orval.config`, `docs/config`), which keeps
/// its spelling.
pub fn says(prose: &str, phrase: &str) -> bool {
    let (prose, phrase) = if phrase.chars().any(char::is_uppercase) {
        (prose.to_string(), phrase.to_string())
    } else {
        (prose.to_lowercase(), phrase.to_string())
    };
    prose.match_indices(&phrase).any(|(at, _)| {
        let mut before = prose[..at].chars().rev();
        let mut after = prose[at + phrase.len()..].chars();
        !in_name(before.next(), before.next()) && !in_name(after.next(), after.next())
    })
}

/// Whether a phrase whose neighbour is `next`, and `beyond` past it, runs
/// on into a word or a name: a letter, a digit or an underscore, a slash,
/// or a dot with a letter or digit past it. A sentence's full stop has
/// none.
fn in_name(next: Option<char>, beyond: Option<char>) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    match next {
        Some(c) if word(c) => true,
        Some('/') => true,
        Some('.') => beyond.is_some_and(word),
        _ => false,
    }
}

fn words(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !crate::site::is_page(tree, path) {
        return Vec::new();
    }
    let said = said_of(listed(), text);
    held(tree, "words", &tree.shown(path), said.is_empty(), || {
        format!("{}: {}", tree.shown(path), said.join("; "))
    })
    .into_iter()
    .collect()
}

/// An IAM page, Markdown in a folder its workspace's `iam-pages.toml`
/// lists, says none of the retired words, outside code and link targets.
fn retired(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    let iam = path.extension().is_some_and(|e| e == "md")
        && tree
            .large
            .iam
            .iter()
            .any(|folder| path.starts_with(tree.root.join(folder)));
    if !iam {
        return Vec::new();
    }
    let said = said_of(&file().retired, text);
    held(
        tree,
        "retired-words",
        &tree.shown(path),
        said.is_empty(),
        || format!("{}: {}", tree.shown(path), said.join("; ")),
    )
    .into_iter()
    .collect()
}

/// Each of `words`' phrases `text`'s prose says, with what to say. An
/// HTML comment is a marker a tool reads, never prose.
fn said_of(words: &[Word], text: &str) -> Vec<String> {
    let prose = crate::abbreviations::plain(&uncommented(&crate::navigation::authored(text)));
    words
        .iter()
        .flat_map(|w| {
            w.not
                .iter()
                .filter(|phrase| says(&prose, phrase))
                .map(move |phrase| format!("\"{phrase}\": say {}. {}", w.say, w.why))
        })
        .collect()
}

/// `text` without its HTML comments, `<!-- ... -->`.
fn uncommented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + "-->".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{file, listed, said_of, says};

    /// "Spec" is the command line's word: prose writes it out, and a
    /// marker, a flag in code or a link's target keeps it.
    #[test]
    fn prose_says_specification_and_code_keeps_spec() {
        for said in [
            "Give it a spec file.",
            "Its specs are TOML.",
            "## Box\n\nThe Spec reads.",
        ] {
            assert!(!said_of(listed(), said).is_empty(), "{said}");
        }
        for kept in [
            "Give it a specification file, as `--spec <file>` names it.",
            "<!-- spec-reference: box -->",
            "See [the specification](config/specs/box.md).",
        ] {
            assert!(
                said_of(listed(), kept).is_empty(),
                "{kept}: {:?}",
                said_of(listed(), kept)
            );
        }
    }

    /// Each retired word is refused in its retired sense, and its
    /// neighbours in a sense that stays are not.
    #[test]
    fn the_retired_iam_words_are_refused_in_their_retired_sense() {
        let retired = &file().retired;
        for said in [
            "The customer signs in with the app.",
            "A wake proof is traded once.",
            "Each service holds a service credential.",
            "The renewal token is kept.",
            "The IAM service decides.",
            "A grant gives the role.",
            "The token's scope lists actions.",
            "Its identity is checked.",
            "Rotate the key.",
        ] {
            assert!(!said_of(retired, said).is_empty(), "{said}");
        }
        for kept in [
            "The customer logs in with the app.",
            "A one-time code is traded once.",
            "Each service holds a client secret.",
            "The refresh token is kept.",
            "Assign the role.",
            "The issuer's public key and private key.",
            "The resource server decides.",
            "A `scope` in code is code.",
        ] {
            assert!(
                said_of(retired, kept).is_empty(),
                "{kept}: {:?}",
                said_of(retired, kept)
            );
        }
    }

    /// Prose says configuration and settings: "config" and "preferences"
    /// are refused, and a file's name or a path holding "config" keeps
    /// its spelling, as code does.
    #[test]
    fn prose_says_configuration_and_settings() {
        for said in [
            "Your config is two kinds of file.",
            "Edit its configs.",
            "## Box\n\nHermes reads its config.",
            "The storage preferences stay.",
            "Each place is a preference.",
        ] {
            assert!(!said_of(listed(), said).is_empty(), "{said}");
        }
        for kept in [
            "Configuration is specifications and settings together.",
            "Codex reads its own `config.toml`.",
            "The model's address in Hermes's config.yaml.",
            "Cargo reads **.cargo/config.toml**.",
            "The orval.config entry point.",
            "See [cli/config](https://example.com/cli/config).",
            "Auto compaction, [config_toml.rs](https://example.com/a.rs).",
            "See [Configuration](config/README.md).",
        ] {
            assert!(
                said_of(listed(), kept).is_empty(),
                "{kept}: {:?}",
                said_of(listed(), kept)
            );
        }
    }

    /// Presale is one word: "pre-sale" is refused in any case.
    #[test]
    fn prose_says_presale() {
        for said in [
            "Buy the pre-sale.",
            "## Buy\n\nThe Pre-Sale opens.",
            "Two pre-sales sold.",
        ] {
            assert!(!said_of(listed(), said).is_empty(), "{said}");
        }
        for kept in ["Buy the presale.", "## The Presale"] {
            assert!(said_of(listed(), kept).is_empty(), "{kept}");
        }
    }

    #[test]
    fn a_phrase_is_said_as_whole_words_in_its_case() {
        assert!(says("Each signing key is kept.", "signing key"));
        assert!(says("Signing key rotates.", "signing key"));
        assert!(!says("its signing keys", "signing key"));
        assert!(says("as Link ships it", "Link ships"));
        assert!(!says("as Link Ships it", "Link ships"));
        assert!(!says("as link ships it", "Link ships"));
        assert!(says("Its config.", "config"));
        assert!(says("its config, and", "config"));
        assert!(!says("its config.toml", "config"));
        assert!(!says("docs/config/README.md", "config"));
    }
}
