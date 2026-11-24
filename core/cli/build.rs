//! Embeds the core's registry and documentation, and writes its command
//! line from its declarations.

fn main() -> anyhow::Result<()> {
    link_component_build::core();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )
}
