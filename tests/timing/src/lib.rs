//! Shared execution timing reports from saved Cargo output or typed cases.
//!
//! Compiler artifacts identify programs without guessing from common test
//! names. The libtest summary measures a whole program, including its
//! concurrent tests, rather than the sum of individual test durations.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod check;
pub mod checks;
pub mod copies;
mod parse;
mod phases;
mod render;
pub mod single;

pub use phases::{Phase, PhaseMeasurements};
pub use render::html;

/// The measured execution model, sharing aggregation and presentation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MeasurementKind {
    #[default]
    TestPrograms,
    Cases,
}

/// Context supplied by the person who captured a run, never guessed.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Context {
    pub revision: Option<String>,
    pub machine: Option<String>,
    pub threads: Option<String>,
    pub status: Option<i32>,
    pub wall_seconds: Option<f64>,
    #[serde(default)]
    pub filtered_skip_reason: Option<String>,
}

/// An observed test program or case. Missing measurements remain unknown.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Program {
    pub package: Option<String>,
    pub group: String,
    pub target: String,
    pub executable: Option<String>,
    pub outcome: Option<String>,
    pub seconds: Option<f64>,
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
    pub filtered: u64,
    /// Each test that failed in it, by its path, with the first line of
    /// what it said when it failed.
    pub failures: Vec<Failure>,
}

/// A test that failed, by its path in its program, and the first line it
/// panicked with, when the log holds one.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Failure {
    pub test: String,
    pub said: Option<String>,
}

/// Module-owned packages share a group; unrelated packages stay separate.
#[derive(Debug, Deserialize, Serialize)]
pub struct Group {
    pub name: String,
    pub seconds: f64,
    pub programs: usize,
    pub incomplete: usize,
    #[serde(default)]
    pub unmeasured: usize,
    #[serde(default)]
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
    #[serde(default)]
    pub skipped: u64,
}

/// The saved report's version: 2 since each program names its failing
/// tests.
pub const SCHEMA: u32 = 2;

/// Versioned saved measurement. A zero or unknown duration is explicit.
#[derive(Debug, Deserialize, Serialize)]
pub struct Report {
    pub schema: u32,
    #[serde(default)]
    pub kind: MeasurementKind,
    pub context: Context,
    pub compilation_seconds: Option<f64>,
    pub measured_program_seconds: f64,
    pub complete: bool,
    pub programs: Vec<Program>,
    pub groups: Vec<Group>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub validation: Option<PhaseMeasurements>,
}

impl Report {
    /// Read a saved report with the same version and numeric checks for all adapters.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let report: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if report.schema != SCHEMA {
            return Err(format!("unsupported report schema {}", report.schema));
        }
        let valid = |s: f64| s.is_finite() && s >= 0.0;
        if !valid(report.measured_program_seconds)
            || report.context.wall_seconds.is_some_and(|s| !valid(s))
            || report.compilation_seconds.is_some_and(|s| !valid(s))
            || report
                .programs
                .iter()
                .any(|p| p.seconds.is_some_and(|s| !valid(s)))
            || report.groups.iter().any(|g| {
                !valid(g.seconds) || g.incomplete > g.programs || g.unmeasured > g.programs
            })
        {
            return Err("report durations and measurement counts must be valid".into());
        }
        if let Some(phases) = &report.validation {
            PhaseMeasurements::parse(&serde_json::to_string(phases).map_err(|e| e.to_string())?)?;
        }
        Ok(report)
    }
}

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub workspace_root: String,
    pub workspace_members: Vec<String>,
    pub packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: String,
    pub targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
}

impl Metadata {
    fn members(&self) -> impl Iterator<Item = &Package> {
        self.packages
            .iter()
            .filter(|p| self.workspace_members.contains(&p.id))
    }

