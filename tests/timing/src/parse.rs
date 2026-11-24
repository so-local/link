use crate::{Failure, Metadata, Package, Program};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Artifact {
    reason: String,
    #[serde(default)]
    package_id: String,
    executable: Option<String>,
}

fn filename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn unique<'a>(mut packages: impl Iterator<Item = &'a Package>) -> Option<&'a Package> {
    let first = packages.next()?;
    packages.next().is_none().then_some(first)
}

fn identify<'a>(
    target: &str,
    executable: Option<&str>,
    artifacts: &BTreeMap<String, String>,
    metadata: &'a Metadata,
) -> Option<&'a Package> {
    if let Some(id) = executable.and_then(|e| artifacts.get(filename(e))) {
        return metadata.members().find(|p| p.id == *id);
    }
    let executable = executable.map(filename);
    unique(metadata.members().filter(|p| {
        p.targets.iter().any(|t| {
            let name = t.name.replace('-', "_");
            if let Some(executable) = executable {
                let stem = executable.strip_suffix(".exe").unwrap_or(executable);
                let prefix = stem.rsplit_once('-').map(|(p, _)| p).unwrap_or(stem);
                let source = target.strip_prefix("unittests ").unwrap_or(target);
                prefix == name
                    && t.src_path
                        .replace('\\', "/")
                        .ends_with(&format!("/{source}"))
            } else {
                target.strip_prefix("Doc-tests ").is_some_and(|doc| {
                    (doc == t.name || doc == name)
                        && t.kind.iter().any(|k| k == "lib" || k == "proc-macro")
                })
            }
        })
    }))
}

fn duration(raw: &str) -> Option<f64> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut seconds = 0.0;
    for piece in raw.split_whitespace() {
        let (amount, multiplier) = if let Some(p) = piece.strip_suffix("ms") {
            (p, 0.001)
        } else if let Some(p) = piece.strip_suffix('s') {
            (p, 1.0)
        } else if let Some(p) = piece.strip_suffix('m') {
            (p, 60.0)
        } else {
            (piece.strip_suffix('h')?, 3600.0)
        };
        let amount: f64 = amount.parse().ok()?;
        if !amount.is_finite() || amount < 0.0 {
            return None;
        }
        seconds += amount * multiplier;
    }
    seconds.is_finite().then_some(seconds)
}

pub(crate) fn summary(line: &str, program: &mut Program) -> bool {
    let Some(rest) = line.strip_prefix("test result: ") else {
        return false;
    };
    let Some((result, counts)) = rest.split_once(". ") else {
        return false;
    };
    if result != "ok" && result != "FAILED" {
        return false;
    }
    let Some((counts, elapsed)) = counts.rsplit_once("; finished in ") else {
        return false;
    };
    let Some(seconds) = duration(elapsed) else {
        return false;
    };
    let mut parsed = [None; 5];
    for count in counts.split("; ") {
        let Some((number, label)) = count.split_once(' ') else {
            return false;
        };
        let Ok(number) = number.parse::<u64>() else {
            return false;
        };
        let index = match label {
            "passed" => 0,
            "failed" => 1,
            "ignored" => 2,
            "measured" => 3,
            "filtered out" => 4,
            _ => return false,
        };
        if parsed[index].replace(number).is_some() {
            return false;
        }
    }
    let [
        Some(passed),
        Some(failed),
        Some(ignored),
        Some(_measured),
        Some(filtered),
    ] = parsed
    else {
        return false;
    };
    program.passed = passed;
    program.failed = failed;
    program.ignored = ignored;
    program.filtered = filtered;
    program.outcome = Some(result.into());
    program.seconds = Some(seconds);
    true
}

fn clear_summary(program: &mut Program) {
    program.outcome = None;
    program.seconds = None;
    program.passed = 0;
    program.failed = 0;
    program.ignored = 0;
    program.filtered = 0;
}

