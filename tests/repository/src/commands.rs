//! One verb per meaning: a command's last word is `verbs.toml`'s lifecycle
//! pairs, `list`, `show`, `check`, `run`, or an `[[allowed]]` word with its
//! reason; no word is `[banned]` and no flag is a `[banned_flags]` one or
//! spells a banned word; and no group pairs lifecycle verbs across (`create` with
//! `remove`, `add` with `delete`). The commands are each module's declared
//! ones (`actions.toml`), hidden ones included. The parser inventory
//! requires every released program to declare them. Commands still to be
//! renamed are `[held]`, which may shrink, never grow. Each workspace runs
//! it on its own root.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The rule, read from `verbs.toml`.
#[derive(serde::Deserialize)]
pub struct Verbs {
    /// The lifecycle pairs: each word means the opposite of its partner.
    pub pairs: Vec<[String; 2]>,
    /// Verbs that never go together: `create` with `remove`, `add` with
    /// `delete`.
    pub crossed: Vec<[String; 2]>,
    /// Verbs with no partner.
    pub alone: Vec<String>,
    /// The other words a command may end in, each group with its reason.
    #[serde(default)]
    pub allowed: Vec<Allowed>,
    /// Banned words, with what to say instead.
    pub banned: BTreeMap<String, String>,
    /// Banned flag names, with what to say instead.
    pub banned_flags: BTreeMap<String, String>,
    /// Hits still to be renamed, by the file that declares them.
    pub held: BTreeMap<String, BTreeSet<String>>,
}

/// What a workspace adds to the rule: the words and held commands of its own
/// modules, which another workspace's rule doesn't name.
#[derive(serde::Deserialize)]
pub struct Extra {
    #[serde(default)]
    pub allowed: Vec<Allowed>,
    #[serde(default)]
    pub held: BTreeMap<String, BTreeSet<String>>,
}

impl Verbs {
    /// The rule with a workspace's own words and held commands put in, its
    /// own `held` replacing this one's.
    #[must_use]
    pub fn extended(mut self, extra: Extra) -> Self {
        self.allowed.extend(extra.allowed);
        self.held = extra.held;
        self
    }

    /// Whether a command may end in `word`.
    fn allows(&self, word: &str) -> bool {
        self.pairs.iter().flatten().any(|w| w == word)
            || self.alone.iter().any(|w| w == word)
            || self.allowed.iter().any(|group| group.words.contains(word))
    }
}

/// Words commands may end in besides the lifecycle's, and why.
#[derive(serde::Deserialize)]
pub struct Allowed {
    pub reason: String,
    pub words: BTreeSet<String>,
}

/// One declared command: its words and its flags' long names.
struct Command {
    words: Vec<String>,
    flags: Vec<String>,
    /// It holds commands of its own: its last word is a noun.
    group: bool,
}

fn declared(file: &str, text: &str) -> Vec<Command> {
    let table: toml::Table = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut commands = if table.contains_key("schema") {
        version_one(file, &table)
    } else {
        unversioned(table)
    };
    let names: Vec<Vec<String>> = commands.iter().map(|c| c.words.clone()).collect();
    for command in &mut commands {
        command.group = names
            .iter()
            .any(|other| other.len() > command.words.len() && other.starts_with(&command.words));
    }
    commands
}

/// A version 1 file's commands: each action on the command line as typed
/// after `lnk`, its flags the arguments it declares that aren't positional.
fn version_one(file: &str, table: &toml::Table) -> Vec<Command> {
    let Some(actions) = table.get("actions").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    actions
        .iter()
        // An action off the command line (a tool's call alone) is no
        // command a person types, so its words take no verb.
        .filter(|(_, action)| {
            action
                .get("surfaces")
                .and_then(toml::Value::as_array)
                .is_none_or(|surfaces| surfaces.iter().any(|s| s.as_str() == Some("cli")))
        })
        .map(|(id, action)| {
            let id: link_actions::ActionId = id.parse().unwrap_or_else(|e| panic!("{file}: {e}"));
            let flags = action
                .get("args")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten()
                .filter(|(_, arg)| {
                    arg.get("x-link-cli")
                        .and_then(|cli| cli.get("position"))
                        .is_none()
                })
                .map(|(name, _)| name.clone())
                .collect();
            Command {
                words: id.typed().into_iter().map(str::to_string).collect(),
                flags,
                group: false,
            }
        })
        .collect()
}

/// An unversioned file: each table a command by its words.
fn unversioned(table: toml::Table) -> Vec<Command> {
    let table = serde_json::to_value(table).unwrap();
    table
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, value)| name.as_str() != "root" && value.is_object())
        .map(|(name, value)| {
            let args = value.get("args");
            let flags = args
                .and_then(|args| args.as_object())
                .into_iter()
                .flat_map(|args| args.values())
                .filter_map(|arg| arg.get("long")?.as_str().map(str::to_string))
                .collect();
            Command {
                words: name.split(' ').map(str::to_string).collect(),
                flags,
                group: false,
            }
        })
        .collect()
}

