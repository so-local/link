//! The rules of the websites' docs: the public workspace's, where every
//! folder's `docs/` is a part of www.local.link or docs.local.link.

use std::path::{Path, PathBuf};

use crate::md::{anchors, headings, links, split};
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "history",
        scope: Scope::Site,
        fix: "public prose contains development history; say how it works now, and keep \
              dates, attribution and experiments out",
        check: Check::Each(history),
    },
    Rule {
        id: "links",
        scope: Scope::Every,
        fix: "broken links: each relative link and anchor resolves, and a link to a page \
              goes to its file by a relative path, never to its address on a website",
        check: Check::Whole(links_everywhere),
    },
    Rule {
        id: "headings",
        scope: Scope::Site,
        fix: "too many headings under one; group them a level deeper, keeping their words",
        check: Check::Each(headings_under),
    },
    Rule {
        id: "pages",
        scope: Scope::Site,
        fix: "too many pages under one, or two at one URL; group them in a folder a level deeper",
        check: Check::Whole(pages_under),
    },
    Rule {
        id: "website-words",
        scope: Scope::Site,
        fix: "website prose lives in docs/, UI copy in its catalog, and navigation.md holds structural page lists only",
        check: Check::Whole(website_words),
    },
    Rule {
        id: "translations",
        scope: Scope::Site,
        fix: "a translation names its English on its first line and, while that English is \
              as it was, keeps its headings and its code blocks byte for byte",
        check: Check::Each(translations),
    },
    Rule {
        id: "decisions",
        scope: Scope::Site,
        fix: "move these `## Decisions` sections to a decisions.md beside the doc",
        check: Check::Each(decisions),
    },
    Rule {
        id: "get-started",
        scope: Scope::Site,
        fix: "give these a `#` title of their own, a short pitch (the tree still calls them \
              Get Started)",
        check: Check::Each(get_started),
    },
    Rule {
        id: "title-case",
        scope: Scope::Site,
        fix: "A website app's own pages put every heading in title case: each word \
              capitalized but the short words inside it (a, and, the, of...)",
        check: Check::Each(title_case),
    },
    Rule {
        id: "titles",
        scope: Scope::Every,
        fix: "a README's or docs page's title, the name every index lists it by, is in \
              title case: each word capitalized but the short words inside it; code goes in \
              backticks",
        check: Check::Whole(titles),
    },
    Rule {
        id: "opening",
        scope: Scope::Site,
        fix: "a page opens with a paragraph under its title, saying what the reader gets: its \
              first sentence is what every README that links the page says of it",
        check: Check::Each(opening),
    },
    Rule {
        id: "comments",
        scope: Scope::Site,
        fix: "move these comments to the line above (zsh passes them on as arguments)",
        check: Check::Each(comments),
    },
    Rule {
        id: "zsh",
        scope: Scope::Site,
        fix: "these comments break the line in a Mac's zsh: plain words only, and none after \
              `--` (put it on the line above, in plain words)",
        check: Check::Each(zsh),
    },
    Rule {
        id: "limits",
        scope: Scope::Site,
        fix: "the writing guide's table of limits is limits.toml's: put this in its place",
        check: Check::Whole(limits_table),
    },
    Rule {
        id: "rules",
        scope: Scope::Site,
        fix: "the Docs Rules page has a line for each rule, naming its id, with its \
              limit's number from limits.toml",
        check: Check::Whole(rules_page),
    },
];

fn history(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(path) {
        return Vec::new();
    }
    let (prose, _) = split(text);
    let mut found = Vec::new();
    // Join wrapped lines so attribution cannot cross a line unnoticed.
    let mut paragraph = String::new();
    for line in prose.into_iter().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if historical_prose(&paragraph) {
                found.push(fail(format!("{}: {}", tree.shown(path), paragraph.trim())));
            }
            paragraph.clear();
        } else {
            paragraph.push_str(line);
            paragraph.push(' ');
        }
    }
    found
}

