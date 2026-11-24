//! A skill, as the Agent Skills standard (agentskills.io) writes one: a
//! folder named for it, holding `SKILL.md`, whose YAML front matter says
//! its `name` and `description` above the instructions. Link reads only
//! those two: a harness lists them in its prompt, and its model reads
//! the rest when a task needs it. `lnk agent skill add` copies one into
//! the agent's `skills/` ([`link_harness_contract::layout::SKILLS`]).

use anyhow::{bail, ensure};

/// The file a skill's folder holds.
pub const FILE: &str = "SKILL.md";

/// The longest name, in characters (the standard's).
pub const NAME_MAX: usize = 64;

/// The longest description, in characters (the standard's).
pub const DESCRIPTION_MAX: usize = 1024;

/// A skill's name and what it's for, from its front matter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    /// On one line, its whitespace each one space.
    pub description: String,
}

/// An error for a name the standard doesn't take: 1 to 64 lowercase
/// letters, digits and `-`, with no `-` first, last or twice in a row.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let plain = (1..=NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--");
    ensure!(
        plain,
        "{:?} isn't a skill's name: 1 to 64 lowercase letters, digits and -, not first, last \
         or twice in a row",
        link_format::clean(name)
    );
    Ok(())
}

/// The skill `text` (a `SKILL.md`) describes: an error when it has no
/// front matter, or its `name` or `description` is missing or not as
/// the standard says.
pub fn parse(text: &str) -> anyhow::Result<Skill> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    ensure!(
        lines.next().map(str::trim_end) == Some("---"),
        "{FILE} starts with its front matter, between two lines of ---"
    );
    let mut front = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        front.push(line);
    }
    ensure!(closed, "{FILE}'s front matter has no closing ---");
    let name = value(&front, "name").unwrap_or_default();
    ensure!(!name.is_empty(), "{FILE}'s front matter has no name");
    check_name(&name)?;
    let description = value(&front, "description").unwrap_or_default();
    ensure!(
        !description.is_empty(),
        "{FILE}'s front matter has no description"
    );
    ensure!(
        description.chars().count() <= DESCRIPTION_MAX,
        "{name}'s description is longer than {DESCRIPTION_MAX} characters"
    );
    Ok(Skill { name, description })
}

/// Top-level key `key`'s value in front matter `lines`, as one line: a
/// plain scalar (with its indented lines after), one in quotes, or a
/// block (`|`, `>`). None when it isn't there; anything else YAML can
/// say there (a list, a map) is no text.
fn value(lines: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let at = lines.iter().position(|line| line.starts_with(&prefix))?;
    let first = lines[at][prefix.len()..].trim();
    // The lines under it: indented, or blank.
    let more = lines[at + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t']))
        .map(|line| line.trim());
    let text = match first.chars().next() {
        Some('|' | '>') => more.collect::<Vec<_>>().join(" "),
        Some(quote @ ('"' | '\'')) => {
            let whole = std::iter::once(first)
                .chain(more)
                .collect::<Vec<_>>()
                .join(" ");
            unquote(&whole, quote).ok()?
        }
        Some('[' | '{' | '&' | '*' | '!') => return None,
        _ => {
            let first = strip_comment(first);
            std::iter::once(first)
                .chain(more.map(strip_comment))
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    // Nothing that turns text around or hides it (bidi, zero-width):
    // a description is printed and goes into a prompt.
    let text = link_format::clean(&text);
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A plain scalar's text without a comment after it (` #`).
fn strip_comment(text: &str) -> &str {
    match text.find(" #") {
        Some(at) => text[..at].trim_end(),
        None if text.starts_with('#') => "",
        None => text,
    }
}

/// The text between `quote`s at the start of `text`: `''` is a quote in
/// single quotes, and double quotes take YAML's common escapes.
fn unquote(text: &str, quote: char) -> anyhow::Result<String> {
    let mut chars = text[1..].chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match (quote, c) {
            ('\'', '\'') if chars.peek() == Some(&'\'') => {
                chars.next();
                out.push('\'');
            }
            ('"', '\\') => match chars.next() {
                Some('n' | 't') => out.push(' '),
                Some(c @ ('"' | '\\' | '/')) => out.push(c),
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => bail!("a quote that doesn't close"),
            },
            (q, c) if c == q => return Ok(out),
            (_, c) => out.push(c),
        }
    }
    bail!("a quote that doesn't close")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_says_its_name_and_description() {
        let skill = parse(
            "---\nname: pdf-tools\ndescription: Extract text and tables from PDF files. Use \
             when\n  working with PDFs.\nlicense: Apache-2.0\nmetadata:\n  author: someone\n---\n\
             # PDF tools\n\nname: not this\n",
        )
        .unwrap();
        assert_eq!(skill.name, "pdf-tools");
        assert_eq!(
            skill.description,
            "Extract text and tables from PDF files. Use when working with PDFs."
        );
    }

    /// A description can't turn text around or hide it.
    #[test]
    fn a_description_has_no_bidi_or_zero_width_characters() {
        let skill =
            parse("---\nname: pdf\ndescription: Read\u{202E}fdp\u{200B} files\u{1b}[2J\n---\n")
                .unwrap();
        assert_eq!(skill.description, "Readfdp files[2J");
    }

    #[test]
    fn its_description_may_be_quoted_or_a_block() {
        let of =
            |front: &str| parse(&format!("---\nname: a\n{front}\n---\n")).map(|s| s.description);
        assert_eq!(of("description: \"Say \\\"hi\\\"\"").unwrap(), "Say \"hi\"");
        assert_eq!(of("description: 'It''s here' ").unwrap(), "It's here");
        assert_eq!(
            of("description: >\n  Folded\n  over lines.\n").unwrap(),
            "Folded over lines."
        );
        assert_eq!(of("description: |\n  One\n\n  two\n").unwrap(), "One two");
        assert_eq!(of("description: Plain # a comment").unwrap(), "Plain");
    }

    #[test]
    fn a_skill_not_as_the_standard_says_is_refused() {
        for text in [
            "",
            "# No front matter\n",
            "---\nname: a\ndescription: b\n",
            "---\ndescription: b\n---\n",
            "---\nname: a\n---\n",
            "---\nname: a\ndescription: [b]\n---\n",
            "---\nname: Big\ndescription: b\n---\n",
            "---\nname: a--b\ndescription: b\n---\n",
            "---\nname: -a\ndescription: b\n---\n",
            "---\nname: ../a\ndescription: b\n---\n",
            "---\nname: a\ndescription: \"b\n---\n",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
        let long = "x".repeat(DESCRIPTION_MAX + 1);
        assert!(parse(&format!("---\nname: a\ndescription: {long}\n---\n")).is_err());
        assert!(check_name(&"a".repeat(NAME_MAX)).is_ok());
        assert!(check_name(&"a".repeat(NAME_MAX + 1)).is_err());
    }
}
