//! Writes the sandbox module's shapes, its spec among them, from its
//! declarations (`../types.toml`): the contract owns them, and the
//! module's program names them.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("the contract is in its module's folder"))?;
    link_actions_build::write(
        module,
        &out.join("declared.rs"),
        link_actions_build::Parts::Args,
    )
}
