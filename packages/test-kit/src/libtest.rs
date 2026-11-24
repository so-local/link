//! Selection and summaries for a test program with a `main` of its own.
//!
//! This module executes nothing. The custom main owns its execution and
//! failure behavior, reports actual outcomes, and leaves child modes alone.

use std::time::Duration;

/// Actual outcomes of a custom test program, including explicit non-runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub filtered: usize,
}

impl Counts {
    /// The same final summary as libtest, with the program's elapsed time.
    pub fn summary(self, elapsed: Duration) -> String {
        let result = if self.failed == 0 { "ok" } else { "FAILED" };
        format!(
            "test result: {result}. {} passed; {} failed; {} ignored; 0 measured; {} filtered out; finished in {:.2}s",
            self.passed,
            self.failed,
            self.ignored,
            self.filtered,
            elapsed.as_secs_f64()
        )
    }
}

/// Supported libtest selection flags for custom mains with no ignored cases,
/// with the flags a system program's runner passes each test's process.
#[derive(clap::Args, Debug, Default)]
pub struct Selection {
    #[arg(long)]
    pub list: bool,
    #[arg(long)]
    exact: bool,
    #[arg(long = "ignored")]
    ignored_only: bool,
    #[arg(long)]
    show_output: bool,
    /// libtest's list formats, as a system program's runner lists with
    /// `terse`; a custom main lists one way.
    #[arg(long, value_parser = ["pretty", "terse"])]
    format: Option<String>,
    /// Output is never captured in a custom main.
    #[arg(long)]
    nocapture: bool,
    /// A custom main runs its cases on one thread.
    #[arg(long)]
    test_threads: Option<usize>,
    /// A custom main has no ignored cases, so this adds none.
    #[arg(long)]
    include_ignored: bool,
    include: Vec<String>,
    #[arg(long)]
    skip: Vec<String>,
}

impl Selection {
    /// Parse selection, never interpreting an option's value as a filter.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Options {
            #[command(flatten)]
            selection: Selection,
        }
        Options::try_parse_from(std::iter::once("test".to_string()).chain(args))
            .map(|given| given.selection)
            .map_err(|error| error.to_string())
    }

    /// Whether an ordinary, non-ignored case is selected by the arguments.
    pub fn matches(&self, name: &str) -> bool {
        let matches = |filter: &String| {
            if self.exact {
                name == filter
            } else {
                name.contains(filter)
            }
        };
        !self.ignored_only
            && (self.include.is_empty() || self.include.iter().any(matches))
            && !self.skip.iter().any(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(args: &[&str]) -> Selection {
        Selection::parse(args.iter().map(|s| (*s).to_owned())).unwrap()
    }

    #[test]
    fn summary_preserves_actual_counts_and_elapsed_time() {
        let counts = Counts {
            passed: 2,
            failed: 1,
            ignored: 3,
            filtered: 4,
        };
        assert_eq!(
            counts.summary(Duration::from_millis(1234)),
            "test result: FAILED. 2 passed; 1 failed; 3 ignored; 0 measured; 4 filtered out; finished in 1.23s"
        );
        assert_eq!(
            Counts::default().summary(Duration::ZERO),
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
        );
    }

    #[test]
    fn skips_do_not_become_include_filters_and_exact_applies_to_both() {
        let options = selection(&["--show-output", "--skip", "first"]);
        assert!(!options.matches("first"));
        assert!(options.matches("second"));
        let options = selection(&["group", "--skip=group::first"]);
        assert!(!options.matches("group::first"));
        assert!(options.matches("group::second"));
        assert!(!options.matches("other"));
        let options = selection(&["--exact", "group::first", "--skip", "first"]);
        assert!(options.matches("group::first"));
        assert!(!options.matches("group::first::child"));
    }

    #[test]
    fn list_and_ignored_do_not_silently_select_ordinary_cases() {
        assert!(selection(&["--list"]).list);
        assert!(!selection(&["--ignored"]).matches("first"));
        assert!(Selection::parse(["--skip".into()]).is_err());
        assert!(Selection::parse(["--unknown".into()]).is_err());
        let runner = selection(&[
            "--exact",
            "a",
            "--nocapture",
            "--test-threads",
            "1",
            "--include-ignored",
        ]);
        assert!(runner.matches("a"));
        assert!(selection(&["--list", "--format", "terse"]).list);
        assert!(Selection::parse(["--format".into(), "json".into()]).is_err());
        assert!(Selection::parse(["--skip".into(), "--list".into()]).is_err());
    }
}
