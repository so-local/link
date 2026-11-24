//! Writes the memory protocol's shapes from its declarations, the types
//! every engine's generated program names, and each command's arguments
//! with the command line that gives them, which `lnk agent` builds.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::Args,
    )?;
    // Link Memory's own commands' arguments, with the command line that
    // gives them: a machine builds its `bridge` line from them.
    let module = manifest.parent().expect("the tool's folder");
    link_actions_build::write_with(
        module,
        &[link_actions_build::Owner {
            directory: &manifest,
            rust: "crate::declared",
        }],
        &out.join("tool.rs"),
        link_actions_build::Parts::Args,
    )?;
    // The calls a model makes of the memory tool, beside this contract,
    // as its program and lnk agent read them.
    link_actions_build::write_tools_with(module, &[&manifest], &out.join("tools.json"))
}