/// The hits of one module file's commands, each a short line naming the
/// command and the word it breaks.
fn hits(verbs: &Verbs, commands: &[Command]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut by_parent: BTreeMap<&[String], BTreeSet<&str>> = BTreeMap::new();
    for command in commands {
        let name = command.words.join(" ");
        for word in command.words.iter().flat_map(|word| word.split('-')) {
            if let Some(instead) = verbs.banned.get(word) {
                found.insert(format!("{name}: `{word}` is no verb, say {instead}"));
            }
        }
        for flag in &command.flags {
            if let Some(instead) = verbs.banned_flags.get(flag) {
                found.insert(format!("{name}: --{flag} is no flag, say {instead}"));
            }
            for word in flag.split('-') {
                if let Some(instead) = verbs.banned.get(word) {
                    found.insert(format!("{name}: --{flag} says `{word}`, say {instead}"));
                }
            }
        }
        if let Some(last) = command.words.last().filter(|_| !command.group)
            && !verbs.allows(last)
            && !last.split('-').any(|word| verbs.banned.contains_key(word))
        {
            found.insert(format!("{name}: `{last}` is on no list of verbs.toml"));
        }
        if let Some((last, parent)) = command.words.split_last() {
            by_parent.entry(parent).or_default().insert(last);
        }
    }
    for (parent, words) in by_parent {
        for [a, b] in &verbs.crossed {
            // Both present is fine when each has its own partner too
            // (`team create | delete` and `team add | remove`).
            let partner = verbs
                .pairs
                .iter()
                .find_map(|[x, y]| (x == a).then_some(y).or((y == a).then_some(x)));
            let has = |word: &str| words.contains(word);
            if has(a) && has(b) && !partner.is_some_and(|p| has(p)) {
                found.insert(format!("{} pairs {a} with {b}", parent.join(" ")));
            }
        }
    }
    found
}

/// The problems of the commands under `root`, by the rule at `list`, none
/// when they pass.
pub fn check(root: &Path, list: &Path) -> Vec<String> {
    problems(root, &rule(list))
}

/// The rule at `list`, a `verbs.toml`.
pub fn rule(list: &Path) -> Verbs {
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()))
}

/// The problems of the commands under `root`, by `verbs`.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
pub fn problems(root: &Path, verbs: &Verbs) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    let listing = String::from_utf8_lossy(&listed.stdout).into_owned();
    let files: Vec<&str> = listing.split('\0').collect();
    for file in files {
        if file.rsplit('/').next() != Some("actions.toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        let commands = declared(file, &text);
        let found = hits(verbs, &commands);
        let held = verbs.held.get(file).cloned().unwrap_or_default();
        seen.insert(file.to_string());
        for hit in found.difference(&held) {
            problems.push(format!("{file}: {hit}"));
        }
        for hit in held.difference(&found) {
            problems.push(format!(
                "{file}: `{hit}` is fixed: take it out of verbs.toml's held"
            ));
        }
    }
    for file in verbs.held.keys().filter(|file| !seen.contains(*file)) {
        problems.push(format!(
            "{file} declares no commands: take it out of verbs.toml's held"
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Verbs {
        toml::from_str(include_str!("../verbs.toml")).unwrap()
    }

    fn commands(names: &[&str]) -> Vec<Command> {
        names
            .iter()
            .map(|name| Command {
                words: name.split(' ').map(str::to_string).collect(),
                flags: vec![],
                group: false,
            })
            .collect()
    }

    #[test]
    fn a_banned_word_is_a_hit_even_inside_a_hyphenated_one() {
        let found = hits(&rule(), &commands(&["bucket forget", "vm cancel-request"]));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn the_pairs_never_cross() {
        let verbs = rule();
        for (names, hit) in [
            (["box create", "box remove"], true),
            (["box add", "box delete"], true),
            (["box create", "box delete"], false),
            (["team create", "team delete"], false),
        ] {
            assert_eq!(
                !hits(&verbs, &commands(&names)).is_empty(),
                hit,
                "{names:?}"
            );
        }
        let both = commands(&["t create", "t delete", "t add", "t remove"]);
        assert!(hits(&verbs, &both).is_empty());
    }

    #[test]
    fn a_last_word_on_no_list_is_a_hit() {
        let verbs = rule();
        assert!(hits(&verbs, &commands(&["box create", "box list", "box run"])).is_empty());
        let found = hits(&verbs, &commands(&["box frobnicate"]));
        assert_eq!(found.len(), 1, "{found:?}");
        let mut group = commands(&["box"]);
        group.extend(commands(&["box frobnicate"]));
        group[0].group = true;
        assert_eq!(hits(&verbs, &group).len(), 1);
    }

    #[test]
    fn a_flag_that_spells_a_banned_verb_is_a_hit() {
        let mut one = commands(&["box create"]);
        one[0].flags = vec!["attach-disk".into()];
        assert_eq!(hits(&rule(), &one).len(), 1);
    }

    #[test]
    fn a_banned_flag_is_a_hit() {
        let mut one = commands(&["box clean"]);
        one[0].flags = vec!["dry-run".into()];
        assert_eq!(hits(&rule(), &one).len(), 1);
    }
}
