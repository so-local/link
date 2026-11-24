//! Writes the cloud contract's shapes from its declarations
//! (`types.toml`): the request and reply every cloud adapter speaks and
//! what they hold.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::Types,
    )
}
