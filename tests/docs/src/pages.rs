//! The rules of a page's place in the site: a section's hook, a module's
//! quickstart, and troubleshooting as a page of its own.

use std::path::Path;

use crate::prose::{Block, blocks, characters, named, shaped};
use crate::site::is_page;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[
    Rule {
        id: "hook-length",
        scope: Scope::Site,
        fix: "a hook is a line, read at a glance, that opens with its keyword and holds no colon: say the rest \
              in the paragraph under it",
        check: Check::Each(hook_length),
    },
    Rule {
        id: "hooks",
        scope: Scope::Site,
        fix: "a section's page, one with pages under it in navigation.md's ## Pages, has a hook: \
              a one-line quote right under its plain title, which the website shows as its \
              heading; any other page has none",
        check: Check::Whole(hooks),
    },
    Rule {
        id: "quickstart",
        scope: Scope::Site,
        fix: "a module's page opens with a quickstart: under its opening paragraph, before its \
              first ## heading or under its marked first task, an sh block that gets it working, assuming only lnk \
              installed and lnk up run; a module only Link runs says quickstart = false in its module.toml's \
              [documentation]",
        check: Check::Whole(quickstart),
    },
    Rule {
        id: "quickstart-env",
        scope: Scope::Site,
        fix: "a quickstart sets no environment variable: run the command alone, and let it ask \
              for what it needs (lnk agent start asks for a model's key)",
        check: Check::Each(quickstart_env),
    },
    Rule {
        id: "output-paths",
        scope: Scope::Site,
        fix: "Link writes only under ~/Link and ~/.lnk: an lnk command's --out, --to or --dir, and \
              LNK_INSTALL_DIR, name a folder there, or are left out for the layout's default",
        check: Check::Each(output_paths),
    },
    Rule {
        id: "troubleshooting",
        scope: Scope::Site,
        fix: "troubleshooting is a page of its own, troubleshooting.md beside the feature's: \
              each symptom a heading, with its fix under it",
        check: Check::Each(troubleshooting),
    },
];

/// A page's hook: the quote right under its title.
pub fn hook(blocks: &[Block]) -> Option<&str> {
    let title = blocks
        .iter()
        .position(|b| matches!(b, Block::Heading(..)))?;
    match blocks.get(title + 1) {
        Some(Block::Quote(hook)) => Some(hook),
        _ => None,
    }
}

/// The pages each website's `## Pages` has pages under, its sections,
/// each with its name there: none for a site's first tab, Get Started,
/// or for a tab with a short label.
fn sections(tree: &Tree) -> std::collections::BTreeMap<std::path::PathBuf, Option<String>> {
    let mut found = std::collections::BTreeMap::new();
    for web in tree.web() {
        let file = web.app.join("docs/navigation.md");
        let dir = file.parent().expect("a folder").to_path_buf();
        fn sections(
            nodes: &[crate::outlines::Node],
            dir: &Path,
            first: bool,
            found: &mut std::collections::BTreeMap<std::path::PathBuf, Option<String>>,
        ) {
            for (i, node) in nodes.iter().enumerate() {
                if !node.children.is_empty() && !node.target.contains("://") {
                    found.insert(
                        crate::site::normal(
                            &dir.join(crate::md::link_path(crate::md::target_path(&node.target))),
                        ),
                        // A site's first tab, and a tab with a short label, keep
                        // a name of their own.
                        (!(first && (i == 0 || node.target.ends_with(" \"tab\""))))
                            .then(|| node.label.clone()),
                    );
                }
                sections(&node.children, dir, false, found);
            }
        }
        if let Ok(nodes) = crate::outlines::expanded(tree, &file) {
            sections(&nodes, &dir, true, &mut found);
        }
    }
    found
}

