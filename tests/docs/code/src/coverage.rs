//! What the docs explain of what the code declares: each command a
//! person runs is shown in the section its `docs` names, and each `LNK_*` variable
//! and state folder is named on a page, outside the generated
//! inventories that only list it. What either rule held when it came in
//! is in `large-files.toml`'s `[held]`, by `<actions.toml>#<name>`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use link_component::actions::{Action, Surface};
use link_docs_check::structure::held;
use link_docs_check::{Finding, Tree, fail};

/// One command a module declares, where, and whether a person runs it.
pub struct Declared {
    pub file: PathBuf,
    pub owner: PathBuf,
    pub path: String,
    pub action: Action,
    /// Its file's group aliases, each group's words as typed after `lnk`
    /// with the word typed in place of its last (`[aliases]`).
    pub aliases: Vec<(String, String)>,
}

impl Declared {
    /// Whether a person runs it, from a terminal or an agent's commands.
    pub fn offered(&self) -> bool {
        !self.action.hidden() && !self.spellings().is_empty() && self.path != "root"
    }

    /// Canonical spellings on the surfaces actually offered by this action.
    pub fn spellings(&self) -> Vec<String> {
        let mut spellings = Vec::new();
        if self.action.on(Surface::Cli) {
            let mut typed = vec![self.path.clone()];
            // Its group, typed bare, runs its default.
            if self.action.default
                && let Some((group, _)) = self.path.rsplit_once(' ')
            {
                typed.push(group.to_string());
            }
            // A group's alias is typed in its place, as its declaration says.
            let short: Vec<String> = typed
                .iter()
                .flat_map(|words| {
                    self.aliases.iter().filter_map(move |(group, alias)| {
                        let rest = match words.strip_prefix(group.as_str()) {
                            Some("") => "",
                            Some(rest) if rest.starts_with(' ') => rest,
                            _ => return None,
                        };
                        let parent = group.rsplit_once(' ').map(|(p, _)| format!("{p} "));
                        Some(format!("{}{alias}{rest}", parent.unwrap_or_default()))
                    })
                })
                .collect();
            spellings.extend(typed.into_iter().chain(short).map(|w| format!("lnk {w}")));
        }
        if self.action.on(Surface::Command) {
            spellings.push(format!(
                "/{}",
                link_component::actions::command_name(&self.path)
            ));
        }
        spellings
    }

    /// Whether authored text shows one of this action's real surface spellings.
    pub fn shown_in(&self, text: &str) -> bool {
        self.spellings()
            .iter()
            .any(|spelling| shows_spelling(text, spelling))
    }

    /// The package explaining the command, which can differ from its parser's.
    /// A tool driven by the agent command keeps its own documentation owner.
    pub fn documentation_owner(&self, tree: &Tree) -> PathBuf {
        self.action
            .docs
            .as_deref()
            .and_then(|docs| docs.split_once('#'))
            .and_then(|(wanted, _)| {
                tree.files
                    .keys()
                    .find(|file| address(file).as_deref() == Some(wanted))
            })
            .and_then(|file| package(tree, file))
            .unwrap_or_else(|| self.owner.clone())
    }
}

/// The nearest package containing a source or page, from the same tree snapshot.
pub(crate) fn package(tree: &Tree, file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(&tree.root))
        .find(|dir| tree.files.contains_key(&dir.join("Cargo.toml")))
        .map(Path::to_path_buf)
}

/// Every command of every `actions.toml` in the workspace, a test's
/// fixtures apart: one that doesn't parse fails the check, naming it.
pub fn declared(tree: &Tree) -> Vec<Declared> {
    let mut out = Vec::new();
    for file in tree.files.keys() {
        if !file.ends_with("actions.toml")
            || !file.starts_with(&tree.root)
            || file.components().any(|c| c.as_os_str() == "fixtures")
        {
            continue;
        }
        let owner = file.parent().expect("a folder").to_path_buf();
        // Read from its folder, where its groups are its module's.
        let actions = link_component::actions::read_module(&owner, None)
            .unwrap_or_else(|e| panic!("{}: {e:#}", file.display()));
        let aliases: Vec<(String, String)> = link_actions::read_actions(&tree.files[file])
            .map(|declared| declared.aliases.into_iter().collect())
            .unwrap_or_default();
        for (path, action) in actions {
            out.push(Declared {
                file: file.clone(),
                owner: owner.clone(),
                path,
                action,
                aliases: aliases.clone(),
            });
        }
    }
    out
}

/// A page's text without what's generated: the docs writer's index and
/// the inventories, which list names without explaining them.
pub fn authored(text: &str) -> String {
    link_docs_check::navigation::explanations(text)
}

/// Whether `text` shows `lnk <command>` as a whole command, not the start
/// of a longer word (`lnk box show`, not `lnk box shows`).
pub fn shows(text: &str, command: &str) -> bool {
    shows_spelling(text, &format!("lnk {command}"))
}

