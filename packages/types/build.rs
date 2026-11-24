//! Writes core's shared shapes from core's declarations, which
//! `link-actions` keeps.

fn main() -> anyhow::Result<()> {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        link_actions::core_directory(),
        &out.join("core.rs"),
        link_actions_build::Parts::Shared,
    )
}
