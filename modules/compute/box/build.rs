//! Embeds the module's declarations, and writes its command line, its
//! spec and output types from them.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    // The box spec names the cloud contract's attachments and deadline.
    let contract = manifest.join("../../cloud/cloud/contract");
    link_actions_build::write_with(
        &manifest,
        &[link_actions_build::Owner {
            directory: &contract,
            rust: "::link_cloud_contract::declared",
        }],
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )
}
