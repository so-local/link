//! `lnk-openclaw` passes the conformance kit (`link_harness_conformance`),
//! with the mini harness's ACP agent standing in for `openclaw acp` where
//! OpenClaw's launcher goes, in its programs folder, so nothing is
//! downloaded. OpenClaw takes a
//! local model only through Ollama.

#![cfg(unix)]

#[test]
fn it_conforms_over_acp() {
    link_harness_conformance::Check::new(std::path::Path::new(env!("CARGO_BIN_EXE_lnk-openclaw")))
        .provider("ollama")
        .stand_in(
            &link_files::layout::Hidden::OpenclawPrograms
                .relative_to_home()
                .join("bin/openclaw")
                .display()
                .to_string(),
            &[],
        )
        .tools_in_settings()
        .without_native_state()
        .run();
}
