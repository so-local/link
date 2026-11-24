//! Rewrite data through held directories; names never redirect an outside write.

use anyhow::{Context, ensure};
use link_files::{
    fs::Trust,
    nofollow::{Dir, Kind},
};
use serde::Deserialize;
use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::Path,
    sync::LazyLock,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Format {
    max_text_bytes: u64,
    database_header: String,
    wal_prefixes: Vec<Vec<u8>>,
    companions: Vec<String>,
}

impl Format {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.max_text_bytes > 0 && self.max_text_bytes < u64::MAX,
            "a positive bounded rewrite limit"
        );
        ensure!(!self.database_header.is_empty(), "a database has a header");
        ensure!(
            self.wal_prefixes
                .iter()
                .all(|prefix| !prefix.is_empty() && prefix.len() <= self.database_header.len()),
            "bounded database prefixes"
        );
        ensure!(
            self.companions
                .iter()
                .all(|suffix| suffix.starts_with('-') && !suffix.contains('/')),
            "database companion suffixes"
        );
        Ok(())
    }
}

static FORMAT: LazyLock<Format> = LazyLock::new(|| {
    let format: Format =
        toml::from_str(include_str!("../../relocation.toml")).expect("built-in relocation format");
    format.validate().expect("checked relocation settings");
    format
});

fn parent(path: &Path) -> anyhow::Result<Dir> {
    let trust = Trust {
        owners: &[],
        deny_mode: 0,
        ancestors: true,
        no_link: true,
    };
    let (dir, _) = link_files::fs::checked_parent(path, trust)?;
    Ok(dir.readable()?)
}

fn header(bytes: &[u8]) -> bool {
    bytes.starts_with(FORMAT.database_header.as_bytes())
        || FORMAT
            .wal_prefixes
            .iter()
            .any(|prefix| bytes.starts_with(prefix))
}

fn database(dir: &Dir, name: &OsStr, bytes: &[u8]) -> bool {
    if header(bytes) {
        return true;
    }
    let Some(name) = name.to_str() else {
        return false;
    };
    FORMAT.companions.iter().any(|suffix| {
        let Some(base) = name.strip_suffix(suffix) else {
            return false;
        };
        if base.is_empty() {
            return false;
        }
        let Ok(file) = dir.open_entry(OsStr::new(base)) else {
            return false;
        };
        if !file.metadata().is_ok_and(|meta| meta.is_file()) {
            return false;
        }
        let mut bytes = Vec::new();
        file.take(FORMAT.database_header.len() as u64)
            .read_to_end(&mut bytes)
            .is_ok()
            && header(&bytes)
    })
}

/// SQLite and companion files are recognized from bounded opened snapshots.
pub fn is_database(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let Ok(dir) = parent(path) else {
        return false;
    };
    let Ok(file) = dir.open_entry(name) else {
        return false;
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut bytes = Vec::new();
    file.take(FORMAT.database_header.len() as u64)
        .read_to_end(&mut bytes)
        .is_ok()
        && database(&dir, name, &bytes)
}

struct Stage<'a> {
    dir: &'a Dir,
    name: std::ffi::OsString,
}
impl Drop for Stage<'_> {
    fn drop(&mut self) {
        let _ = self.dir.remove_file(&self.name);
    }
}

fn rewrite(dir: &Dir, name: &OsStr, replace: &dyn Fn(&str) -> String) -> anyhow::Result<bool> {
    let source = match dir.open_entry(name) {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || link_files::nofollow::is_link(&error) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = source.metadata()?;
    if !metadata.is_file() || metadata.len() > FORMAT.max_text_bytes {
        return Ok(false);
    }
    let bytes = match link_files::nofollow::read_open(source, FORMAT.max_text_bytes) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if database(dir, name, &bytes) {
        return Ok(false);
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(false);
    };
    let output = replace(text);
    if output == text {
        return Ok(false);
    }
    let name_of_stage: std::ffi::OsString = link_files::layout::temporary_name().into();
    let mut file = dir.create_new(&name_of_stage, metadata.permissions().mode())?;
    let stage = Stage {
        dir,
        name: name_of_stage,
    };
    file.write_all(output.as_bytes())?;
    file.set_permissions(metadata.permissions())?;
    file.sync_all()?;
    drop(file);
    dir.rename(&stage.name, name)?;
    dir.sync_all()?;
    Ok(true)
}

fn walk(dir: &Dir, replace: &dyn Fn(&str) -> String, depth: usize) -> anyhow::Result<usize> {
    ensure!(
        depth <= link_files::nofollow::DEEPEST,
        "a relocation tree is too deep"
    );
    let mut changed = 0;
    for name in dir.names()? {
        match dir.kind(&name) {
            Ok(Kind::Folder) => match dir.folder(&name) {
                Ok(child) => changed += walk(&child, replace, depth + 1)?,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || link_files::nofollow::is_link(&error) => {}
                Err(error) => return Err(error.into()),
            },
            Ok(Kind::File) => changed += usize::from(rewrite(dir, &name, replace)?),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(changed)
}

fn tree(path: &Path, replace: &dyn Fn(&str) -> String) -> anyhow::Result<usize> {
    // The last component is only a placeholder for opening the directory's
    // checked parent; it is never looked up or created.
    let dir = match parent(&path.join(link_files::layout::temporary_name())) {
        Ok(dir) => dir,
        Err(error) if link_files::fs::missing(&error) => return Ok(0),
        Err(error) => return Err(error),
    };
    walk(&dir, replace, 0)
}

/// Rewrite one text file while holding its real parent and original file.
pub fn rewrite_paths(path: &Path, was: &Path, now: &Path) -> anyhow::Result<bool> {
    let dir = match parent(path) {
        Ok(dir) => dir,
        Err(error) if link_files::fs::missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let name = path.file_name().context("a rewrite file has a name")?;
    rewrite(&dir, name, &|text| {
        super::replace_paths(text, &was.display().to_string(), &now.display().to_string())
    })
}

/// Rewrite bounded text files through no links, preserving their modes.
pub fn rewrite_tree(path: &Path, was: &Path, now: &Path) -> anyhow::Result<usize> {
    tree(path, &|text| {
        super::replace_paths(text, &was.display().to_string(), &now.display().to_string())
    })
}

/// Rewrite transferred-home paths through the same pinned traversal.
pub fn rewrite_arrived(path: &Path, tail: &str, now: &Path) -> anyhow::Result<usize> {
    tree(path, &|text| {
        super::replace_arrived(text, tail, &now.display().to_string())
    })
}

#[cfg(test)]
mod tests;