fn shows_spelling(text: &str, spelling: &str) -> bool {
    text.match_indices(spelling).any(|(at, _)| {
        text[at + spelling.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '-' | '_' | ':')))
    })
}

/// `<actions.toml>#<name>`, as `[held]` lists it.
fn item(tree: &Tree, file: &Path, name: &str) -> String {
    format!("{}#{name}", tree.shown(file))
}

/// Each `[held]` entry of `rule` that no longer names something the rule
/// looks at fails, so the list never keeps a gone command or variable.
fn gone(tree: &Tree, rule: &str, seen: &BTreeSet<String>) -> Vec<Finding> {
    tree.large
        .held
        .get(rule)
        .into_iter()
        .flatten()
        .filter(|entry| !seen.contains(*entry))
        .map(|entry| fail(format!("[held] {rule}: {entry} is gone")))
        .collect()
}

/// A page's website address, from where it is under its owner's
/// `docs/`: `.../docs/services/boxes/README.md` is `/services/boxes`.
pub fn address(file: &Path) -> Option<String> {
    let text = file.to_str()?;
    let (_, page) = text.rsplit_once("/docs/")?;
    let page = page.strip_suffix(".md")?;
    let page = page.strip_suffix("README").unwrap_or(page);
    Some(format!("/{}", page.trim_end_matches('/')))
}

/// The section at `anchor` of a page's text: its heading's level and the
/// lines from it to the next heading of the same or a higher level.
pub fn section(text: &str, anchor: &str) -> Option<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut fence = false;
    let mut headings = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if line.starts_with("```") {
            fence = !fence;
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if !fence && (1..=6).contains(&level) && line[level..].starts_with(' ') {
            headings.push((at, level, link_docs_check::md::slug(&line[level..])));
        }
    }
    let index = headings.iter().position(|(_, _, slug)| slug == anchor)?;
    let (start, level, _) = headings[index];
    let end = headings[index + 1..]
        .iter()
        .find(|(_, other, _)| *other <= level)
        .map_or(lines.len(), |(at, _, _)| *at);
    Some((level, lines[start..end].join("\n")))
}

/// Every command a person runs names the section explaining it, `docs =
/// "<address>#<anchor>"`, and that section shows a canonical spelling on
/// one of its offered surfaces (`lnk <command>` or `/namespace:command`); a
/// primary command's section is a heading of its own, below its page's
/// title and no other primary command's of its module.
pub fn command_pages(tree: &Tree) -> Vec<Finding> {
    use link_component::actions::Role;
    let declared = declared(tree);
    let mut by_address: std::collections::BTreeMap<String, Vec<&PathBuf>> = Default::default();
    for file in tree.files.keys() {
        if let Some(address) = address(file) {
            by_address.entry(address).or_default().push(file);
        }
    }
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for command in declared.iter().filter(|c| c.offered()) {
        let entry = item(tree, &command.file, &command.path);
        let problem = match &command.action.docs {
            None => Some("names no docs section (`docs = \"<address>#<anchor>\"`)".to_string()),
            Some(docs) => {
                let (address, anchor) = docs.split_once('#').unwrap_or((docs, ""));
                let sections: Vec<(usize, String)> = by_address
                    .get(address)
                    .into_iter()
                    .flatten()
                    .filter_map(|file| section(&authored(&tree.files[*file]), anchor))
                    .collect();
                let shared = command.action.role == Role::Primary
                    && declared.iter().any(|other| {
                        other.file == command.file
                            && other.path != command.path
                            && other.action.role == Role::Primary
                            && other.action.docs.as_ref() == Some(docs)
                    });
                if !by_address.contains_key(address) {
                    Some(format!("names {docs}, but no page is at {address}"))
                } else if sections.is_empty() {
                    Some(format!(
                        "names {docs}, but its page has no heading #{anchor}"
                    ))
                } else if !sections.iter().any(|(_, text)| command.shown_in(text)) {
                    Some(format!(
                        "names {docs}, whose section doesn't show `{}`",
                        command.spellings().join("` or `")
                    ))
                } else if command.action.role == Role::Primary
                    && (sections.iter().all(|(level, _)| *level == 1) || shared)
                {
                    Some(format!(
                        "is primary, so {docs} is a heading of its own: below its page's title, \
                         and no other primary command's"
                    ))
                } else {
                    None
                }
            }
        };
        found.extend(held(
            tree,
            "command-pages",
            &entry,
            problem.is_none(),
            || format!("{entry}: {}", problem.unwrap_or_default()),
        ));
        seen.insert(entry);
    }
    found.extend(gone(tree, "command-pages", &seen));
    found
}

