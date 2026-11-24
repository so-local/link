//! Logical file revisions survive equal payloads and removals independently
//! of the backend's opaque compare-and-swap token.
use super::*;
use base64::Engine as _;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    revision: u64,
    data: Option<String>,
}

link_files::specification!(FileRecord, include_str!("files/schema.toml"));

impl FileRecord {
    fn read(path: &str, bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() as u64 <= file_record_max(file_max(path))?,
            "{}'s stored file record is too large",
            link_format::clean(path)
        );
        let record: Self =
            link_files::saved::parse_json(bytes).context("reading a stored file record")?;
        anyhow::ensure!(
            record.revision != 0,
            "a stored file revision cannot be zero"
        );
        record.payload(path)?;
        Ok(record)
    }

    fn payload(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        self.data
            .as_deref()
            .map(|data| file_data(path, data))
            .transpose()
    }

    fn visible_version(&self) -> u64 {
        if self.data.is_some() {
            self.revision
        } else {
            0
        }
    }

    fn next(&self) -> anyhow::Result<u64> {
        self.revision
            .checked_add(1)
            .context("file revisions are exhausted")
    }
}

/// Most bytes in a canonical stored file record with this payload cap.
/// Object backends use the same bound as the typed reader: base64 plus
/// the serializer's largest metadata envelope, including a tombstone.
pub fn file_record_max(payload_max: u64) -> anyhow::Result<u64> {
    // A future version still reaches the shared version refusal even at
    // the payload cap; reserve the longest schema number's envelope.
    use link_files::schema::Specification;
    let mut format = FileRecord::format().clone();
    format.current = u32::MAX;
    let envelope = [None, Some(String::new())]
        .into_iter()
        .map(|data| {
            format
                .print_json(&FileRecord {
                    revision: u64::MAX,
                    data,
                })
                .map(|data| data.len() as u64)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or_default();
    payload_max
        .div_ceil(3)
        .checked_mul(4)
        .and_then(|payload| payload.checked_add(envelope))
        .context("the encoded file record size overflows")
}

impl<S: Store> Objects<S> {
    fn file_record(&self, path: &str) -> anyhow::Result<Option<(FileRecord, Version)>> {
        let Some((bytes, version)) = self.store.get(path)? else {
            return Ok(None);
        };
        let record = FileRecord::read(path, &bytes)?;
        Ok(Some((record, version)))
    }

    pub(super) fn file(&self, path: &str) -> anyhow::Result<Option<(Vec<u8>, u64)>> {
        let Some((record, _)) = self.file_record(path)? else {
            return Ok(None);
        };
        Ok(record.payload(path)?.map(|bytes| (bytes, record.revision)))
    }

    pub(super) fn write(&self, path: &str, data: &[u8], expected: u64) -> anyhow::Result<Reply> {
        let held = self.file_record(path)?;
        let now = held
            .as_ref()
            .map_or(0, |(record, _)| record.visible_version());
        if now != expected {
            return Ok(Reply::Stale { version: now });
        }
        let (revision, expect) = match held {
            Some((record, version)) => (record.next()?, Expect::Version(version)),
            None => (1, Expect::Absent),
        };
        let record = FileRecord {
            revision,
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
        };
        match self
            .store
            .put(path, &link_files::saved::print_json(&record)?, &expect)
        {
            Ok(_) => Ok(Reply::Wrote { version: revision }),
            Err(error) if is_conflict(&error) => Ok(Reply::Stale {
                version: self
                    .file_record(path)?
                    .map_or(0, |(record, _)| record.visible_version()),
            }),
            Err(error) => Err(error),
        }
    }

    pub(super) fn remove_file(&self, path: &str, expected: u64) -> anyhow::Result<bool> {
        let Some((record, version)) = self.file_record(path)? else {
            return Ok(false);
        };
        if expected == 0 || record.visible_version() != expected {
            return Ok(false);
        }
        let tombstone = FileRecord {
            revision: record.next()?,
            data: None,
        };
        match self.store.put(
            path,
            &link_files::saved::print_json(&tombstone)?,
            &Expect::Version(version),
        ) {
            Ok(_) => Ok(true),
            Err(error) if is_conflict(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "backs off after a conflicting write"
    )]
    pub(super) fn append_file(&self, path: &str, data: &[u8]) -> anyhow::Result<u64> {
        for _ in 0..TRIES {
            let (mut body, revision, expect) = match self.file_record(path)? {
                Some((record, version)) => (
                    record.payload(path)?.unwrap_or_default(),
                    record.next()?,
                    Expect::Version(version),
                ),
                None => (Vec::new(), 1, Expect::Absent),
            };
            super::super::check_file_size(
                path,
                body.len()
                    .checked_add(data.len())
                    .context("file length overflows")?,
            )?;
            body.extend_from_slice(data);
            let record = FileRecord {
                revision,
                data: Some(base64::engine::general_purpose::STANDARD.encode(body)),
            };
            match self
                .store
                .put(path, &link_files::saved::print_json(&record)?, &expect)
            {
                Ok(_) => return Ok(revision),
                Err(error) if is_conflict(&error) => std::thread::sleep(Duration::from_millis(1)),
                Err(error) => return Err(error),
            }
        }
        bail!(
            "{} kept changing as it was written: try again",
            link_format::clean(path)
        )
    }
}

#[cfg(all(test, feature = "files-bridge"))]
mod tests;
