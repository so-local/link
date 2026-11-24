//! A list names each page by its title: a list item that opens with a
//! link to a Markdown page says that page's title as the link's text, so
//! a reader who follows it lands on the heading they clicked. A link in
//! prose reads as part of its sentence and keeps its own words, and so
//! does a link to a heading (`page.md#part`), a link whose text is code
//! (a file's or a command's own name), a navigation outline's entry, and
//! an entry of a generated inventory, which names a declared thing by its
//! own name. Backticks don't count, since a reading index writes a title
//! without them.

use std::path::{Path, PathBuf};

use crate::{Check, Finding, Rule, Scope, Tree};

pub const RULES: &[Rule] = &[Rule {
    id: "list-titles",
    scope: Scope::Every,
    fix: "a list item's link says the title of the page it opens: make the link's text that \
          title, and say anything more outside the link (\"Agents: [Security](...)\")",
    check: Check::Each(check),
}];

/// The outline files whose entries are the website's own labels.
const OUTLINE: &str = "navigation.md";

fn check(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if path.extension().is_none_or(|e| e != "md")
        || path.file_name().is_some_and(|n| n == OUTLINE)
        || tree.is_data(path)
    {
        return Vec::new();
    }
    let (prose, _) = crate::md::split(text);
    let mut found = Vec::new();
    let mut inventory = false;
    for line in prose {
        let marker = line.trim();
        if marker.starts_with("<!-- inventory:") {
            inventory = marker.ends_with(":start -->");
            continue;
        }
        if inventory {
            continue;
        }
        let Some((said, target)) = item_link(line) else {
            continue;
        };
        if said.starts_with('`') && said.ends_with('`') {
            continue;
        }
        let Some(page) = page(path, target) else {
            continue;
        };
        let Some(title) = tree.text(&page).and_then(title) else {
            continue;
        };
        if said.replace('`', "") != title.replace('`', "") {
            found.push(crate::fail(format!(
                "{}: [{said}]({target}) opens a page titled \"{title}\"",
                tree.shown(path)
            )));
        }
    }
    found
}

/// The text and target of the link a list item opens with, bold or not:
/// `- [text](target)` or `- **[text](target)**`.
pub fn item_link(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))?;
    let rest = rest.strip_prefix("**").unwrap_or(rest);
    let rest = rest.strip_prefix('[')?;
    let (said, rest) = rest.split_once("](")?;
    // A task's box (`- [ ] ...`) opens no link, so text holding a bracket
    // isn't one link's text.
    if said.contains(['[', ']']) {
        return None;
    }
    let end = rest.find([')', ' '])?;
    Some((said, &rest[..end]))
}

/// The Markdown file a link's target opens, from the file at `from`:
/// none for another site, a heading, or a file that isn't Markdown.
fn page(from: &Path, target: &str) -> Option<PathBuf> {
    if target.contains("://") || target.contains('#') || !target.ends_with(".md") {
        return None;
    }
    let mut page = from.parent()?.to_path_buf();
    for part in Path::new(target).components() {
        match part {
            std::path::Component::ParentDir => {
                page.pop();
            }
            std::path::Component::Normal(name) => page.push(name),
            _ => {}
        }
    }
    Some(page)
}

/// A page's title: its first `# ` heading outside code.
pub fn title(text: &str) -> Option<&str> {
    let (prose, _) = crate::md::split(text);
    prose
        .into_iter()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
}

#[cfg(test)]
mod tests {
    use super::{item_link, title};

    #[test]
    fn a_list_items_opening_link_is_read_bold_or_not() {
        assert_eq!(
            item_link("- [Boxes](boxes.md): machines"),
            Some(("Boxes", "boxes.md"))
        );
        assert_eq!(
            item_link("  * **[Boxes](a/b.md)**"),
            Some(("Boxes", "a/b.md"))
        );
        assert_eq!(item_link("- Agents: [Security](s.md)"), None);
        assert_eq!(item_link("Read [Boxes](boxes.md) first."), None);
        assert_eq!(item_link("- [ ] **A task:** see [Boxes](boxes.md)"), None);
    }

    #[test]
    fn a_pages_title_is_its_first_heading_outside_code() {
        assert_eq!(
            title("```\n# not this\n```\n# Boxes\n## Part"),
            Some("Boxes")
        );
        assert_eq!(title("No heading"), None);
    }
}
