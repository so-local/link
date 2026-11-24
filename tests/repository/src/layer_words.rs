//! Layer vocabulary: a lower layer never names a higher layer's concepts.
//! Each group folder's `layer.toml` may declare `words`, the concepts its
//! layer introduces (`agent`, `team`, `harness` for the agents' layer),
//! and every file git tracks in a layer it is built on, directly or
//! through another, says none of them: not in an identifier, a file's
//! name, a TOML key, an error's text or its docs. A word is matched as a
//! whole part of a name, singular or plural, in any case and however the
//! name is joined (`for_agent`, `AgentAt`, `AGENT_KIND`, `agent.toml`).
//! What a lower layer says today is counted, by file and word, in its
//! workspace's `layer-words.toml`, which may shrink, never grow, as
//! `large-files.toml` does. Each workspace runs it on its own root.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

/// What the check reads of a group folder's `layer.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// The group folders it is built on.
    pub depends_on: Vec<String>,
    /// The concepts the layer introduces, each one lowercase word, singular.
    #[serde(default)]
    pub words: Vec<String>,
}

/// Each file's count of each word it may not say, by its path from the
/// workspace's root, then by the word.
pub type Found = BTreeMap<String, BTreeMap<String, usize>>;

/// A workspace's `layer-words.toml`: what each file said when the rule
/// came in.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    #[serde(default)]
    pub held: Found,
}

/// The name of each group folder's declaration.
pub const LAYER: &str = "layer.toml";

/// The parts of `text`'s names, lowercase: split at whatever isn't a
/// letter or a digit, between letters and digits, and where a name's
/// case turns (`AgentAt`, `HTTPAgent`).
pub fn parts(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for run in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        let chars: Vec<char> = run.chars().collect();
        let mut start = 0;
        for at in 1..chars.len() {
            let (before, here) = (chars[at - 1], chars[at]);
            let after = chars.get(at + 1).copied();
            let turns = (here.is_ascii_uppercase()
                && (before.is_ascii_lowercase()
                    || before.is_ascii_digit()
                    || (before.is_ascii_uppercase()
                        && after.is_some_and(|a| a.is_ascii_lowercase()))))
                || (here.is_ascii_digit() != before.is_ascii_digit());
            if turns {
                parts.push(
                    chars[start..at]
                        .iter()
                        .collect::<String>()
                        .to_ascii_lowercase(),
                );
                start = at;
            }
        }
        if start < chars.len() {
            parts.push(
                chars[start..]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        }
    }
    parts
}

/// The word `part` is a form of, of `words`: itself, or its plural.
fn form_of<'a>(part: &str, words: &'a BTreeSet<String>) -> Option<&'a String> {
    words.iter().find(|word| {
        part == word.as_str()
            || part
                .strip_prefix(word.as_str())
                .is_some_and(|rest| rest == "s" || rest == "es")
    })
}

/// How many times `text` says each of `words`.
pub fn count(text: &str, words: &BTreeSet<String>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for part in parts(text) {
        if let Some(word) = form_of(&part, words) {
            *counts.entry(word.clone()).or_insert(0) += 1;
        }
    }
    counts
}

/// The group folders among `layers`, each with the words it may not say:
/// those of every layer built on it, directly or through another. Its
/// problems are a word that isn't one lowercase word, and a word a layer
/// declares that a layer it's built on declares too.
pub fn forbidden(
    layers: &BTreeMap<String, Layer>,
) -> (BTreeMap<String, BTreeSet<String>>, Vec<String>) {
    let mut problems = Vec::new();
    let mut forbidden: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (group, layer) in layers {
        for word in &layer.words {
            if word.is_empty() || !word.bytes().all(|b| b.is_ascii_lowercase()) {
                problems.push(format!(
                    "{group}/{LAYER}: {word:?} isn't one lowercase word: declare each concept \
                     as its singular word"
                ));
            }
        }
        for below in below(layers, group) {
            for word in &layer.words {
                if layers.get(&below).is_some_and(|l| l.words.contains(word)) {
                    problems.push(format!(
                        "{group}/{LAYER}: {word:?} is declared by {below} too, which {group} is \
                         built on: declare it once, in the lowest layer that has the concept"
                    ));
                }
                forbidden
                    .entry(below.clone())
                    .or_default()
                    .insert(word.clone());
            }
        }
    }
    (forbidden, problems)
}

/// Every group folder `group` is built on, directly or through another.
fn below(layers: &BTreeMap<String, Layer>, group: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut next: Vec<String> = layers
        .get(group)
        .map(|l| l.depends_on.clone())
        .unwrap_or_default();
    while let Some(dep) = next.pop() {
        if dep != group && seen.insert(dep.clone()) {
            next.extend(
                layers
                    .get(&dep)
                    .map(|l| l.depends_on.clone())
                    .unwrap_or_default(),
            );
        }
    }
    seen
}

/// The files git tracks under `root`, by their paths from it.
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
pub fn tracked(root: &Path) -> Vec<String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

/// Each group folder's declaration among `files`, by the group folder:
/// a `layer.toml` directly in `packages/`, `core/`, `tests/` or a domain
/// of `modules/`, `interop/` or `services/`.
pub fn layers(root: &Path, files: &[String]) -> Result<BTreeMap<String, Layer>, String> {
    let mut layers = BTreeMap::new();
    for file in files {
        let Some(group) = file.strip_suffix(&format!("/{LAYER}")) else {
            continue;
        };
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        let layer: Layer = toml::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
        layers.insert(group.to_string(), layer);
    }
    Ok(layers)
}