/// Every entry of an inventory the generated list holds is named, in
/// backticks, in a page's prose somewhere beyond that list: the list says
/// it exists, a page what it's for.
fn explained(tree: &Tree, rule: &str, kind: &str, keep: fn(&str) -> bool) -> Vec<Finding> {
    let items = match crate::inventories::catalog(tree, kind) {
        Ok(items) => items,
        Err(e) => return vec![fail(e)],
    };
    let pages: Vec<String> = tree
        .files
        .iter()
        .filter(|(path, _)| tree.is_doc(path))
        .map(|(_, text)| authored(text))
        .collect();
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for (name, sources) in items.iter().filter(|(name, _)| keep(name)) {
        let Some(source) = sources.iter().next() else {
            continue;
        };
        let entry = item(tree, source, name);
        let named = format!("`{name}");
        let ok = pages.iter().any(|t| t.contains(&named));
        found.extend(held(tree, rule, &entry, ok, || {
            format!("{entry}: no page names `{name}` beyond the {kind} inventory")
        }));
        seen.insert(entry);
    }
    found.extend(gone(tree, rule, &seen));
    found
}

/// Every `LNK_*` variable is named on a page beyond the environment
/// inventory; others' variables (`AWS_PROFILE`) are their owners' to explain.
pub fn variable_pages(tree: &Tree) -> Vec<Finding> {
    explained(tree, "variable-pages", "environment", |name| {
        name.starts_with("LNK_")
    })
}

/// Every folder of Link's state is named on a page beyond the state
/// inventory.
pub fn state_pages(tree: &Tree) -> Vec<Finding> {
    explained(tree, "state-pages", "state", |_| true)
}

/// The verbs a command may end in that name what it does to a thing:
/// `verbs.toml`'s lifecycle pairs and its `list`, `show`, `check` and `run`.
fn approved_verbs(tree: &Tree) -> Result<BTreeSet<String>, String> {
    let path = tree.root.join("tests/repository/verbs.toml");
    let text = tree
        .text(&path)
        .ok_or_else(|| format!("{} is missing", tree.shown(&path)))?;
    let verbs: link_repository_check::commands::Verbs =
        toml::from_str(text).map_err(|e| format!("{}: {e}", tree.shown(&path)))?;
    Ok(verbs
        .pairs
        .into_iter()
        .flatten()
        .chain(verbs.alone)
        .collect())
}

/// Whether a folder's name is a command's word: `image` for `box image`,
/// or its plural.
fn names(folder: &Path, word: &str) -> bool {
    folder
        .file_name()
        .is_some_and(|name| name == word || name.to_string_lossy() == format!("{word}s"))
}

/// A command group a person runs, `<module> <word>` with commands of its
/// own and a word that's no approved verb, has a folder of its own: a
/// folder named its word in its owner's docs, whose pages show it.
pub fn command_folders(tree: &Tree) -> Vec<Finding> {
    let verbs = match approved_verbs(tree) {
        Ok(verbs) => verbs,
        Err(e) => return vec![fail(e)],
    };
    let declared = declared(tree);
    let offered: Vec<&Declared> = declared.iter().filter(|c| c.offered()).collect();
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for group in &offered {
        let words: Vec<&str> = group.path.split(' ').collect();
        let [_, word] = words[..] else {
            continue;
        };
        let has_commands = offered
            .iter()
            .any(|c| c.file == group.file && c.path.starts_with(&format!("{} ", group.path)));
        if verbs.contains(word) || !has_commands {
            continue;
        }
        let owner = group.documentation_owner(tree);
        let ok = tree.files.iter().any(|(path, text)| {
            path.starts_with(&owner)
                && path.extension().is_some_and(|e| e == "md")
                && path.ancestors().take_while(|a| *a != owner).any(|a| {
                    names(a, word) && a.strip_prefix(&owner).is_ok_and(|r| r.starts_with("docs"))
                })
                && group.shown_in(&authored(text))
        });
        let entry = item(tree, &group.file, &group.path);
        found.extend(held(tree, "command-folders", &entry, ok, || {
            format!(
                "{entry}: no folder named {word} under {}'s docs shows `lnk {}`",
                tree.shown(&owner),
                group.path
            )
        }));
        seen.insert(entry);
    }
    found.extend(gone(tree, "command-folders", &seen));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_shown_whole() {
        assert!(shows("Run `lnk box show aws-1`.", "box show"));
        assert!(shows("lnk box show", "box show"));
        assert!(!shows("lnk box shows", "box show"));
        assert!(!shows("lnk box show-all", "box show"));
    }

    #[test]
    fn inventories_and_indexes_are_not_explanations() {
        let text = "# A\n\nUses `LNK_A`.\n\n<!-- inventory:environment:start -->\n\n- `LNK_B`\n\n<!-- inventory:environment:end -->\n";
        let kept = authored(text);
        assert!(kept.contains("LNK_A"));
        assert!(!kept.contains("LNK_B"));
    }
}
