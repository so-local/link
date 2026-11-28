//! A failure's record: everything about it, paths and all, kept on this
//! machine for the person and for `lnk issue create`, which scopes and scrubs
//! what leaves it. The screen shows its path.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::{Command, Report};

/// How many records a machine keeps: the newest.
pub const KEPT: usize = 20;

/// A record's format, which its own type can't declare, holding another
/// crate's values.
static FORMAT: std::sync::LazyLock<link_files::schema::Format> = std::sync::LazyLock::new(|| {
    link_files::schema::Format::shipped(include_str!("record.schema.toml"))
});

/// A record's text read, its schema advanced: `lnk issue create` reads it.
pub fn parse(bytes: &[u8]) -> anyhow::Result<serde_json::Value> {
    Ok(FORMAT.read_json(bytes)?.spec)
}

/// What a record holds: which `lnk`, where, the command as typed, when,
/// and the whole report with its trace.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Record {
    pub version: String,
    pub platform: String,
    pub command: Command,
    pub time: crate::Time,
    pub report: Report,
    /// What `lnk issue create` makes of it: scoped to the report and scrubbed.
    pub issue: crate::issue::Issue,
}

/// `lnk 0.0.1`: the version this program was built as.
pub fn version() -> String {
    format!("lnk {}", env!("CARGO_PKG_VERSION"))
}

/// `linux x86_64`.
pub fn platform() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

impl Record {
    /// `report`'s record, from `command` as typed: as scrubbed of what
    /// looks like a secret as its issue is, the value of each flag the
    /// scrubber takes for a secret's (`--token`, `--password`) shown as
    /// `<secret>`, and each tail line the scrubber leaves out, left out.
    pub fn new(report: &Report, command: Command) -> Self {
        let scrub = crate::scrub::Scrub::here();
        let flags = secret_flags(&command, &scrub);
        let command = command.hiding(&flags.iter().map(String::as_str).collect::<Vec<_>>());
        let mut report = report.clone();
        if let Some(tail) = &mut report.tail {
            tail.lines.retain(|line| scrub.line(&line.0).is_some());
        }
        Record {
            version: version(),
            platform: platform(),
            issue: crate::issue::Issue::of(&report, &command, &scrub),
            command,
            time: crate::Time(SystemTime::now()),
            report,
        }
    }

    /// Writes it in `dir`, private to this user, as JSON named by its
    /// time, and keeps only the newest [`KEPT`]: its path.
    pub fn write(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // Its time to the nanosecond, so names sort oldest first.
        let nanos = self
            .time
            .0
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let stamp = crate::time::rfc3339(self.time.0)
            .trim_end_matches('Z')
            .replace(':', "-");
        let stamp = format!("{stamp}.{nanos:09}");
        let text = FORMAT.print_json(self).map_err(std::io::Error::other)?;
        let mut n = 0;
        let (path, mut file) = loop {
            let name = match n {
                0 => format!("{stamp}.json"),
                n => format!("{stamp}-{n:03}.json"),
            };
            let path = dir.join(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => break (path, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
                Err(e) => return Err(e),
            }
        };
        file.write_all(&text)?;
        file.write_all(b"\n")?;
        prune(dir);
        Ok(path)
    }
}

/// The flags of `command` (`--bot-token`, `--password=...`) whose value
/// the scrubber takes for a secret: the line `<flag>=` it leaves out.
fn secret_flags(command: &Command, scrub: &crate::scrub::Scrub) -> Vec<String> {
    let mut flags = Vec::new();
    for word in command.words() {
        let (crate::Word::Fixed(text) | crate::Word::Value(text)) = word else {
            continue;
        };
        if text == "--" {
            break;
        }
        let Some(flag) = text.strip_prefix("--").map(|_| text.split('=').next()) else {
            continue;
        };
        let flag = flag.unwrap_or(text).to_string();
        if scrub.line(&format!("{flag}=")).is_none() && !flags.contains(&flag) {
            flags.push(flag);
        }
    }
    flags
}

/// The records in `dir` past the newest [`KEPT`], removed. Their names
/// are their times, so they sort oldest first.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut records: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    records.sort();
    let over = records.len().saturating_sub(KEPT);
    for old in &records[..over] {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentence;

    #[test]
    fn a_record_is_private_json_and_only_the_newest_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_test_kit::TempDir::new("records");
        let dir = temp.join("records");
        let _ = std::fs::remove_dir_all(&dir);
        let r = Report::error(sentence!("Couldn't start it."));
        let typed = Command::typed(["lnk".into(), "agent".into(), "start".into()]);
        let mut written = Vec::new();
        for _ in 0..KEPT + 2 {
            written.push(Record::new(&r, typed.clone()).write(&dir).unwrap());
        }
        let first = &written[KEPT + 1];
        let doc = parse(&std::fs::read(first).unwrap()).unwrap();
        assert_eq!(doc["report"]["headline"], "Couldn't start it.");
        assert_eq!(doc["command"][2], "start");
        let mode = std::fs::metadata(first).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), KEPT);
        assert!(!written[0].exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A record keeps no more of a secret than its issue does: the value
    /// of a flag named for one, and a tail line that looks like one, are
    /// never in it.
    #[test]
    fn a_record_is_scrubbed_as_its_issue_is() {
        let report = Report::error(sentence!("Couldn't log in.")).tail(crate::Tail::last(
            "connecting to the registry\npassword: hunter2-in-the-tail",
            10,
        ));
        let typed = Command::typed(
            [
                "lnk",
                "example",
                "connect",
                "--bot-token",
                "s3cret-value",
                "--api-key=s3cret-joined",
                "--name",
                "main",
            ]
            .map(String::from),
        );
        let record = Record::new(&report, typed);
        let text = serde_json::to_string(&record).unwrap();
        for secret in ["s3cret-value", "s3cret-joined", "hunter2-in-the-tail"] {
            assert!(!text.contains(secret), "{secret}: {text}");
        }
        assert_eq!(
            record.command.text(),
            "lnk example connect --bot-token <secret> --api-key=<secret> --name main"
        );
        let tail = record.report.tail.as_ref().unwrap();
        assert_eq!(tail.lines.len(), 1);
        assert_eq!(tail.lines[0].0, "connecting to the registry");
    }
}