fn hooks(tree: &Tree) -> Vec<Finding> {
    let sections = sections(tree);
    let mut found = Vec::new();
    for (file, text) in &tree.files {
        if !is_page(tree, file) {
            continue;
        }
        let hooked = hook(&blocks(text)).is_some();
        let listed = tree.large.unhooked.contains(&tree.shown(file));
        let section = sections.get(file);
        // A hooked section's title is its plain name, the tree's.
        if let (Some(Some(label)), true) = (section, hooked) {
            let title = blocks(text).into_iter().find_map(|b| match b {
                Block::Heading(_, h) => Some(h),
                _ => None,
            });
            if title.as_deref() != Some(label.as_str()) {
                found.push(fail(format!(
                    "{}: titled {title:?}, but ## Pages calls it {label:?}",
                    tree.shown(file)
                )));
            }
        }
        match (section.is_some() && !listed, hooked) {
            (true, false) => found.push(fail(format!(
                "{}: a section, with no hook under its title",
                tree.shown(file)
            ))),
            (false, true) => found.push(fail(format!(
                "{}: a hook, but no pages under it in ## Pages",
                tree.shown(file)
            ))),
            _ => {}
        }
        if listed && (hooked || section.is_none()) {
            found.push(fail(format!(
                "{}: take it out of large-files.toml's unhooked",
                tree.shown(file)
            )));
        }
    }
    for listed in &tree.large.unhooked {
        if !tree.files.contains_key(&tree.root.join(listed)) {
            found.push(fail(format!("{listed}: unhooked, but gone")));
        }
    }
    found
}

fn hook_length(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) {
        return Vec::new();
    }
    let most = tree.limits.of("hook-length");
    let blocks = blocks(text);
    let Some(hook) = hook(&blocks) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let n = characters(hook);
    if n > most {
        found.push(fail(format!(
            "{}: {}: {n} characters, over {most}",
            tree.shown(file),
            named(hook)
        )));
    }
    // It opens with its keyword, never a pronoun or an article.
    let first = hook
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase();
    if [
        "it", "its", "it's", "this", "that", "these", "they", "a", "an", "the",
    ]
    .contains(&first.as_str())
    {
        found.push(fail(format!(
            "{}: {} opens with {first:?}, not its keyword",
            tree.shown(file),
            named(hook)
        )));
    }
    // It reads as one line, never a label and its gloss.
    if hook.contains(':') {
        found.push(fail(format!(
            "{}: {} holds a colon",
            tree.shown(file),
            named(hook)
        )));
    }
    found
}

/// The exact opening `sh` block, either before section headings or in a
/// first task explicitly marked `<!-- quickstart -->`.
pub fn quickstart_block(text: &str) -> Option<&str> {
    let mut offset = 0;
    let mut fence: Option<(&str, usize)> = None;
    let mut task = false;
    let mut entered_task = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(info) = trimmed.strip_prefix("```") {
            match fence.take() {
                Some(("sh", start)) => return Some(&text[start..offset]),
                Some(_) => {}
                None => fence = Some((info.trim(), offset + line.len())),
            }
        } else if fence.is_none() {
            if trimmed.trim() == "<!-- quickstart -->" {
                task = true;
            } else if trimmed.starts_with("## ") {
                if !task || entered_task {
                    return None;
                }
                entered_task = true;
            }
        }
        offset += line.len();
    }
    None
}

/// The runnable quickstart, including explicitly marked steps and example files.
/// Ordinary pages retain their exact opening shell block. Staged examples keep
/// each terminal short while their fixture executes the complete documented route.
pub fn quickstart_script(text: &str) -> Option<String> {
    let Some((_, steps)) = text.split_once("<!-- quickstart:steps -->") else {
        return quickstart_block(text).map(String::from);
    };
    let (steps, _) = steps.split_once("<!-- quickstart:end -->")?;
    let mut script = String::new();
    let mut file = None;
    let mut fence = None;
    let mut offset = 0;
    let mut commands = false;
    for line in steps.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(info) = trimmed.strip_prefix("```") {
            if let Some((language, start)) = fence.take() {
                let body = &steps[start..offset];
                match language {
                    "sh" => {
                        script.push_str(body);
                        commands = true;
                    }
                    "toml" => {
                        let name: &str = file.take()?;
                        if name.is_empty()
                            || name.contains("..")
                            || !name
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
                        {
                            return None;
                        }
                        let mut delimiter = String::from("LINK_DOC_FILE");
                        while body.lines().any(|line| line == delimiter) {
                            delimiter.push('_');
                        }
                        script.push_str(&format!(
                            "cat > '{name}' <<'{delimiter}'\n{body}{delimiter}\n"
                        ));
                    }
                    _ => {}
                }
            } else {
                fence = Some((info.trim(), offset + line.len()));
            }
        } else if fence.is_none()
            && let Some(name) = trimmed
                .strip_prefix("<!-- quickstart-file: ")
                .and_then(|name| name.strip_suffix(" -->"))
        {
            file = Some(name);
        }
        offset += line.len();
    }
    (commands && fence.is_none()).then_some(script)
}

