//! One meaning per flag: every long flag declared in a module's
//! `actions.toml` has one type, and one set of values
//! where it has a fixed set, across every module, hidden commands
//! included. The counted shared flags of `shared.toml` also have one meaning, which
//! each declaration's help says in that entry's words. Flags still
//! meaning two things are `[held]`, which may shrink, never grow. The
//! workspace runs it on its own root.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The rule: the counted shared flags' meanings, from `link-component`'s
/// `shared.toml`, and the flags `flags.toml` holds.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flags {
    /// The flags with one meaning, by long name, from `shared.toml`.
    #[serde(skip)]
    pub flag: BTreeMap<String, Meaning>,
    /// Flags whose name still means two things, with who owns the rename.
    #[serde(default)]
    pub held: BTreeMap<String, String>,
}

/// What a workspace adds to the rule: the flags its own modules hold,
/// which the other workspace's rule doesn't name.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    #[serde(default)]
    pub held: BTreeMap<String, String>,
}

impl Flags {
    /// The rule with a workspace's own held flags put in.
    #[must_use]
    pub fn extended(mut self, extra: Extra) -> Self {
        self.held.extend(extra.held);
        self
    }
}

/// What one flag means, wherever it's declared.
pub struct Meaning {
    pub meaning: String,
    /// Words every declaration's help holds.
    pub says: String,
    /// Its type, `string`, `integer` or `boolean`.
    pub kind: String,
}

/// Each counted shared flag's meaning, as `shared.toml` declares it.
fn meanings() -> BTreeMap<String, Meaning> {
    let mut out = BTreeMap::new();
    for ty in link_component::shared::types()
        .values()
        .filter(|t| t.counted)
    {
        for (long, flag) in &ty.flags {
            let kind = match flag.r#type {
                link_component::ValueType::String => "string",
                link_component::ValueType::Integer => "integer",
                link_component::ValueType::Boolean => "boolean",
            };
            out.insert(
                long.clone(),
                Meaning {
                    meaning: flag.help.clone(),
                    says: flag.says.clone().unwrap_or_default(),
                    kind: kind.to_string(),
                },
            );
        }
    }
    out
}

/// One declared flag: where, its type, its fixed values and its help.
#[derive(Debug, Clone)]
pub struct Declared {
    pub file: String,
    pub command: String,
    pub long: String,
    pub kind: String,
    pub values: Vec<String>,
    pub help: String,
}

// The help of a composed flag the command refuses begins this way, and
// owes no meaning.
use link_component::shared::NOT_TAKEN;

