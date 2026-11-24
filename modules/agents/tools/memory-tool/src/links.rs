//! The links in a document: markdown's (`[text](notes/other.md)`) and
//! wiki links (`[[Other note]]`), as `links` gives them. Only what stays
//! in the folder: a link to the web, or one that climbs out of the
//! folder (`../../x`), is no link here. Nothing is followed on disk: a
//! link names a document the index may hold.

use std::collections::HashSet;

/// The most links kept of one document.
pub const MAX_PER_DOCUMENT: usize = 1000;

/// The longest link target, in bytes: a longer one is no link. It also
/// bounds how far each link is looked for, so parsing is linear.
pub const MAX_TARGET: usize = 512;

/// A link, as its document writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// A markdown link's path from the folder (`notes/other.md`); a wiki
    /// link's name, lowercase (`other note`).
    pub target: String,
    pub wiki: bool,
    /// The line it's on, from 1.
    pub line: usize,
}

/// The links in document `path`'s `body`, in order, each once per line,
/// the first [`MAX_PER_DOCUMENT`].
pub fn parse(path: &str, body: &str) -> Vec<Link> {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut links: Vec<Link> = Vec::new();
    let mut fenced = false;
    for (n, line) in body.lines().enumerate() {
        if links.len() >= MAX_PER_DOCUMENT {
            break;
        }
        // Nothing in a code block is a link.
        if is_fence(line) {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut on_line = HashSet::new();
        let mut found = |target: Option<String>, wiki: bool| {
            let Some(target) = target.filter(|t| !t.is_empty() && t.len() <= MAX_TARGET) else {
                return;
            };
            if links.len() < MAX_PER_DOCUMENT && on_line.insert((target.clone(), wiki)) {
                links.push(Link {
                    target,
                    wiki,
                    line: n + 1,
                });
            }
        };
        for name in wiki_names(line) {
            found(Some(name), true);
        }
        for target in markdown_targets(line) {
            found(resolve(dir, &target), false);
        }
    }
    links
}

/// Whether `line` opens or closes a fenced code block: three backticks
/// or more.
fn is_fence(line: &str) -> bool {
    line.trim_start().chars().take_while(|c| *c == '`').count() >= 3
}

/// `[[name]]`, `[[name|shown]]` and `[[name#heading]]`: the names.
fn wiki_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = near(after).find("]]") else {
            rest = after;
            continue;
        };
        let inner = &after[..close];
        let name = inner.split(['|', '#']).next().unwrap_or("").trim();
        if !name.is_empty() {
            names.push(name.to_lowercase());
        }
        rest = &after[close + 2..];
    }
    names
}

/// `[text](target)`, not an image's `![alt](target)`: the targets as
/// written.
fn markdown_targets(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut from = 0;
    while let Some(at) = line[from..].find("](") {
        let at = from + at;
        // Its text starts after the last link's `](`, never further back.
        let before = &line[from..at];
        from = at + 2;
        let Some(open) = before.rfind('[') else {
            continue;
        };
        // A wiki link's second bracket, or an image.
        if before[..open].ends_with('[') || before[..open].ends_with('!') {
            continue;
        }
        let after = near(&line[at + 2..]);
        let target = match after.strip_prefix('<') {
            Some(angled) => angled.split_once('>').map(|(t, _)| t),
            None => after
                .split_once(')')
                .map(|(t, _)| t.split(' ').next().unwrap_or(t)),
        };
        if let Some(target) = target {
            targets.push(target.trim().to_string());
        }
    }
    targets
}

/// The start of `text`, as far as a link's end is looked for.
fn near(text: &str) -> &str {
    let mut end = text.len().min(MAX_TARGET + 2);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A markdown link's target as a path from the folder, from the folder
/// `dir` of the document it's in; none for the web, an anchor alone, or
/// out of the folder.
fn resolve(dir: &str, target: &str) -> Option<String> {
    if target.contains("://") || target.starts_with("mailto:") || target.starts_with('#') {
        return None;
    }
    let target = target.split(['#', '?']).next().unwrap_or("");
    let target = decode(target);
    let (base, target) = match target.strip_prefix('/') {
        Some(rooted) => ("", rooted.to_string()),
        None => (dir, target),
    };
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `%20` and the like, as a link writes a space.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether wiki link `name` names document `path`: its path, with or
/// without its extension, or its file's name without it.
pub fn names(name: &str, path: &str) -> bool {
    names_of(path).iter().any(|n| n == name)
}

/// The names a wiki link finds document `path` by, lowercase: its path,
/// with and without its extension, and its file's name without it.
pub fn names_of(path: &str) -> [String; 3] {
    let path = path.to_lowercase();
    let without = path.rsplit_once('.').map_or(path.as_str(), |(p, _)| p);
    let file = without.rsplit('/').next().unwrap_or(without);
    let (without, file) = (without.to_string(), file.to_string());
    [path, without, file]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(path: &str, body: &str) -> Vec<(String, bool, usize)> {
        parse(path, body)
            .into_iter()
            .map(|l| (l.target, l.wiki, l.line))
            .collect()
    }

    #[test]
    fn markdown_and_wiki_links_inside_the_folder() {
        let body = "See [the plan](plan.md) and [[Garden Beds|the beds]].\n\
                    Also [up](../people/maya.md#birthday), [root](/notes/car.md),\n\
                    [spaced](my%20trip.md) and [angled](<other note.md>).\n\
                    [[Maya#Allergies]] and [[]] and [[ ]].\n";
        assert_eq!(
            targets("notes/today.md", body),
            [
                ("garden beds".into(), true, 1),
                ("notes/plan.md".into(), false, 1),
                ("people/maya.md".into(), false, 2),
                ("notes/car.md".into(), false, 2),
                ("notes/my trip.md".into(), false, 3),
                ("notes/other note.md".into(), false, 3),
                ("maya".into(), true, 4),
            ]
        );
    }

    #[test]
    fn nothing_outside_the_folder_the_web_or_code_is_a_link() {
        let body = "[web](https://example.com/a.md) [mail](mailto:a@b.c) [top](#top)\n\
                    [out](../../etc/passwd) ![image](photo.png)\n\
                    ```\n[in code](code.md) [[In code]]\n```\n\
                    [after](after.md)\n";
        assert_eq!(
            targets("notes/today.md", body),
            [("notes/after.md".into(), false, 6)]
        );
    }

    #[test]
    fn parsing_takes_time_in_proportion_to_the_document() {
        // A megabyte of unique links, then of openings that never close.
        let unique: String = (0..80_000).map(|n| format!("[[n{n}]]")).collect();
        let open = "[[".repeat(500_000);
        let brackets = "](".repeat(500_000);
        let angled = "[a](<".repeat(200_000);
        let started = std::time::Instant::now();
        assert_eq!(parse("a.md", &unique).len(), MAX_PER_DOCUMENT);
        for body in [&open, &brackets, &angled] {
            assert!(parse("a.md", body).is_empty());
        }
        let long = format!("[[{}]] [x]({}.md)", "a".repeat(600), "b".repeat(600));
        assert!(parse("a.md", &long).is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_wiki_name_names_a_path_or_a_file() {
        assert!(names("garden beds", "notes/Garden Beds.md"));
        assert!(names("notes/garden beds", "notes/Garden Beds.md"));
        assert!(names("notes/garden beds.md", "notes/Garden Beds.md"));
        assert!(!names("garden", "notes/Garden Beds.md"));
        assert!(!names("beds.md", "notes/Garden Beds.md"));
    }
}
