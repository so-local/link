//! Writes the fixture's command line from its declarations (`declared/`):
//! the commands of an account that hosts agents, whose shapes are the
//! agent place contract's.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write_with(
        &manifest.join("declared"),
        &[
            link_actions_build::Owner {
                directory: &manifest.join("../../../modules/agents/agent/contract/place"),
                rust: "::link_agent_place::declared",
            },
            // The account the contract's requests carry.
            link_actions_build::Owner {
                directory: &manifest.join("../../../modules/cloud/cloud/contract"),
                rust: "::link_cloud_contract::declared",
            },
        ],
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )
}
