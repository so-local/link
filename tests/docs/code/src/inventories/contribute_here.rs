//! Contribute Here: every page's Contribute Here section, gathered on one
//! page as its title and first sentence, so each call to action is
//! written once, beside the feature it asks for.

use std::path::Path;

use link_docs_check::Tree;

use super::{Items, entry, title_of};

/// The page the calls to action are gathered on.
pub const PAGE: &str = "docs/more/contribute-here.md";

/// The heading that marks a call to action on a feature's page, as a
/// section or as a subsection of the task it extends.
pub const HEADINGS: [&str; 2] = ["## Contribute Here", "### Contribute Here"];

/// Each page's call to action, keyed `<title>: <first sentence>`.
pub fn catalog(tree: &Tree) -> Items {
    let mut items = Items::new();
    for (path, text) in tree.docs() {
        if let Some(sentence) = first_sentence(text)
            && let Some(title) = title_of(text)
        {
            entry(&mut items, format!("{title}: {sentence}"), path);
        }
    }
    items
}

/// One entry, linking the section on its own page.
pub fn line(name: &str, page: &Path, source: &Path) -> String {
    let (title, sentence) = name.split_once(": ").expect("a title and its sentence");
    format!(
        "- [{title}]({}#contribute-here): {sentence}\n",
        super::relative(page.parent().unwrap(), source)
    )
}

/// The first sentence of the paragraph under the section's heading, its
/// links reduced to their text, since their targets are relative to it.
fn first_sentence(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .skip_while(|line| !HEADINGS.contains(&line.trim_end()));
    lines.next()?;
    let paragraph: Vec<&str> = lines
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::trim)
        .collect();
    let paragraph = unlink(&paragraph.join(" "));
    let end = paragraph
        .match_indices(". ")
        .map(|(at, _)| at + 1)
        .next()
        .unwrap_or(paragraph.len());
    let sentence = paragraph[..end].trim().to_owned();
    (!sentence.is_empty()).then_some(sentence)
}

/// `[text](target)` becomes `text`.
fn unlink(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|at| open + at) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|at| close + at) else {
            break;
        };
        out += &rest[..open];
        out += &rest[open + 1..close];
        rest = &rest[end + 1..];
    }
    out + rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_first_sentence_without_its_links() {
        let page = "# Researcher\n\n## Contribute Here\n\nA [skill](skill.md) for your field\nmakes it better. More here.\n\n## Docs\n";
        assert_eq!(
            first_sentence(page).as_deref(),
            Some("A skill for your field makes it better.")
        );
    }

    #[test]
    fn a_subsection_asks_too() {
        let page = "# Page\n\n## Task\n\nDo it.\n\n### Contribute Here\n\nAn adapter helps.\n";
        assert_eq!(first_sentence(page).as_deref(), Some("An adapter helps."));
    }

    #[test]
    fn a_page_without_the_section_asks_nothing() {
        assert_eq!(first_sentence("# Page\n\n## Help\n\nNo.\n"), None);
    }
}
