//! A cloud's bucket: made, tested and deleted, its password checked, its
//! files compared and hashed, and its objects named.

use super::*;
use link_format::WithinExt;

impl Rclone {
    /// Whether `cloud`'s encryption password opens what its bucket already
    /// holds: yes when it holds nothing, or when a stored file's first
    /// block, which is authenticated, decrypts. A stored name decrypting is
    /// no proof: under a wrong password about one name in 256 still does,
    /// its padding right by chance. So a name counts alone only when no
    /// file it names has a block to read (each is empty). A stray file
    /// that isn't encrypted doesn't count against it.
    pub fn password_opens(&self, cloud: &Cloud) -> anyhow::Result<bool> {
        let output = self.run(&[
            "lsf",
            "-R",
            "--files-only",
            "--format",
            "sp",
            "--separator",
            ";",
            &cloud.plain_remote(),
        ])?;
        let stored = String::from_utf8_lossy(&output.stdout).to_string();
        let files: Vec<(u64, &str)> = stored
            .lines()
            .filter_map(|line| {
                let (size, name) = line.split_once(';')?;
                Some((size.parse().ok()?, name))
            })
            .filter(|(_, name)| !name.is_empty())
            .take(5)
            .collect();
        if files.is_empty() {
            return Ok(true);
        }
        let mut named = false;
        let mut readable = false;
        for (size, name) in files {
            let decoded = self.output(&["backend", "decode", &cloud.remote(), "--", name])?;
            if !decoded.status.success() {
                continue;
            }
            // A file's header (its magic and nonce) is 32 bytes; past it,
            // blocks, each sealed.
            if size <= 32 {
                named = true;
                continue;
            }
            readable = true;
            let plain = String::from_utf8_lossy(&decoded.stdout).trim().to_string();
            let file = format!("{}{plain}", cloud.remote());
            if self
                .output(&["cat", "--count", "1", &file])?
                .status
                .success()
            {
                return Ok(true);
            }
        }
        Ok(named && !readable)
    }

    /// Creates the bucket (or folder) if it isn't there. A drive cloud's
    /// folder is made readable only by you.
    pub fn make_bucket(&self, cloud: &Cloud) -> anyhow::Result<()> {
        if let (Kind::Drive, Some(path)) = (cloud.kind, &cloud.path) {
            let dir = Path::new(path).join(&cloud.bucket);
            if !dir.exists() {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
                builder
                    .create(&dir)
                    .within(format!("creating {}", dir.display()))?;
            }
        }
        self.run(&["mkdir", &cloud.remote()])?;
        Ok(())
    }

    /// Whether the bucket (or folder) is there.
    pub fn exists(&self, cloud: &Cloud) -> anyhow::Result<bool> {
        let output = self.output(&["lsf", "--max-depth", "1", &cloud.plain_remote()])?;
        if output.status.success() {
            return Ok(true);
        }
        let said = said(&output);
        ensure!(
            said.contains("directory not found") || said.contains("NoSuchBucket"),
            "couldn't look for {} in {}: {said}",
            cloud.bucket,
            cloud.name
        );
        Ok(false)
    }

    /// Deletes the bucket (or folder) and everything in it; done already
    /// when it isn't there.
    pub fn delete_bucket(&self, cloud: &Cloud) -> anyhow::Result<()> {
        if self.exists(cloud)? {
            self.run(&["purge", &cloud.plain_remote()])?;
        }
        Ok(())
    }

    /// Deletes the bucket (or folder) when it's empty, such as one just
    /// made: no listing needed, which a key may not be allowed.
    pub fn remove_empty(&self, cloud: &Cloud) -> anyhow::Result<()> {
        self.run(&["rmdir", &cloud.plain_remote()])?;
        Ok(())
    }

