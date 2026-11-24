//! An owner invoking namespace-only commands gets an ordinary usage error.
use link_test_kit::{TempDir, env};

#[test]
fn namespace_commands_without_an_agent_context_are_usage_errors() {
    let home = TempDir::new("model-context");
    for action in ["usage", "context", "chat", "embed", "transcribe", "count"] {
        let output = env::command(env!("CARGO_BIN_EXE_lnk-model"), &home)
            .args([action])
            .env_remove("LNK_MODEL_URL")
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(2), "{action}: {said}");
        assert!(said.contains("lnk agent model status"), "{action}: {said}");
        assert!(said.contains("lnk model list"), "{action}: {said}");
        assert!(
            !said.contains("Report it:") && !said.contains("environment variable not found"),
            "{action}: {said}"
        );
    }
}

/// A route that answers each request with `body`, for as many requests as
/// it is asked: what an agent's proxy answers on the model namespace.
fn route(body: &'static str, answers: usize) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, Write};
    let listener = link_test_kit::net::Held::loopback().listener();
    let url = format!("http://{}/capability/model", listener.local_addr().unwrap());
    let serving = std::thread::spawn(move || {
        for _ in 0..answers {
            let (stream, _) = listener.accept().unwrap();
            let mut reading = std::io::BufReader::new(&stream);
            // The whole request's head, before the answer.
            let mut line = String::new();
            while reading.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(
                &stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (url, serving)
}

/// A person reads the namespace's answer as fields, each by its name;
/// only `--json` prints the JSON, as the route answered it.
#[test]
fn a_namespace_answer_is_fields_for_a_person_and_json_with_json() {
    let home = TempDir::new("model-usage");
    let (url, serving) = route(
        r#"{"requests":12,"unmetered":2,"tokens":{"input":3400,"output":560,"cached":100}}"#,
        2,
    );
    let usage = |json: bool| {
        let mut command = env::command(env!("CARGO_BIN_EXE_lnk-model"), &home);
        command.args(["usage"]).env("LNK_MODEL_URL", &url);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let said = usage(false);
    assert!(!said.contains('{'), "{said}");
    for shown in [
        "Requests",
        "12",
        "Unmetered",
        "Tokens",
        "Input",
        "3400",
        "Cached",
    ] {
        assert!(said.contains(shown), "{shown}: {said}");
    }
    let json: serde_json::Value = serde_json::from_str(&usage(true)).unwrap();
    assert_eq!(json["tokens"]["output"], 560);
    serving.join().unwrap();
}

#[test]
fn namespace_inspection_without_a_model_requests_its_selected_model() {
    use std::io::{BufRead, Write};
    let home = TempDir::new("model-selected-namespace");
    let listener = link_test_kit::net::Held::loopback().listener();
    let url = format!("http://{}/capability/model", listener.local_addr().unwrap());
    let serving = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut request = std::io::BufReader::new(&stream);
        let mut line = String::new();
        request.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "GET /capability/model/show HTTP/1.1");
        loop {
            line.clear();
            if request.read_line(&mut line).unwrap() <= 2 {
                break;
            }
        }
        let body = r#"{"name":"qa-selected-model","context":4096}"#;
        write!(
            &stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let output = env::command(env!("CARGO_BIN_EXE_lnk-model"), &home)
        .args(["show", "--json"])
        .env("LNK_MODEL_URL", url)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let shown: link_model_contract::show::Shown = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shown.name, "qa-selected-model");
    assert_eq!(shown.context, Some(4096));
    serving.join().unwrap();
}
