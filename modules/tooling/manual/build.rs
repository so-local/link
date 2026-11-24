//! Embeds the module's declarations and its pages, and writes its command
//! line and output types from the declarations.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    link_component_build::manual();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )
}