/// The `lnk` commands from the opening shell block, as whitespace words.
/// Execution uses [`quickstart_block`] so shell syntax is never rebuilt.
pub fn quickstart_commands(text: &str) -> Option<Vec<Vec<String>>> {
    let commands: Vec<Vec<String>> = quickstart_block(text)?
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("lnk "))
        .map(|line| {
            line.split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .skip(1)
                .map(String::from)
                .collect()
        })
        .collect();
    (!commands.is_empty()).then_some(commands)
}

/// The opening quickstart sets an environment variable, `NAME=value` or
/// `export NAME=value`, instead of letting the command ask for one.
fn quickstart_env(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) {
        return Vec::new();
    }
    let Some(block) = quickstart_block(text) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for line in block.lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("export ").unwrap_or(t);
        let name = t.split('=').next().unwrap_or("");
        let is_name = !name.is_empty()
            && t.len() > name.len()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_name {
            found.push(fail(format!(
                "{}: its quickstart sets {name}",
                tree.shown(file)
            )));
        }
    }
    found
}

/// The flags of an `lnk` command, and the variable, that name where Link
/// writes.
const OUTPUTS: [&str; 4] = ["--out", "--to", "--dir", "LNK_INSTALL_DIR"];

/// Whether `path`, a home path, is outside Link's own folders.
fn outside_link(path: &str) -> bool {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("$HOME/"))
        .or_else(|| path.strip_prefix("${HOME}/"));
    rest.is_some_and(|rest| {
        !(rest == "Link"
            || rest == ".lnk"
            || rest.starts_with("Link/")
            || rest.starts_with(".lnk/"))
    })
}

/// Every `sh` block's lines, of a page.
fn sh_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut fence: Option<bool> = None;
    for line in text.lines() {
        if let Some(info) = line.trim_start().strip_prefix("```") {
            fence = match fence {
                Some(_) => None,
                None => Some(info.trim() == "sh"),
            };
            continue;
        }
        if fence == Some(true) {
            lines.push(line);
        }
    }
    lines
}

/// An `lnk` command in a page's `sh` block never writes outside `~/Link`
/// and `~/.lnk`: its output flags and `LNK_INSTALL_DIR` name a folder there.
fn output_paths(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, file) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for line in sh_lines(text) {
        if !line
            .split_whitespace()
            .any(|w| w == "lnk" || w.ends_with("/lnk") || w.starts_with("lnk-"))
        {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let value = OUTPUTS.iter().find_map(|flag| {
                word.strip_prefix(&format!("{flag}="))
                    .or_else(|| (word == flag).then(|| words.get(i + 1).copied()).flatten())
            });
            if let Some(value) = value
                && outside_link(value)
            {
                found.push(fail(format!(
                    "{}: {} writes to {value}",
                    tree.shown(file),
                    word.split('=').next().unwrap_or(word)
                )));
            }
        }
    }
    found
}

