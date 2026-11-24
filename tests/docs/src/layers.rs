//! The layers: each group folder (`packages/`, `core/`, `tests/`, and
//! each `modules/<domain>/` and `interop/<domain>/`) built only on
//! those its `layer.toml` lists. Read from the manifests and the source,
//! as written: nothing is built.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "layers",
    scope: Scope::Site,
    fix: "each group folder has a layer.toml naming every group folder it is built on, \
          with no cycle; a crate depends on, runs (`lnk <group>`, or through `ensure`) and \
          answers the questions of only crates of its group or of one its layer.toml lists. \
          Move the part to where it belongs, never add a `depends_on` for it",
    check: Check::Whole(layers),
}];

/// A group folder's `layer.toml`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    depends_on: Vec<String>,
    /// The concepts the layer introduces, which no layer it's built on
    /// says: the repository checks' `layer_words` rule reads them.
    #[serde(default, rename = "words")]
    _words: Vec<String>,
}

/// A package, as its manifest says it.
pub(crate) struct Package {
    pub(crate) dir: PathBuf,
    /// The workspace packages it depends on, in Cargo: normal and build.
    uses: BTreeSet<String>,
    /// The modules its programs are (`lnk-<id>`), by their IDs, and the
    /// core's `lnk`, which asks the questions named `lnk.<what>`.
    pub(crate) modules: Vec<String>,
    /// The groups `lnk` routes to it (`lnk <group> ...`), as its
    /// `module.toml` says.
    routes: Vec<String>,
    /// The questions it answers, as its `module.toml` says
    /// (`<module>.<what>`, of the module that asks).
    answers: Vec<String>,
}

fn layers(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    let groups = groups(tree, &mut found);
    let packages = packages(tree);
    let group_of = |dir: &Path| -> Option<&str> {
        let relative = dir.strip_prefix(&tree.root).ok()?;
        groups
            .keys()
            .filter(|g| relative.starts_with(g.as_str()))
            .max_by_key(|g| g.len())
            .map(String::as_str)
    };
    let mut ids = BTreeMap::new();
    let mut routes = BTreeMap::new();
    for (name, package) in &packages {
        for module in &package.modules {
            ids.insert(module.clone(), name.clone());
        }
        for route in &package.routes {
            routes.insert(route.clone(), name.clone());
        }
    }
    for (name, package) in &packages {
        // A caller may provide dependency manifests from another workspace to
        // resolve module identities. Their own workspace checks their layers.
        if !package.dir.starts_with(&tree.root) {
            continue;
        }
        let Some(group) = group_of(&package.dir) else {
            found.push(fail(format!(
                "{}: in no group folder with a layer.toml",
                package.dir.display()
            )));
            continue;
        };
        let allowed = |other: &str| -> bool {
            group_of(&packages[other].dir).is_none_or(|g| g == group || groups[group].contains(g))
        };
        for used in package.uses.iter().filter(|u| packages.contains_key(*u)) {
            if !allowed(used) {
                found.push(fail(format!(
                    "{name} ({group}) depends on {used} ({}), which its layer.toml doesn't list",
                    group_of(&packages[used].dir).unwrap_or("none")
                )));
            }
        }
        for (path, module) in ensured(tree, &package.dir, &packages) {
            if let Some(other) = ids.get(&module)
                && other != name
                && !allowed(other)
            {
                found.push(fail(format!(
                    "{}: ensure({module:?}) starts {other} ({}), which {group}'s layer.toml \
                     doesn't list",
                    path.display(),
                    group_of(&packages[other].dir).unwrap_or("none")
                )));
            }
        }
        for (path, run) in run(tree, &package.dir, &packages) {
            if let Some(other) = routes.get(&run)
                && other != name
                && !allowed(other)
            {
                found.push(fail(format!(
                    "{}: `lnk {run}` runs {other} ({}), which {group}'s layer.toml doesn't \
                     list",
                    path.display(),
                    group_of(&packages[other].dir).unwrap_or("none")
                )));
            }
        }
        for question in &package.answers {
            let asker = question.split_once('.').and_then(|(p, _)| ids.get(p));
            match asker {
                Some(asker) if asker == name || allowed(asker) => {}
                Some(asker) => found.push(fail(format!(
                    "{name} ({group}) answers {question}, {asker}'s ({}), which its layer.toml \
                     doesn't list",
                    group_of(&packages[asker].dir).unwrap_or("none")
                ))),
                None => found.push(fail(format!(
                    "{name} answers {question}, which no module of the workspace asks"
                ))),
            }
        }
    }
    found
}

