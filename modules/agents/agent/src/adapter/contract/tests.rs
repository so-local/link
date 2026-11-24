use super::*;

#[cfg(unix)]
fn answer(text: &str, status: i32) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(status << 8),
        stdout: text.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

#[test]
#[cfg(unix)]
fn successful_contract_answers_are_typed_and_versioned() {
    for text in [
        "",
        "not JSON",
        "{}",
        r#"{"version":"2"}"#,
        r#"{"version":0,"proxy":true}"#,
    ] {
        assert!(checked("example", &answer(text, 0)).is_err(), "{text}");
    }
    for version in [link_harness_contract::harness::PROTOCOL.current, 333] {
        let text = format!("{{\"version\":{version},\"proxy\":true}}");
        assert!(checked("example", &answer(&text, 0)).unwrap().proxy);
    }
    let missing = checked("example", &answer("no such command", 2)).unwrap();
    assert_eq!(missing, Contract::default());
}

#[test]
#[cfg(unix)]
fn malformed_contract_errors_survive_caching_and_program_replacement() {
    let dir = link_test_kit::TempDir::new("harness-contract-error");
    let program = dir.join("lnk-example");
    let log = dir.join("asked");
    let script = |answer: &str| {
        format!("#!/bin/sh\nprintf x >> \"${{0%/*}}/asked\"\nprintf '%s' '{answer}'\n")
    };
    link_test_kit::write_program(&program, script("{}")).unwrap();
    let adapter = Adapter {
        name: "example".into(),
        program: program.clone(),
    };
    for _ in 0..2 {
        let error = adapter.contract().unwrap_err();
        let causes = format!("{error:#}");
        assert!(
            causes.contains("reading lnk-example contract")
                && causes.contains("missing field `version`"),
            "{causes}"
        );
    }
    assert_eq!(std::fs::read(&log).unwrap(), b"x");
    link_test_kit::write_program(&program, script(r#"{"version":0,"proxy":true}"#)).unwrap();
    assert!(adapter.contract().is_err());
    link_test_kit::write_program(&program, script(r#"{"version":2,"proxy":true}"#)).unwrap();
    assert!(adapter.contract().unwrap().proxy);
    assert_eq!(std::fs::read(&log).unwrap(), b"xxx");
}