/// The lexical form of a path: `..` and `.` taken out.
pub fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Every page of the website, and every document the README indexes hold,
/// in any workspace: a link in any of them resolves.
fn links_everywhere(tree: &Tree) -> Vec<Finding> {
    let packages = crate::shape::packages(tree);
    tree.files
        .iter()
        .filter(|(doc, _)| tree.is_doc(doc) || crate::navigation::selected(tree, &packages, doc))
        .flat_map(|(doc, text)| links_resolve(tree, doc, text))
        .collect()
}

fn links_resolve(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    let pages = &tree.pages;
    let anchors_of = |path: &Path| match tree.text(path) {
        Some(text) => anchors(text),
        None => anchors(&std::fs::read_to_string(path).unwrap_or_default()),
    };
    let mut broken = Vec::new();
    for target in links(&split(text).0) {
        // A page of the site is linked by its file, so a reader stays in
        // the repository: the website turns the path into the page's URL.
        // Only a website itself, `https://docs.local.link/`, is linked as
        // such, and the checkout's `.github/`, which GitHub shows outside
        // the tree, names a page by its address beside its file.
        if let Some((website, address)) = tree.site_page(target) {
            if crate::outlines::metadata(tree, doc) {
                if !pages.contains_key(address) {
                    broken.push(format!("{}: {target} (no such page)", tree.shown(doc)));
                }
                continue;
            }
            let community = doc.starts_with(tree.checkout.join(".github"));
            if !community && address != website.url {
                let file = pages.get(address).and_then(|p| p.first());
                broken.push(match file {
                    Some(file) => format!(
                        "{}: {target} (link to the page's file: {})",
                        tree.shown(doc),
                        tree.shown(file)
                    ),
                    None => format!("{}: {target} (no such page)", tree.shown(doc)),
                });
            }
            continue;
        }
        if target.contains("://") || target.starts_with("mailto:") || target.is_empty() {
            continue;
        }
        let (path, anchor) = target.split_once('#').unwrap_or((target, ""));
        let to = match path {
            "" => doc.to_path_buf(),
            path => normal(&doc.parent().unwrap().join(crate::md::link_path(
                path.split('?').next().unwrap_or_default(),
            ))),
        };
        if !(tree.files.contains_key(&to) || tree.dirs.contains(&to) || to.exists()) {
            broken.push(format!("{}: {target} (no such file)", tree.shown(doc)));
        } else if !anchor.is_empty()
            && to.extension().is_some_and(|e| e == "md")
            && !anchors_of(&to).contains(anchor)
        {
            broken.push(format!("{}: {target} (no such heading)", tree.shown(doc)));
        }
    }
    broken.into_iter().map(fail).collect()
}

/// Whether a file is a page of the site, in a folder's `docs/`.
pub fn is_page(tree: &Tree, path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md")
        && !crate::outlines::metadata(tree, path)
        && tree
            .folders
            .iter()
            .any(|folder| path.starts_with(folder.join("docs")))
}

/// No heading has more than the limit under it, so a reader takes a
/// page in at a glance: a longer list is grouped a level deeper.
fn headings_under(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) || tree.generated_exempt("headings", file, text) {
        return Vec::new();
    }
    let most = tree.limits.of("headings");
    let mut crowded = Vec::new();
    // Each open heading's level, text and count of headings under it.
    let mut open: Vec<(usize, &str, usize)> = Vec::new();
    let mut top = 0;
    let mut close = |open: &mut Vec<(usize, &str, usize)>, level: usize| {
        while open.last().is_some_and(|(l, _, _)| *l >= level) {
            let (_, heading, under) = open.pop().unwrap();
            if under > most {
                crowded.push(fail(format!("{} \"{heading}\": {under}", tree.shown(file))));
            }
        }
    };
    let authored = if file.file_name().is_some_and(|n| n == "README.md") {
        crate::navigation::authored(text)
    } else {
        text.to_owned()
    };
    for (level, heading) in headings(&split(&authored).0) {
        close(&mut open, level);
        match open.last_mut() {
            Some((_, _, under)) => *under += 1,
            None => top += 1,
        }
        open.push((level, heading, 0));
    }
    close(&mut open, 0);
    if top > most {
        crowded.push(fail(format!(
            "{}: {top} headings at its top",
            tree.shown(file)
        )));
    }
    crowded
}

