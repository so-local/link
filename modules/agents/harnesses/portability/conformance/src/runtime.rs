//! Controlled Mini/full Link workloads. Local first-party services only,
//! with Rust-owned children and Measure's shared phase records.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use link_measure_contract::client::{process, record::Phase};
use link_test_kit::{ChildGroup, TempDir, env, gate, wait};
use serde_json::{Value, json};

use crate::{agent, fixture_connection};

mod model;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Confined,
    /// Explicit diagnostic only. Protected tools aren't offered here.
    Diagnostic,
}

struct Running {
    child: ChildGroup,
    bin: PathBuf,
    home: PathBuf,
    name: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        // The agent owns detached helpers as well as the foreground
        // group. Ask that owner first, including while unwinding.
        let _ = command(&self.bin, &self.home)
            .args(["agent", "stop", "--agent", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        self.child.stop();
    }
}

fn command(bin: &Path, home: &Path) -> Command {
    let mut command = env::command(bin.join("lnk"), home);
    command
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LNK_EXE", bin.join("lnk"))
        .env("LNK_SERVICES", "off")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(Stdio::null());
    command
}

fn json_command(bin: &Path, home: &Path, args: &[&str]) -> Value {
    let output = command(bin, home).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A matched workload, one or two agents in the same fixture installation.
/// `bin` selects already built programs, allowing the same runner to compare
/// two revisions. No build, download, paid model or automatic unsafe fallback.
pub async fn run(bin: &Path, harness: &str, mode: Mode, count: usize) -> Value {
    assert!(matches!(harness, "mini" | "link"));
    assert!(matches!(count, 1 | 2));
    let bin = bin.canonicalize().unwrap();
    let root = TempDir::short_out_of_tmp("runtime");
    let home = root.join("h");
    std::fs::create_dir_all(&home).unwrap();
    let mut report = json!({"schema": 1, "harness": harness, "agents": count,
        "confinement": if mode == Mode::Confined { "requested" } else { "explicit-unconfined-diagnostic" },
        "services": "off", "keychain": "off", "monitor_interval_seconds": 1, "outcome": "unknown",
        "coverage": ["local first-party model/connection", "native Linux process trees; detached roots from agent status",
            "timings must come from untraced runs", "root secret service and real service managers not exercised",
            "RSS/PSS and installation bytes not collected", "CPU includes waited children; tree sums can overlap"]});
    if mode == Mode::Confined
        && (!gate::bubblewrap() || !gate::landlock() || !gate::out_of_tmp(&root))
    {
        let output = command(&bin, &home)
            .args(["sandbox", "check", "--ready"])
            .output()
            .unwrap();
        report["outcome"] = json!("skipped");
        report["reason"] = json!(
            "confinement or fixture-location prerequisite unavailable; no unconfined fallback"
        );
        report["ready_exit"] = json!(output.status.code());
        report["ready_stderr"] = json!(String::from_utf8_lossy(&output.stderr));
        let native = command(&bin, &home)
            .args([
                "sandbox",
                "confine",
                "--only",
                "--exec",
                "/usr/bin/true",
                "--",
                "/usr/bin/true",
            ])
            .output()
            .unwrap();
        report["native_confinement_exit"] = json!(native.status.code());
        report["native_confinement_stderr"] = json!(String::from_utf8_lossy(&native.stderr));
        return report;
    }
    // One permit an agent, held to the end, before any wait begins: the
    // tests of one program start no more confined agents at once than the
    // machine starts in their waits (`link_test_kit::permit`).
    let _permit = link_test_kit::permit::confined(count);
    let asked: agent::Shared<Vec<Value>> = Default::default();
    let model = agent::serve(
        axum::Router::new()
            .route("/v1/chat/completions", axum::routing::post(model::answer))
            .with_state(asked.clone()),
    )
    .await;
    let chat = fixture_connection::ConnectionFixture::serve().await;
    let names = ["main", "work"];
    let mut running = Vec::new();
    let mut blocks = Vec::new();
    let mut roots = Vec::new();
    let mut phases = vec![Phase::linux("start", &[], Duration::ZERO)];
    for (i, name) in names[..count].iter().enumerate() {
        let mut block = agent::free_block();
        let token = fixture_connection::token(i as u32 + 1);
        let settings = format!(
            "harness = {harness:?}\n{}\n[model]\nprovider = \"local\"\nname = \"fixture\"\nbase_url = \"http://127.0.0.1:{model}\"\n\n{}",
            block.settings(),
            fixture_connection::section(&token, "42")
        );
        // The tools Link Harness's workloads call, each off until added.
        let settings = match harness {
            "link" => format!(
                "{settings}\n[behavior.tool.scheduler]\nenabled = true\n\
                 [behavior.tool.run]\nenabled = true\n[behavior.tool.file]\nenabled = true\n"
            ),
            _ => settings,
        };
        link_agent_contract::testing::write_agent(&home, name, &settings);
        let files = home.join(format!("Link/Agents/{name}/home"));
        std::fs::create_dir_all(files.join("work")).unwrap();
        std::fs::write(files.join("work/identity"), name).unwrap();
        let mut start = command(&bin, &home);
        start
            .args([
                "agent",
                "start",
                "--foreground",
                "--agent",
                name,
                "--run.files",
            ])
            .arg(&files)
            .env(chat.env().0, chat.env().1)
            .stdout(std::fs::File::create(root.join(format!("{name}.log"))).unwrap())
            .stderr(
                std::fs::File::options()
                    .append(true)
                    .open(root.join(format!("{name}.log")))
                    .unwrap(),
            );
        // Confined runs keep Link's sandbox file: `work` written, `sh` run.
        if mode == Mode::Diagnostic {
            start.arg("--no-sandbox");
        }
        block.let_go();
        let began = Instant::now();
        let child = ChildGroup::spawn(&mut start).unwrap();
        roots.push(child.id());
        running.push(Running {
            child,
            bin: bin.clone(),
            home: home.clone(),
            name: name.to_string(),
        });
        blocks.push(block);
        wait::eventually_async(
            "the fixture connection connected",
            Duration::from_secs(30),
            || {
                let run = running.last().unwrap();
                assert!(
                    run.child.try_status().unwrap().is_none(),
                    "{}",
                    std::fs::read_to_string(root.join(format!("{name}.log"))).unwrap_or_default()
                );
                chat.tokens().contains(&token)
            },
        )
        .await;
        phases.push(Phase::linux(
            &format!("{name}_ready"),
            &roots,
            began.elapsed(),
        ));
        let state = json_command(&bin, &home, &["agent", "show", "--agent", name, "--json"]);
        roots.extend(
            state["processes"]["pids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_u64())
                .filter_map(|p| u32::try_from(p).ok()),
        );
    }
    roots.sort_unstable();
    roots.dedup();
    link_files::write_private(
        &home.join(".lnk/safe/measures/settings.toml"),
        b"every = \"1s\"\n",
    )
    .unwrap();
    // Ordinary observation and controlled timing are separate. This is
    // an explicit watcher owned by the fixture, never automatic tracing.
    let mut watch_command = command(&bin, &home);
    watch_command
        .args(["measure", "watch"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let watcher = ChildGroup::spawn(&mut watch_command).unwrap();
    roots.push(watcher.id());
    let measure =
        |name: &str| json_command(&bin, &home, &["agent", "show", "--agent", name, "--json"]);
    for (i, name) in names[..count].iter().enumerate() {
        // A sandbox's port takes connections before its harness listens
        // inside, and closes them unanswered: the harness's own answer is
        // the one checked, and it must refuse.
        let endpoint = format!("ws://127.0.0.1:{}/acp", blocks[i].0);
        let answered = wait::eventually_await(
            "the harness answered its endpoint",
            Duration::from_secs(30),
            || async {
                match tokio_tungstenite::connect_async(endpoint.as_str()).await {
                    Ok(_) => Ok(None),
                    Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                        Ok(Some(response.status().as_u16()))
                    }
                    Err(unanswered) => Err(format!("{unanswered:?}")),
                }
            },
        )
        .await;
        assert!(
            matches!(answered, Some(401 | 403)),
            "an unauthenticated endpoint was not refused: {answered:?}"
        );
        let token = fixture_connection::token(i as u32 + 1);
        for turn in ["cold", "warm"] {
            let text = if mode == Mode::Confined && harness == "link" {
                format!("runtime protected {name} {turn}")
            } else if harness == "link" {
                format!("runtime scheduler {name} {turn}")
            } else {
                format!("runtime echo {name} {turn}")
            };
            phases.push(Phase::linux(
                &format!("{name}_{turn}_start"),
                &roots,
                Duration::ZERO,
            ));
            let began = Instant::now();
            chat.message(&token, "42", &text);
            let answered = wait::eventually_async(
                "the model/tool round trip completed",
                Duration::from_secs(30),
                || {
                    assert!(
                        running[i].child.try_status().unwrap().is_none(),
                        "{}",
                        std::fs::read_to_string(root.join(format!("{name}.log")))
                            .unwrap_or_default()
                    );
                    if let Some(at) = chat.answered_at(&token, "42", &format!("completed {text}")) {
                        Ok(at)
                    } else {
                        Err(format!(
                            "{} model requests; agent log: {}",
                            asked.lock().unwrap().len(),
                            std::fs::read_to_string(root.join(format!("{name}.log")))
                                .unwrap_or_default()
                        ))
                    }
                },
            )
            .await;
            // The turn ends when the reply was sent, not when a poll saw it.
            phases.push(Phase::linux(
                &format!("{name}_{turn}_end"),
                &roots,
                answered.duration_since(began),
            ));
        }
        if mode == Mode::Confined && harness == "link" {
            assert_eq!(
                std::fs::read_to_string(home.join(format!("Link/Agents/{name}/home/work/made")))
                    .unwrap(),
                "runtime\n"
            );
        }
        // Same owner and installation, distinct conversations and files.
        assert_eq!(
            std::fs::read_to_string(home.join(format!("Link/Agents/{name}/home/work/identity")))
                .unwrap(),
            *name
        );
        let state = measure(name);
        assert_eq!(state["wire"], Value::Null);
    }
    // Observe completed samples, rather than sleeping for a guessed
    // polling interval. The accelerated 1s setting is explicit in the record.
    let now_path = home.join(".lnk/safe/measures/now.json");
    let latest = || {
        std::fs::read(&now_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v["at"].as_u64())
    };
    let first = wait::eventually_async(
        "the watcher's first completed sample",
        Duration::from_secs(10),
        latest,
    )
    .await;
    phases.push(Phase::linux("idle_start", &roots, Duration::ZERO));
    let start = Instant::now();
    wait::eventually_async(
        "two more completed monitor samples",
        Duration::from_secs(10),
        || {
            assert!(watcher.try_status().unwrap().is_none(), "the watcher ended");
            latest().is_some_and(|now| now >= first + 2)
        },
    )
    .await;
    phases.push(Phase::linux("idle_end", &roots, start.elapsed()));
    let watcher_pid = watcher.id();
    drop(watcher);
    roots.retain(|pid| *pid != watcher_pid);
    // Stop monitoring before the isolated status series so a simultaneous
    // periodic query is not attributed to a requested status operation.
    phases.push(Phase::linux("status_start", &roots, Duration::ZERO));
    let began = Instant::now();
    let mut times = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        let _ = measure("main");
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    phases.push(Phase::linux("status_end", &roots, began.elapsed()));
    report["status_ms"] = json!(times);
    let live = Phase::linux("before_stop", &roots, Duration::ZERO);
    let observed = live.processes.clone();
    phases.push(live);
    let start = Instant::now();
    drop(running);
    wait::eventually_async(
        "all observed fixture processes stopped",
        Duration::from_secs(10),
        || observed.iter().all(|p| !same_live_process(p)),
    )
    .await;
    phases.push(Phase::linux("stopped", &roots, start.elapsed()));
    report["phases"] = serde_json::to_value(phases).unwrap();
    report["checks"] = json!({"round_trips": true, "cold_and_warm": true,
        "unauthenticated_endpoint_refused": true, "completed_idle_monitor_samples": true,
        "no_observed_live_processes_after_stop": true,
        "protected_tools_and_refusals": mode == Mode::Confined && harness == "link"});
    report["model_requests"] = json!(asked.lock().unwrap().len());
    report["outcome"] = json!("passed");
    report
}

fn same_live_process(p: &link_measure_contract::client::record::Process) -> bool {
    process::state(p.pid).is_some_and(|state| state != 'Z')
        && process::process(p.pid, false).is_some_and(|now| now.started == p.started)
}
