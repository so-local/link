//! `lnk-deepseek` passes the conformance kit (`link_harness_conformance`),
//! with the mini harness's ACP agent standing in for `dsh`: Node's place
//! in its programs folder holds the stand-in, and `dsh`'s files say the
//! pinned release is installed, so nothing is downloaded.

#![cfg(unix)]

#[test]
fn it_conforms_over_acp() {
    let pins: serde_json::Value =
        serde_json::from_str(include_str!("../../externals.json")).unwrap();
    let (node, dsh) = (
        pins["node"]["version"].as_str().unwrap(),
        pins["deepseek"]["version"].as_str().unwrap(),
    );
    // Node and `dsh` where Link installs them, its programs folder.
    let programs = link_files::layout::Hidden::DeepseekPrograms.relative_to_home();
    let programs = programs.display();
    let package = format!("{programs}/dsh/lib/node_modules/@deepseek-ai/dsh");
    let manifest = serde_json::json!({ "version": dsh }).to_string();
    link_harness_conformance::Check::new(std::path::Path::new(env!("CARGO_BIN_EXE_lnk-deepseek")))
        .stand_in(
            &format!("{programs}/node-v{node}/bin/node"),
            &[
                (&format!("{package}/package.json"), &manifest),
                (&format!("{package}/lib/bin.js"), ""),
            ],
        )
        .tools_in_settings()
        .without_native_state()
        .run();
}