/// Reads one line of a program's output for its failures: libtest's
/// `test <path> ... FAILED`, then each one's `---- <path> stdout ----`
/// block, whose line after `panicked at <where>:` is what it said.
fn failure(
    line: &str,
    failures: &mut Vec<Failure>,
    telling: &mut Option<String>,
    panicked: &mut bool,
) {
    if let Some(test) = line
        .strip_prefix("test ")
        .and_then(|rest| rest.strip_suffix(" ... FAILED"))
    {
        if !failures.iter().any(|f| f.test == test) {
            failures.push(Failure {
                test: test.to_owned(),
                said: None,
            });
        }
    } else if let Some(test) = line
        .strip_prefix("---- ")
        .and_then(|rest| rest.strip_suffix(" stdout ----"))
    {
        *telling = Some(test.to_owned());
        *panicked = false;
    } else if line.starts_with("thread '") && line.contains(" panicked at ") {
        *panicked = telling.is_some();
    } else if *panicked && !line.is_empty() {
        *panicked = false;
        if let Some(failed) = failures
            .iter_mut()
            .find(|f| Some(&f.test) == telling.as_ref() && f.said.is_none())
        {
            failed.said = Some(line.to_owned());
        }
    }
}

pub(crate) fn programs(log: &str, metadata: &Metadata) -> (Vec<Program>, Option<f64>, Vec<String>) {
    let mut artifacts = BTreeMap::new();
    for line in log.lines() {
        if let Ok(artifact) = serde_json::from_str::<Artifact>(line)
            && artifact.reason == "compiler-artifact"
            && let Some(executable) = artifact.executable
        {
            artifacts.insert(filename(&executable).to_owned(), artifact.package_id);
        }
    }
    let mut programs: Vec<Program> = Vec::new();
    let mut current: Option<usize> = None;
    let mut compilation = None;
    let mut warnings = Vec::new();
    // The failing test whose output is being read (`---- <test> stdout
    // ----`), and whether its panic's location was just read: the next
    // line is what it said.
    let mut telling: Option<String> = None;
    let mut panicked = false;
    for line in log.lines() {
        let line = line.trim();
        if let Some(index) = current {
            failure(
                line,
                &mut programs[index].failures,
                &mut telling,
                &mut panicked,
            );
        }
        if line.starts_with("Finished ")
            && let Some((_, time)) = line.rsplit_once(" in ")
        {
            compilation = duration(time);
        }
        let header = if let Some(rest) = line.strip_prefix("Running ") {
            rest.rsplit_once(" (").and_then(|(name, executable)| {
                executable.strip_suffix(')').map(|e| (name, Some(e)))
            })
        } else if line.starts_with("Doc-tests ") {
            Some((line, None))
        } else {
            None
        };
        if let Some((target, executable)) = header {
            let package = identify(target, executable, &artifacts, metadata);
            programs.push(Program {
                package: package.map(|p| p.name.clone()),
                group: package
                    .map(|p| metadata.group(p))
                    .unwrap_or_else(|| "unassigned".into()),
                target: target.into(),
                executable: executable.map(str::to_owned),
                outcome: None,
                seconds: None,
                passed: 0,
                failed: 0,
                ignored: 0,
                filtered: 0,
                failures: Vec::new(),
            });
            current = Some(programs.len() - 1);
            telling = None;
            panicked = false;
        } else if line.starts_with("test result:") {
            // A test can print subprocess summaries before its own final
            // summary. Keep this Cargo program until the next header, and
            // let its last summary describe the complete outer harness.
            if let Some(index) = current {
                if !summary(line, &mut programs[index]) {
                    clear_summary(&mut programs[index]);
                    warnings.push(format!(
                        "Unrecognized summary for {}: {line}",
                        programs[index].target
                    ));
                }
            } else {
                warnings.push(format!("Summary without a Cargo Running header: {line}"));
            }
        } else if line.starts_with("error: test failed")
            && let Some(index) = current
            && programs[index].outcome.as_deref() == Some("ok")
        {
            clear_summary(&mut programs[index]);
            warnings.push(format!(
                "Cargo reports failure without a failing outer summary for {}",
                programs[index].target
            ));
        }
    }
    if programs.iter().any(|p| p.outcome.is_none()) {
        warnings.push("Some programs have no valid libtest summary. A crash, truncated log or nonconforming harness can omit it. Their duration and outcome counts are unknown, not zero.".into());
    }
    (programs, compilation, warnings)
}
