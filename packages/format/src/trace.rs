//! The trace: where in Link's code it went wrong, in a release build too.
//! Release builds are stripped, so a Rust backtrace there has no names;
//! instead each step records its own place, from `#[track_caller]`, which
//! a stripped binary keeps.

use std::fmt;
use std::panic::Location;

use serde::{Deserialize, Serialize};

/// The steps from a report down to what failed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Trace {
    pub steps: Vec<Step>,
}

/// One step: what it was, its place in the code (the workspace's path and
/// line, never the build machine's), and the program it ran in when it
/// came from another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub program: Option<String>,
}

impl Trace {
    /// A trace of one step: the caller's place.
    #[track_caller]
    pub fn here() -> Self {
        Trace {
            steps: vec![Step {
                what: String::new(),
                at: Some(place(Location::caller())),
                program: None,
            }],
        }
    }

    /// `err`'s chain, each link that came through [`Within`] with its place.
    pub fn of(err: &anyhow::Error) -> Self {
        let steps = err
            .chain()
            .map(|cause| match cause.downcast_ref::<Within>() {
                Some(w) => Step {
                    what: w.what.clone(),
                    at: Some(w.at.clone()),
                    program: None,
                },
                None => Step {
                    what: crate::clean(&cause.to_string()),
                    at: None,
                    program: None,
                },
            })
            .collect();
        Trace { steps }
    }

    pub fn extend(&mut self, other: Trace) {
        self.steps.extend(other.steps);
    }

    /// Every step as from `program`, for a trace read from another's report.
    pub fn from_program(mut self, program: &str) -> Self {
        for step in &mut self.steps {
            step.program.get_or_insert_with(|| program.to_string());
        }
        self
    }
}

/// A place in the code, from the workspace's root.
pub(crate) fn place(at: &Location<'_>) -> String {
    format!("{}:{}", at.file(), at.line())
}

/// A cause added on the way up, with its place: `link-format`'s
/// `context`. Shown as its words, as anyhow shows a context; its own
/// error type, so the trace can read its place back.
#[derive(Debug)]
pub struct Within {
    what: String,
    at: String,
    source: Option<anyhow::Error>,
}

impl fmt::Display for Within {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Within {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| &**e as &(dyn std::error::Error + 'static))
    }
}

/// `.within("installing hermes")` where anyhow has `.context(...)`: the
/// same chain, each link with its file and line.
pub trait WithinExt<T> {
    #[track_caller]
    fn within(self, what: impl fmt::Display) -> anyhow::Result<T>;
}

impl<T, E: Into<anyhow::Error>> WithinExt<T> for Result<T, E> {
    #[track_caller]
    fn within(self, what: impl fmt::Display) -> anyhow::Result<T> {
        let at = place(Location::caller());
        self.map_err(|e| {
            anyhow::Error::new(Within {
                what: what.to_string(),
                at,
                source: Some(e.into()),
            })
        })
    }
}

impl<T> WithinExt<T> for Option<T> {
    #[track_caller]
    fn within(self, what: impl fmt::Display) -> anyhow::Result<T> {
        let at = place(Location::caller());
        self.ok_or_else(|| {
            anyhow::Error::new(Within {
                what: what.to_string(),
                at,
                source: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_step_has_its_place() {
        fn install() -> anyhow::Result<()> {
            Err(anyhow::anyhow!("pip exited with status 1")).within("ran the installer")
        }
        let err = install().within("installing hermes").unwrap_err();
        let trace = Trace::of(&err);
        assert_eq!(trace.steps.len(), 3);
        assert_eq!(trace.steps[0].what, "installing hermes");
        assert!(trace.steps[0].at.as_deref().unwrap().contains("trace.rs:"));
        assert_eq!(trace.steps[2].what, "pip exited with status 1");
        assert_eq!(trace.steps[2].at, None);
        // Its words are anyhow's, so nothing printing the chain changes.
        assert_eq!(
            format!("{err:#}"),
            "installing hermes: ran the installer: pip exited with status 1"
        );
    }

    #[test]
    fn a_trace_crosses_programs() {
        let inner: Trace =
            serde_json::from_str(r#"[{"what":"no key","at":"model/src/key.rs:9"}]"#).unwrap();
        let inner = inner.from_program("lnk-model");
        assert_eq!(inner.steps[0].program.as_deref(), Some("lnk-model"));
    }
}