/// What each file of `files` under `root` says that its layer may not.
pub fn found(
    root: &Path,
    files: &[String],
    forbidden: &BTreeMap<String, BTreeSet<String>>,
) -> Found {
    let mut found = Found::new();
    for file in files {
        let Some(words) = forbidden
            .iter()
            .filter(|(group, _)| file.starts_with(&format!("{group}/")))
            .max_by_key(|(group, _)| group.len())
            .map(|(_, words)| words)
        else {
            continue;
        };
        let mut counts = count(file, words);
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            for (word, n) in count(&text, words) {
                *counts.entry(word).or_insert(0) += n;
            }
        }
        if !counts.is_empty() {
            found.insert(file.clone(), counts);
        }
    }
    found
}

/// The problems of `found` against `held`, a `layer-words.toml`'s
/// `[held]`: none when every file is held at exactly what it says.
pub fn compare(found: &Found, held: &Found) -> Vec<String> {
    let mut problems = Vec::new();
    for (file, counts) in found {
        for (word, &n) in counts {
            match held.get(file).and_then(|h| h.get(word)) {
                None => problems.push(format!(
                    "{file}: says {word:?} {n} times, a concept of a layer built on it: move \
                     the part to that layer, or ask a question in this layer's own types; \
                     layer-words.toml takes no new file or word"
                )),
                Some(&was) if n > was => problems.push(format!(
                    "{file}: says {word:?} {n} times, up from {was}: say it in the layer that \
                     owns it"
                )),
                Some(&was) if n < was => problems.push(format!(
                    "{file}: says {word:?} {n} times, down from {was}: lower it in \
                     layer-words.toml"
                )),
                Some(_) => {}
            }
        }
    }
    for (file, counts) in held {
        for word in counts.keys() {
            if !found.get(file).is_some_and(|f| f.contains_key(word)) {
                problems.push(format!(
                    "{file} says {word:?} no more: take it out of layer-words.toml"
                ));
            }
        }
    }
    problems
}

/// What `root`'s files say that their layers may not, and the problems
/// of its declarations.
pub fn survey(root: &Path) -> (Found, Vec<String>) {
    let files = tracked(root);
    let layers = match layers(root, &files) {
        Ok(layers) => layers,
        Err(err) => return (Found::new(), vec![err]),
    };
    let (forbidden, problems) = forbidden(&layers);
    (found(root, &files, &forbidden), problems)
}

/// The list at `list`.
pub fn held(list: &Path) -> Found {
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    toml::from_str::<Held>(&text)
        .unwrap_or_else(|e| panic!("{}: {e}", list.display()))
        .held
}

/// The check on `root`, by the list at `list`.
pub fn check(root: &Path, list: &Path) -> Vec<String> {
    let (found, mut problems) = survey(root);
    problems.extend(compare(&found, &held(list)));
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn a_word_is_found_however_its_name_is_joined() {
        let agent = words(&["agent", "harness"]);
        for text in [
            "for_agent",
            "AgentAt",
            "AGENT_KIND",
            "agent.toml",
            "forgetAgent",
            "HTTPAgent",
            "an agent's own",
            "the agents",
            "--agent-id",
            "agent2",
        ] {
            assert_eq!(count(text, &agent).get("agent"), Some(&1), "{text}");
        }
        assert_eq!(count("harnesses", &agent).get("harness"), Some(&1));
        for text in ["agentic", "reagent", "management", "Agentless", "teamwork"] {
            assert!(count(text, &agent).is_empty(), "{text}");
        }
    }

    fn layer(depends_on: &[&str], words: &[&str]) -> Layer {
        Layer {
            depends_on: depends_on.iter().map(|d| d.to_string()).collect(),
            words: words.iter().map(|w| w.to_string()).collect(),
        }
    }

    #[test]
    fn every_layer_below_a_declaring_one_is_held_to_it() {
        let layers = BTreeMap::from([
            ("packages".to_string(), layer(&[], &[])),
            ("modules/security".to_string(), layer(&["packages"], &[])),
            ("modules/cloud".to_string(), layer(&["packages"], &[])),
            (
                "modules/agents".to_string(),
                layer(&["modules/security"], &["agent"]),
            ),
        ]);
        let (forbidden, problems) = forbidden(&layers);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(forbidden.get("packages"), Some(&words(&["agent"])));
        assert_eq!(forbidden.get("modules/security"), Some(&words(&["agent"])));
        assert_eq!(forbidden.get("modules/cloud"), None);
        assert_eq!(forbidden.get("modules/agents"), None);
    }

    #[test]
    fn a_word_is_declared_once_and_as_one_lowercase_word() {
        let layers = BTreeMap::from([
            ("packages".to_string(), layer(&[], &["agent"])),
            (
                "modules/agents".to_string(),
                layer(&["packages"], &["agent", "Team"]),
            ),
        ]);
        let (_, problems) = forbidden(&layers);
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn the_list_holds_each_file_at_exactly_what_it_says() {
        let found = Found::from([(
            "packages/a.rs".to_string(),
            BTreeMap::from([("agent".to_string(), 2)]),
        )]);
        assert!(compare(&found, &found).is_empty());
        assert_eq!(compare(&found, &Found::new()).len(), 1);
        let more = Found::from([(
            "packages/a.rs".to_string(),
            BTreeMap::from([("agent".to_string(), 3), ("team".to_string(), 1)]),
        )]);
        let problems = compare(&found, &more);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("down from 3")));
        assert!(problems.iter().any(|p| p.contains("no more")));
        let fewer = Found::from([(
            "packages/a.rs".to_string(),
            BTreeMap::from([("agent".to_string(), 1)]),
        )]);
        assert!(compare(&found, &fewer)[0].contains("up from 1"));
    }
}
