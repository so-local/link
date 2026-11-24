//! A fresh capacity observation, independent of the usage watcher's cache.

use std::path::Path;

use link_format::{Done, Fields, Out, Report, Section, Size, Table, cmd, sentence};
use link_measure_contract::client::machine::Snapshot;

pub fn show(out: &mut Out, path: Option<&Path>) -> Result<Done, Report> {
    let current = std::env::current_dir().map_err(anyhow::Error::from)?;
    let temporary = std::env::temp_dir();
    let snapshot = Snapshot::capture(&[
        ("workspace", path.unwrap_or(&current)),
        ("temporary", &temporary),
    ]);
    out.reply(&exact(
        serde_json::to_value(&snapshot).map_err(anyhow::Error::from)?,
    ));
    let number = |value: Option<u64>| value.map_or("unknown".to_owned(), |n| n.to_string());
    let size = |value: Option<u64>| value.map_or("unknown".to_owned(), |n| format!("{} bytes", n));
    out.block(
        Section::new("Machine").with(
            Fields::new()
                .row("System", snapshot.os.clone())
                .row("Architecture", snapshot.architecture.clone())
                .row(
                    "Kernel",
                    snapshot.kernel.clone().unwrap_or_else(|| "unknown".into()),
                )
                .row(
                    "CPU",
                    snapshot
                        .cpu
                        .model
                        .clone()
                        .unwrap_or_else(|| "unknown".into()),
                )
                .row("Logical cores", number(snapshot.cpu.logical_cores))
                .row("Physical cores", number(snapshot.cpu.physical_cores))
                .row(
                    "Usable cores",
                    snapshot
                        .cpu
                        .available_cores
                        .map_or("unknown".into(), |n| n.to_string()),
                )
                .row(
                    "CPU quota",
                    snapshot
                        .cpu
                        .quota_cores
                        .map_or("unknown".into(), |n| n.to_string()),
                )
                .row("Observed clock (Hz)", number(snapshot.cpu.observed_hz))
                .row("Host memory", size(snapshot.memory.machine_total_bytes))
                .row("Memory limit", size(snapshot.memory.limit_bytes))
                .row("Available memory", size(snapshot.memory.available_bytes)),
        ),
    );
    let mut filesystems = Table::new(
        ["Filesystem", "Total", "Available", "Read only"],
        sentence!("No filesystem capacity could be observed."),
    );
    for filesystem in &snapshot.filesystems {
        let value = |n: Option<u64>| {
            n.map_or_else(|| link_format::Value::from("unknown"), |n| Size(n).into())
        };
        filesystems.row([
            filesystem.label.clone().into(),
            value(filesystem.total_bytes),
            value(filesystem.available_bytes),
            filesystem
                .read_only
                .map_or("unknown".into(), |v| v.to_string())
                .into(),
        ]);
    }
    out.block(filesystems);
    Ok(Done::shown().next(cmd!("lnk measure"), "see current resource use"))
}

/// `value` with every field that has no value left out, as its declared
/// shape says: a field it may leave out is absent, never null.
fn exact(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => fields
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k, exact(v)))
            .collect(),
        serde_json::Value::Array(items) => items.into_iter().map(exact).collect(),
        other => other,
    }
}
