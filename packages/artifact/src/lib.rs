//! Verified immutable downloads, shared across checkouts under a byte budget.

use anyhow::ensure;
use link_format::WithinExt;
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub struct Cache {
    root: PathBuf,
    budget: u64,
}
impl Cache {
    pub fn shared() -> anyhow::Result<Self> {
        let root = std::env::var_os("LNK_ARTIFACT_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| link_files::layout::Hidden::Artifacts.path());
        let budget = std::env::var("LNK_ARTIFACT_BUDGET")
            .ok()
            .map(|v| v.parse::<u64>())
            .transpose()
            .within("LNK_ARTIFACT_BUDGET is a byte count")?
            .unwrap_or(2 * 1024 * 1024 * 1024);
        ensure!(budget > 0, "artifact cache budget must be positive");
        Ok(Self { root, budget })
    }
    pub async fn bytes(&self, url: &str, digest: &str, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
        ensure!(
            url.starts_with("https://"),
            "pinned downloads require HTTPS"
        );
        self.acquire(digest, max_bytes, || async {
            link_fetch::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .streaming(std::time::Duration::from_secs(60))
                .outside()
                .follow_redirects(10)
                .build()?
                .get(url)
                .limit(max_bytes.min(self.budget))
                .bytes()
                .await
                .within(format!("downloading {url}"))
        })
        .await
    }
    async fn acquire<F, Fut>(
        &self,
        digest: &str,
        max_bytes: u64,
        fetch: F,
    ) -> anyhow::Result<Vec<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<Vec<u8>>>,
    {
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "artifact needs a SHA-256 content identity"
        );
        link_files::create_private_dir(&self.root)?;
        let lock_path = self.root.join(".lock");
        let lock = tokio::task::spawn_blocking(move || link_files::lock_file(&lock_path)).await??;
        let cached = self.root.join(digest);
        let mut bytes = None;
        if let Ok(meta) = std::fs::symlink_metadata(&cached) {
            if meta.file_type().is_file() && meta.len() <= max_bytes.min(self.budget) {
                let found = std::fs::read(&cached)?;
                if link_digest::sha256(&found) == digest {
                    bytes = Some(found);
                }
            }
            if bytes.is_none() {
                std::fs::remove_file(&cached)?;
            }
        }
        let bytes = match bytes {
            Some(bytes) => bytes,
            None => {
                let bytes = fetch().await?;
                ensure!(
                    bytes.len() as u64 <= max_bytes.min(self.budget),
                    "artifact exceeds its size or cache bound"
                );
                ensure!(
                    link_digest::sha256(&bytes) == digest,
                    "artifact integrity check failed"
                );
                self.prune(bytes.len() as u64, None)?;
                let partial = self.root.join(format!(
                    "download-{}-{}",
                    std::process::id(),
                    link_files::random_token()?
                ));
                let written = (|| -> anyhow::Result<()> {
                    link_files::write_private(&partial, &bytes)?;
                    std::fs::File::open(&partial)?.sync_all()?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o444))?;
                    }
                    std::fs::rename(&partial, &cached)?;
                    Ok(())
                })();
                if written.is_err() {
                    let _ = std::fs::remove_file(&partial);
                }
                written?;
                bytes
            }
        };
        std::fs::File::open(&cached)?.set_modified(SystemTime::now())?;
        self.prune(0, Some(&cached))?;
        drop(lock);
        Ok(bytes)
    }
    fn prune(&self, reserve: u64, keep: Option<&Path>) -> anyhow::Result<()> {
        let mut files = vec![];
        let mut total = 0u64;
        for entry in std::fs::read_dir(&self.root)?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.len() != 64
                || !name.bytes().all(|b| b.is_ascii_hexdigit())
                || !entry.file_type()?.is_file()
            {
                continue;
            }
            let meta = entry.metadata()?;
            total = total.saturating_add(meta.len());
            files.push((meta.modified()?, entry.path(), meta.len()));
        }
        files.sort();
        for (_, path, len) in files {
            if total.saturating_add(reserve) <= self.budget {
                break;
            }
            if keep == Some(path.as_path()) {
                continue;
            }
            std::fs::remove_file(path)?;
            total = total.saturating_sub(len);
        }
        ensure!(
            total.saturating_add(reserve) <= self.budget,
            "selected artifact exceeds the cache budget"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_content_is_checked_and_bounded_while_callers_own_their_bytes() {
        let root = link_test_kit::TempDir::new("artifact-cache");
        let bytes = b"one immutable download".to_vec();
        let digest = link_digest::sha256(&bytes);
        let first = Cache {
            root: root.join("cache"),
            budget: 30,
        };
        let mut local = first
            .acquire(&digest, 30, || async { Ok(bytes.clone()) })
            .await
            .unwrap();
        let second = Cache {
            root: first.root.clone(),
            budget: 30,
        };
        let other = second
            .acquire(&digest, 30, || async {
                panic!("warm cache downloaded again")
            })
            .await
            .unwrap();
        local[0] = b'X';
        assert_eq!(other, bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                first.root.join(&digest),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
        std::fs::write(first.root.join(&digest), b"corrupted").unwrap();
        assert_eq!(
            second
                .acquire(&digest, 30, || async { Ok(bytes.clone()) })
                .await
                .unwrap(),
            bytes
        );
        assert!(
            second
                .acquire(&"0".repeat(64), 30, || async { Ok(bytes.clone()) })
                .await
                .is_err()
        );
        let replacement = b"a different immutable file".to_vec();
        let next = link_digest::sha256(&replacement);
        second
            .acquire(&next, 30, || async { Ok(replacement) })
            .await
            .unwrap();
        assert!(!first.root.join(digest).exists());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