    /// Writes a small object, reads it back and deletes it: proof the
    /// keys can do what `lnk storage` needs.
    pub fn test(&self, cloud: &Cloud) -> anyhow::Result<()> {
        let object = object(cloud, ".link/connect-test");
        let text = "lnk storage connect\n";
        let mut child = self
            .command(&["rcat", &object])?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .within("running rclone")?;
        child
            .stdin
            .take()
            .within("no stdin")?
            .write_all(text.as_bytes())?;
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "couldn't write to {}: {}",
            cloud.name,
            said(&output)
        );
        let read = self
            .run(&["cat", &object])
            .within(format!("couldn't read from {}", cloud.name))?;
        ensure!(
            read.stdout == text.as_bytes(),
            "{} returned something else than was written",
            cloud.name
        );
        self.run(&["deletefile", &object])
            .within(format!("couldn't delete from {}", cloud.name))?;
        // Storage with real folders keeps an empty `.link`; buckets don't.
        let _ = self.output(&["rmdir", &object_dir(cloud)]);
        Ok(())
    }

    /// `files` as they're named in the storage under an encrypted cloud.
    pub(super) fn encrypted_names(
        &self,
        cloud: &Cloud,
        files: &[String],
    ) -> anyhow::Result<Vec<String>> {
        for file in files {
            ensure!(
                !file.contains(['\n', '\r']),
                "{file:?}: a name with a line break can't be restored"
            );
        }
        let remote = cloud.remote();
        let mut encoded = Vec::with_capacity(files.len());
        // A batch at a time: every name of a large folder on one command
        // line passes the system's limit (2 MB on Linux, 1 MB on a Mac).
        for batch in files.chunks(ENCODE_BATCH) {
            // `--`: what follows are names, never rclone's options.
            let mut args = vec!["backend", "encode", remote.as_str(), "--"];
            args.extend(batch.iter().map(String::as_str));
            let output = self.run(&args)?;
            let said = String::from_utf8(output.stdout)?;
            let before = encoded.len();
            encoded.extend(said.lines().map(str::to_string));
            ensure!(
                encoded.len() - before == batch.len(),
                "rclone didn't encode every name"
            );
        }
        Ok(encoded)
    }

    /// `check` of `files` against `cloud`: by checksum (through the
    /// encryption, for an encrypted cloud), or by reading every copy back.
    pub(super) fn compare(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        download: bool,
    ) -> anyhow::Result<Vec<(String, Found)>> {
        let remote = cloud.remote();
        let list = FileList::new(files)?;
        let root_text = root.to_string_lossy();
        let list_path = list.path.to_string_lossy();
        let run = |download: bool| {
            let command = if cloud.encrypted && !download {
                "cryptcheck"
            } else {
                "check"
            };
            let mut args = vec![
                command,
                &*root_text,
                &remote,
                "--one-way",
                "--combined",
                "-",
                "--files-from-raw",
                &list_path,
            ];
            if download {
                args.push("--download");
            }
            self.output(&args)
        };
        let mut output = run(download)?;
        // Copies without a checksum (sent by another program in parts)
        // would count as the same by size: read those back instead.
        if !download
            && String::from_utf8_lossy(&output.stderr).contains("hashes could not be checked")
        {
            output = run(true)?;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() && !output.status.success() {
            bail!("couldn't check {}: {}", cloud.name, said(&output));
        }
        Ok(parse_combined(&stdout, files))
    }

    /// Which of `files` have, in S3 `cloud`, a checksum S3 computed rather
    /// than one their uploader set. rclone takes an object's MD5 from its
    /// `md5chksum` metadata, which S3 never checks against the contents,
    /// whenever the ETag isn't an MD5 (an upload in parts, SSE-KMS).
    /// Listed treating no ETag as an MD5, so an object's MD5 shows only
    /// when it is in that metadata: one HEAD request per file, nothing
    /// read. A file the listing leaves out isn't trusted.
    pub(super) fn hashed_by_storage(
        &self,
        cloud: &Cloud,
        files: &[String],
    ) -> anyhow::Result<std::collections::HashSet<String>> {
        let stored = if cloud.encrypted {
            self.encrypted_names(cloud, files)?
        } else {
            files.to_vec()
        };
        let list = FileList::new(&stored)?;
        let list_path = list.path.to_string_lossy();
        let output = self.run(&[
            "lsjson",
            "-R",
            "--files-only",
            "--no-mimetype",
            "--no-modtime",
            "--hash",
            "--hash-type",
            "md5",
            "--s3-directory-bucket",
            "--checkers",
            "32",
            "--files-from-raw",
            &list_path,
            &cloud.plain_remote(),
        ])?;
        let items: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).within("reading rclone's listing")?;
        let by_storage: std::collections::HashSet<&str> = items
            .iter()
            .filter(|item| {
                item["Hashes"]["md5"]
                    .as_str()
                    .is_none_or(|hash| hash.is_empty())
            })
            .filter_map(|item| item["Path"].as_str())
            .collect();
        Ok(stored
            .iter()
            .zip(files)
            .filter(|(name, _)| by_storage.contains(name.as_str()))
            .map(|(_, file)| file.clone())
            .collect())
    }
}

/// Of `stored` (names in the bucket), those `listed` (what `rclone
/// backend list-multipart-uploads` prints: each bucket's uploads, each
/// with its `Key`, the bucket's path to it) has an upload begun on. A
/// key under a folder the cloud keeps its files in ends with the name.
/// None for what isn't such a listing.
pub(super) fn pending_in(
    listed: &str,
    stored: &[String],
) -> Option<std::collections::HashSet<String>> {
    let listed: serde_json::Value = serde_json::from_str(listed).ok()?;
    let keys: Vec<&str> = listed
        .as_object()?
        .values()
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(|upload| upload["Key"].as_str())
        .collect();
    Some(
        stored
            .iter()
            .filter(|name| {
                keys.iter()
                    .any(|key| key == name || key.ends_with(&format!("/{name}")))
            })
            .cloned()
            .collect(),
    )
}

