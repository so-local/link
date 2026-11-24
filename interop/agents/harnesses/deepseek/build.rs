//! Embeds the harness's declarations, and writes its command line from
//! them: the harness protocol's commands, whose shapes are the contract's.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    // Its command line, from its declarations: the harness protocol's
    // commands, whose shapes are the contract's.
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write_with(
        &manifest,
        &[link_actions_build::Owner {
            directory: &manifest.join("../../../../modules/agents/harnesses/portability/contract"),
            rust: "::link_harness_contract::declared",
        }],
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )?;
    let pins: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("externals.json").unwrap()).unwrap();
    let mut code = format!(
        "const VERSION: &str = {:?};\nconst NODE: &str = {:?};\nconst NODE_ARCHIVES: &[(&str,&str)] = &[\n",
        pins["deepseek"]["version"].as_str().unwrap(),
        pins["node"]["version"].as_str().unwrap()
    );
    for (platform, pin) in pins["node"]["platforms"].as_object().unwrap() {
        code += &format!(
            "({:?},{:?}),\n",
            platform
                .replace("macos-", "darwin-")
                .replace("-aarch64", "-arm64")
                .replace("-x86_64", "-x64"),
            pin["sha256"].as_str().unwrap()
        );
    }
    code += "];\n";
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("installer-pins.rs"),
        code,
    )
    .unwrap();
    Ok(())
}
