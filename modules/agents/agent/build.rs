//! Embeds the module's declarations, and writes its command line, the
//! handler trait and dispatch from them, over the shapes its spec
//! contract generates.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
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
