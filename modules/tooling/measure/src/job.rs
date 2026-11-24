//! Retained job profiles, obtained from their owner through `measure.job`.
//! Reading and exporting never consume a profile or start the watcher.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, ensure};
use link_format::{Done, Foreign, Out, Report, Table, WithinExt, cmd, phrase, sentence};
use link_measure_contract::client::{By, Kind, LINE_MAX, Report as Sample, asked, otlp, read_otlp};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    max_bytes: u64,
}

fn limits() -> &'static Limits {
    static LIMITS: std::sync::OnceLock<Limits> = std::sync::OnceLock::new();
    LIMITS.get_or_init(|| toml::from_str(include_str!("job.toml")).expect("job profile limits"))
}

/// Read only the selected job and its child stages. Unknown units and
/// nonnumeric attributes cannot reach the view or the exported request.
pub fn read(path: &Path, id: &str) -> anyhow::Result<Vec<Sample>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .context("opening the retained job profile")?;
    ensure!(
        file.metadata()?.is_file(),
        "a profile must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(limits().max_bytes + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limits().max_bytes,
        "the job profile exceeds the read limit"
    );
    let mut samples = Vec::new();
    for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        ensure!(
            line.len() <= LINE_MAX,
            "a job profile line exceeds the read limit"
        );
        let request: Value = serde_json::from_slice(line).context("reading job profile OTLP")?;
        for mut sample in read_otlp(&request) {
            let unit = &sample.unit;
            let selected = (unit.kind == "job" && unit.id == id && unit.parent.is_none())
                || (unit.kind == "stage"
                    && unit.parent.as_deref() == Some(id)
                    && unit.id.starts_with(&format!("{id}/")));
            if !unit.valid() || !selected {
                continue;
            }
            sample.points.retain(|p| p.measure.by == By::Outside);
            samples.push(sample);
        }
    }
    ensure!(
        samples
            .iter()
            .any(|s| s.unit.kind == "job" && !s.points.is_empty()),
        "the job has no completed numeric profile"
    );
    Ok(samples)
}

fn profile(id: &str) -> anyhow::Result<Vec<Sample>> {
    ensure!(
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "a job ID contains only letters, digits and hyphens"
    );
    let mut found = None;
    for answer in asked::JOB.ask(&asked::Job { id: id.into() }) {
        if let Some(path) = answer.reply? {
            ensure!(
                found.is_none(),
                "more than one module owns this job profile"
            );
            found = Some(path);
        }
    }
    let path = found.context("no installed provider has a retained profile for that job")?;
    read(&path, id)
}

/// A job and its child units, with the most recent value of each
/// counter and the largest observed memory use. Missing stays absent.
pub fn json(samples: &[Sample]) -> Value {
    let mut units: BTreeMap<&str, Value> = BTreeMap::new();
    let mut at = 0;
    for sample in samples {
        let unit = &sample.unit;
        let value = units.entry(&unit.id).or_insert_with(|| {
            json!({
                "id": unit.id, "kind": unit.kind, "parent": unit.parent, "name": unit.name,
                "measures": {},
            })
        });
        for point in &sample.points {
            at = at.max(point.at);
            let old = &value["measures"][point.measure.name];
            let replace = match point.measure.kind {
                Kind::Gauge => old["value"].as_f64().is_none_or(|v| v < point.value),
                _ => old["at"].as_u64().is_none_or(|t| t <= point.at),
            };
            if replace {
                value["measures"][point.measure.name] =
                    json!({"value": point.value, "at": point.at});
            }
        }
    }
    let mut units: Vec<Value> = units.into_values().collect();
    units.sort_by_key(|u| u["kind"] != "job");
    json!({"at": at, "units": units})
}

pub fn show(out: &mut Out, id: &str) -> Result<Done, Report> {
    let samples = profile(id).within("reading the job's retained profile")?;
    let value = json(&samples);
    out.reply(&value);
    if !out.is_json() {
        let mut table = Table::new(
            ["Unit", "Kind", "Measure", "Value", "Unit of value"],
            sentence!("No measurements were retained for this job."),
        );
        for unit in value["units"].as_array().into_iter().flatten() {
            for measure in link_measure_contract::MEASURES {
                if let Some(number) = unit["measures"][measure.name]["value"].as_f64() {
                    table.row([
                        link_format::Value::from(Foreign::new(
                            unit["id"].as_str().unwrap_or_default(),
                        )),
                        Foreign::new(unit["kind"].as_str().unwrap_or_default()).into(),
                        measure.about.into(),
                        number.to_string().into(),
                        measure.unit.into(),
                    ]);
                }
            }
        }
        out.block(table);
    }
    Ok(Done::shown().next(
        cmd!("lnk measure export set --job {}", id),
        phrase!("send this profile to your configured endpoint"),
    ))
}

pub fn export(id: &str) -> Result<Done, Report> {
    let export = crate::settings::yours()
        .map_err(crate::settings::unusable)?
        .export
        .context("configure an endpoint with lnk measure export set --url <url> first")?;
    let samples = profile(id).within("reading the job's retained profile")?;
    crate::export::once(&export, &otlp(&samples, "lnk-job")).within("sending the job profile")?;
    Ok(Done::success(sentence!("The job profile was sent.")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_measure_contract::{Point, Unit};
    use link_test_kit::TempDir;

    #[test]
    fn reads_job_and_stages_but_never_unrelated_or_unknown_units() {
        let root = TempDir::new("job-measure");
        let samples = [
            ("job", "a1", None),
            ("stage", "a1/build", Some("a1")),
            ("job", "other", None),
            ("unknown", "a1", None),
            ("stage", "else/build", Some("a1")),
        ]
        .map(|(kind, id, parent)| Sample {
            unit: Unit {
                id: id.into(),
                kind: kind.into(),
                parent: parent.map(String::from),
                ..Unit::default()
            },
            points: vec![Point::new("link.cpu.used.sum", 1.0, 20)],
        });
        let path = root.join("profile.jsonl");
        std::fs::write(&path, otlp(&samples, "lnk-job").to_string()).unwrap();
        let read = read(&path, "a1").unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read_otlp(&otlp(&read, "lnk-job")), read);
        assert_eq!(json(&read)["units"][0]["kind"], "job");
        assert_eq!(json(&read)["units"][1]["parent"], "a1");
        assert!(read.iter().all(|s| s.unit.valid()));
        assert!(super::read(&path, "absent").is_err());
    }

    #[test]
    fn corrupt_incomplete_and_linked_profiles_are_errors() {
        let root = TempDir::new("job-measure-errors");
        let path = root.join("profile.jsonl");
        for text in ["", "not JSON", "{}"] {
            std::fs::write(&path, text).unwrap();
            assert!(read(&path, "a1").is_err());
        }
        std::os::unix::fs::symlink(&path, root.join("linked")).unwrap();
        assert!(read(&root.join("linked"), "a1").is_err());
    }
}
