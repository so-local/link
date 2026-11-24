//! A command's output, through descriptors only: written, cut past its
//! most, and read.

use super::*;

/// The output folder of `files`, made if it isn't there; never a link.
fn output_dir(files: &Path, make: bool) -> anyhow::Result<Dir> {
    let top = Dir::open(files).with_context(|| format!("opening {}", files.display()))?;
    let name = OsStr::new(FOLDER);
    let dir = match make {
        true => top.create_folder(name, 0o755),
        false => top.folder(name),
    };
    dir.with_context(|| format!("opening {FOLDER}/ in the files folder"))
}

/// File `name` of folder `dir`, opened with `flags`: a regular file,
/// never through a link, nor one with another name written through.
fn open_in(dir: &Dir, name: &str, flags: libc::c_int) -> anyhow::Result<File> {
    dir.open_file(OsStr::new(name), flags, 0o644)
        .with_context(|| format!("opening {FOLDER}/{name}"))
}

fn remove_in(dir: &Dir, name: &str) {
    let _ = dir.remove_file(OsStr::new(name));
}

pub(super) fn file_name(id: &str) -> String {
    format!("{id}.{ENDING}")
}

/// The size of command `id`'s output file, what's kept of it.
pub(super) fn output_size(files: &Path, id: &str) -> anyhow::Result<u64> {
    let dir = output_dir(files, false)?;
    Ok(open_in(&dir, &file_name(id), libc::O_RDONLY)?
        .metadata()?
        .len())
}

/// What `read` gives: the output from byte `from` to `to`, of `total`.
pub struct Read {
    pub from: u64,
    pub to: u64,
    pub total: u64,
    pub dropped: u64,
    pub text: String,
}

/// At most `most` bytes of `record`'s output from byte `from` of the
/// stream, or from the first kept when it was dropped.
pub fn read_output(record: &Record, from: u64, most: u64) -> anyhow::Result<Read> {
    use std::os::unix::fs::{FileExt, MetadataExt};
    let dir = output_dir(&record.files, false)?;
    let file = open_in(&dir, &file_name(&record.id), libc::O_RDONLY)?;
    let meta = file.metadata()?;
    if record.output_file != 0 && meta.ino() != record.output_file {
        return Err(Cutting.into());
    }
    let kept = meta.len();
    let total = record.dropped + kept;
    let from = from.clamp(record.dropped, total);
    let mut buffer = vec![0u8; (total - from).min(most) as usize];
    file.read_exact_at(&mut buffer, from - record.dropped)?;
    Ok(Read {
        from,
        to: from + buffer.len() as u64,
        total,
        dropped: record.dropped,
        text: String::from_utf8_lossy(&buffer).into_owned(),
    })
}

/// The output was just cut, and its record doesn't say so yet: read
/// again, with the record loaded again.
#[derive(Debug)]
pub struct Cutting;

impl std::fmt::Display for Cutting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("its output is being cut: read again")
    }
}

impl std::error::Error for Cutting {}

/// Removes command `id`'s output.
pub(super) fn remove_output(files: &Path, id: &str) {
    if let Ok(dir) = output_dir(files, false) {
        remove_in(&dir, &file_name(id));
        remove_in(&dir, &format!("{}.cut", file_name(id)));
    }
}

/// The output being written: past its most, its oldest half dropped.
pub(super) struct Output {
    dir: Dir,
    name: String,
    file: File,
    kept: u64,
    most: u64,
    pub(super) dropped: u64,
}

impl Output {
    pub(super) fn create(files: &Path, id: &str, most: u64) -> anyhow::Result<Self> {
        let dir = output_dir(files, true)?;
        let name = file_name(id);
        let file = open_in(
            &dir,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_APPEND,
        )?;
        Ok(Self {
            dir,
            name,
            file,
            kept: 0,
            most,
            dropped: 0,
        })
    }

    /// Its file's inode, which a cut changes.
    pub(super) fn file_id(&self) -> u64 {
        use std::os::unix::fs::MetadataExt;
        self.file.metadata().map_or(0, |m| m.ino())
    }