/// Each website's tree, its `navigation.md`'s `## Pages`, holds at most the
/// limit's pages under a page, and each website one page at each URL.
fn pages_under(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (url, files) in &tree.pages {
        if files.len() > 1 {
            let files: Vec<String> = files.iter().map(|f| tree.shown(f)).collect();
            found.push(fail(format!("two pages at {url}: {}", files.join(" and "))));
        }
    }
    let addresses = crate::structure::addresses(tree);
    for web in tree.web() {
        let file = web.app.join("docs/navigation.md");
        let site = match crate::outlines::expanded_text(tree, &file) {
            Ok(site) => site,
            Err(error) => {
                found.push(fail(error));
                continue;
            }
        };
        let directory = file.parent().unwrap();
        // A page's own limit, by its address: `limits.toml`'s exception
        // for it, else the rule's. The top is the home page, `/`.
        let most = |target: &str| {
            let address = match target {
                "" => Some("/"),
                target => {
                    let at = target.find(['#', '?', ' ']).unwrap_or(target.len());
                    let page = normal(&directory.join(crate::md::link_path(&target[..at])));
                    addresses.get(&page).map(String::as_str)
                }
            };
            address.map_or_else(
                || tree.limits.of("pages"),
                |address| tree.limits.at("pages", address),
            )
        };
        for (page, count) in crowded(&site, &most) {
            found.push(fail(format!(
                "{}'s ## Pages: {page}: {count} pages under it",
                tree.shown(&file)
            )));
        }
    }
    found
}

/// Each item of `navigation.md`'s `## Pages` list with more items under
/// it than `most` gives for its target, and how many: `(top)` for the
/// root pages themselves, whose target is empty.
pub fn crowded(site: &str, most: &dyn Fn(&str) -> usize) -> Vec<(String, usize)> {
    fn visit(
        nodes: &[crate::outlines::Node],
        parent: (&str, &str),
        most: &dyn Fn(&str) -> usize,
        found: &mut Vec<(String, usize)>,
    ) {
        for node in nodes {
            let shown = format!("[{}]({})", node.label, node.target);
            visit(&node.children, (&shown, &node.target), most, found);
        }
        let (shown, target) = parent;
        if nodes.len() > most(target) {
            found.push((shown.to_owned(), nodes.len()));
        }
    }
    let mut found = Vec::new();
    if let Ok(nodes) = crate::outlines::nodes(site) {
        visit(&nodes, ("(top)", ""), most, &mut found);
    }
    found
}

/// Product prose stays in docs, UI copy in its catalog, and navigation
/// metadata holds structure only. Website source holds no Markdown prose.
fn website_words(tree: &Tree) -> Vec<Finding> {
    let mut code: Vec<PathBuf> = tree.web().map(|w| w.app.join("src")).collect();
    if code.is_empty() {
        return Vec::new();
    }
    code.push(tree.root.join("web/packages/site/src"));
    let mut found: Vec<Finding> = tree
        .files
        .keys()
        .filter(|p| code.iter().any(|c| p.starts_with(c)))
        .filter(|p| p.extension().is_some_and(|e| e == "md" || e == "mdx"))
        .map(|p| fail(format!("{}: words in the website's code", tree.shown(p))))
        .collect();
    for (file, text) in &tree.files {
        if crate::outlines::metadata(tree, file) && !file.ends_with("website.md") {
            let translated = crate::navigation::translation(file).is_some();
            if let Err(error) = crate::outlines::structure(text, translated) {
                found.push(fail(format!("{}: {error}", tree.shown(file))));
            }
        }
    }
    if tree.folders.len() <= 2 {
        found.push(fail("the documentation index lists no folders"));
    }
    found
}

