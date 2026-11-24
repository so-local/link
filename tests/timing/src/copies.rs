//! One test run as many copies of its program at once, to find a flake
//! again or prove its fix: `lnk-check copies -p <package> <test>`.
//!
//! A flake shows under load, and the whole suite rarely meets it twice.
//! Hundreds of copies of one test at once give it the load and the
//! chances. The copies that fail are counted, and the first one's output
//! is kept. A fix is proved when the same number of copies, on the same
//! machine, all pass.

use std::path::{Path, PathBuf};

/// What the copies came to.
#[derive(Debug, PartialEq, Eq)]
pub struct Copies {
    pub ran: usize,
    pub failed: usize,
    /// The first failing copy's output, saved.
    pub first: Option<PathBuf>,
}

/// The test program of `package`, built under `root`, that holds `test`
/// by its whole name.
pub fn program(root: &Path, package: &str, test: &str) -> Result<PathBuf, String> {
    let built = crate::checks::program("cargo")
        .args([
            "test",
            "--locked",
            "--no-run",
            "--message-format=json",
            "-p",
            package,
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo couldn't start: {e}"))?;
    if !built.status.success() {
        return Err(format!(
            "{package}'s tests don't build: {}",
            String::from_utf8_lossy(&built.stderr)
        ));
    }
    let programs = executables(&String::from_utf8_lossy(&built.stdout));
    let listed = format!("{test}: test");
    programs
        .into_iter()
        .find(|program| {
            crate::checks::program(&program.to_string_lossy())
                .args(["--list", "--format", "terse", "--exact", test])
                .env("LNK_TEST_ONE", "--list")
                .output()
                .is_ok_and(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .any(|l| l == listed)
                })
        })
        .ok_or(format!("no test program of {package} holds {test}"))
}

/// The test programs Cargo's JSON messages say it built.
pub fn executables(messages: &str) -> Vec<PathBuf> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true)
        .filter_map(|m| m["executable"].as_str().map(PathBuf::from))
        .collect()
}

/// Runs `test` of `program` as `copies` copies at once, their output in
/// `logs`, and counts the ones that failed.
pub fn run(program: &Path, test: &str, copies: usize, logs: &Path) -> std::io::Result<Copies> {
    std::fs::create_dir_all(logs)?;
    let mut running = Vec::with_capacity(copies);
    for copy in 0..copies {
        let log = logs.join(format!("{copy}.log"));
        let out = std::fs::File::create(&log)?;
        let child = crate::checks::program(&program.to_string_lossy())
            .args(["--exact", test])
            .stdin(std::process::Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .spawn()?;
        running.push((log, child));
    }
    let mut result = Copies {
        ran: copies,
        failed: 0,
        first: None,
    };
    for (log, mut child) in running {
        let passed = child.wait()?.success()
            && std::fs::read_to_string(&log)
                .is_ok_and(|said| said.contains("test result: ok. 1 passed"));
        if passed {
            let _ = std::fs::remove_file(&log);
        } else {
            result.failed += 1;
            match &result.first {
                None => result.first = Some(log),
                Some(_) => {
                    let _ = std::fs::remove_file(&log);
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_test_programs_are_read_from_cargos_messages() {
        let messages = concat!(
            r#"{"reason":"compiler-artifact","profile":{"test":true},"executable":"/t/deps/a-1"}"#,
            "\n",
            r#"{"reason":"compiler-artifact","profile":{"test":false},"executable":"/t/lnk"}"#,
            "\n",
            r#"{"reason":"compiler-artifact","profile":{"test":true},"executable":null}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
            "\n",
        );
        assert_eq!(
            super::executables(messages),
            [std::path::PathBuf::from("/t/deps/a-1")]
        );
    }
}