    /// Appends `bytes`; true when it dropped some of its start.
    pub(super) fn push(&mut self, bytes: &[u8]) -> anyhow::Result<bool> {
        self.file.write_all(bytes)?;
        self.kept += bytes.len() as u64;
        if self.kept <= self.most {
            return Ok(false);
        }
        // Keep the newest half: copied beside, then moved in place.
        use std::os::unix::fs::FileExt;
        let keep = self.most / 2;
        let start = self.kept - keep;
        let reader = open_in(&self.dir, &self.name, libc::O_RDONLY)?;
        let cut = format!("{}.cut", self.name);
        remove_in(&self.dir, &cut);
        let mut beside = open_in(
            &self.dir,
            &cut,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_APPEND,
        )?;
        // A piece at a time, never the half whole in memory.
        let mut buffer = vec![0u8; (keep as usize).min(64 << 10)];
        let mut at = start;
        while at < self.kept {
            let n = buffer.len().min((self.kept - at) as usize);
            reader.read_exact_at(&mut buffer[..n], at)?;
            beside.write_all(&buffer[..n])?;
            at += n as u64;
        }
        self.dir
            .rename(OsStr::new(&cut), OsStr::new(&self.name))
            .context("keeping the output's end")?;
        self.file = beside;
        self.dropped += start;
        self.kept = keep;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A read between a cut and its record saying so is read again, not
    /// read at the old offsets.
    #[test]
    fn a_read_during_a_cut_is_read_again() {
        let files = link_test_kit::TempDir::new("run-cut");
        let mut output = Output::create(&files, "c1", 8).unwrap();
        let mut record = record_in(&files, "c1", 8);
        output.push(b"abcd").unwrap();
        record.output_file = output.file_id();
        assert_eq!(read_output(&record, 0, 100).unwrap().text, "abcd");
        // Past its most: the newest half kept, in another file.
        assert!(output.push(b"efghij").unwrap());
        let read = read_output(&record, 0, 100);
        assert!(read.is_err_and(|err| err.is::<Cutting>()));
        (record.dropped, record.output_file) = (output.dropped, output.file_id());
        let read = read_output(&record, 0, 100).unwrap();
        assert_eq!((read.from, read.text.as_str()), (6, "ghij"));
    }

    /// A record of command `id` in `files`, its output kept to `most`.
    fn record_in(files: &Path, id: &str, most: u64) -> Record {
        serde_json::from_value(json!({
            "id": id, "command": "sh", "argv": ["sh"], "path": "",
            "files": files, "thread": "t", "hops": 0, "way_in": files, "timeout_ms": 0,
            "output_max": most, "tail": most, "keep_secs": 0, "started": 0,
            "state": "exited", "exit": 0}))
        .unwrap()
    }

    /// The ending a turn starts with holds its output's end as text,
    /// fenced as a program's output: a terminal's escapes and other
    /// control characters dropped, its newlines and tabs kept.
    #[test]
    fn an_ending_says_its_output_fenced_without_control_characters() {
        let files = link_test_kit::TempDir::new("run-ending");
        let mut output = Output::create(&files, "c2", 1024).unwrap();
        output.push(b"\x1b[31mred\x1b[0m\x07\tdone\r\n").unwrap();
        let mut record = record_in(&files, "c2", 1024);
        record.output_file = output.file_id();
        let ending = record.ending();
        let said = link_untrusted::fenced(&ending).expect("its output fenced");
        assert_eq!((said.kind, said.from), ("output", "c2"), "{ending:?}");
        assert_eq!(said.text, "[31mred[0m\tdone\n", "{ending:?}");
        assert!(
            !ending
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
            "{ending:?}"
        );
    }

    /// Past its most, the newest half is kept, copied a piece at a time:
    /// what's left is the output's end, after what was dropped.
    #[test]
    fn output_keeps_its_newest_half() {
        let files = link_test_kit::TempDir::new("run-output-half");
        let mut output = Output::create(&files, "abcdefghijkl", 300_000).unwrap();
        let all: String = (1..=100_000).map(|n| format!("{n}\n")).collect();
        let mut cut = false;
        for piece in all.as_bytes().chunks(1000) {
            cut |= output.push(piece).unwrap();
        }
        assert!(cut && output.dropped > 0);
        let dir = output_dir(&files, false).unwrap();
        let mut kept = String::new();
        open_in(&dir, &file_name("abcdefghijkl"), libc::O_RDONLY)
            .unwrap()
            .read_to_string(&mut kept)
            .unwrap();
        assert_eq!(kept.len() as u64, output.kept);
        assert_eq!(kept, all[output.dropped as usize..]);
    }
}