/// The first 16 hex digits of a text's SHA-256, which a translation
/// records of the English it was made from.
pub fn hash16(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A translated page, `<page>.<language>.md` beside its English
/// `<page>.md`, names on its first line that English and the hash of it
/// the page was made from. While the English is as it was, the page has
/// its headings and, byte for byte, its code blocks, so no translation
/// changes a command. A page whose English changed since is only behind:
/// the website says so, and English never waits.
fn translations(tree: &Tree, page: &Path, text: &str) -> Vec<Finding> {
    let Some((english_path, _)) = crate::navigation::translation(page) else {
        return Vec::new();
    };
    let count = |text: &str| headings(&split(text).0).len();
    let file = english_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shown = tree.shown(page);
    let named = text
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("<!-- Translated from "))
        .and_then(|rest| rest.split_once(" ("))
        .map(|(source, rest)| (source.to_string(), rest.get(..16).unwrap_or("").to_string()));
    let Some((source, made)) = named else {
        return vec![fail(format!("{shown}: no first line naming its English"))];
    };
    if source != file {
        return vec![fail(format!("{shown}: names {source}, not {file}"))];
    }
    let Some(english) = tree.text(&english_path) else {
        return vec![fail(format!("{shown}: {file} is gone"))];
    };
    if hash16(english) != made {
        return Vec::new();
    }
    // navigation.md's URLs are the translation's own.
    let text = text.split("\n## URLs\n").next().unwrap_or_default();
    let mut found = Vec::new();
    if count(text) != count(english) {
        found.push(fail(format!(
            "{shown}: {} headings, its English {}",
            count(text),
            count(english)
        )));
    }
    if split(text).1 != split(english).1 {
        found.push(fail(format!(
            "{shown}: its code blocks differ from its English"
        )));
    }
    found
}

fn decisions(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    match tree.is_doc(doc) && split(text).0.iter().any(|l| l.trim() == "## Decisions") {
        true => vec![fail(tree.shown(doc))],
        false => Vec::new(),
    }
}

/// The tree calls a section's first page Get Started; its `#` title is a
/// pitch of its own, so no two Get Started pages look the same.
fn get_started(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    let titled = split(text)
        .0
        .iter()
        .any(|l| l.trim().eq_ignore_ascii_case("# Get Started"));
    match tree.is_doc(doc) && titled {
        true => vec![fail(tree.shown(doc))],
        false => Vec::new(),
    }
}

/// Words that stay lowercase inside a title.
const SMALL: &[&str] = &[
    "a", "an", "and", "as", "at", "but", "by", "for", "from", "in", "into", "nor", "of", "on",
    "onto", "or", "per", "the", "to", "via", "vs", "with",
];

/// The words of a heading that aren't in title case: each capitalized
/// but the short words inside it, each part of a hyphenated word too,
/// and the first and the last always. Code, amounts and addresses are
/// left as they are; a link's words are its text.
pub fn untitled(heading: &str) -> Vec<String> {
    let mut text = String::new();
    let mut rest = heading;
    // `[text](target)` read as its text.
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|c| open + c) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|e| close + e) else {
            break;
        };
        text.push_str(&rest[..open]);
        text.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    text.push_str(rest);
    // Code, spaces and all, is one word left as it is.
    let text: String = text
        .split('`')
        .enumerate()
        .map(|(i, part)| if i % 2 == 1 { "`" } else { part })
        .collect();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut wrong = Vec::new();
    for (i, raw) in words.iter().enumerate() {
        let word = raw
            .trim_start_matches(['(', '"', '\'', '“', '‘'])
            .trim_end_matches([')', '"', '\'', '”', '’', '.', ',', ':', ';', '!', '?', '…']);
        // A name cased its own way, such as macOS, is left as it is.
        let brand = word.starts_with(|c: char| c.is_ascii_lowercase())
            && word.chars().any(|c| c.is_ascii_uppercase());
        let code = brand || word.contains(['.', '/', '@', '_', '`', '$']);
        if code || !word.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        // A title's first word, its last, and the first after a colon.
        let edge = i == 0 || i + 1 == words.len() || words[i - 1].ends_with(':');
        let lower = word.split('-').enumerate().any(|(n, part)| {
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && ((edge && n == 0) || !SMALL.contains(&part))
        });
        if lower {
            wrong.push(raw.to_string());
        }
    }
    wrong
}

