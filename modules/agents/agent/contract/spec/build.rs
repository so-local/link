//! Writes the agent module's shapes, its agent and team spec kinds and
//! its outputs among them, from its declarations (`types.toml`).

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest.join("../..");
    // The sandbox's spec, which `permission add` and `remove` bind.
    let sandbox = module.join("../../security/sandbox");
    link_actions_build::write_with(
        &module,
        &[
            link_actions_build::Owner {
                directory: &sandbox,
                rust: "::link_sandbox_contract::spec",
            },
            // The shapes the agent's spec holds that their owners declare.
            link_actions_build::Owner {
                directory: &module.join("..").join("stores/contract"),
                rust: "::link_store_contract::declared",
            },
            link_actions_build::Owner {
                directory: &module
                    .join("..")
                    .join("harnesses/portability/contract/wire"),
                rust: "::link_harness_wire::declared",
            },
            link_actions_build::Owner {
                directory: &module
                    .join("..")
                    .join("harnesses/portability/agent-state-protocol"),
                rust: "::link_agent_state_protocol::declared",
            },
            // The modules whose answers the forwards relay, as theirs.
            link_actions_build::Owner {
                directory: &module.join("..").join("model"),
                rust: "::link_model_contract::declared",
            },
            link_actions_build::Owner {
                directory: &module.join("..").join("tools/web-tool"),
                rust: "::link_web_tool_contract",
            },
            link_actions_build::Owner {
                directory: &module.join("..").join("tools/memory-tool/contract"),
                rust: "::link_memory_tool_contract::declared",
            },
            link_actions_build::Owner {
                directory: &module.join("..").join("tools/scheduler-tool"),
                rust: "::link_scheduler_tool_contract::cli",
            },
            link_actions_build::Owner {
                directory: &module.join("../../../packages/interactions/contract"),
                rust: "::link_interactions_contract::declared",
            },
            link_actions_build::Owner {
                directory: &module.join("..").join("connections/contract/policy"),
                rust: "::link_connection_policy::declared",
            },
        ],
        &out.join("declared.rs"),
        link_actions_build::Parts::Types,
    )
}