/// The long flags an `actions.toml` declares.
#[must_use]
pub fn declared(file: &str, text: &str) -> Vec<Declared> {
    let table: toml::Table = toml::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
    let table = serde_json::to_value(table).unwrap();
    let mut found = Vec::new();
    for (command, value) in table.as_object().into_iter().flatten() {
        let args = value.get("args");
        for arg in args
            .and_then(|a| a.as_object())
            .into_iter()
            .flat_map(|a| a.values())
        {
            let Some(long) = arg.get("long").and_then(|l| l.as_str()) else {
                continue;
            };
            let kind = arg.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let values = if kind == "boolean" {
                Vec::new()
            } else {
                arg.get("values")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                    .collect()
            };
            found.push(Declared {
                file: file.to_string(),
                command: command.clone(),
                long: long.to_string(),
                kind: kind.to_string(),
                values,
                help: arg
                    .get("help")
                    .and_then(|h| h.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    found
}

/// The problems of `flags` by `rule`, none when they pass.
#[must_use]
pub fn hits(rule: &Flags, flags: &[Declared]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut by_long: BTreeMap<&str, Vec<&Declared>> = BTreeMap::new();
    for flag in flags {
        by_long.entry(&flag.long).or_default().push(flag);
    }
    let at = |d: &Declared| format!("{}: {} --{}", d.file, d.command, d.long);
    for (long, declared) in &by_long {
        if let Some(meaning) = rule.flag.get(*long) {
            for d in declared {
                if d.kind != meaning.kind {
                    problems.push(format!(
                        "{} is {}, but --{long} is {}: {}",
                        at(d),
                        d.kind,
                        meaning.kind,
                        meaning.meaning
                    ));
                } else if !d.help.starts_with(NOT_TAKEN) && !d.help.contains(&meaning.says) {
                    problems.push(format!(
                        "{}: its help doesn't say {:?}, and --{long} is {}",
                        at(d),
                        meaning.says,
                        meaning.meaning
                    ));
                }
            }
        }
        let kinds: BTreeSet<&str> = declared.iter().map(|d| d.kind.as_str()).collect();
        let sets: BTreeSet<&[String]> = declared
            .iter()
            .filter(|d| d.kind != "boolean")
            .map(|d| d.values.as_slice())
            .collect();
        let twice = kinds.len() > 1 || sets.len() > 1;
        match (twice, rule.held.contains_key(*long)) {
            (true, false) => {
                let said: Vec<String> = declared
                    .iter()
                    .map(|d| {
                        let values = if d.values.is_empty() {
                            String::new()
                        } else {
                            format!(" of {}", d.values.join("|"))
                        };
                        format!("{} ({}{values})", at(d), d.kind)
                    })
                    .collect();
                problems.push(format!(
                    "--{long} means two things, one type and one set of values each: {}",
                    said.join(", ")
                ));
            }
            (false, true) => problems.push(format!(
                "--{long} means one thing now: take it out of flags.toml's held"
            )),
            _ => {}
        }
    }
    for long in rule
        .held
        .keys()
        .filter(|l| !by_long.contains_key(l.as_str()))
    {
        problems.push(format!(
            "--{long} is declared nowhere: take it out of flags.toml's held"
        ));
    }
    for long in rule.flag.keys().filter(|l| rule.held.contains_key(*l)) {
        problems.push(format!(
            "--{long} has one meaning in flags.toml and is held there too"
        ));
    }
    problems
}

/// The rule at `list`, a `flags.toml`.
#[must_use]
pub fn rule(list: &Path) -> Flags {
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let mut rule: Flags =
        toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    rule.flag = meanings();
    rule
}

/// The flags declared under `root`'s tracked `actions.toml` files.
#[must_use]
#[expect(clippy::disallowed_methods, reason = "git lists the tracked files")]
pub fn all(root: &Path) -> Vec<Declared> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z", "*actions.toml"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(listed.status.success(), "git ls-files failed");
    let listing = String::from_utf8_lossy(&listed.stdout).into_owned();
    listing
        .split('\0')
        .filter(|f| f.ends_with("/actions.toml"))
        .filter_map(|f| Some(declared(f, &std::fs::read_to_string(root.join(f)).ok()?)))
        .flatten()
        .collect()
}

/// The problems of the flags under `root` by the rule at `list`, none
/// when they pass.
#[must_use]
pub fn check(root: &Path, list: &Path) -> Vec<String> {
    hits(&rule(list), &all(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule as `flags.toml` declares it, holding `held` alone, each
    /// with a reason of its own.
    fn rule_holding(held: &[&str]) -> Flags {
        let mut rule: Flags = toml::from_str(include_str!("../flags.toml")).unwrap();
        rule.held = held
            .iter()
            .map(|long| (long.to_string(), "held for this test".to_string()))
            .collect();
        rule.flag = meanings();
        rule
    }

    fn rule() -> Flags {
        rule_holding(&[])
    }

    fn flag(command: &str, long: &str, kind: &str, values: &[&str], help: &str) -> Declared {
        Declared {
            file: "modules/x/actions.toml".into(),
            command: command.into(),
            long: long.into(),
            kind: kind.into(),
            values: values.iter().map(|v| (*v).to_string()).collect(),
            help: help.into(),
        }
    }

    #[test]
    fn a_flag_with_a_meaning_says_it_in_its_help() {
        let rule = rule();
        let good = flag(
            "box create",
            "cloud",
            "string",
            &[],
            "A cloud account (`lnk cloud list`)",
        );
        assert!(hits(&rule, &[good]).is_empty());
        let bad = flag(
            "storage push",
            "cloud",
            "string",
            &[],
            "Only these clouds (`lnk storage list`)",
        );
        assert_eq!(hits(&rule, &[bad]).len(), 1);
        let refused = flag(
            "env reset",
            "cloud",
            "string",
            &[],
            "Not taken: it acts on a box",
        );
        assert!(hits(&rule, &[refused]).is_empty());
    }

    #[test]
    fn a_flag_with_a_meaning_has_its_type() {
        let switch = flag(
            "agent send",
            "owner",
            "boolean",
            &[],
            "Send it to the owner",
        );
        assert_eq!(hits(&rule(), &[switch]).len(), 1);
    }

    #[test]
    fn a_flag_has_one_type_and_one_set_of_values() {
        let rule = rule();
        let one = [
            flag("box create", "arch", "string", &["x86_64", "arm64"], ""),
            flag("box spec", "arch", "string", &["x86_64", "arm64"], ""),
        ];
        assert!(hits(&rule, &one).is_empty());
        let values = [
            flag("box create", "arch", "string", &["x86_64", "arm64"], ""),
            flag("vm create", "arch", "string", &[], ""),
        ];
        assert_eq!(hits(&rule, &values).len(), 1);
        let kinds = [
            flag("a b", "quiet", "boolean", &[], ""),
            flag("c d", "quiet", "string", &[], ""),
        ];
        assert_eq!(hits(&rule, &kinds).len(), 1);
    }

    #[test]
    fn a_held_flag_leaves_once_it_means_one_thing() {
        let rule = rule_holding(&["keys"]);
        let mixed = [
            flag("sandbox spec", "keys", "boolean", &[], ""),
            flag("model bridge", "keys", "string", &[], ""),
        ];
        assert!(hits(&rule, &mixed).is_empty());
        let fixed = [flag("model bridge", "keys", "string", &[], "")];
        let found = hits(&rule, &fixed);
        assert!(
            found.iter().any(|p| p.contains("means one thing now")),
            "{found:?}"
        );
    }
}
