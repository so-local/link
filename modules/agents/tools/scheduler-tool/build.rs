//! Embeds the module's declarations, and writes its command line over the
//! shapes, flag group and runtime arguments its contract generates.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write(
        &manifest,
        &out.join("declared.rs"),
        link_actions_build::Parts::ProgramOverArgs("::link_scheduler_tool_contract::cli"),
    )
}