    fn group(&self, package: &Package) -> String {
        let dir = parent(&package.manifest_path);
        let root = self.workspace_root.replace('\\', "/");
        let relative = dir.strip_prefix(&format!("{root}/")).unwrap_or(&dir);
        let module = |path: &str| {
            path.starts_with("modules/")
                || path.starts_with("adapters/")
                || path.contains("/modules/")
                || path.contains("/adapters/")
        };
        let owner = self
            .members()
            .filter(|p| p.targets.iter().any(|t| t.kind.iter().any(|k| k == "bin")))
            .map(|p| parent(&p.manifest_path))
            .filter(|p| dir == *p || dir.starts_with(&format!("{p}/")))
            .filter(|p| module(p.strip_prefix(&format!("{root}/")).unwrap_or(p)))
            .max_by_key(String::len);
        if let Some(owner) = owner {
            return format!(
                "module: {}",
                owner.strip_prefix(&format!("{root}/")).unwrap_or(&owner)
            );
        }
        let category = if relative.starts_with("tests/") || relative.contains("/tests/") {
            "tests"
        } else if relative.starts_with("core/") || relative.contains("/core/") {
            "core"
        } else {
            "shared"
        };
        format!("{category}: {relative}")
    }
}

fn parent(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_owned())
        .unwrap_or_default()
}

/// Parse one Cargo invocation. The caller provides its actual exit status.
pub fn report(log: &str, metadata: &Metadata, context: Context) -> Report {
    let (programs, compilation_seconds, warnings) = parse::programs(log, metadata);
    let mut report = measured(MeasurementKind::TestPrograms, context, programs, warnings);
    report.compilation_seconds = compilation_seconds;
    report
}

/// Aggregate typed measurements from an execution adapter without running it.
pub fn measured(
    kind: MeasurementKind,
    mut context: Context,
    mut programs: Vec<Program>,
    mut warnings: Vec<String>,
) -> Report {
    if context
        .wall_seconds
        .is_some_and(|s| !s.is_finite() || s < 0.0)
    {
        context.wall_seconds = None;
        warnings.push("Invalid command wall duration; overall elapsed is unknown.".into());
    }
    for program in &mut programs {
        if program.seconds.is_some_and(|s| !s.is_finite() || s < 0.0) {
            warnings.push(format!(
                "Invalid duration for {}; timing is unknown.",
                program.target
            ));
            program.seconds = None;
        }
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for program in &programs {
        let group = groups
            .entry(program.group.clone())
            .or_insert_with(|| Group {
                name: program.group.clone(),
                seconds: 0.0,
                programs: 0,
                incomplete: 0,
                unmeasured: 0,
                passed: 0,
                failed: 0,
                ignored: 0,
                skipped: 0,
            });
        group.programs += 1;
        group.seconds += program.seconds.unwrap_or(0.0);
        group.incomplete += usize::from(program.outcome.is_none());
        group.unmeasured += usize::from(program.seconds.is_none());
        group.passed += program.passed;
        group.failed += program.failed;
        group.ignored += program.ignored;
        group.skipped += program.ignored
            + if context.filtered_skip_reason.is_some() {
                program.filtered
            } else {
                0
            };
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then(a.name.cmp(&b.name)));
    let measured_program_seconds = groups.iter().map(|g| g.seconds).sum();
    let complete = context.status == Some(0)
        && warnings.is_empty()
        && !programs.is_empty()
        && programs.iter().all(|p| {
            p.package.is_some()
                && p.seconds.is_some()
                && matches!(p.outcome.as_deref(), Some("ok" | "pass"))
                && p.failed == 0
        });
    if context.status.is_none() {
        warnings.push(
            match kind {
                MeasurementKind::TestPrograms => {
                    "Cargo exit status was not supplied; completion is unknown."
                }
                MeasurementKind::Cases => {
                    "Run exit status was not captured; completion is unknown."
                }
            }
            .into(),
        );
    }
    if programs.is_empty() {
        warnings.push("No executions were found; this is not a complete measurement.".into());
    }
    if programs.iter().any(|p| p.package.is_none()) {
        warnings.push(match kind {
            MeasurementKind::TestPrograms => "Some programs are unassigned. Capture compiler artifacts with --message-format=json-render-diagnostics and matching metadata.",
            MeasurementKind::Cases => "Some cases have no recorded owning member; their grouping is unknown.",
        }.into());
    }
    Report {
        schema: SCHEMA,
        kind,
        context,
        compilation_seconds: None,
        measured_program_seconds,
        complete,
        programs,
        groups,
        warnings,
        validation: None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(system);
