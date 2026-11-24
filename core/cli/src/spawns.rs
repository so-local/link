//! How many programs a command starts, held to a budget: `lnk` and its
//! modules start one another freely, and every program started costs
//! the user its start. Each command in `spawns.toml` runs under `strace`
//! in a home of its own, and every program it starts, itself and the
//! module it becomes included, counts against its budget.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use serde::Deserialize;

/// `spawns.toml`: each command and the most programs it may start.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Budgets {
    command: Vec<Budget>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    /// What follows `lnk`.
    args: Vec<String>,
    /// The most programs it may start, counting each `execve`.
    most: usize,
}

/// Each program a trace (`strace -f -e trace=execve`) shows started,
/// by its name and first arguments, and how many times.
fn started(trace: &str) -> BTreeMap<String, usize> {
    let mut pending: BTreeMap<&str, String> = BTreeMap::new();
    let mut started = BTreeMap::new();
    for line in trace.lines() {
        let Some((pid, call)) = line.split_once(' ') else {
            continue;
        };
        let call = call.trim_start();
        if let Some(rest) = call.strip_prefix("execve(") {
            let named = named(rest);
            match call.ends_with("<unfinished ...>") {
                true => {
                    pending.insert(pid, named);
                }
                false if call.ends_with("= 0") => *started.entry(named).or_insert(0) += 1,
                false => {}
            }
        } else if call.starts_with("<... execve resumed>")
            && let Some(named) = pending.remove(pid)
            && call.ends_with("= 0")
        {
            *started.entry(named).or_insert(0) += 1;
        }
    }
    started
}

/// `"/path/lnk-x", ["lnk-x", "a", "b", ...` as `lnk-x a b`: its
/// program's name and up to three arguments.
fn named(call: &str) -> String {
    let quoted: Vec<&str> = call.split('"').skip(1).step_by(2).collect();
    let program = quoted
        .first()
        .and_then(|p| Path::new(p).file_name())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    std::iter::once(program)
        .chain(quoted.iter().skip(2).take(3).map(|a| a.to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn commands_start_no_more_programs_than_their_budget() {
    if !link_test_kit::gate::program("strace", &["-f", "-o", "/dev/null", "true"]) {
        return;
    }
    let budgets: Budgets = toml::from_str(include_str!("spawns.toml")).unwrap();
    let lnk = link_test_kit::programs().join("lnk");
    link_test_kit::programs::require(&lnk, link_test_kit::programs::BUILD);
    let over: Vec<String> = std::thread::scope(|s| {
        let running: Vec<_> = budgets
            .command
            .iter()
            .map(|budget| {
                let lnk = &lnk;
                s.spawn(move || {
                    let home = link_test_kit::TempDir::new("spawns");
                    let trace = home.join("trace");
                    let output = link_test_kit::env::command("strace", &home)
                        .args(["-f", "-qq", "-s", "256", "-e", "trace=execve"])
                        .args(["-e", "signal=none", "-o"])
                        .arg(&trace)
                        .arg(lnk)
                        .args(&budget.args)
                        .stdin(Stdio::null())
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "lnk {} failed: {}",
                        budget.args.join(" "),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let started = started(&std::fs::read_to_string(&trace).unwrap());
                    let count: usize = started.values().sum();
                    (count > budget.most).then(|| {
                        let each: Vec<String> =
                            started.iter().map(|(p, n)| format!("  {n} {p}")).collect();
                        format!(
                            "lnk {} started {count} programs, over its {}:\n{}",
                            budget.args.join(" "),
                            budget.most,
                            each.join("\n")
                        )
                    })
                })
            })
            .collect();
        running
            .into_iter()
            .filter_map(|r| r.join().unwrap())
            .collect()
    });
    assert!(over.is_empty(), "{}", over.join("\n"));
}

#[test]
fn a_trace_counts_each_program_started() {
    let trace = "\
10 execve(\"/b/lnk\", [\"lnk\", \"agent\", \"list\"], 0x1 /* 9 vars */) = 0
11 execve(\"/b/lnk-agent\", [\"/b/lnk-agent\", \"--link-describe\"], 0x2 /* 9 vars */) = 0
10 execve(\"/b/lnk-agent\", [\"/b/lnk-agent\", \"agent\", \"list\"], 0x3 <unfinished ...>
12 execve(\"/usr/bin/nope\", [\"nope\"], 0x4 /* 9 vars */) = -1 ENOENT (No such file or directory)
10 <... execve resumed>)                = 0
13 execve(\"/b/lnk\", [\"/b/lnk\", \"agent\", \"list\"], 0x5 /* 9 vars */) = 0
";
    let started = started(trace);
    assert_eq!(started["lnk agent list"], 2);
    assert_eq!(started["lnk-agent --link-describe"], 1);
    assert_eq!(started["lnk-agent agent list"], 1);
    assert_eq!(started.values().sum::<usize>(), 4);
}
