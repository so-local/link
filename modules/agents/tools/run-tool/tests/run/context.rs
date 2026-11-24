//! Actual foreground and supervisor/gate process handoffs, without a native sandbox.
use super::command::{call, files, run, seen};
use link_run_tool_contract::context::Context;
use link_sandbox_binding::{Identity, Keyring};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};
const AGENT: &str = "aaaaaaaaaaaa";
const KEY: &str = "a-private-agent-binding-key-with-32-characters";

fn context(dir: &Path) -> Context {
    let keys = Keyring::at(&dir.join("keys"));
    keys.bind(
        &Identity {
            agent: AGENT.into(),
            owner: "local-link-aaaaaaaaaaaa".into(),
            ..Default::default()
        },
        &KEY.to_string().into(),
    )
    .unwrap();
    let route: link_sandbox_route::Route = serde_json::from_value(json!({
        "name":"secret","prefix":"/_link/secret","bridge":{"socket":dir.join("secret.sock"),"start":[]},
        "given":"named-only-sealed-grant", "env":{"QA_TOKEN":"a-placeholder","LNK_SEALED_QA_TOKEN_URL":"{proxy}/_link/secret/qa-token"}
    })).unwrap();
    Context {
        agent: AGENT.into(),
        owner: "local-link-aaaaaaaaaaaa".into(),
        keys: keys.dir().into(),
        routes: vec![route],
    }
}

fn sandbox(dir: &Path) -> Vec<String> {
    let path = dir.join("sandbox");
    link_test_kit::write_program(
        &path,
        r##"#!/usr/bin/python3
import json,os,sys
assert sys.argv[1:3] == ['--with-fd','4'], sys.argv
with os.fdopen(4) as pipe:
    context=json.load(pipe)
assert context['named']['caller']['identity']['agent']=='aaaaaaaaaaaa'
assert len(context['named']['caller']['key'])>=32
assert 'LNK_RUN_CONTEXT' not in os.environ
for route in context['named']['routes']:
    for name,value in route['env'].items():
        os.environ[name]=value.replace('{proxy}','http://127.0.0.1:40781')
command=sys.argv[sys.argv.index('--')+1:]
os.execv(command[0],command)
"##,
    )
    .unwrap();
    vec![path.display().to_string()]
}

#[test]
fn foreground_commands_get_placeholders_without_the_context_or_binding() {
    let files = files("named-context");
    let context = context(&files);
    let mut run = run(&files, &["sh"], sandbox(&files));
    run.context = Some(Arc::new(context));
    let result = call(
        &run,
        json!({"command":"printf '%s\\n' \"$QA_TOKEN\" \"$LNK_SEALED_QA_TOKEN_URL\"; test -z \"$LNK_RUN_CONTEXT\"; test ! -e /proc/self/fd/4"}),
    );
    assert_eq!(seen(&result)["exit"], 0, "{result}");
    assert_eq!(
        seen(&result)["stdout"],
        "a-placeholder\nhttp://127.0.0.1:40781/_link/secret/qa-token\n"
    );
    assert!(!result.to_string().contains(KEY));
    run.env.push(("QA_TOKEN".into(), "override".into()));
    assert_eq!(call(&run, json!({"command":":"}))["isError"], true);
}

#[test]
fn a_replaced_or_missing_binding_is_checked_for_each_actual_call() {
    let files = files("context-binding");
    let context = context(&files);
    assert_eq!(
        context
            .resolve()
            .unwrap()
            .named
            .unwrap()
            .caller
            .key
            .expose(),
        KEY
    );
    let keys = Keyring::at(&context.keys);
    keys.bind(
        &Identity {
            agent: AGENT.into(),
            owner: "local-link-aaaaaaaaaaaa".into(),
            ..Default::default()
        },
        &"a-new-binding-key-with-at-least-32-characters"
            .to_string()
            .into(),
    )
    .unwrap();
    assert_ne!(
        context
            .resolve()
            .unwrap()
            .named
            .unwrap()
            .caller
            .key
            .expose(),
        KEY
    );
    keys.bind(
        &Identity {
            agent: AGENT.into(),
            owner: "local-link-bbbbbbbbbbbb".into(),
            ..Default::default()
        },
        &KEY.to_string().into(),
    )
    .unwrap();
    assert!(
        context.resolve().is_err(),
        "a binding for a foreign owner is not this context's caller"
    );
    let foreign = "bbbbbbbbbbbb";
    keys.bind(
        &Identity {
            agent: foreign.into(),
            owner: "local-link-aaaaaaaaaaaa".into(),
            ..Default::default()
        },
        &KEY.to_string().into(),
    )
    .unwrap();
    std::fs::copy(
        context.keys.join(format!("{foreign}.json")),
        context.keys.join(format!("{AGENT}.json")),
    )
    .unwrap();
    assert!(context.resolve().is_err());
    std::fs::remove_file(context.keys.join(format!("{AGENT}.json"))).unwrap();
    assert!(context.resolve().is_err());
}

