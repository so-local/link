//! Programs Link doesn't build, such as Caddy, declared by their caller:
//! each target's download, its SHA-256 sum and the file inside it to take.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub program: Vec<Program>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub name: String,
    pub version: String,
    pub targets: BTreeMap<String, Download>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    /// The file to take from an archive; without it, the download is the program.
    #[serde(default)]
    pub take: Option<String>,
}

/// The programs `path` declares.
pub fn read(path: &Path) -> anyhow::Result<Vec<Program>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: File = toml::from_str(&text)
        .with_context(|| format!("{} isn't an outside file", path.display()))?;
    for program in &file.program {
        ensure!(
            program.url_safe(),
            "{}: {} downloads only over https",
            path.display(),
            program.name
        );
    }
    Ok(file.program)
}

impl Program {
    fn url_safe(&self) -> bool {
        self.targets.values().all(|d| d.url.starts_with("https://"))
    }

    /// Downloads it for `target` into `work`, checks its sum, and returns
    /// the program's file, named as the program.
    pub fn fetch(&self, target: &str, work: &Path) -> anyhow::Result<PathBuf> {
        let download = self.targets.get(target).with_context(|| {
            format!(
                "{} {} declares no download for {target}",
                self.name, self.version
            )
        })?;
        let dir = work.join(format!("{}-{target}", self.name));
        std::fs::create_dir_all(&dir)?;
        let fetched = dir.join("download");
        crate::run(
            "curl",
            &[
                "-fsSL",
                "--retry",
                "3",
                "-o",
                &fetched.to_string_lossy(),
                &download.url,
            ],
            &dir,
        )?;
        let file = std::fs::File::open(&fetched)?;
        let sum = link_digest::sha256_of(file)?;
        ensure!(
            sum.eq_ignore_ascii_case(&download.sha256),
            "{} {} for {target} has SHA-256 {sum}, not the declared {}",
            self.name,
            self.version,
            download.sha256
        );
        let program = dir.join("out").join(&self.name);
        std::fs::create_dir_all(program.parent().unwrap_or(&dir))?;
        match &download.take {
            Some(inside) => {
                let unpacked = dir.join("unpacked");
                std::fs::create_dir_all(&unpacked)?;
                // A Debian package is an `ar` archive whose files are in
                // its `data.tar.*`.
                let archive = if download.url.ends_with(".deb") {
                    crate::run("ar", &["x", &fetched.to_string_lossy()], &dir)?;
                    std::fs::read_dir(&dir)?
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .find(|p| {
                            p.file_name()
                                .is_some_and(|n| n.to_string_lossy().starts_with("data.tar"))
                        })
                        .context("the package holds no data.tar")?
                } else {
                    fetched.clone()
                };
                crate::run(
                    "tar",
                    &[
                        "-xf",
                        &archive.to_string_lossy(),
                        "-C",
                        &unpacked.to_string_lossy(),
                        inside,
                    ],
                    &dir,
                )
                .context("unpacking it (a .deb's zstd data needs zstd)")?;
                std::fs::rename(unpacked.join(inside), &program)?;
            }
            None => std::fs::rename(&fetched, &program)?,
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(program)
    }
}
