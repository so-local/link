//! `lnk-hermes` passes the conformance kit (`link_harness_conformance`),
//! with the mini harness's ACP agent standing in for `hermes-acp` in
//! Hermes's sealed payload, and a stand-in for its owned gateway, so
//! nothing is downloaded.

#![cfg(unix)]

#[test]
fn it_conforms_over_acp() {
    const PROGRAMS: &str = ".lnk/exec/harnesses/hermes";
    let files = [
        (
            format!("{PROGRAMS}/manifest.json"),
            r#"{"repo":"hermes-agent","venv":"installs/0000000000000000/environments/selected","store":"tools"}"#,
        ),
        (
            format!("{PROGRAMS}/installs/0000000000000000/environments/selected/pyvenv.cfg"),
            "version = 3.11\n",
        ),
        (format!("{PROGRAMS}/tools/fixture"), ""),
        (format!("{PROGRAMS}/tools/python"), "fixture interpreter"),
        (
            format!("{PROGRAMS}/pm-runtime/pm-runtime.json"),
            r#"{"python":"../tools/python","sitePackages":"lib/python3.11/site-packages"}"#,
        ),
        (
            format!("{PROGRAMS}/pm-runtime/lib/python3.11/site-packages/fixture"),
            "",
        ),
        (
            "gateway-fixture.py".into(),
            include_str!("../../src/gateway/fixture.py"),
        ),
    ];
    let files: Vec<_> = files
        .iter()
        .map(|(path, text)| (path.as_str(), *text))
        .collect();
    link_harness_conformance::Check::new(std::path::Path::new(env!("CARGO_BIN_EXE_lnk-hermes")))
        .stand_in(
            &format!("{PROGRAMS}/hermes-agent/.hermes/bin/hermes"),
            &files,
        )
        .stand_in_entrypoint(&format!("{PROGRAMS}/hermes-agent/.hermes/bin/hermes-acp"))
        .stand_in_prelude(
            "case \" $* \" in *' gateway run '*) exec python3 \"$HOME/gateway-fixture.py\";; esac",
        )
        .tools_in_settings()
        .run();
}