#[test]
fn background_context_crosses_supervisor_and_gate_without_entering_its_record() {
    let files = files("background-context");
    let context = context(&files);
    let keys = context.keys.clone();
    let mut run = run(&files, &["sh"], sandbox(&files));
    run.context = Some(Arc::new(context));
    let state = files.join("state");
    run.background = Some(link_run_tool::Background {
        program: env!("CARGO_BIN_EXE_lnk-run-tool").into(),
        state: state.clone(),
        way_in: files.join("no-bridge.sock"),
        caller: link_run_tool::background::Caller {
            agent: AGENT.into(),
            keyring: Some(keys),
        },
        at_once: 1,
        timeout: Duration::from_secs(10),
        output: 1024,
        keep: Duration::from_secs(60),
        per_day: 10,
        hops: 8,
    });
    let result=link_run_tool::answer(&run,&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"command","arguments":{"command":"printf '%s' \"$QA_TOKEN\"; test -z \"$LNK_RUN_CONTEXT\"; test ! -e /proc/self/fd/4","run_in_background":true},"_meta":{"link.local/thread":"test:owner","link.local/hops":1}}}).to_string()).unwrap();
    let id = result["result"]["structuredContent"]["id"]
        .as_str()
        .expect("background start");
    link_test_kit::eventually("the context command ends", Duration::from_secs(15), || {
        link_run_tool::background::load(&state, id).unwrap().state
            != link_run_tool::background::State::Running
    });
    // The final state is saved before the supervisor reports its ending.
    // Wait for its lifetime lock too, so the fixture outlives that cleanup.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join(format!("{id}.lock")))
        .unwrap();
    link_test_kit::eventually(
        "the context supervisor releases its lifetime lock",
        Duration::from_secs(15),
        || lock.try_lock().is_ok(),
    );
    let record = link_run_tool::background::load(&state, id).unwrap();
    assert_eq!(
        record.exit,
        Some(0),
        "{}",
        record.why.as_deref().unwrap_or_default()
    );
    let serialized = serde_json::to_string(&record).unwrap();
    for private in [KEY, "named-only-sealed-grant"] {
        assert!(!serialized.contains(private), "record retains context");
    }
    let output = link_run_tool::background::read_output(&record, 0, 1024).unwrap();
    assert!(output.text.contains("a-placeholder"));
}

#[test]
fn failed_context_handoffs_and_supervisor_spawns_leave_no_running_record() {
    for oversized in [false, true] {
        let files = files("context-start-failure");
        let mut context = context(&files);
        if oversized {
            context.routes[0].given = "x".repeat(1024 * 1024 + 1).into();
        }
        let mut run = run(&files, &["sh"], sandbox(&files));
        run.context = Some(Arc::new(context));
        let state = files.join("state");
        run.background = Some(link_run_tool::Background {
            program: files.join("missing-supervisor"),
            state: state.clone(),
            way_in: files.join("no-bridge.sock"),
            caller: link_run_tool::background::Caller {
                agent: AGENT.into(),
                keyring: Some(files.join("keys")),
            },
            at_once: 1,
            timeout: Duration::from_secs(10),
            output: 1024,
            keep: Duration::from_secs(60),
            per_day: 10,
            hops: 8,
        });
        let result=link_run_tool::answer(&run,&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"command","arguments":{"command":":","run_in_background":true},"_meta":{"link.local/thread":"test:owner","link.local/hops":1}}}).to_string()).unwrap();
        assert_eq!(result["result"]["isError"], true, "{result}");
        assert!(link_run_tool::background::records(&state).is_empty());
        assert!(
            !std::fs::read_dir(&state)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json")),
            "failed start retained a record"
        );
    }
}
