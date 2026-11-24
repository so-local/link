//! Writes the fixture's command line from its declarations (`declared/`):
//! the connection protocol's commands, whose shapes are the contract's.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write_with(
        &manifest.join("declared"),
        &[link_actions_build::Owner {
            directory: &manifest.join("../../../modules/agents/connections/contract"),
            rust: "::link_connection_contract::declared",
        }],
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )
}
