//! Documentation prominence from one workspace, independent of execution or case history.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use link_component::actions::{self, Surface};
use serde::{Deserialize, Serialize};

use crate::{Tree, examples, md, navigation};

mod selection;
pub use selection::{ArgumentWeights, SelectionWeights};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub homepage: u64,
    pub incoming: u64,
    pub mentioned: u64,
    pub outgoing: u64,
    pub arguments: ArgumentWeights,
    pub selection: SelectionWeights,
}

impl Weights {
    pub fn bundled() -> Self {
        toml::from_str(include_str!("../priority.toml")).expect("valid priority weights")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Example {
    pub file: String,
    pub line: usize,
    pub section: String,
    pub argv: Vec<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Feature {
    pub member: String,
    pub groups: BTreeSet<String>,
    pub score: u64,
    pub homepage: bool,
    pub incoming_pages: BTreeSet<String>,
    pub mentioned_pages: BTreeSet<String>,
    pub outgoing_pages: BTreeSet<String>,
    pub readme_incoming_pages: BTreeSet<String>,
    pub readme_outgoing_pages: BTreeSet<String>,
    pub navigation_incoming_pages: BTreeSet<String>,
    pub examples: Vec<Example>,
    pub distinct_examples: usize,
}

#[derive(Serialize)]
pub struct Ranking {
    pub schema: u32,
    pub scope: String,
    pub interpretation: &'static str,
    pub weights: Weights,
    pub documents: usize,
    pub readmes: usize,
    pub example_branches: usize,
    pub distinct_examples: usize,
    pub workflow_groups: usize,
    pub unresolved_examples: Vec<Example>,
    pub problems: Vec<String>,
    pub features: Vec<Feature>,
    pub argument_scores: Vec<selection::ArgumentScore>,
    pub command_scores: Vec<selection::CommandScore>,
    pub priority_queue: Vec<selection::Pick>,
    pub selection_interpretation: &'static str,
    pub selection_problems: Vec<String>,
}

fn readme(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("README.md"))
}

fn document(path: &Path) -> bool {
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    if parts.iter().any(|p| {
        matches!(
            *p,
            "fixtures" | "node_modules" | "target" | "vendor" | "artifacts"
        )
    }) {
        return false;
    }
    readme(path)
        || (path.extension().is_some_and(|s| s == "md")
            && parts.contains(&"docs")
            && path.file_name().is_none_or(|s| s != "navigation.md"))
}

fn owner(path: &Path, members: &BTreeMap<PathBuf, Feature>) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|p| members.contains_key(*p))
        .map(Path::to_path_buf)
}

fn normalized(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => out.push(part),
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
        }
    }
    Some(out)
}

fn target(tree: &Tree, source: &Path, link: &str) -> Option<PathBuf> {
    let link = link.split(['#', '?']).next()?;
    if link.is_empty() || link.contains("://") || link.starts_with("mailto:") {
        return None;
    }
    if link.starts_with('/') {
        return tree
            .pages
            .get(link.trim_end_matches('/'))
            .and_then(|p| p.first())
            .cloned();
    }
    let candidate = normalized(&source.parent()?.join(md::link_path(link)))?;
    if !candidate.starts_with(&tree.root) {
        return None;
    }
    if tree.files.contains_key(&candidate) {
        return Some(candidate);
    }
    let readme = candidate.join("README.md");
    tree.files.contains_key(&readme).then_some(readme)
}

fn members(tree: &Tree) -> Result<BTreeMap<PathBuf, Feature>, String> {
    let mut members = BTreeMap::new();
    for (path, text) in &tree.files {
        if path.file_name().is_none_or(|p| p != "actions.toml")
            || path
                .components()
                .any(|p| matches!(p.as_os_str().to_str(), Some("fixtures" | "tests")))
        {
            continue;
        }
        let groups: BTreeSet<_> = actions::parse(text)
            .map_err(|e| format!("{}: {e}", tree.shown(path)))?
            .into_iter()
            .filter(|(name, action)| name != "root" && !action.hidden() && action.on(Surface::Cli))
            .filter_map(|(name, _)| name.split_whitespace().next().map(str::to_owned))
            .collect();
        if !groups.is_empty() {
            let folder = path
                .parent()
                .ok_or("action file needs an owner")?
                .to_path_buf();
            members.insert(
                folder.clone(),
                Feature {
                    member: tree.shown(&folder),
                    groups,
                    ..Feature::default()
                },
            );
        }
    }
    Ok(members)
}

