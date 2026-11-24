//! Guards the workspace layout described in docs/architecture/README.md: which internal crates
//! each crate may depend on. Fails when a new dependency crosses a boundary.

use std::{fs, path::Path};

/// Only explicitly declared fixtures beneath tests may link implementations
/// to inspect their parsers and contracts. Production packages retain the seam.
fn test_fixture(package: &serde_json::Value, workspace: &Path) -> bool {
    package["metadata"]["link"]["role"] == "fixture"
        && package["manifest_path"]
            .as_str()
            .is_some_and(|manifest| Path::new(manifest).starts_with(workspace.join("tests")))
}

/// The policy contract remains useful without any of the programs or
/// implementations that consume it, under every optional feature.
#[test]
fn the_sandbox_contract_has_no_path_back_to_an_implementation() {
    let output = link_test_kit::cargo()
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
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
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let mut next = vec!["link-sandbox-contract".to_string()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(name) = next.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let package = packages.iter().find(|p| p["name"] == name).unwrap();
        assert!(
            !matches!(
                name.as_str(),
                "link-sandbox-confine" | "link-sandbox-proxy" | "link-e2e" | "link-cli"
            ),
            "contract reaches {name}"
        );
        assert!(
            !package["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["kind"].as_array().unwrap().iter().any(|k| k == "bin")),
            "contract reaches program {name}"
        );
        for dep in package["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"] != "dev")
        {
            let dep = dep["name"].as_str().unwrap();
            if packages.iter().any(|p| p["name"] == dep) {
                next.push(dep.into());
            }
        }
    }
    for package in packages {
        let name = package["name"].as_str().unwrap();
        for dep in package["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"] != "dev")
        {
            if matches!(
                dep["name"].as_str(),
                Some("link-sandbox-confine" | "link-sandbox-proxy" | "link-sandbox")
            ) {
                assert!(
                    name == "link-sandbox"
                        || test_fixture(
                            package,
                            Path::new(metadata["workspace_root"].as_str().unwrap())
                        ),
                    "{name} links sandbox implementation {}",
                    dep["name"]
                );
            }
        }
    }
}

#[test]
fn a_production_package_cannot_claim_the_test_fixture_exception() {
    let workspace = Path::new("/repo");
    let mut package = serde_json::json!({
        "metadata": {"link": {"role": "fixture"}},
        "manifest_path": "/repo/tests/docs/code/Cargo.toml"
    });
    assert!(test_fixture(&package, workspace));
    package["manifest_path"] = "/repo/modules/security/consumer/Cargo.toml".into();
    assert!(!test_fixture(&package, workspace));
    package["manifest_path"] = "/repo/tests/consumer/Cargo.toml".into();
    package["metadata"]["link"]["role"] = "library".into();
    assert!(!test_fixture(&package, workspace));
}

/// The document parsers, Link Document Converter's alone.
const DOCUMENT_PARSERS: [&str; 2] = ["lopdf", "zip"];

