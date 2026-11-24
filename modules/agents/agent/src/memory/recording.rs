//! What a memory service serves: each agent's [`Record`], and the
//! generation they make, kept while no record changes.

use super::*;
use link_format::WithinExt;

/// What the memory service of an agent's owner serves it with, as
/// [`define`] kept it ([`settings::memory_record_of`]).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Record {
    /// The engine's program.
    pub(super) program: PathBuf,
    /// The agent's Home, read.
    pub(super) folder: PathBuf,
    /// What it reads of the agent's own folder: its conversations and
    /// skills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) reads: Vec<PathBuf>,
    /// The other modules' programs its engine runs, installed, read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) programs: Vec<PathBuf>,
    /// The engine's state for it, written.
    pub(super) state: PathBuf,
    /// The folder its notes are in, written when it may write them.
    pub(super) notes: Option<PathBuf>,
    /// Whether it reaches this machine's models.
    pub(super) network: bool,
    /// Its embedding model, served for it outside.
    pub(super) model: Option<String>,
    /// What its engine's `command` printed for it.
    pub(super) argv: Vec<String>,
    /// Where its conversations are kept: the drivers it reads them with.
    #[serde(default)]
    pub(super) store: Store,
    /// The agent, by its owner and name, whose storage bridge its memory
    /// reaches (`connect`), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) whose: Option<Whose>,
}

link_files::specification!(Record, include_str!("recording/record.schema.toml"));

/// An agent whose memory a service serves: its owner, and its name.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Whose {
    pub(super) owner: String,
    pub(super) name: String,
}

/// Every agent the memory service in `dir` serves, by key, as their
/// records say now.
pub(super) fn records(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Record>> {
    let mut records = std::collections::BTreeMap::new();
    let entries = match std::fs::read_dir(dir.join("agents")) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(records),
        Err(err) => return Err(err).within(format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(key) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|key| settings::is_key(key))
        else {
            continue;
        };
        let record: Record = link_files::saved::parse_json(
            &std::fs::read(&path).within(format!("reading {}", path.display()))?,
        )
        .within(format!("reading {}", path.display()))?;
        records.insert(key.to_string(), record);
    }
    Ok(records)
}

/// The name of what serves `records`: the same agents served the same
/// way are one service, and a change starts another beside it, the old
/// one ending after its last session.
pub(super) fn generation(records: &std::collections::BTreeMap<String, Record>) -> String {
    let json = serde_json::to_vec(records).unwrap_or_default();
    link_digest::sha256(&json)[..16].to_string()
}

/// Each record in `dir` by key, as the folder says of its file without
/// opening it: which file it is, when it was written, and its size.
pub(super) fn stamps(dir: &Path) -> anyhow::Result<Stamps> {
    use std::os::unix::fs::MetadataExt;
    let mut stamps = Stamps::new();
    let entries = match std::fs::read_dir(dir.join("agents")) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(stamps),
        Err(err) => return Err(err).within(format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(key) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|key| settings::is_key(key))
        else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        stamps.insert(
            key.to_string(),
            (meta.ino(), meta.mtime(), meta.mtime_nsec(), meta.size()),
        );
    }
    Ok(stamps)
}

/// A record's file as [`stamps`] tells it apart: its inode, when it was
/// written (seconds and nanoseconds) and its size. A record is written
/// whole, as a new file.
type Stamps = std::collections::BTreeMap<String, (u64, i64, i64, u64)>;

/// The [`generation`] of the records in `dir` stamped `stamps`, as
/// [`GENERATION`] kept it for files stamped so; else read and hashed,
/// and kept for the next session while no record changed meanwhile. So
/// a session reads one file, not every record of its owner's agents.
pub(super) fn current_generation(dir: &Path, stamps: Stamps) -> anyhow::Result<String> {
    let kept = dir.join(GENERATION);
    if let Some(seen) = std::fs::read(&kept)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Seen>(&bytes).ok())
        .filter(|seen| seen.stamps == stamps)
    {
        return Ok(seen.generation);
    }
    let generation = generation(&records(dir)?);
    if self::stamps(dir).is_ok_and(|now| now == stamps) {
        let seen = Seen {
            generation: generation.clone(),
            stamps,
        };
        let _ = link_files::write_private(&kept, &link_files::saved::print_json(&seen)?);
    }
    Ok(generation)
}

