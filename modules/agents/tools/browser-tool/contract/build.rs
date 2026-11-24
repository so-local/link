//! Writes the browser tool's runtime arguments and the command line that
//! gives them from the tool's declarations, beside this crate.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest.parent().expect("the tool's folder");
    link_actions_build::write(
        module,
        &out.join("args.rs"),
        link_actions_build::Parts::Args,
    )?;
    // The calls a model makes of the tool, as its program and lnk agent
    // read them.
    link_actions_build::write_tools(module, &out.join("tools.json"))
}