/// Each README's and `docs/` page's title, the name the indexes list
/// it by; a record such as a session's log keeps the title it was given.
fn titles(tree: &Tree) -> Vec<Finding> {
    let navigation = crate::navigation::Navigation::new(tree);
    navigation
        .documents()
        .filter(|doc| doc.ends_with("README.md") || crate::navigation::owned_docs(tree, doc))
        .filter_map(|doc| {
            let title = split(tree.text(doc)?)
                .0
                .into_iter()
                .find_map(|l| l.strip_prefix("# "))?;
            let wrong = untitled(title);
            (!wrong.is_empty()).then(|| {
                fail(format!(
                    "{}: {title} ({})",
                    tree.shown(doc),
                    wrong.join(", ")
                ))
            })
        })
        .collect()
}

/// A website app's own pages, whose headings are in title case, a
/// hook among them: a website app's own `docs/`, but its outline.
fn title_case(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    let own = tree
        .websites
        .iter()
        .any(|web| doc.starts_with(web.app.join("docs")));
    if !own || !is_page(tree, doc) {
        return Vec::new();
    }
    let prose = split(text).0;
    // A hook, the quote under the title, is shown as the page's heading.
    let hook = prose
        .iter()
        .skip_while(|l| !l.starts_with("# "))
        .skip(1)
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.strip_prefix("> "));
    headings(&prose)
        .into_iter()
        .map(|(_, heading)| heading)
        .chain(hook)
        .filter_map(|heading| {
            let wrong = untitled(heading);
            (!wrong.is_empty()).then(|| {
                fail(format!(
                    "{}: {heading} ({})",
                    tree.shown(doc),
                    wrong.join(", ")
                ))
            })
        })
        .collect()
}

/// A page's first paragraph, under its title and past any comment: the
/// first sentence each README linking the page gives as what it is.
fn opening(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, doc) {
        return Vec::new();
    }
    let prose = split(text).0;
    let mut comment = false;
    let first = prose
        .iter()
        .skip_while(|l| !l.starts_with("# "))
        .skip(1)
        // A hook, a quote under the title, comes before the paragraph.
        .skip_while(|l| l.trim().is_empty() || l.starts_with('>'))
        .find(|l| {
            if comment || l.starts_with("<!--") {
                comment = !l.contains("-->");
                return false;
            }
            !l.trim().is_empty()
        });
    match first {
        Some(l) if !l.starts_with(['#', '|']) && !l.starts_with("- ") => Vec::new(),
        _ => vec![fail(tree.shown(doc))],
    }
}

fn comments(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(doc) {
        return Vec::new();
    }
    split(text)
        .1
        .into_iter()
        .filter(|(info, line)| {
            matches!(*info, "sh" | "bash" | "shell" | "zsh") && commented_other(line)
        })
        .map(|(_, line)| fail(format!("{}: {line}", tree.shown(doc))))
        .collect()
}

fn zsh(tree: &Tree, doc: &Path, text: &str) -> Vec<Finding> {
    if !tree.is_doc(doc) {
        return Vec::new();
    }
    split(text)
        .1
        .into_iter()
        .filter(|(info, _)| matches!(*info, "sh" | "bash" | "shell" | "zsh"))
        .filter_map(|(_, line)| {
            zsh_breaks(line).map(|why| fail(format!("{} ({why}): {line}", tree.shown(doc))))
        })
        .collect()
}

/// The writing guide shows the limits as `limits.toml` states them, so
/// a limit changes in both at once.
fn limits_table(tree: &Tree) -> Vec<Finding> {
    let guide = tree
        .root
        .join("tests/docs/docs/more/developing/documentation/writing-docs.md");
    let table = tree.limits.table();
    match tree.text(&guide).is_some_and(|text| text.contains(&table)) {
        true => Vec::new(),
        false => vec![fail(format!("{}:\n{table}", tree.shown(&guide)))],
    }
}