fn links(
    tree: &Tree,
    pages: &BTreeMap<PathBuf, String>,
    features: &mut BTreeMap<PathBuf, Feature>,
    authored: bool,
) {
    for (path, text) in pages {
        let from = owner(path, features);
        let prose = md::split(text).0;
        for link in md::links(&prose) {
            let Some(destination) = target(tree, path, link).filter(|p| pages.contains_key(p))
            else {
                continue;
            };
            let to = owner(&destination, features);
            if from == to {
                continue;
            }
            if let Some(to) = to {
                let feature = features.get_mut(&to).unwrap();
                if authored {
                    feature.incoming_pages.insert(tree.shown(path));
                    if readme(path) {
                        feature.readme_incoming_pages.insert(tree.shown(path));
                    }
                    if *path == tree.root.join("README.md") {
                        feature.homepage = true;
                    }
                } else {
                    feature.navigation_incoming_pages.insert(tree.shown(path));
                }
            }
            if authored && let Some(from) = &from {
                let feature = features.get_mut(from).unwrap();
                feature.outgoing_pages.insert(tree.shown(&destination));
                if readme(path) {
                    feature
                        .readme_outgoing_pages
                        .insert(tree.shown(&destination));
                }
            }
        }
    }
}

/// Rank only sources below the selected workspace. Generated navigation pages, fixtures and vendored files do not score.
pub fn rank(tree: &Tree, weights: Weights) -> Result<Ranking, String> {
    let mut features = members(tree)?;
    let raw: BTreeMap<_, _> = tree
        .files
        .iter()
        .filter(|(p, _)| document(p))
        .map(|(p, t)| (p.clone(), t.clone()))
        .collect();
    let authored: BTreeMap<_, _> = raw
        .iter()
        .map(|(p, t)| (p.clone(), navigation::explanations(t)))
        .collect();
    links(tree, &authored, &mut features, true);
    links(tree, &raw, &mut features, false);
    let mut problems = Vec::new();
    let mut unresolved = Vec::new();
    let mut unique = BTreeSet::new();
    let mut example_count = 0;
    let mut workflows = 0;
    for (path, text) in &authored {
        // Mentions include explanatory prose and printed Next suggestions,
        // which need not be parseable shell input.
        let mut mentioned: BTreeSet<_> = text
            .lines()
            .flat_map(|line| md::commands(&line.replace('`', "")))
            .map(|(group, _, _)| group)
            .collect();
        match examples::read(text) {
            Ok(examples) => {
                mentioned.extend(examples.into_iter().filter_map(|e| e.argv.get(1).cloned()))
            }
            Err(error) => problems.push(format!("{}: {error}", tree.shown(path))),
        }
        for feature in features.values_mut() {
            if !mentioned.is_disjoint(&feature.groups) {
                feature.mentioned_pages.insert(tree.shown(path));
                if *path == tree.root.join("README.md") {
                    feature.homepage = true;
                }
            }
        }
        if !readme(path) {
            continue;
        }
        let discovered = examples::terminal_commands(text);
        problems.extend(
            discovered
                .problems
                .iter()
                .map(|e| format!("{}: {e}", tree.shown(path))),
        );
        let mut groups: BTreeMap<String, BTreeSet<(usize, usize, usize)>> = BTreeMap::new();
        for invocation in discovered.invocations {
            example_count += 1;
            unique.insert(invocation.argv.clone());
            groups
                .entry(invocation.section.clone())
                .or_default()
                .insert((invocation.block, invocation.line, invocation.ordinal));
            let example = Example {
                file: tree.shown(path),
                line: invocation.line,
                section: invocation.section,
                argv: invocation.argv,
            };
            let matching = features
                .iter()
                .filter(|(_, f)| example.argv.get(1).is_some_and(|g| f.groups.contains(g)))
                .map(|(p, _)| p.clone())
                .collect::<Vec<_>>();
            if matching.len() == 1 {
                features
                    .get_mut(&matching[0])
                    .unwrap()
                    .examples
                    .push(example);
            } else {
                unresolved.push(example);
            }
        }
        workflows += groups.values().filter(|g| g.len() > 1).count();
    }
    for feature in features.values_mut() {
        feature.navigation_incoming_pages = feature
            .navigation_incoming_pages
            .difference(&feature.incoming_pages)
            .cloned()
            .collect();
        feature.distinct_examples = feature
            .examples
            .iter()
            .map(|e| &e.argv)
            .collect::<BTreeSet<_>>()
            .len();
        feature.score = [
            (u64::from(feature.homepage), weights.homepage),
            (feature.incoming_pages.len() as u64, weights.incoming),
            (feature.mentioned_pages.len() as u64, weights.mentioned),
            (feature.outgoing_pages.len() as u64, weights.outgoing),
        ]
        .into_iter()
        .try_fold(0u64, |sum, (count, weight)| {
            sum.checked_add(count.checked_mul(weight)?)
        })
        .ok_or_else(|| format!("{}: priority weights overflow the score", feature.member))?;
    }
    let mut ranked: Vec<_> = features.into_values().collect();
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.member.cmp(&b.member)));
    let selection = selection::build(tree, &ranked, &weights)?;
    Ok(Ranking {
        schema: 1,
        scope: tree.root.display().to_string(),
        interpretation: "Documentation prominence, not usage, risk, execution time or verified coverage. Counts use distinct authored pages or targets. Generated navigation is reported separately and does not score.",
        weights,
        documents: authored.len(),
        readmes: authored.keys().filter(|p| readme(p)).count(),
        example_branches: example_count,
        distinct_examples: unique.len(),
        workflow_groups: workflows,
        unresolved_examples: unresolved,
        problems,
        features: ranked,
        argument_scores: selection.arguments,
        command_scores: selection.commands,
        priority_queue: selection.queue,
        selection_interpretation: "Suggested coverage authoring order for distinct README forms. Units and prior selections are hypothetical, not passing evidence. Prerequisites, negative counterparts, complete workflows and execution durations are not resolved, so this is not a runnable test plan.",
        selection_problems: selection.problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (link_test_kit::temp::TempDir, Tree) {
        let temp = link_test_kit::temp::TempDir::new("doc-priority");
        let package = Path::new(env!("CARGO_MANIFEST_DIR"));
        let tree = Tree::read(temp.path(), false, &package.join("large-files.toml"));
        (temp, tree)
    }

    fn add(tree: &mut Tree, name: &str, value: &str) {
        tree.files.insert(tree.root.join(name), value.to_owned());
    }

    fn feature(tree: &mut Tree, name: &str) {
        add(
            tree,
            &format!("modules/{name}/actions.toml"),
            &format!(
                "schema = 1\n[actions.'{name}.{name}.list']\ndescription = 'operation'\n\
                 role = 'primary'\noutput = 'core.report'\n"
            ),
        );
        add(
            tree,
            &format!("modules/{name}/README.md"),
            &format!("# {name}\n"),
        );
    }

    #[test]
    fn authored_links_and_mentions_score_once_and_navigation_does_not_score() {
        let (_temp, mut tree) = tree();
        feature(&mut tree, "agent");
        feature(&mut tree, "box");
        add(
            &mut tree,
            "README.md",
            "# Product\n[Agent](modules/agent/README.md)\n[Again](modules/agent/README.md)\n`lnk agent`\n`lnk agent`\n<!-- docs-tree:start -->\n[Box](modules/box/README.md)\n<!-- docs-tree:end -->\n",
        );
        let ranking = rank(&tree, Weights::bundled()).unwrap();
        assert_eq!(ranking.features[0].member, "modules/agent");
        assert_eq!(ranking.features[0].incoming_pages.len(), 1);
        assert_eq!(ranking.features[0].mentioned_pages.len(), 1);
        assert_eq!(ranking.features[0].score, 25);
        assert_eq!(ranking.features[1].score, 0);
        assert_eq!(ranking.features[1].navigation_incoming_pages.len(), 1);
    }

    #[test]
    fn source_examples_and_cross_feature_links_need_no_case_catalogue() {
        let (_temp, mut tree) = tree();
        feature(&mut tree, "agent");
        feature(&mut tree, "box");
        add(
            &mut tree,
            "modules/agent/README.md",
            "# Agent\n[Box](../box/README.md)\n```text\nlnk agent start|migrate\n```\n```sh\nlnk box create\n```\n",
        );
        let ranking = rank(&tree, Weights::bundled()).unwrap();
        assert_eq!(ranking.example_branches, 3);
        assert_eq!(ranking.workflow_groups, 1);
        assert!(ranking.unresolved_examples.is_empty());
        let a = ranking
            .features
            .iter()
            .find(|f| f.member.ends_with("agent"))
            .unwrap();
        let b = ranking
            .features
            .iter()
            .find(|f| f.member.ends_with("box"))
            .unwrap();
        assert_eq!(a.outgoing_pages.len(), 1);
        assert_eq!(a.examples.len(), 2);
        assert_eq!(b.incoming_pages.len(), 1);
        assert_eq!(b.examples.len(), 1);
    }

    #[test]
    fn authored_docs_pages_contribute_without_a_language_folder() {
        let (_temp, mut tree) = tree();
        feature(&mut tree, "agent");
        add(
            &mut tree,
            "docs/guide.md",
            "# Guide\n[Agent](../modules/agent/README.md)\n`lnk agent`\n",
        );
        let ranking = rank(&tree, Weights::bundled()).unwrap();
        assert_eq!(ranking.documents, 2);
        assert_eq!(ranking.readmes, 1);
        assert_eq!(ranking.features[0].incoming_pages.len(), 1);
        assert_eq!(ranking.features[0].mentioned_pages.len(), 1);
        assert_eq!(ranking.features[0].score, 5);
    }

    #[test]
    fn marked_up_commands_count_but_generated_inventories_do_not() {
        let (_temp, mut tree) = tree();
        feature(&mut tree, "agent");
        feature(&mut tree, "box");
        add(
            &mut tree,
            "README.md",
            "# Product\n**`lnk agent`**\n<!-- inventory:commands:start -->\n`lnk box`\n<!-- inventory:commands:end -->\n",
        );
        let ranking = rank(&tree, Weights::bundled()).unwrap();
        assert_eq!(ranking.features[0].member, "modules/agent");
        assert_eq!(ranking.features[0].score, 22);
        assert_eq!(ranking.features[1].score, 0);
    }
}
