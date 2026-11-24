//! Embeds the adapter's declarations, and writes its command line from
//! them: its own `connect` and the cloud contract's protocol it answers,
//! over the contract crate's shapes.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let contract = manifest.join("../../../modules/cloud/cloud/contract");
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
