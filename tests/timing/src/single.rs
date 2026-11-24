//! One exact Cargo test, interpreted through the shared libtest grammar.
use crate::Program;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Passed,
    Failed,
    Skipped(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Executed {
    pub outcome: Outcome,
    pub seconds: f64,
}

/// Requires one complete libtest summary and one selected test. Callers must
/// capture `--show-output` and preserve a failing command's exit status.
pub fn read(log: &str) -> Result<Executed, String> {
    let mut summary = Program {
        package: None,
        group: String::new(),
        target: String::new(),
        executable: None,
        outcome: None,
        seconds: None,
        passed: 0,
        failed: 0,
        ignored: 0,
        filtered: 0,
        failures: Vec::new(),
    };
    let summaries: Vec<&str> = log
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("test result:"))
        .collect();
    let line = match summaries.as_slice() {
        [] => return Err("no libtest summary".into()),
        [line] => *line,
        several => {
            return Err(format!(
                "{} libtest summaries: a test that runs its program again keeps \
                 its fixture's output in a log of its own (the testing kit's \
                 process::rerun)",
                several.len()
            ));
        }
    };
    if !crate::parse::summary(line, &mut summary) {
        return Err("a malformed libtest summary".into());
    }
    if summary.passed + summary.failed + summary.ignored != 1 {
        return Err("exactly one selected test must run or explicitly skip".into());
    }
    let outcome = if summary.failed != 0 || summary.outcome.as_deref() != Some("ok") {
        Outcome::Failed
    } else if summary.ignored != 0 {
        Outcome::Skipped("the selected test is ignored".into())
    } else if let Some(reason) = log.lines().find_map(|line| {
        line.split_once(": skipping, as it lacks ")
            .map(|(_, reason)| reason)
    }) {
        Outcome::Skipped(format!("the selected test lacks {reason}"))
    } else {
        Outcome::Passed
    };
    Ok(Executed {
        outcome,
        seconds: summary.seconds.ok_or("test elapsed time is unknown")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const PASSED: &str = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.02s";
    #[test]
    fn an_explicit_capability_skip_does_not_become_passing_coverage() {
        let log = format!("case: skipping, as it lacks Landlock: kernel refuses it\n{PASSED}");
        assert!(
            matches!(read(&log).unwrap().outcome, Outcome::Skipped(reason) if reason.contains("Landlock"))
        );
        assert_eq!(
            read(PASSED).unwrap(),
            Executed {
                outcome: Outcome::Passed,
                seconds: 0.02
            }
        );
    }
    #[test]
    fn unknown_counts_and_timings_are_not_success() {
        for log in [
            "",
            "test result: ok. 1 passed; 0 failed",
            &PASSED.replace("0.02s", "unknown"),
            &format!("{PASSED}\n{PASSED}"),
            &PASSED.replace("1 passed", "0 passed"),
        ] {
            assert!(read(log).is_err(), "{log}");
        }
        let twice = read(&format!("{PASSED}\n{PASSED}")).unwrap_err();
        assert!(twice.starts_with("2 libtest summaries"), "{twice}");
        let failed = PASSED
            .replace("ok.", "FAILED.")
            .replace("1 passed; 0 failed", "0 passed; 1 failed");
        assert_eq!(read(&failed).unwrap().outcome, Outcome::Failed);
        let ignored = PASSED.replace(
            "1 passed; 0 failed; 0 ignored",
            "0 passed; 0 failed; 1 ignored",
        );
        assert!(matches!(
            read(&ignored).unwrap().outcome,
            Outcome::Skipped(_)
        ));
    }
}
