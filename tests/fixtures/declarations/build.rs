//! Writes the example module's generated code from its declarations: the
//! whole program, and its types and its handler alone, as a contract
//! crate and a served module would, and the program again over the
//! arguments a contract crate generated.

fn main() -> anyhow::Result<()> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let module = manifest.join("module");
    for (file, parts) in [
        ("module.rs", link_actions_build::Parts::Program),
        ("types.rs", link_actions_build::Parts::Types),
        ("handler.rs", link_actions_build::Parts::Handler),
        ("args.rs", link_actions_build::Parts::Args),
        (
            "over.rs",
            link_actions_build::Parts::ProgramOverArgs("crate::args"),
        ),
    ] {
        link_actions_build::write(&module, &out.join(file), parts)?;
    }
    // A contract's shapes, and a store implementing its protocol over them.
    let contract = manifest.join("contract");
    link_actions_build::write(
        &contract,
        &out.join("contract.rs"),
        link_actions_build::Parts::Types,
    )?;
    link_actions_build::write_with(
        &manifest.join("file-store"),
        &[link_actions_build::Owner {
            directory: &contract,
            rust: "crate::contract",
        }],
        &out.join("file_store.rs"),
        link_actions_build::Parts::Program,
    )?;
    // Core's own program, `lnk`, beside the shapes every module shares.
    link_actions_build::write(
        &manifest.join("core"),
        &out.join("core.rs"),
        link_actions_build::Parts::Program,
    )?;
    Ok(())
}
