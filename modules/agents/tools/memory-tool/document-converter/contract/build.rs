//! Writes the converter's shapes, its runtime arguments and the command
//! line that gives them from the module's declarations, beside this crate.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest.parent().expect("the module's folder");
    link_actions_build::write(
        module,
        &out.join("args.rs"),
        link_actions_build::Parts::Args,
    )
}