/// Each module a user acts on, a folder with a `module.toml` whose
/// `[documentation]` doesn't say `quickstart = false`, has a quickstart
/// on its own first page.
fn quickstart(tree: &Tree) -> Vec<Finding> {
    let modules: Vec<(&std::path::Path, toml::Table)> = tree
        .files
        .iter()
        .filter(|(path, _)| path.ends_with("module.toml") && path.starts_with(&tree.root))
        .filter_map(|(path, text)| Some((path.parent()?, toml::from_str(text).ok()?)))
        .collect();
    let mut found = Vec::new();
    for (dir, module) in &modules {
        let shown = tree.shown(dir);
        let quickstart = module
            .get("documentation")
            .and_then(|d| d.get("quickstart"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        // A contract crate has no program of its own to start.
        let package = module
            .get("module")
            .and_then(|m| m.get("kind"))
            .and_then(toml::Value::as_str)
            == Some("package");
        if shown.contains("fixtures/") || !quickstart || package {
            continue;
        }
        // Its first page: the README nearest the top of its docs/.
        let docs = dir.join("docs");
        let first = tree
            .files
            .keys()
            .filter(|f| f.starts_with(&docs) && f.ends_with("README.md"))
            .min_by_key(|f| f.components().count());
        let Some(page) = first else {
            continue;
        };
        if !shaped(tree, page) {
            continue;
        }
        let page_shown = tree.shown(page);
        if quickstart_commands(&tree.files[page]).is_none() {
            found.push(fail(format!("{page_shown}: no quickstart")));
        }
    }
    found
}

/// Whether a page holds a Troubleshooting section, which is a page of its
/// own.
pub fn has_troubleshooting(blocks: &[Block]) -> bool {
    blocks
        .iter()
        .skip(1)
        .any(|b| matches!(b, Block::Heading(2, h) if h == "Troubleshooting"))
}

fn troubleshooting(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !shaped(tree, file) || file.ends_with("troubleshooting.md") {
        return Vec::new();
    }
    match has_troubleshooting(&blocks(text)) {
        true => vec![fail(format!(
            "{}: a Troubleshooting section",
            tree.shown(file)
        ))],
        false => Vec::new(),
    }
}

#[cfg(test)]
mod quickstart_tests {
    use super::{quickstart_block, quickstart_script};

    #[test]
    fn raw_quickstarts_preserve_quoting_and_heredocs_and_stop_at_sections() {
        let body =
            "cat <<'EOF'\n## content, not a section\nEOF\nprintf '%s\\n' 'kept # literally'\n";
        let page = format!(
            "# Example\n\nAn introduction.\n\n```sh\n{body}```\n\n## Later\n\n```sh\nexit 99\n```\n"
        );
        assert_eq!(quickstart_block(&page), Some(body));
        assert_eq!(
            quickstart_block("# Example\n\n## Later\n```sh\nexit 99\n```"),
            None
        );
        assert_eq!(quickstart_block("# Example\n```sh\nunfinished"), None);
    }

    #[test]
    fn a_marked_first_task_keeps_its_exact_example_and_does_not_borrow_later_tasks() {
        let body = "lnk agent start\n";
        let page = format!(
            "# Agents\n\nStart here.\n\n<!-- quickstart -->\n\n## Start\n\nChoose a model.\n\n```sh\n{body}```\n\n## Chat\n\n```sh\nlnk agent chat start\n```\n"
        );
        assert_eq!(quickstart_block(&page), Some(body));
        let missing = "# Agents\n\n<!-- quickstart -->\n\n## Start\n\nNo example.\n\n## Chat\n\n```sh\nlnk agent chat start\n```\n";
        assert_eq!(quickstart_block(missing), None);
    }

    #[test]
    fn staged_quickstarts_execute_the_documented_commands_and_file_contents_in_order() {
        let page = "<!-- quickstart:steps -->\n```sh\nmkdir example\ncd example\n```\n\nSave the file.\n<!-- quickstart-file: env.toml -->\n```toml\nname = 'example'\n```\n\n```sh\nlnk env check --spec env.toml\n```\n<!-- quickstart:end -->\n```sh\nexit 99\n```\n";
        assert_eq!(
            quickstart_script(page).unwrap(),
            "mkdir example\ncd example\ncat > 'env.toml' <<'LINK_DOC_FILE'\nname = 'example'\nLINK_DOC_FILE\nlnk env check --spec env.toml\n"
        );
        assert!(quickstart_script(&page.replace("env.toml -->", "../env.toml -->")).is_none());
        assert!(quickstart_script(&page.replace("<!-- quickstart:end -->", "")).is_none());
    }
}
