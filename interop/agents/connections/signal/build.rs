//! Embeds the connection's declarations, and writes its command line from
//! them (the connection protocol's commands, whose shapes are the
//! contract's), and the pins of the programs it installs.

fn main() -> anyhow::Result<()> {
    link_component_build::embed();
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    link_actions_build::write_with(
        &manifest,
        &[link_actions_build::Owner {
            directory: &manifest.join("../../../../modules/agents/connections/contract"),
            rust: "::link_connection_contract::declared",
        }],
        &out.join("declared.rs"),
        link_actions_build::Parts::Program,
    )?;
    let pins: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("externals.json").unwrap()).unwrap();
    let pinned = |p: &serde_json::Value| {
        format!(
            "Pinned {{url:{:?},sha256:{:?}}}",
            p["url"].as_str().unwrap(),
            p["sha256"].as_str().unwrap()
        )
    };
    let mut code = format!(
        "pub const VERSION:&str={:?};\nconst NATIVE:Pinned={};\nconst JAVA_BUILD:Pinned={};\nconst JAVA:&[(&str,&str,Pinned)]=&[\n",
        pins["signal"]["version"].as_str().unwrap(),
        pinned(&pins["signal"]["platforms"]["linux-x86_64"]),
        pinned(&pins["signal"]["platforms"]["java"])
    );
    for (platform, pin) in pins["jre"]["platforms"].as_object().unwrap() {
        let (os, arch) = platform.split_once('-').unwrap();
        code += &format!("({os:?},{arch:?},{}),\n", pinned(pin));
    }
    code += &format!(
        "];\nconst JAVA_HOME_MAC:&str={:?};\nconst JAVA_HOME_LINUX:&str={:?};\n",
        pins["jre"]["platforms"]["macos-aarch64"]["home"]
            .as_str()
            .unwrap(),
        pins["jre"]["platforms"]["linux-aarch64"]["home"]
            .as_str()
            .unwrap()
    );
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("installer-pins.rs"),
        code,
    )
    .unwrap();
    Ok(())
}