/// Where a memory service's folder keeps the generation its records
/// make ([`current_generation`]), outside its working folder, which its
/// engine writes.
const GENERATION: &str = "generation.json";

/// A [`generation`] and the records' [`stamps`] it was made of.
#[derive(serde::Serialize, serde::Deserialize)]
struct Seen {
    generation: String,
    stamps: Stamps,
}

link_files::specification!(Seen, include_str!("recording/seen.schema.toml"));

/// The socket of service `generation`, in its working folder ([`working`]:
/// the service and its sessions both work there, as a Unix socket's path
/// is short).
pub(super) fn socket_name(generation: &str) -> String {
    format!("memory-{generation}.sock")
}

/// The service's working folder in `dir`, the only one of it its sandbox
/// writes, made: the records of what it serves, what it's handed of
/// them and its log are beside it, where the engine can't change what
/// its next start is confined to, or point the log elsewhere.
pub(super) fn working(dir: &Path) -> anyhow::Result<PathBuf> {
    let run = link_files::layout::MemoryService::at(dir).working();
    link_files::create_private_dir(&run).within(format!("creating {}", run.display()))?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every agent's record is served, its ID starting with a digit or a
    /// letter, or its name when it has none; a file of another name isn't.
    #[test]
    fn every_agents_record_is_served() {
        let dir = std::env::temp_dir().join(format!("lnk-memory-records-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("agents")).unwrap();
        let record = Record {
            program: "/bin/true".into(),
            folder: "/f".into(),
            reads: Vec::new(),
            programs: Vec::new(),
            state: "/s".into(),
            notes: None,
            network: false,
            model: None,
            argv: Vec::new(),
            store: Store::default(),
            whose: None,
        };
        for key in ["0a1b2c3d4e5f", "a1b2c3d4e5f6", "main", "Not A Key"] {
            std::fs::write(
                dir.join("agents").join(format!("{key}.json")),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        let keys: Vec<String> = records(&dir).unwrap().into_keys().collect();
        assert_eq!(keys, ["0a1b2c3d4e5f", "a1b2c3d4e5f6", "main"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session's generation is the records' as they are, kept for the
    /// next while no record changes, and made again once one does, or
    /// one goes.
    #[test]
    fn a_generation_is_kept_until_a_record_changes() {
        let dir = std::env::temp_dir().join(format!("lnk-memory-gen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("agents")).unwrap();
        let mut record = Record {
            program: "/bin/true".into(),
            folder: "/f".into(),
            reads: Vec::new(),
            programs: Vec::new(),
            state: "/s".into(),
            notes: None,
            network: false,
            model: None,
            argv: Vec::new(),
            store: Store::default(),
            whose: None,
        };
        let write = |key: &str, record: &Record| {
            link_files::write_private(
                &dir.join("agents").join(format!("{key}.json")),
                &serde_json::to_vec(record).unwrap(),
            )
            .unwrap()
        };
        write("a1b2c3d4e5f6", &record);
        write("b1b2c3d4e5f6", &record);
        let now = || current_generation(&dir, stamps(&dir).unwrap()).unwrap();
        let first = now();
        assert_eq!(first, generation(&records(&dir).unwrap()));
        assert!(dir.join(GENERATION).exists());
        // Kept: a record unreadable now is never read.
        std::fs::write(dir.join(GENERATION), {
            let mut seen: Seen =
                serde_json::from_slice(&std::fs::read(dir.join(GENERATION)).unwrap()).unwrap();
            seen.generation = "kept".into();
            serde_json::to_vec(&seen).unwrap()
        })
        .unwrap();
        assert_eq!(now(), "kept");
        // A record written again, or gone: made again.
        record.network = true;
        write("a1b2c3d4e5f6", &record);
        let second = now();
        assert_eq!(second, generation(&records(&dir).unwrap()));
        assert_ne!(second, first);
        std::fs::remove_file(dir.join("agents/b1b2c3d4e5f6.json")).unwrap();
        assert_eq!(now(), generation(&records(&dir).unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
