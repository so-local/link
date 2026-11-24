//! Argument value and diminishing marginal return over source examples.

use std::collections::{BTreeMap, BTreeSet};

use link_component::{
    OptionDeclaration, ValueType,
    actions::{self, Action, Surface},
};
use serde::{Deserialize, Serialize};

use super::{Example, Feature, Tree, Weights};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArgumentWeights {
    pub command: u64,
    pub default_path: u64,
    pub positional: u64,
    pub flag: u64,
    pub required_bonus: u64,
    pub case_floor: u64,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub positionals: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    pub category: Category,
    pub points: u64,
    pub value_sensitive: bool,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Positional,
    Flag,
    Behavior,
    Security,
    Target,
    Presentation,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Curve {
    Log,
    SquareRoot,
    Linear,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionWeights {
    pub importance_curve: Curve,
    pub command_exponent: f64,
    pub decay: f64,
    pub preview: usize,
    pub positional_round_robin: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArgumentScore {
    pub member: String,
    pub command: String,
    pub argument: String,
    pub spelling: String,
    pub category: Category,
    pub required: bool,
    pub points: u64,
    pub value_sensitive: bool,
    pub reason: String,
}

#[derive(Serialize)]
pub struct CommandScore {
    pub member: String,
    pub command: String,
    pub points: u64,
    pub homepage: bool,
    pub mentioned_pages: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Unit {
    Command {
        command: String,
    },
    DefaultPath {
        command: String,
    },
    Argument {
        command: String,
        argument: String,
        value: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Gain {
    #[serde(flatten)]
    pub unit: Unit,
    pub points: u64,
}

#[derive(Serialize)]
pub struct Pick {
    pub order: usize,
    pub member: String,
    pub command: String,
    pub argv: Vec<String>,
    pub sources: Vec<Example>,
    pub feature_points: u64,
    pub command_points: u64,
    pub selected_before: usize,
    pub positional_turn: usize,
    pub positional_group: Vec<Unit>,
    pub new_units: Vec<Gain>,
    pub marginal_points: u64,
    pub priority: f64,
}

pub struct Selection {
    pub arguments: Vec<ArgumentScore>,
    pub commands: Vec<CommandScore>,
    pub queue: Vec<Pick>,
    pub problems: Vec<String>,
}

struct Command {
    member: String,
    name: String,
    args: BTreeMap<String, OptionDeclaration>,
    points: u64,
    mentioned_pages: BTreeSet<String>,
    homepage: bool,
}

struct Candidate {
    member: String,
    command: String,
    argv: Vec<String>,
    sources: Vec<Example>,
    units: Vec<Gain>,
    feature_points: u64,
    command_points: u64,
    source_order: usize,
    positionals: Vec<Unit>,
}

fn argument(
    member: &str,
    command: &str,
    name: &str,
    arg: &OptionDeclaration,
    weights: &ArgumentWeights,
) -> Result<ArgumentScore, String> {
    let rule = weights.rules.iter().find(|rule| {
        (arg.long
            .as_ref()
            .is_some_and(|flag| rule.flags.contains(flag))
            || (arg.position.is_some() && rule.positionals.iter().any(|p| p == name)))
            && (rule.commands.is_empty() || rule.commands.iter().any(|c| c == command))
    });
    let (category, base, sensitive, reason) = if let Some(rule) = rule {
        (
            rule.category,
            rule.points,
            rule.value_sensitive,
            rule.reason.clone(),
        )
    } else if arg.position.is_some() {
        (
            Category::Positional,
            weights.positional,
            false,
            "Declared positional input.".into(),
        )
    } else {
        (
            Category::Flag,
            weights.flag,
            false,
            "Ordinary flag without a reviewed promotion.".into(),
        )
    };
    let points = base
        .checked_add(if arg.required {
            weights.required_bonus
        } else {
            0
        })
        .ok_or("argument points overflow")?;
    Ok(ArgumentScore {
        member: member.to_owned(),
        command: command.to_owned(),
        argument: name.to_owned(),
        spelling: actions::spelling(name, arg),
        category,
        required: arg.required,
        points,
        value_sensitive: sensitive
            || arg.r#type == ValueType::Boolean
            || arg.values.as_ref().is_some_and(|v| !v.is_empty()),
        reason,
    })
}

fn inherited(
    name: &str,
    own: &Action,
    declared: &BTreeMap<String, Action>,
) -> BTreeMap<String, OptionDeclaration> {
    let mut args = BTreeMap::new();
    let words: Vec<_> = name.split_whitespace().collect();
    for count in 1..words.len() {
        if let Some(parent) = declared.get(&words[..count].join(" ")) {
            args.extend(parent.args.clone());
        }
    }
    args.extend(own.args.clone());
    args
}

fn commands(
    tree: &Tree,
    features: &[Feature],
    weights: &ArgumentWeights,
) -> Result<(Vec<Command>, Vec<ArgumentScore>), String> {
    let mut commands = Vec::new();
    let mut scores = Vec::new();
    for feature in features {
        let path = tree.root.join(&feature.member).join("actions.toml");
        let declared = actions::parse(tree.files.get(&path).ok_or("missing action declarations")?)
            .map_err(|e| e.to_string())?;
        for (name, own) in &declared {
            if name == "root" || own.hidden() || !own.on(Surface::Cli) {
                continue;
            }
            let args = inherited(name, own, &declared);
            for (key, arg) in &args {
                scores.push(argument(&feature.member, name, key, arg, weights)?);
            }
            commands.push(Command {
                member: feature.member.clone(),
                name: name.clone(),
                args,
                points: 0,
                mentioned_pages: BTreeSet::new(),
                homepage: false,
            });
        }
    }
    Ok((commands, scores))
}

fn command_prominence(
    tree: &Tree,
    commands: &mut [Command],
    weights: &Weights,
) -> Result<(), String> {
    let pages = tree
        .files
        .iter()
        .filter(|(path, _)| super::document(path))
        .map(|(path, text)| {
            let text = crate::navigation::explanations(text);
            let mut argv = super::examples::read(&text)
                .unwrap_or_default()
                .into_iter()
                .map(|e| e.argv)
                .collect::<Vec<_>>();
            argv.extend(
                super::examples::terminal_commands(&text)
                    .invocations
                    .into_iter()
                    .map(|e| e.argv),
            );
            (path, argv)
        })
        .collect::<Vec<_>>();
    for command in commands {
        let words: Vec<_> = command.name.split_whitespace().collect();
        for (path, argv) in &pages {
            if argv
                .iter()
                .any(|v| v.len() > words.len() && words.iter().zip(&v[1..]).all(|(a, b)| *a == b))
            {
                command.mentioned_pages.insert(tree.shown(path));
                command.homepage |= **path == tree.root.join("README.md");
            }
        }
        command.points = (command.mentioned_pages.len() as u64)
            .checked_mul(weights.mentioned)
            .and_then(|v| {
                v.checked_add(if command.homepage {
                    weights.homepage
                } else {
                    0
                })
            })
            .ok_or("command points overflow")?;
    }
    Ok(())
}

fn resolve<'a>(feature: &str, argv: &[String], commands: &'a [Command]) -> Option<&'a Command> {
    commands
        .iter()
        .filter(|c| c.member == feature)
        .filter(|c| {
            let words: Vec<_> = c.name.split_whitespace().collect();
            argv.len() > words.len() && words.iter().zip(&argv[1..]).all(|(a, b)| *a == b)
        })
        .max_by_key(|c| c.name.split_whitespace().count())
}

fn units(
    command: &Command,
    argv: &[String],
    weights: &ArgumentWeights,
    problems: &mut Vec<String>,
) -> Result<Vec<Gain>, String> {
    let mut units = BTreeMap::new();
    units.insert(
        Unit::Command {
            command: command.name.clone(),
        },
        weights.command,
    );
    let mut positional: Vec<_> = command
        .args
        .iter()
        .filter(|(_, arg)| arg.position.is_some())
        .collect();
    positional.sort_by_key(|(_, arg)| arg.position);
    let mut at = 1 + command.name.split_whitespace().count();
    let mut position = 0;
    let mut passthrough = false;
    let mut flags = false;
    while let Some(word) = argv.get(at) {
        at += 1;
        if word == "--" {
            passthrough = true;
            continue;
        }
        let (key, arg, value) =
            if !passthrough && word.starts_with('-') && word.parse::<f64>().is_err() {
                flags = true;
                let (flag, attached) = word
                    .split_once('=')
                    .map_or((word.as_str(), None), |(f, v)| (f, Some(v)));
                let found = command.args.iter().find(|(_, arg)| {
                    arg.long
                        .as_ref()
                        .is_some_and(|name| flag == format!("--{name}"))
                        || arg
                            .short
                            .as_ref()
                            .is_some_and(|name| flag == format!("-{name}"))
                });
                let Some((key, arg)) = found else {
                    problems.push(format!("{}: unparsed flag {flag}", argv.join(" ")));
                    continue;
                };
                let value = if let Some(value) = attached {
                    Some(value.to_owned())
                } else if arg.r#type == ValueType::Boolean {
                    Some("true".into())
                } else if let Some(value) = argv.get(at).filter(|v| !v.starts_with('-')) {
                    at += 1;
                    Some(value.clone())
                } else if arg.optional_value {
                    None
                } else {
                    problems.push(format!("{}: missing value for {flag}", argv.join(" ")));
                    None
                };
                (key, arg, value)
            } else {
                let Some((key, arg)) = positional.get(position).copied() else {
                    problems.push(format!("{}: unparsed positional {word}", argv.join(" ")));
                    continue;
                };
                if !arg.multiple {
                    position += 1;
                }
                (key, arg, Some(word.clone()))
            };
        let score = argument(&command.member, &command.name, key, arg, weights)?;
        units.insert(
            Unit::Argument {
                command: command.name.clone(),
                argument: key.clone(),
                value: if score.value_sensitive { value } else { None },
            },
            score.points,
        );
    }
    if !flags {
        units.insert(
            Unit::DefaultPath {
                command: command.name.clone(),
            },
            weights.default_path,
        );
    }
    Ok(units
        .into_iter()
        .map(|(unit, points)| Gain { unit, points })
        .collect())
}

fn marginal(
    candidate: &Candidate,
    seen: &BTreeSet<Unit>,
    weights: &ArgumentWeights,
) -> Result<(u64, Vec<Gain>), String> {
    let fresh: Vec<_> = candidate
        .units
        .iter()
        .filter(|g| !seen.contains(&g.unit))
        .cloned()
        .collect();
    let command = fresh
        .iter()
        .filter(|g| matches!(g.unit, Unit::Command { .. }))
        .map(|g| g.points)
        .max()
        .unwrap_or(0);
    let argument = fresh
        .iter()
        .filter(|g| matches!(g.unit, Unit::Argument { .. } | Unit::DefaultPath { .. }))
        .map(|g| g.points)
        .max()
        .unwrap_or(0);
    let value = weights
        .case_floor
        .checked_add(command)
        .and_then(|v| v.checked_add(argument))
        .ok_or("marginal points overflow")?;
    Ok((value, fresh))
}

fn priority(
    feature: u64,
    command: u64,
    value: u64,
    selected: usize,
    weights: &SelectionWeights,
) -> f64 {
    let importance = match weights.importance_curve {
        Curve::Log => (feature as f64).ln_1p(),
        Curve::SquareRoot => (feature as f64).sqrt(),
        Curve::Linear => feature as f64,
    };
    importance * (command as f64).ln_1p().powf(weights.command_exponent) * value as f64
        / (1.0 + selected as f64).powf(weights.decay)
}

pub fn build(tree: &Tree, features: &[Feature], weights: &Weights) -> Result<Selection, String> {
    if [weights.selection.decay, weights.selection.command_exponent]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err("selection curve parameters must be finite and nonnegative".into());
    }
    let (mut commands, arguments) = commands(tree, features, &weights.arguments)?;
    command_prominence(tree, &mut commands, weights)?;
    let mut problems = Vec::new();
    let mut candidates: BTreeMap<(String, Vec<String>), Candidate> = BTreeMap::new();
    for feature in features {
        for example in &feature.examples {
            let key = (feature.member.clone(), example.argv.clone());
            if let Some(candidate) = candidates.get_mut(&key) {
                candidate.sources.push(example.clone());
                continue;
            }
            let Some(command) = resolve(&feature.member, &example.argv, &commands) else {
                problems.push(format!(
                    "{}:{}: unresolved command",
                    example.file, example.line
                ));
                continue;
            };
            let units = units(command, &example.argv, &weights.arguments, &mut problems)?;
            let positionals = units
                .iter()
                .filter_map(|g| match &g.unit {
                    Unit::Argument { argument, .. }
                        if command
                            .args
                            .get(argument)
                            .is_some_and(|a| a.position.is_some()) =>
                    {
                        Some(g.unit.clone())
                    }
                    _ => None,
                })
                .collect();
            candidates.insert(
                key,
                Candidate {
                    member: feature.member.clone(),
                    command: command.name.clone(),
                    argv: example.argv.clone(),
                    sources: vec![example.clone()],
                    units,
                    feature_points: feature.score,
                    command_points: command.points,
                    source_order: candidates.len(),
                    positionals,
                },
            );
        }
    }
    let queue = schedule(candidates.into_values().collect(), weights)?;
    problems.sort();
    problems.dedup();
    Ok(Selection {
        arguments,
        commands: commands
            .into_iter()
            .map(|c| CommandScore {
                member: c.member,
                command: c.name,
                points: c.points,
                homepage: c.homepage,
                mentioned_pages: c.mentioned_pages,
            })
            .collect(),
        queue,
        problems,
    })
}

type Group = (String, String, Vec<Unit>);

fn group(candidate: &Candidate) -> Group {
    (
        candidate.member.clone(),
        candidate.command.clone(),
        candidate.positionals.clone(),
    )
}

fn schedule(mut candidates: Vec<Candidate>, weights: &Weights) -> Result<Vec<Pick>, String> {
    let mut seen: BTreeMap<String, BTreeSet<Unit>> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut turns: BTreeMap<Group, usize> = BTreeMap::new();
    let mut queue = Vec::new();
    while !candidates.is_empty() {
        let mut minimum: BTreeMap<String, usize> = BTreeMap::new();
        for candidate in &candidates {
            let turn = turns.get(&group(candidate)).copied().unwrap_or(0);
            minimum
                .entry(candidate.member.clone())
                .and_modify(|n| *n = (*n).min(turn))
                .or_insert(turn);
        }
        let mut best = None;
        for (index, candidate) in candidates.iter().enumerate() {
            let turn = turns.get(&group(candidate)).copied().unwrap_or(0);
            if weights.selection.positional_round_robin && turn > minimum[&candidate.member] {
                continue;
            }
            let used = seen.entry(candidate.member.clone()).or_default();
            let (value, fresh) = marginal(candidate, used, &weights.arguments)?;
            let count = counts.get(&candidate.member).copied().unwrap_or(0);
            let score = priority(
                candidate.feature_points,
                candidate.command_points,
                value,
                count,
                &weights.selection,
            );
            let homepage = candidate.sources.iter().any(|s| s.file == "README.md");
            let key = (
                score,
                homepage,
                std::cmp::Reverse(candidate.argv.len()),
                std::cmp::Reverse(candidate.source_order),
            );
            if best.as_ref().is_none_or(|(_, old, _, _, _)| key > *old) {
                best = Some((index, key, value, fresh, count));
            }
        }
        let (index, (score, _, _, _), value, fresh, count) = best.ok_or("no priority candidate")?;
        let candidate = candidates.remove(index);
        let turn = turns.entry(group(&candidate)).or_default();
        let positional_turn = *turn;
        *turn += 1;
        seen.entry(candidate.member.clone())
            .or_default()
            .extend(candidate.units.iter().map(|g| g.unit.clone()));
        *counts.entry(candidate.member.clone()).or_default() += 1;
        queue.push(Pick {
            order: queue.len() + 1,
            member: candidate.member,
            command: candidate.command,
            argv: candidate.argv,
            sources: candidate.sources,
            feature_points: candidate.feature_points,
            command_points: candidate.command_points,
            selected_before: count,
            positional_turn,
            positional_group: candidate.positionals,
            new_units: fresh,
            marginal_points: value,
            priority: score,
        });
    }
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(position: Option<usize>, flag: Option<&str>, required: bool) -> OptionDeclaration {
        toml::from_str(&format!(
            "type='string'\nrequired={required}\n{}{}",
            position
                .map(|p| format!("position={p}\n"))
                .unwrap_or_default(),
            flag.map(|f| format!("long='{f}'\n")).unwrap_or_default()
        ))
        .unwrap()
    }

    #[test]
    fn important_flags_can_outrank_positionals_and_ordinary_flags_cannot() {
        let weights = Weights::bundled();
        let score = |a| {
            argument("member", "command", "value", &a, &weights.arguments)
                .unwrap()
                .points
        };
        assert_eq!(score(arg(None, Some("ordinary"), false)), 1);
        assert_eq!(score(arg(Some(1), None, false)), 5);
        assert_eq!(score(arg(None, Some("harness"), true)), 10);
        assert_eq!(score(arg(Some(1), None, true)), 7);
    }

    #[test]
    fn agent_depth_falls_below_a_first_exit_check() {
        let weights = Weights::bundled();
        assert!(
            priority(1004, 20, 19, 0, &weights.selection)
                > priority(69, 20, 19, 0, &weights.selection)
        );
        assert!(
            priority(1004, 20, 19, 3, &weights.selection)
                < priority(69, 20, 19, 0, &weights.selection)
        );
    }

    #[test]
    fn repeated_arguments_and_kitchen_sink_flags_do_not_inflate_value() {
        let weights = Weights::bundled();
        let base = Unit::Command {
            command: "go".into(),
        };
        let candidate = Candidate {
            member: "member".into(),
            command: "go".into(),
            argv: vec![],
            sources: vec![],
            feature_points: 1004,
            command_points: 20,
            source_order: 0,
            positionals: vec![],
            units: vec![
                Gain {
                    unit: base.clone(),
                    points: 10,
                },
                Gain {
                    unit: Unit::Argument {
                        command: "go".into(),
                        argument: "name".into(),
                        value: None,
                    },
                    points: 5,
                },
                Gain {
                    unit: Unit::Argument {
                        command: "go".into(),
                        argument: "json".into(),
                        value: Some("true".into()),
                    },
                    points: 1,
                },
            ],
        };
        assert_eq!(
            marginal(&candidate, &BTreeSet::new(), &weights.arguments)
                .unwrap()
                .0,
            16
        );
        let seen = candidate.units.iter().map(|g| g.unit.clone()).collect();
        assert_eq!(
            marginal(&candidate, &seen, &weights.arguments).unwrap().0,
            1
        );
    }

    fn candidate(name: &str, points: u64, order: usize, units: Vec<Gain>) -> Candidate {
        Candidate {
            member: "agents".into(),
            command: name.into(),
            argv: vec!["lnk".into(), name.into()],
            sources: vec![],
            units,
            feature_points: 1004,
            command_points: points,
            source_order: order,
            positionals: vec![],
        }
    }

    #[test]
    fn positional_rotation_visits_another_command_before_flag_depth() {
        let weights = Weights::bundled();
        let command = |name: &str| Gain {
            unit: Unit::Command {
                command: name.into(),
            },
            points: 10,
        };
        let first = candidate(
            "start",
            1000,
            0,
            vec![
                command("start"),
                Gain {
                    unit: Unit::DefaultPath {
                        command: "start".into(),
                    },
                    points: 8,
                },
            ],
        );
        let mut variation = candidate(
            "start",
            1000,
            1,
            vec![
                command("start"),
                Gain {
                    unit: Unit::Argument {
                        command: "start".into(),
                        argument: "harness".into(),
                        value: Some("other".into()),
                    },
                    points: 8,
                },
            ],
        );
        variation.argv.push("--harness=other".into());
        let other = candidate("stop", 1, 2, vec![command("stop")]);
        let queue = schedule(vec![first, variation, other], &weights).unwrap();
        assert_eq!(queue[1].command, "stop");
        assert_eq!(queue[2].command, "start");
        assert_eq!(queue[2].positional_turn, 1);
    }

    #[test]
    fn meaningful_positional_choices_differ_but_resource_names_do_not() {
        let weights = Weights::bundled();
        let name = arg(Some(1), None, false);
        assert!(
            !argument("member", "box ssh", "name", &name, &weights.arguments)
                .unwrap()
                .value_sensitive
        );
        assert!(
            argument(
                "member",
                "agent connect",
                "connection",
                &name,
                &weights.arguments
            )
            .unwrap()
            .value_sensitive
        );
    }

    #[test]
    fn positional_choices_rotate_before_a_second_providers_flags() {
        let weights = Weights::bundled();
        let choice = |provider: &str, flag: bool, order| {
            let positional = Unit::Argument {
                command: "connect".into(),
                argument: "kind".into(),
                value: Some(provider.into()),
            };
            let mut c = candidate(
                "connect",
                20,
                order,
                vec![
                    Gain {
                        unit: Unit::Command {
                            command: "connect".into(),
                        },
                        points: 10,
                    },
                    Gain {
                        unit: positional.clone(),
                        points: 7,
                    },
                    Gain {
                        unit: if flag {
                            Unit::Argument {
                                command: "connect".into(),
                                argument: "harness".into(),
                                value: Some("other".into()),
                            }
                        } else {
                            Unit::DefaultPath {
                                command: "connect".into(),
                            }
                        },
                        points: 8,
                    },
                ],
            );
            c.positionals = vec![positional];
            c.argv.push(provider.into());
            if flag {
                c.argv.push("--harness=other".into());
            }
            c
        };
        let queue = schedule(
            vec![
                choice("aws", false, 0),
                choice("aws", true, 1),
                choice("gcp", false, 2),
            ],
            &weights,
        )
        .unwrap();
        assert_eq!(queue[0].argv.last().unwrap(), "aws");
        assert_eq!(queue[1].argv.last().unwrap(), "gcp");
        assert_eq!(queue[2].positional_turn, 1);
    }

    #[test]
    fn a_flag_variant_cannot_consume_the_default_path() {
        let weights = Weights::bundled();
        let command = Command {
            member: "member".into(),
            name: "go".into(),
            args: [("harness".into(), arg(None, Some("harness"), false))].into(),
            points: 20,
            mentioned_pages: BTreeSet::new(),
            homepage: false,
        };
        let mut problems = vec![];
        let explicit = units(
            &command,
            &["lnk".into(), "go".into(), "--harness".into(), "link".into()],
            &weights.arguments,
            &mut problems,
        )
        .unwrap();
        let normal = units(
            &command,
            &["lnk".into(), "go".into()],
            &weights.arguments,
            &mut problems,
        )
        .unwrap();
        let used = explicit.into_iter().map(|g| g.unit).collect();
        let candidate = candidate("go", 20, 0, normal);
        assert_eq!(
            marginal(&candidate, &used, &weights.arguments).unwrap().0,
            9
        );
        assert!(problems.is_empty());
    }
}
