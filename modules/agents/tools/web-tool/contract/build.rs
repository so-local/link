//! Writes Link Web's shapes from the module's declarations, beside this
//! crate: what its program answers and what other modules name, and the
//! runtime arguments of its command lines, each with its `argv`.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest.parent().expect("the tool's folder");
    link_actions_build::write(
        module,
        &out.join("declared.rs"),
        link_actions_build::Parts::Args,
    )?;
    // The calls a model makes of the tool, as its program and lnk agent
    // read them.
    link_actions_build::write_tools(module, &out.join("tools.json"))
}