/// The workspace as `cargo metadata` describes it.
fn metadata() -> serde_json::Value {
    let output = link_test_kit::cargo()
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Every package `roots` build, themselves among them, by id.
fn reached(
    metadata: &serde_json::Value,
    roots: impl IntoIterator<Item = String>,
) -> std::collections::BTreeSet<String> {
    let mut reached: std::collections::BTreeSet<String> = roots.into_iter().collect();
    let mut pending: Vec<_> = reached.iter().cloned().collect();
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    while let Some(id) = pending.pop() {
        let node = nodes.iter().find(|node| node["id"] == id).unwrap();
        for dependency in node["deps"].as_array().unwrap() {
            let id = dependency["pkg"].as_str().unwrap().to_string();
            if reached.insert(id.clone()) {
                pending.push(id);
            }
        }
    }
    reached
}

/// Workspace consolidation must preserve the core's compilation boundary,
/// including transitive dependencies added by generated feature unions.
#[test]
fn the_default_build_reaches_no_connection_or_document_parser() {
    let metadata = metadata();
    let defaults = metadata["workspace_default_members"].as_array().unwrap();
    let reached = reached(
        &metadata,
        defaults.iter().map(|id| id.as_str().unwrap().to_string()),
    );
    let workspace = Path::new(metadata["workspace_root"].as_str().unwrap());
    let connections = ["modules/agents/connections", "interop/agents/connections"];
    for package in metadata["packages"].as_array().unwrap() {
        let id = package["id"].as_str().unwrap();
        let name = package["name"].as_str().unwrap();
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        if defaults.contains(&package["id"]) {
            assert!(
                !connections
                    .iter()
                    .any(|c| manifest.starts_with(workspace.join(c)))
                    && !manifest.starts_with(workspace.join("modules/agents/harnesses/link"))
                    && !manifest.starts_with(workspace.join("modules/agents/tools")),
                "{name} is an unexpected default member"
            );
        }
        if connections
            .iter()
            .any(|c| manifest.starts_with(workspace.join(c)))
            || DOCUMENT_PARSERS.contains(&name)
        {
            assert!(!reached.contains(id), "the core builds {name}");
        }
    }
}

/// Link Memory reads PDFs and Word documents through Link Document Converter, a
/// module of its own: memory's build compiles no document parser.
#[test]
fn memory_builds_no_document_parser() {
    let metadata = metadata();
    let packages = metadata["packages"].as_array().unwrap();
    let id_of = |name: &str| {
        packages
            .iter()
            .find(|p| p["name"] == name)
            .map(|p| p["id"].as_str().unwrap().to_string())
            .unwrap_or_else(|| panic!("no package {name}"))
    };
    let reached = reached(&metadata, [id_of("link-memory-tool")]);
    for parser in DOCUMENT_PARSERS {
        assert!(!reached.contains(&id_of(parser)), "memory builds {parser}");
    }
    let documents = reached_names(&metadata, &id_of("link-document-converter"));
    assert!(documents.contains("lopdf"), "{documents:?}");
}

/// The names of the packages `id` builds.
fn reached_names(metadata: &serde_json::Value, id: &str) -> std::collections::BTreeSet<String> {
    let ids = reached(metadata, [id.to_string()]);
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| ids.contains(p["id"].as_str().unwrap()))
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

/// The sandbox's packages keep each file under 600 lines but its tests
/// (a `tests.rs`, or the lines from its `mod tests` on): one file, one
/// part.
#[test]
fn the_sandboxs_files_stay_small() {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for package in [
        "modules/security/sandbox/src",
        "modules/security/sandbox/contract/src",
        "modules/security/sandbox/confine/src",
        "modules/security/sandbox/proxy/src",
    ] {
        rust_files(&local.join(package), &mut files);
    }
    assert!(!files.is_empty());
    let mut over = Vec::new();
    for file in files {
        if file.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        let lines = text.lines().take_while(|l| *l != "mod tests {").count();
        if lines > 600 {
            let name = file.strip_prefix(&local).unwrap().display();
            over.push(format!("{name}: {lines} lines, over 600: split it"));
        }
    }
    assert!(over.is_empty(), "{over:#?}");
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The core and Link's modules name no other project's harness but where
/// Link lists what it comes with: each adapter is a crate of its own, outside
/// this workspace, riding the harness contract, and what's specific to it
/// is its adapter's to say. Link's release builds and includes some of
/// them, so the core's `MODULES` and the harness module's `KNOWN` list
/// them, with the core's help (the component's command declarations) and its tests. The
/// other names left are for what Link's releases left installed: the
/// default harness of settings saved before Link Harness was, which those
/// settings still mean, and the adapters that kept their keys on the
/// wire before they could say so.
#[test]
fn no_code_names_a_harness_from_outside_link() {
    // Where Link lists the harnesses it comes with.
    let lists = [
        Path::new("core/cli/src/main.rs"),
        Path::new("core/cli/Cargo.toml"),
        Path::new("modules/agents/agent/src/settings.rs"),
    ];
    // Spelled apart, so this file doesn't name them either.
    let names = [
        ["open", "claw"].concat(),
        ["her", "mes"].concat(),
        ["lnk-", "deepseek"].concat(),
        ["deepseek", " harness"].concat(),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    let mut stack = vec![root.join("core")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                if ["target", "node_modules", ".git", ".next", ".turbo"]
                    .iter()
                    .any(|name| path.ends_with(name))
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if lists.iter().any(|list| path.ends_with(list)) {
                continue;
            }
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_lowercase();
                if names.iter().any(|name| lower.contains(name.as_str())) {
                    found.push(format!("{}:{}: {line}", path.display(), n + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "named under src/:\n{}", found.join("\n"));
}

/// One HMAC, `link_secretive`'s: no other crate under `src/` signs or checks
/// one, or decodes a signature's hex by hand, so every receiver's
/// signature is checked once, the same way. Exempt: `box`'s match of an
/// OpenSSH `known_hosts` line, HMAC-SHA1 by OpenSSH's own format, which
/// is no signature Link checks.
#[test]
fn only_link_secret_does_hmac() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exempt = [
        Path::new("packages/secretive/src"),
        Path::new("modules/compute/box/src/ssh/hosts.rs"),
    ];
    // Spelled apart, so this file doesn't match itself.
    let marks = [
        ["ring::", "hmac"].concat(),
        ["use ", "hmac"].concat(),
        ["Hmac", "<"].concat(),
        ["fn ", "unhex"].concat(),
    ];
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let within = path.strip_prefix(&root).unwrap_or(&path);
            if exempt.iter().any(|e| within.starts_with(e)) {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                if ["target", "node_modules", ".git", ".next", ".turbo"]
                    .iter()
                    .any(|name| path.ends_with(name))
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (n, line) in text.lines().enumerate() {
                if marks.iter().any(|mark| line.contains(mark.as_str())) {
                    found.push(format!("{}:{}: {line}", within.display(), n + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "an HMAC outside link_secretive (use link_secretive::hmac):\n{}",
        found.join("\n")
    );
}

/// What reaches the network or runs a harness.
const NETWORK: [&str; 12] = [
    "reqwest",
    "hyper",
    "hyper-util",
    "h2",
    "tokio",
    "mio",
    "rustls",
    "socket2",
    "link-fetch",
    "link-api",
    "link-harness-contract",
    "link-sandbox-client",
];

/// The names of the packages `id` links: its normal dependencies, theirs
/// and so on, never what only its tests or build use.
fn linked_names(metadata: &serde_json::Value, id: &str) -> std::collections::BTreeSet<String> {
    let packages = metadata["packages"].as_array().unwrap();
    let name_of = |id: &str| {
        packages.iter().find(|p| p["id"] == id).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let mut seen = std::collections::BTreeSet::from([id.to_string()]);
    let mut pending = vec![id.to_string()];
    while let Some(id) = pending.pop() {
        for (_, dep) in direct_of(metadata, &id) {
            if seen.insert(dep.clone()) {
                pending.push(dep);
            }
        }
    }
    seen.iter().map(|id| name_of(id)).collect()
}

/// The direct dependencies of package `name` of the normal kind, by name
/// and id.
fn direct(metadata: &serde_json::Value, name: &str) -> Vec<(String, String)> {
    let packages = metadata["packages"].as_array().unwrap();
    let id = packages
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no package {name}"))["id"]
        .as_str()
        .unwrap()
        .to_string();
    direct_of(metadata, &id)
}

/// The direct dependencies of package `id` of the normal kind, by name
/// and id.
fn direct_of(metadata: &serde_json::Value, id: &str) -> Vec<(String, String)> {
    let packages = metadata["packages"].as_array().unwrap();
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    let node = nodes.iter().find(|node| node["id"] == id).unwrap();
    node["deps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dep| {
            dep["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind["kind"].is_null())
        })
        .map(|dep| {
            let id = dep["pkg"].as_str().unwrap().to_string();
            let name = packages.iter().find(|p| p["id"] == id.as_str()).unwrap()["name"]
                .as_str()
                .unwrap()
                .to_string();
            (name, id)
        })
        .collect()
}

/// `lnk archive` reads and writes files alone. The archive package
/// reaches no network or harness crate at all. The module and its
/// contract link only what's listed: the module protocol (`link-core`),
/// which every module links and whose install and update `lnk archive`
/// never calls, and packages that reach no network themselves.
#[test]
fn an_archive_reads_and_writes_files_alone() {
    let metadata = metadata();
    let reaches_network = |id: &str| {
        let names = linked_names(&metadata, id);
        NETWORK
            .iter()
            .filter(|n| names.contains(**n))
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
    };
    let id_of = |name: &str| {
        metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .map(|p| p["id"].as_str().unwrap().to_string())
            .unwrap()
    };
    assert_eq!(
        reaches_network(&id_of("link-archive")),
        Vec::<String>::new(),
        "the archive package reaches the network"
    );
    let allowed = [
        "anyhow",
        "clap",
        "serde",
        // Its command line and shapes are generated from its
        // declarations, which link-actions reads and link-types holds;
        // the check below still proves neither reaches the network.
        "link-actions",
        "link-types",
        // The generated command line reads and writes its JSON.
        "serde_json",
        "link-archive",
        "link-archive-contract",
        "link-core",
        "link-files",
        "link-format",
        "link-secretive",
        "link-settings",
        "link-spec",
    ];
    // Nor does its own code reach the network or start a program: its
    // owners' answers are started by the module protocol's `Answerer`.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for folder in [
        "packages/archive/src",
        "modules/tooling/archive/src",
        "modules/tooling/archive/contract/src",
    ] {
        let mut files = Vec::new();
        rust_files(&root.join(folder), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            for word in ["std::net", "process::Command", "Command::new", "tokio"] {
                assert!(
                    !text.contains(word),
                    "{} names {word}: an archive reads and writes files alone",
                    file.display()
                );
            }
        }
    }
    for crate_name in ["link-archive-module", "link-archive-contract"] {
        for (name, id) in direct(&metadata, crate_name) {
            assert!(
                allowed.contains(&name.as_str()),
                "{crate_name} links {name}, which isn't on the list a files-only archive may link"
            );
            if name != "link-core" && name != "link-archive-contract" {
                assert_eq!(
                    reaches_network(&id),
                    Vec::<String>::new(),
                    "{crate_name} links {name}, which reaches the network"
                );
            }
        }
    }
}
