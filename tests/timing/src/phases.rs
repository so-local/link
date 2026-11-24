//! Explicit wall measurements of validation commands, independent of test time.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One measured validation phase, or an explicit unknown/not-run phase.
#[derive(Debug, Deserialize, Serialize)]
pub struct Phase {
    pub name: String,
    pub wall_seconds: Option<f64>,
    pub status: Option<i32>,
}

/// Saved phase evidence. Overlapping work is not added into elapsed time.
#[derive(Debug, Deserialize, Serialize)]
pub struct PhaseMeasurements {
    pub schema: u32,
    pub elapsed_seconds: Option<f64>,
    pub cache_state: Option<String>,
    pub phases: Vec<Phase>,
}

impl PhaseMeasurements {
    /// Read a phase file without inferring absent durations or cache state.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let measurements: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if measurements.schema != 1 {
            return Err(format!("unsupported phase schema {}", measurements.schema));
        }
        let valid_time = |time: Option<f64>| time.is_none_or(|s| s.is_finite() && s >= 0.0);
        if !valid_time(measurements.elapsed_seconds) {
            return Err("phase elapsed_seconds must be finite and nonnegative".into());
        }
        let mut names = BTreeSet::new();
        let mut sum = 0.0;
        for phase in &measurements.phases {
            if phase.name.trim().is_empty() || !names.insert(&phase.name) {
                return Err("phase names must be nonempty and unique".into());
            }
            if !valid_time(phase.wall_seconds) {
                return Err(format!(
                    "{} wall_seconds must be finite and nonnegative",
                    phase.name
                ));
            }
            sum += phase.wall_seconds.unwrap_or(0.0);
        }
        if !sum.is_finite() {
            return Err("summed phase wall time must be finite".into());
        }
        Ok(measurements)
    }

    /// Sum of known command durations, not the elapsed validation cycle.
    pub fn summed_seconds(&self) -> Option<f64> {
        self.phases
            .iter()
            .filter_map(|p| p.wall_seconds)
            .reduce(|a, b| a + b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_phases_preserve_elapsed_separately_from_summed_work() {
        let phases = PhaseMeasurements::parse(r#"{"schema":1,"elapsed_seconds":150,"phases":[{"name":"clippy","wall_seconds":120,"status":0},{"name":"bin-build","wall_seconds":90,"status":101}]}"#).unwrap();
        assert_eq!(phases.summed_seconds(), Some(210.0));
        assert_eq!(phases.elapsed_seconds, Some(150.0));
        assert_eq!(phases.phases[1].status, Some(101));
        assert_eq!(phases.cache_state, None);
    }

    #[test]
    fn absent_measurements_are_unknown_and_zero_is_measured() {
        let phases = PhaseMeasurements::parse(r#"{"schema":1,"phases":[{"name":"fmt"},{"name":"clippy","wall_seconds":null,"status":null}]}"#).unwrap();
        assert_eq!(phases.summed_seconds(), None);
        assert_eq!(phases.elapsed_seconds, None);
        let phases = PhaseMeasurements::parse(
            r#"{"schema":1,"phases":[{"name":"fmt","wall_seconds":0,"status":0}]}"#,
        )
        .unwrap();
        assert_eq!(phases.summed_seconds(), Some(0.0));
    }

    #[test]
    fn invalid_durations_schema_and_ambiguous_names_are_rejected() {
        for raw in [
            r#"{"schema":2,"phases":[]}"#,
            r#"{"schema":1,"elapsed_seconds":-1,"phases":[]}"#,
            r#"{"schema":1,"phases":[{"name":"fmt","wall_seconds":-1}]}"#,
            r#"{"schema":1,"phases":[{"name":"fmt","wall_seconds":1e999}]}"#,
            r#"{"schema":1,"phases":[{"name":" "}]}"#,
            r#"{"schema":1,"phases":[{"name":"fmt"},{"name":"fmt"}]}"#,
            r#"{"schema":1,"phases":[{"name":"fmt","wall_seconds":1e308},{"name":"clippy","wall_seconds":1e308}]}"#,
        ] {
            assert!(PhaseMeasurements::parse(raw).is_err(), "accepted {raw}");
        }
    }
}