/// Whether `cloud` is S3 storage, whose checksums can be what an
/// object's uploader set rather than computed from what it holds.
pub(super) fn uploader_hashes(cloud: &Cloud) -> bool {
    match cloud.kind {
        Kind::Aws | Kind::S3 => true,
        Kind::Rclone => cloud.backend.as_deref() == Some("s3"),
        Kind::Gcp | Kind::Drive => false,
    }
}

/// The file `path` in `cloud`, as rclone names it.
pub(super) fn object(cloud: &Cloud, path: &str) -> String {
    let remote = cloud.remote();
    if remote.ends_with(':') {
        format!("{remote}{path}")
    } else {
        format!("{remote}/{path}")
    }
}

/// Link's folder in `cloud`, as rclone names it.
fn object_dir(cloud: &Cloud) -> String {
    object(cloud, ".link")
}

/// A file listing paths for `--files-from-raw`, deleted when dropped.
pub(super) struct FileList {
    pub(super) path: PathBuf,
}

impl FileList {
    pub(super) fn new(files: &[String]) -> anyhow::Result<Self> {
        for file in files {
            ensure!(
                !file.contains(['\n', '\r']),
                "{file:?}: a name with a line break can't be sent"
            );
        }
        // Clouds sent to side by side each make one at the same time.
        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lnk-bucket-{}-{}-{}.txt",
            std::process::id(),
            crate::files::now_nanos(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        link_files::write_private(&path, files.join("\n").as_bytes())?;
        Ok(Self { path })
    }
}

impl Drop for FileList {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// rclone's flag for `class` on `cloud`.
pub(super) fn class_flag(cloud: &Cloud, class: &str) -> anyhow::Result<String> {
    Ok(match cloud.kind {
        Kind::Aws => format!("--s3-storage-class={class}"),
        Kind::Gcp => format!("--gcs-storage-class={class}"),
        Kind::S3 | Kind::Drive | Kind::Rclone => bail!("{} has no storage classes", cloud.name),
    })
}

/// Reads `check --combined` output: `= path` same, `+ path` only here
/// (missing there), `* path` differs, `! path` an error.
fn parse_combined(stdout: &str, files: &[String]) -> Vec<(String, Found)> {
    let mut found = std::collections::HashMap::new();
    for line in stdout.lines() {
        let Some((mark, path)) = line.split_once(' ') else {
            continue;
        };
        let result = match mark {
            "=" => Found::Same,
            "+" => Found::Missing,
            "*" => Found::Differs,
            "!" => Found::Error("couldn't read or hash it".into()),
            _ => continue,
        };
        found.insert(path.to_string(), result);
    }
    files
        .iter()
        .map(|file| {
            let result = found
                .remove(file)
                .unwrap_or_else(|| Found::Error("not checked".into()));
            (file.clone(), result)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_begun_are_read_from_the_listing() {
        let listed = r#"{"bucket": [{"Key": "personal/photos/a.jpg", "UploadId": "x"},
            {"Key": "other.jpg"}], "empty": []}"#;
        let stored = [
            "photos/a.jpg".to_string(),
            "photos/b.jpg".into(),
            "other.jpg".into(),
        ];
        let found = pending_in(listed, &stored).unwrap();
        assert!(found.contains("photos/a.jpg") && found.contains("other.jpg"));
        assert!(!found.contains("photos/b.jpg"));
        assert!(pending_in("{}", &stored).unwrap().is_empty());
        assert!(pending_in("not json", &stored).is_none());
    }

    #[test]
    fn reads_check_output() {
        let files: Vec<String> = ["a", "b c", "d", "e", "f"].map(String::from).into();
        let out = "= a\n+ b c\n* d\n! e\n- only-there\n";
        assert_eq!(
            parse_combined(out, &files),
            [
                ("a".into(), Found::Same),
                ("b c".into(), Found::Missing),
                ("d".into(), Found::Differs),
                ("e".into(), Found::Error("couldn't read or hash it".into())),
                ("f".into(), Found::Error("not checked".into())),
            ]
        );
    }

    #[test]
    fn names_objects() {
        let mut cloud = Cloud {
            name: "aws".into(),
            kind: Kind::Aws,
            folder: "personal".into(),
            bucket: "b".into(),
            region: None,
            path: None,
            backend: None,
            encrypted: false,
        };
        assert_eq!(object(&cloud, ".link/x"), "aws:b/.link/x");
        cloud.encrypted = true;
        assert_eq!(object(&cloud, ".link/x"), "aws-personal-crypt:.link/x");
    }
}
