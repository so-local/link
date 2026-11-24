//! Owner metadata and the complete authored reading outline for the docs survey.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use link_docs_check::{Tree, md, navigation, structure};

fn cell(value: impl std::fmt::Display) -> String {
    value.to_string().replace('|', "\\|").replace('\n', " ")
}

/// Join declarations to their nearest Cargo or npm owner, without treating
/// dependency counts or a large code folder as a required documentation branch.
pub fn owners(tree: &Tree, out: &mut String) -> Result<(), String> {
    let packages = structure::packages(tree);
    let mut names = BTreeMap::new();
    let mut dependencies: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for owner in &packages {
        let mut deps = BTreeSet::new();
        if let Some(text) = tree.text(&owner.join("Cargo.toml")) {
            let manifest: toml::Value =
                toml::from_str(text).map_err(|e| format!("{}: {e}", tree.shown(owner)))?;
            if let Some(name) = manifest
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(toml::Value::as_str)
            {
                names.insert(owner.clone(), name.to_string());
            }
            fn cargo_deps(value: &toml::Value, deps: &mut BTreeSet<String>) {
                if let Some(table) = value.as_table() {
                    for (key, value) in table {
                        if matches!(
                            key.as_str(),
                            "dependencies" | "dev-dependencies" | "build-dependencies"
                        ) {
                            if let Some(table) = value.as_table() {
                                for (alias, dependency) in table {
                                    deps.insert(
                                        dependency
                                            .get("package")
                                            .and_then(toml::Value::as_str)
                                            .unwrap_or(alias)
                                            .to_string(),
                                    );
                                }
                            }
                        } else if key == "target"
                            && let Some(targets) = value.as_table()
                        {
                            for target in targets.values() {
                                cargo_deps(target, deps);
                            }
                        }
                    }
                }
            }
            cargo_deps(&manifest, &mut deps);
        }
        if let Some(text) = tree.text(&owner.join("package.json")) {
            let manifest: serde_json::Value =
                serde_json::from_str(text).map_err(|e| format!("{}: {e}", tree.shown(owner)))?;
            if let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) {
                names.insert(owner.clone(), name.to_string());
            }
            for kind in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                if let Some(table) = manifest.get(kind).and_then(serde_json::Value::as_object) {
                    deps.extend(table.keys().cloned());
                }
            }
        }
        dependencies.insert(owner.clone(), deps);
    }
    let actions = link_docs_check_code::coverage::declared(tree);
    let mut roles_by_owner: BTreeMap<PathBuf, [usize; 3]> = BTreeMap::new();
    for command in &actions {
        let roles = roles_by_owner
            .entry(command.documentation_owner(tree))
            .or_default();
        roles[match command.action.role {
            link_component::actions::Role::Primary => 0,
            link_component::actions::Role::Secondary => 1,
            link_component::actions::Role::Plumbing => 2,
        }] += 1;
    }
    let _ = writeln!(
        out,
        "## Owner Declarations\n\nDependencies include declared build and test dependencies. The layer's allowed dependencies are separate from the owner's package dependencies. Fan-in counts current owners that name this package; it suggests shared explanations, rather than proving runtime use. Source folders are inspection leads. A missing declaration is shown as N/A.\n\n| Owner | Name | Layer | Layer depends on | Package depends on | Fan-in | Component | Required | Primary / Secondary / Plumbing | Rust files | Source folders |\n|---|---|---|---|---|---|---|---|---|---|---|"
    );
    let mut families: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for owner in &packages {
        let layer = owner
            .ancestors()
            .find(|dir| tree.files.contains_key(&dir.join("layer.toml")));
        let layer_name = layer
            .map(|dir| tree.shown(dir))
            .unwrap_or_else(|| "N/A".into());
        let layer_dependencies = layer
            .and_then(|dir| tree.text(&dir.join("layer.toml")))
            .map(|text| {
                let declaration: toml::Value =
                    toml::from_str(text).map_err(|e| format!("{layer_name}/layer.toml: {e}"))?;
                Ok::<_, String>(
                    declaration
                        .get("depends_on")
                        .and_then(toml::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(toml::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            })
            .transpose()?
            .unwrap_or_else(|| "N/A".into());
        let mut component = "N/A".to_string();
        let mut capabilities = "N/A".to_string();
        if let Some(text) = tree.text(&owner.join("module.toml")) {
            let source = link_component::Source::parse(text)
                .map_err(|e| format!("{}: {e:#}", tree.shown(owner)))?;
            component = source.module.id;
            capabilities =
                cell(serde_json::to_string(&source.requires).map_err(|e| e.to_string())?);
        }
        let belongs = |file: &std::path::Path| {
            packages
                .iter()
                .filter(|p| file.starts_with(p))
                .max_by_key(|p| p.as_os_str().len())
                == Some(owner)
        };
        let rust_files = tree
            .files
            .keys()
            .filter(|file| {
                file.extension().is_some_and(|e| e == "rs")
                    && file.starts_with(owner)
                    && belongs(file)
            })
            .count();
        let folders: BTreeSet<_> = tree
            .files
            .keys()
            .filter(|file| {
                file.extension().is_some_and(|e| e == "rs")
                    && file.starts_with(owner.join("src"))
                    && belongs(file)
            })
            .filter_map(|file| file.parent())
            .map(|dir| dir.strip_prefix(owner).unwrap().display().to_string())
            .collect();
        let roles = roles_by_owner.get(owner).copied().unwrap_or_default();
        let name = names.get(owner).map(String::as_str).unwrap_or("N/A");
        let fan_in = dependencies
            .iter()
            .filter(|(other, deps)| *other != owner && deps.contains(name))
            .count();
        let _ = writeln!(
            out,
            "| `{}` | `{}` | `{}` | {} | {} | {fan_in} | `{}` | {capabilities} | {} / {} / {} | {rust_files} | {} |",
            tree.shown(owner),
            cell(name),
            layer_name,
            cell(layer_dependencies),
            cell(
                dependencies[owner]
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            cell(component),
            roles[0],
            roles[1],
            roles[2],
            cell(folders.into_iter().collect::<Vec<_>>().join(", "))
        );
        let family = families.entry(layer_name).or_default();
        family.0 += 1;
        family.1 += rust_files;
        family.2 += roles.iter().sum::<usize>();
    }
    let _ = writeln!(
        out,
        "\n### Layer Rollups\n\nOwners are assigned once to their nearest layer; these totals do not add nested packages to their parents.\n\n| Layer | Owners | Rust files | Declared actions |\n|---|---|---|---|"
    );
    for (layer, (owners, files, actions)) in families {
        let _ = writeln!(out, "| `{layer}` | {owners} | {files} | {actions} |");
    }
    Ok(())
}

/// Include every owned page and H1–H6 heading, retaining source order within
/// each page and marking generated references rather than treating them as prose.
pub fn outline(tree: &Tree, out: &mut String) {
    let _ = writeln!(
        out,
        "\n## Reading Outline\n\nEach page's address and complete authored headings follow. Generated reference pages are identified separately.\n"
    );
    let addresses = structure::addresses(tree);
    let navigation = navigation::Navigation::new(tree);
    let mut pages: Vec<_> = navigation
        .documents()
        .filter(|file| {
            navigation::translation(file).is_none()
                && !file
                    .components()
                    .any(|part| matches!(part.as_os_str().to_str(), Some("fixtures" | "prompts")))
        })
        .cloned()
        .collect();
    pages.sort_by_key(|file| {
        addresses
            .get(file)
            .cloned()
            .unwrap_or_else(|| format!("~{}", tree.shown(file)))
    });
    for file in pages {
        let address = addresses
            .get(&file)
            .map(String::as_str)
            .unwrap_or("no website address");
        let text = tree.text(&file).unwrap_or_default();
        let _ = writeln!(
            out,
            "- `{address}` — `{}`{}",
            tree.shown(&file),
            if link_docs_check::generated_reference(text) {
                " (generated reference)"
            } else {
                ""
            }
        );
        if !link_docs_check::generated_reference(text) {
            let authored = navigation::explanations(text);
            let prose = md::split(&authored).0;
            for (level, heading) in md::headings(&prose) {
                let _ = writeln!(out, "  {}- H{level}: {}", "  ".repeat(level - 1), heading);
            }
        }
    }
}
