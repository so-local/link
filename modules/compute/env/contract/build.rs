//! Writes the environment module's shapes from its declarations
//! (`../types.toml`): the environment spec, its parts and the outputs its
//! program answers, which the program and every other reader share.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest.join(".."),
        &out.join("declared.rs"),
        link_actions_build::Parts::Types,
    )
}