/// Every group folder's `depends_on`, by its folder from the workspace's;
/// an error for a group folder without one, a name that is none, or a
/// cycle.
fn groups(tree: &Tree, found: &mut Vec<Finding>) -> BTreeMap<String, BTreeSet<String>> {
    let mut groups = BTreeMap::new();
    for (path, text) in &tree.files {
        if path.file_name().is_none_or(|n| n != "layer.toml") {
            continue;
        }
        let Some(dir) = path.parent().and_then(|d| d.strip_prefix(&tree.root).ok()) else {
            continue;
        };
        match toml::from_str::<Layer>(text) {
            Ok(layer) => {
                groups.insert(
                    dir.to_string_lossy().into_owned(),
                    layer.depends_on.into_iter().collect::<BTreeSet<_>>(),
                );
            }
            Err(err) => found.push(fail(format!("{}: {err}", path.display()))),
        }
    }
    for group in group_folders(tree) {
        if !groups.contains_key(&group) {
            found.push(fail(format!(
                "{group}/ is a group folder with no layer.toml"
            )));
        }
    }
    for (group, depends) in &groups {
        for depend in depends.iter().filter(|d| !groups.contains_key(*d)) {
            found.push(fail(format!(
                "{group}/layer.toml: {depend} is no group folder with a layer.toml"
            )));
        }
    }
    if let Some(cycle) = cycle(&groups) {
        found.push(fail(format!("the layers go round: {}", cycle.join(" -> "))));
    }
    groups
}

