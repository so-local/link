//! What a release ships: every member but those `[package.metadata.link]`
//! marks `released = false`. No released crate builds with one that isn't,
//! and the website's index marks the docs of each one held back. `lnk
//! bundle` leaves them out of a release (its `discover::default_programs`).

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The workspace's packages, as `cargo metadata` describes them.
fn packages() -> Vec<Value> {
    let output = link_test_kit::cargo()
        .current_dir(root())
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    metadata["packages"].as_array().unwrap().clone()
}

/// The packages held back from a release, by name; a mark that isn't
/// `false` is refused.
fn held(packages: &[Value]) -> BTreeSet<String> {
    packages
        .iter()
        .filter(|package| {
            let name = &package["name"];
            !link_component::release::released(package["metadata"].get("link"))
                .unwrap_or_else(|error| panic!("{name}: {error}"))
        })
        .map(|package| package["name"].as_str().unwrap().to_string())
        .collect()
}

/// Each normal or build dependency of a released package on a held one.
/// A test may use one: dev-dependencies and the fixtures declared beneath
/// `tests/` are never shipped.
fn refusals(packages: &[Value], workspace: &Path) -> Vec<String> {
    let held = held(packages);
    let mut refused = vec![];
    for package in packages {
        let name = package["name"].as_str().unwrap();
        let fixture = package["metadata"]["link"]["role"] == "fixture"
            && package["manifest_path"]
                .as_str()
                .is_some_and(|manifest| Path::new(manifest).starts_with(workspace.join("tests")));
        if held.contains(name) || fixture {
            continue;
        }
        for dependency in package["dependencies"].as_array().unwrap() {
            let on = dependency["name"].as_str().unwrap();
            if dependency["kind"] != "dev" && held.contains(on) {
                refused.push(format!(
                    "released {name} depends on {on}, which isn't released"
                ));
            }
        }
    }
    refused
}

#[test]
fn no_released_crate_depends_on_one_that_isnt() {
    let refused = refusals(&packages(), &root().canonicalize().unwrap());
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

#[test]
fn a_held_dependency_is_refused_but_a_held_test_dependency_is_not() {
    let package = |name: &str, released: Option<bool>, dependencies: Value| {
        let link = match released {
            Some(released) => json!({"released": released}),
            None => json!({}),
        };
        json!({
            "name": name,
            "manifest_path": format!("/w/modules/{name}/Cargo.toml"),
            "metadata": {"link": link},
            "dependencies": dependencies,
        })
    };
    let gadget = package("link-gadget", Some(false), json!([]));
    for kind in [Value::Null, json!("build")] {
        let agent = package(
            "link-agent",
            None,
            json!([{"name": "link-gadget", "kind": kind}]),
        );
        assert_eq!(
            refusals(&[agent, gadget.clone()], Path::new("/w")),
            ["released link-agent depends on link-gadget, which isn't released"]
        );
    }
    let tested = package(
        "link-agent",
        None,
        json!([{"name": "link-gadget", "kind": "dev"}]),
    );
    let held_on_held = package(
        "link-widget",
        Some(false),
        json!([{"name": "link-gadget", "kind": null}]),
    );
    let mut fixture = package(
        "link-gadget-fixture",
        None,
        json!([{"name": "link-gadget", "kind": null}]),
    );
    fixture["metadata"]["link"]["role"] = "fixture".into();
    fixture["manifest_path"] = "/w/tests/fixtures/gadget/Cargo.toml".into();
    let workspace = Path::new("/w");
    assert!(
        refusals(
            &[tested, held_on_held, fixture.clone(), gadget.clone()],
            workspace
        )
        .is_empty()
    );
    fixture["manifest_path"] = "/w/modules/gadget/fixture/Cargo.toml".into();
    assert_eq!(refusals(&[fixture, gadget], workspace).len(), 1);
}

#[test]
#[should_panic(expected = "link-gadget")]
fn a_mark_that_isnt_false_is_refused() {
    held(&[json!({"name": "link-gadget", "metadata": {"link": {"released": true}}})]);
}

/// Whether the docs root at `directory`, owned by `owner`, are held back:
/// the crate at either is.
fn held_root(held: &BTreeSet<String>, directories: [&Path; 2]) -> bool {
    directories.iter().any(|directory| {
        let manifest = directory.join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            return false;
        };
        let manifest: toml::Value = toml::from_str(&text).unwrap();
        manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .is_some_and(|name| held.contains(name))
    })
}

/// The website leaves out a root marked `"released": false` in
/// `scripts/documentation-index.json`, and the mark follows the crate's.
#[test]
fn the_documentation_index_marks_each_held_root() {
    let root = root();
    let held = held(&packages());
    let index: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scripts/documentation-index.json")).unwrap(),
    )
    .unwrap();
    let mut wrong = vec![];
    for entry in index["roots"].as_array().unwrap() {
        let directory = entry["directory"].as_str().unwrap();
        let owner = entry["owner"].as_str().unwrap_or(directory);
        let marked = match entry.get("released") {
            None => true,
            Some(Value::Bool(false)) => false,
            Some(other) => panic!("{directory}: \"released\" is false or absent, not {other}"),
        };
        let released = !held_root(&held, [&root.join(directory), &root.join(owner)]);
        if marked != released {
            wrong.push(format!(
                "{directory}: its crate is {}released, so its entry {}",
                if released { "" } else { "not " },
                if released {
                    "has no \"released\""
                } else {
                    "says \"released\": false"
                }
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every public member ships. The registry includes every declared
/// component; the docs also include standalone public programs.
#[test]
fn a_release_ships_every_public_module() {
    let root = root().canonicalize().unwrap();
    let packages = packages();
    let held = held(&packages);
    assert!(held.is_empty(), "every public member ships; held: {held:?}");
    let modules: std::collections::BTreeMap<PathBuf, String> = packages
        .iter()
        .filter_map(|package| {
            // A module owns one program, `lnk-<id>`; a fixture owns none here.
            if package["metadata"]["link"]["role"] == "fixture" {
                return None;
            }
            let id = package["targets"]
                .as_array()?
                .iter()
                .find_map(|t| t["name"].as_str()?.strip_prefix("lnk-"))?;
            let manifest = Path::new(package["manifest_path"].as_str()?);
            Some((manifest.parent()?.to_path_buf(), id.to_string()))
        })
        .collect();
    let listed: BTreeSet<&str> = modules.values().map(String::as_str).collect();
    assert!(!listed.is_empty(), "Cargo declares public modules");
    let components: BTreeSet<&str> = modules
        .iter()
        .filter(|(directory, _)| directory.join("module.toml").is_file())
        .map(|(_, id)| id.as_str())
        .collect();
    let registry = link_component_build::registry(&root, &[], true).unwrap();
    assert_eq!(
        registry.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        components,
        "the registry"
    );

    // Each module by its folder; the website shows one whose folder the
    // index lists unmarked.
    let index: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scripts/documentation-index.json")).unwrap(),
    )
    .unwrap();
    let shown: BTreeSet<&str> = index["roots"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry.get("released").is_none())
        .filter_map(|entry| modules.get(&root.join(entry["directory"].as_str()?)))
        .map(String::as_str)
        .collect();
    assert_eq!(shown, listed, "the website");
}
