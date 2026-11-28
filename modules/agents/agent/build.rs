//! Embeds the module's declarations, and writes its command line, the
//! handler trait and dispatch from them, over the shapes its spec
//! contract generates.

use std::path::{Path, PathBuf};

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    first_agent(&manifest, &out)?;
    // The sandbox's spec, which `permission add` and `remove` bind.
    let sandbox = manifest.join("../../security/sandbox");
    link_actions_build::write_with(
        &manifest,
        &[
            link_actions_build::Owner {
                directory: &sandbox,
                rust: "::link_sandbox_contract::spec",
            },
            // The shapes the agent's spec holds that their owners declare.
            link_actions_build::Owner {
                directory: &manifest.join("..").join("stores/contract"),
                rust: "::link_store_contract::declared",
            },
            link_actions_build::Owner {
                directory: &manifest
                    .join("..")
                    .join("harnesses/portability/contract/wire"),
                rust: "::link_harness_wire::declared",
            },
            link_actions_build::Owner {
                directory: &manifest
                    .join("..")
                    .join("harnesses/portability/agent-state-protocol"),
                rust: "::link_agent_state_protocol::declared",
            },
            // The modules whose answers the forwards relay, as theirs.
            link_actions_build::Owner {
                directory: &manifest.join("..").join("model"),
                rust: "::link_model_contract::declared",
            },
            link_actions_build::Owner {
                directory: &manifest.join("..").join("tools/web-tool"),
                rust: "::link_web_tool_contract",
            },
            link_actions_build::Owner {
                directory: &manifest.join("..").join("tools/memory-tool/contract"),
                rust: "::link_memory_tool_contract::declared",
            },
            link_actions_build::Owner {
                directory: &manifest.join("..").join("tools/scheduler-tool"),
                rust: "::link_scheduler_tool_contract::cli",
            },
            link_actions_build::Owner {
                directory: &manifest.join("../../../packages/interactions/contract"),
                rust: "::link_interactions_contract::declared",
            },
            link_actions_build::Owner {
                directory: &manifest.join("..").join("connections/contract/policy"),
                rust: "::link_connection_policy::declared",
            },
        ],
        &out.join("declared.rs"),
        link_actions_build::Parts::ProgramOver("::link_agent_spec::declared"),
    )
}

/// The first agent's archive folder, which `first_agent.toml` names,
/// carried into the module as `first_agent.rs`: its name, and each file
/// by its path in the folder, so a first start needs no network.
fn first_agent(manifest: &Path, out: &Path) -> anyhow::Result<()> {
    let declared = manifest.join("first_agent.toml");
    println!("cargo:rerun-if-changed={}", declared.display());
    let table: toml::Table = std::fs::read_to_string(&declared)?.parse()?;
    let field = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{} needs {key}", declared.display()))
    };
    let name = field("name")?;
    let folder = manifest
        .join(field("folder")?)
        .canonicalize()
        .with_context(|| format!("the folder {} names", declared.display()))?;
    let mut files = Vec::new();
    walk(&folder, &folder, &mut files)?;
    let mut rust = format!(
        "/// The first agent's name in its archive.\npub const NAME: &str = {name:?};\n\
         /// Its archive folder's files, by their paths in it.\n\
         pub const FILES: &[(&str, &[u8])] = &[\n"
    );
    for (path, absolute) in files {
        rust.push_str(&format!("    ({path:?}, include_bytes!({absolute:?})),\n"));
    }
    rust.push_str("];\n");
    std::fs::write(out.join("first_agent.rs"), rust)?;
    Ok(())
}

/// Each file below `dir`, by its path from `root` with `/` between its
/// parts, in order.
fn walk(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, files)?;
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let relative = path
            .strip_prefix(root)?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        files.push((relative, path.display().to_string()));
    }
    Ok(())
}
