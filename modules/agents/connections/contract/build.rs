//! Writes the connection protocol's shapes from its declarations, the
//! types every connection's generated program names, and each command's
//! arguments with the command line that gives them, which `lnk agent`
//! builds.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::Args,
    )
}