fn rules_page(tree: &Tree) -> Vec<Finding> {
    let rules: Vec<Rule> = crate::rules()
        .into_iter()
        .filter(|r| r.id != "rules")
        .collect();
    crate::rules_page(tree, &rules)
}

/// Public prose describes current behavior. Examples and legal effective
/// dates are product facts; dated development records belong elsewhere.
pub fn historical_prose(line: &str) -> bool {
    let prose = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
    let text = prose
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if [
        "decided by ",
        "we considered ",
        "recommendation taken",
        "was decided against",
        "we rejected ",
        "it used to ",
        "we used to ",
        "they used to ",
        "the program used to ",
        "it started as ",
        "it was a file ",
        "there were two",
        "we tried ",
        "we measured ",
        "measured on ",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
    {
        return true;
    }
    let words: Vec<_> = text
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .collect();
    let day = |word: &str| word.parse::<u8>().is_ok_and(|n| (1..=31).contains(&n));
    words.iter().enumerate().any(|(i, word)| {
        let month = [
            "jan",
            "january",
            "feb",
            "february",
            "mar",
            "march",
            "apr",
            "april",
            "may",
            "jun",
            "june",
            "jul",
            "july",
            "aug",
            "august",
            "sep",
            "september",
            "oct",
            "october",
            "nov",
            "november",
            "dec",
            "december",
        ]
        .contains(word);
        let natural =
            month && (words.get(i + 1).is_some_and(|next| day(next)) || i > 0 && day(words[i - 1]));
        let bytes = word.as_bytes();
        let iso = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(at, byte)| matches!(at, 4 | 7) || byte.is_ascii_digit());
        // Cutoffs, expiry and legal dates are meaningful product facts.
        // The qualifier must be next to this date, not elsewhere on the page.
        let product = words[i.saturating_sub(3)..i]
            .iter()
            .any(|word| ["until", "expires", "effective", "updated", "cutoff"].contains(word));
        (natural || iso) && !product
    })
}

/// Whether a shell line runs something other than `lnk` with a `# note`
/// after it, which zsh (the Mac's shell, without `interactive_comments`)
/// passes on as arguments; `lnk` drops it.
pub fn commented_other(line: &str) -> bool {
    let Some((command, _)) = line.split_once(" # ") else {
        return false;
    };
    let command = command.trim();
    let words: Vec<&str> = command.split_whitespace().collect();
    let program = words
        .iter()
        .find(|w| !(w.contains('=') && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())));
    !command.is_empty()
        && !command.starts_with('#')
        && !program.is_some_and(|p| *p == "lnk" || p.ends_with("/lnk"))
}

/// Why a shell line's `# note`, after `lnk` or on its own, breaks it in
/// zsh without `interactive_comments` (a Mac's Terminal with no
/// `~/.zshrc`), where the note is words: one after `--` goes to the program behind it, and
/// one holding a quote, a glob, a bracket, a `;`, a pipe, a redirection
/// or a backtick is read as that. None when it pastes as it reads.
pub fn zsh_breaks(line: &str) -> Option<&'static str> {
    // A comment on its own line is words too: `#` run as a command.
    let (command, note) = match line.trim_start().strip_prefix('#') {
        Some(note) => ("", note),
        None => line.split_once(" # ")?,
    };
    let command = command.trim();
    if commented_other(line) {
        return None;
    }
    if command.split_whitespace().any(|w| w == "--") {
        return Some("after `--`, it's the program's");
    }
    const READ_AS_SHELL: &[char] = &[
        '\'', '"', '?', '*', '[', ']', '(', ')', '{', '}', ';', '|', '&', '<', '>', '`',
    ];
    note.contains(READ_AS_SHELL)
        .then_some("it holds one of ' \" ? * [ ] ( ) { } ; | & < > `")
}
