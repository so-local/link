//! Live owner routes, separate from question decisions. Kernel locks expire on exit.
use crate::*;
use std::path::PathBuf;

pub struct Presence {
    path: PathBuf,
    _held: std::fs::File,
}
impl Drop for Presence {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: String,
    route: Option<String>,
}

impl Store {
    pub fn presence(&self, key: &str, route: Option<&str>) -> anyhow::Result<Presence> {
        let dir = self.dir().join("presence");
        link_files::create_private_dir(&dir)?;
        let path = dir.join(link_files::random_token()?);
        let held = link_files::lock_file(&path)?;
        let bytes = serde_json::to_vec(&Entry {
            key: key.into(),
            route: route.map(str::to_string),
        })?;
        std::io::Write::write_all(&mut &held, &bytes)?;
        Ok(Presence { path, _held: held })
    }

    pub fn active_routes(&self, key: &str) -> anyhow::Result<Vec<String>> {
        let dir = self.dir().join("presence");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let folder = link_files::nofollow::Dir::open(&dir)?;
        let mut routes = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file = folder.open_file(&entry.file_name(), libc::O_RDONLY, 0)?;
            match file.try_lock() {
                Err(std::fs::TryLockError::WouldBlock) => {
                    let bytes =
                        folder.read(&entry.file_name(), crate::constants::MAX_TEXT_BYTES as u64)?;
                    let entry: Entry = serde_json::from_slice(&bytes)?;
                    if entry.key == key {
                        routes.push(entry.route.unwrap_or_default());
                    }
                }
                Ok(()) => {
                    folder.remove_file(&entry.file_name())?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        routes.sort();
        routes.dedup();
        Ok(routes)
    }
}