/// The group folders there are: `packages`, `core`, `tests`, and each
/// domain folder of `modules/` and `interop/`.
fn group_folders(tree: &Tree) -> Vec<String> {
    let mut groups = Vec::new();
    for top in ["packages", "core", "tests"] {
        if tree.dirs.contains(&tree.root.join(top)) {
            groups.push(top.to_string());
        }
    }
    for top in ["modules", "interop"] {
        let top = tree.root.join(top);
        for dir in tree
            .dirs
            .iter()
            .filter(|d| d.parent() == Some(top.as_path()))
        {
            if let Ok(relative) = dir.strip_prefix(&tree.root) {
                groups.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    groups
}

/// A cycle among `groups`, as the folders on it, when there is one.
fn cycle(groups: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    fn visit(
        group: &str,
        groups: &BTreeMap<String, BTreeSet<String>>,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(at) = path.iter().position(|g| g == group) {
            let mut cycle = path[at..].to_vec();
            cycle.push(group.to_string());
            return Some(cycle);
        }
        if !done.insert(group.to_string()) {
            return None;
        }
        path.push(group.to_string());
        for depend in groups.get(group).into_iter().flatten() {
            if let Some(cycle) = visit(depend, groups, path, done) {
                return Some(cycle);
            }
        }
        path.pop();
        None
    }
    let mut done = BTreeSet::new();
    groups
        .keys()
        .find_map(|g| visit(g, groups, &mut Vec::new(), &mut done))
}

/// Every package of the workspace, by its name.
pub(crate) fn packages(tree: &Tree) -> BTreeMap<String, Package> {
    let mut packages = BTreeMap::new();
    for (path, text) in &tree.files {
        if path.file_name().is_none_or(|n| n != "Cargo.toml") {
            continue;
        }
        let Ok(manifest) = text.parse::<toml::Table>() else {
            continue;
        };
        let Some(name) = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        else {
            continue;
        };
        let mut uses = BTreeSet::new();
        let mut tables = vec![
            manifest.get("dependencies"),
            manifest.get("build-dependencies"),
        ];
        for target in manifest
            .get("target")
            .and_then(|t| t.as_table())
            .into_iter()
            .flat_map(|t| t.values())
        {
            tables.push(target.get("dependencies"));
            tables.push(target.get("build-dependencies"));
        }
        for (alias, spec) in tables
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_table())
            .flatten()
        {
            let named = spec.get("package").and_then(|p| p.as_str());
            uses.insert(named.unwrap_or(alias).to_string());
        }
        let dir = path.parent().expect("a manifest's folder").to_path_buf();
        // A module is what its `module.toml` declares, never a program's
        // name alone: every program is `lnk-<name>`, a fixture's or a
        // tool's too.
        let declared = tree.files.contains_key(&dir.join("module.toml"));
        let modules = manifest
            .get("bin")
            .and_then(|b| b.as_array())
            .into_iter()
            .flatten()
            .filter(|_| declared)
            .filter_map(|b| b.get("name").and_then(|n| n.as_str()))
            // The core asks as `lnk` (`lnk.<what>`), a module as its ID.
            .filter_map(|n| n.strip_prefix("lnk-").or((n == "lnk").then_some(n)))
            .map(str::to_string)
            .collect();
        let manifest = tree
            .files
            .get(&dir.join("module.toml"))
            .and_then(|text| text.parse::<toml::Table>().ok())
            .unwrap_or_default();
        // A module's commands are its `actions.toml`; the sandbox's, its
        // `module.toml`'s until sandbox grants.
        let actions = tree
            .files
            .get(&dir.join("actions.toml"))
            .and_then(|text| text.parse::<toml::Table>().ok());
        let routes = actions
            .as_ref()
            .map(|actions| (actions, "dispatch"))
            .or_else(|| {
                manifest
                    .get("commands")
                    .and_then(|c| c.as_table())
                    .map(|c| (c, "route"))
            })
            .into_iter()
            .flat_map(|(commands, field)| {
                commands.iter().filter(move |(command, spec)| {
                    !command.contains(' ')
                        && spec.get(field).and_then(|r| r.as_bool()) == Some(true)
                })
            })
            .map(|(command, _)| command.clone())
            .collect();
        let answers = manifest
            .get("answers")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
            .filter_map(|q| q.as_str().map(String::from))
            .collect();
        packages.insert(
            name.to_string(),
            Package {
                dir,
                uses,
                modules,
                routes,
                answers,
            },
        );
    }
    packages
}

/// The groups package `dir`'s source runs as `lnk <group> ...`, each
/// with its file: `lnk().args(["box", ...])`, `lnk().arg("box")`, or a
/// command made by `lnk()` and given its group in a later line. Its tests
/// run what they like.
fn run(tree: &Tree, dir: &Path, packages: &BTreeMap<String, Package>) -> Vec<(PathBuf, String)> {
    let group_at = |rest: &str| -> Option<String> {
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(".args(")
            .or_else(|| rest.strip_prefix(".arg("))?
            .trim_start();
        let rest = rest.strip_prefix('[').unwrap_or(rest).trim_start();
        let word: String = rest
            .strip_prefix('"')?
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        (!word.is_empty()).then_some(word)
    };
    let mut found = Vec::new();
    for (path, text) in sources(tree, dir, packages) {
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (at, _) in code.match_indices("lnk()") {
            let after = &code[at + "lnk()".len()..];
            if let Some(group) = group_at(after) {
                found.push((path.clone(), group));
                continue;
            }
            // `let mut start = link_core::lnk();`, then `start.args([...])`.
            let line = code[..at].rsplit('\n').next().unwrap_or_default();
            let Some(name) = line
                .trim_start()
                .strip_prefix("let ")
                .map(|l| l.trim_start_matches("mut "))
                .and_then(|l| l.split([' ', '=', ':']).next())
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            let window: String = after.chars().take(1500).collect();
            if let Some(group) = window
                .match_indices(&format!("{name}."))
                .find_map(|(i, _)| group_at(&window[i + name.len()..]))
            {
                found.push((path.clone(), group));
            }
        }
    }
    found
}

/// Each Rust file of package `dir`'s `src/`, but another package's
/// folder inside it.
fn sources<'a>(
    tree: &'a Tree,
    dir: &Path,
    packages: &BTreeMap<String, Package>,
) -> Vec<(&'a PathBuf, &'a String)> {
    let src = dir.join("src");
    let nested: Vec<&Path> = packages
        .values()
        .map(|p| p.dir.as_path())
        .filter(|d| *d != dir && d.starts_with(dir))
        .collect();
    tree.files
        .range(src.clone()..)
        .take_while(|(path, _)| path.starts_with(&src))
        .filter(|(path, _)| {
            path.extension().is_some_and(|e| e == "rs")
                && !nested.iter().any(|n| path.starts_with(n))
        })
        .collect()
}

/// Each module package `dir`'s source starts through
/// `ensure("<module>")`, the one way a module starts another: the file
/// and the module. Another package's folder inside it is its own.
fn ensured(
    tree: &Tree,
    dir: &Path,
    packages: &BTreeMap<String, Package>,
) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for (path, text) in sources(tree, dir, packages) {
        let mut rest = text.as_str();
        while let Some(at) = rest.find("ensure(") {
            rest = &rest[at + "ensure(".len()..];
            let quoted = rest.trim_start();
            if let Some(name) = quoted
                .strip_prefix('"')
                .and_then(|q| q.split_once('"'))
                .map(|(name, _)| name)
                .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
            {
                found.push((path.clone(), name.to_string()));
            }
        }
    }
    found
}
