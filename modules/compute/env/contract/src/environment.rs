//! An environment carried between machines, and where it was prepared.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_format::WithinExt;
use serde::{Deserialize, Serialize};

use crate::file::Read;

/// The environment hand-off, as `environment/protocol.toml` declares it:
/// what an environment carried to another machine states it's written in.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped(include_str!("environment/protocol.toml"))
});

/// An environment as it is carried to another machine: the hand-off's
/// version ([`PROTOCOL`]), its file and every file the file names, by
/// their paths from its folder, and its image's hash (`lnk env check`).
/// Never a secret's value: those stay on the computer they were set on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packed {
    /// The hand-off's version it's written in.
    pub protocol: u32,
    /// The file's name, among `files`.
    pub file: String,
    pub hash: String,
    pub files: BTreeMap<String, String>,
}

/// `read`'s environment, packed with `hash`: the file and every file it
/// names, each as text. No secret's value.
pub fn pack(read: &Read, hash: &str) -> anyhow::Result<Packed> {
    let file = read
        .path
        .file_name()
        .within("the file's name")?
        .to_string_lossy()
        .into_owned();
    let mut files = BTreeMap::from([(file.clone(), read.text.clone())]);
    let origin = link_files::nofollow::Dir::open_unlinked(&read.dir)?;
    for path in read.file.named_files() {
        let text = input(&origin, &path).within(format!("reading {}", path.display()))?;
        let Ok(text) = String::from_utf8(text) else {
            bail!(
                "{} isn't text: an environment carried to another machine brings its files as \
                 text",
                path.display()
            );
        };
        files.insert(path.display().to_string(), text);
    }
    Ok(Packed {
        protocol: PROTOCOL.current,
        file,
        hash: hash.to_string(),
        files,
    })
}

/// An environment another machine sent, as `bytes`: its version read
/// before anything else of it, so one written by another release is
/// refused by name, naming the upgrade, never a bare error of its shape.
pub fn unpack(bytes: &[u8]) -> anyhow::Result<Packed> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).within("reading the environment's files")?;
    let version = value
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .within("the environment sent names no hand-off version: the machine that sent it runs another release")?;
    PROTOCOL.read("the machine that sent this environment", version)?;
    serde_json::from_value(value).within("reading the environment's files")
}

/// Capture declared files through held directories, with the same no-link boundary as copies.
fn input(origin: &link_files::nofollow::Dir, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut file = opened_input(origin, path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn opened_input(
    origin: &link_files::nofollow::Dir,
    path: &std::path::Path,
) -> anyhow::Result<std::fs::File> {
    use std::{os::unix::fs::MetadataExt, path::Component};
    let mut parts = path
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name.to_owned()),
            _ => anyhow::bail!("an environment input must stay beneath its authoring origin"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let name = parts.pop().within("the input's filename")?;
    let mut folder = origin.try_clone()?;
    for part in parts {
        folder = folder.folder(&part)?;
    }
    let file = folder.open_entry(&name)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "environment inputs must be regular files with one name, never links or special files"
    );
    Ok(file)
}

/// `lnk env prepare` and `import`'s answer: paths on the machine that
/// prepared it, never paths from the machine it came from.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub env: PathBuf,
    pub workdir: PathBuf,
}

/// A captured environment's files, written to a private folder and read
/// back as the environment they were captured from: nothing is read from
/// its source again. The folder goes when the returned guard does.
pub fn staged(
    input: &link_box_contract::EnvironmentInput,
) -> anyhow::Result<(Staged, crate::file::Read)> {
    ensure!(
        input.provenance.digest == link_digest::content_digest(&input.files)?,
        "environment content differs from its captured identity"
    );
    let staged = Staged(link_files::private_temp_dir("guest-environment")?);
    for (relative, text) in &input.files {
        let path = Path::new(relative);
        ensure!(
            path.components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "captured environment path is not relative"
        );
        let path = staged.0.join(path);
        std::fs::create_dir_all(path.parent().context("environment file folder")?)?;
        link_files::write_private(&path, text.as_bytes())?;
    }
    let mut read = crate::file::read(&staged.0.join(&input.file))?;
    // Secret authorization retains the source's provenance, not the staging path.
    read.path = input.provenance.reference.clone().into();
    Ok((staged, read))
}

/// The private folder [`staged`] wrote, removed when dropped.
pub struct Staged(std::path::PathBuf);
impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    fn read(dir: &link_test_kit::TempDir, input: &str) -> crate::file::Read {
        let text = format!(
            "schema = 1\nname = \"input\"\nversion = 1\n[[stage]]\nname = \"prepare\"\nrun = \"true\"\nuser = \"root\"\ninputs = [\"{input}\"]\n"
        );
        crate::file::Read {
            file: crate::file::parse(&text).unwrap(),
            path: dir.join("env.toml"),
            dir: dir.to_path_buf(),
            text,
        }
    }

    /// A hand-off names its version, read first: one of another release
    /// is refused by name, and one naming none is refused too.
    #[test]
    fn a_hand_off_says_its_version_and_another_is_refused_by_name() {
        let packed = super::Packed {
            protocol: super::PROTOCOL.current,
            file: "env.toml".into(),
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&packed).unwrap();
        assert_eq!(super::unpack(&bytes).unwrap(), packed);
        let mut newer = serde_json::to_value(&packed).unwrap();
        newer["protocol"] = (super::PROTOCOL.current + 1).into();
        newer["more"] = "a field a newer release adds".into();
        let refused = super::unpack(&serde_json::to_vec(&newer).unwrap()).unwrap_err();
        assert!(
            format!("{refused:#}").contains("lnk upgrade"),
            "{refused:#}"
        );
        let mut none = serde_json::to_value(&packed).unwrap();
        none.as_object_mut().unwrap().remove("protocol");
        let refused = super::unpack(&serde_json::to_vec(&none).unwrap()).unwrap_err();
        assert!(
            format!("{refused:#}").contains("no hand-off version"),
            "{refused:#}"
        );
    }

    #[test]
    fn captured_environment_inputs_refuse_symbolic_and_hard_links() {
        let dir = link_test_kit::TempDir::new("captured-environment-links");
        let read = read(&dir, "asset.txt");
        std::fs::write(dir.join("asset.txt"), "captured").unwrap();
        assert_eq!(
            super::pack(&read, "").unwrap().files["asset.txt"],
            "captured"
        );
        std::fs::remove_file(dir.join("asset.txt")).unwrap();
        std::fs::write(dir.join("private.txt"), "never copied").unwrap();
        std::os::unix::fs::symlink(dir.join("private.txt"), dir.join("asset.txt")).unwrap();
        assert!(super::pack(&read, "").is_err());
        std::fs::remove_file(dir.join("asset.txt")).unwrap();
        std::fs::hard_link(dir.join("private.txt"), dir.join("asset.txt")).unwrap();
        assert!(super::pack(&read, "").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("private.txt")).unwrap(),
            "never copied"
        );
    }

    #[test]
    fn captured_environment_inputs_refuse_linked_parent_folders() {
        let dir = link_test_kit::TempDir::new("captured-environment-parent-link");
        let read = read(&dir, "declared/asset.txt");
        std::fs::create_dir(dir.join("private")).unwrap();
        std::fs::write(dir.join("private/asset.txt"), "never copied").unwrap();
        std::os::unix::fs::symlink(dir.join("private"), dir.join("declared")).unwrap();
        assert!(super::pack(&read, "").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("private/asset.txt")).unwrap(),
            "never copied"
        );
    }

    #[test]
    fn captured_inputs_keep_opened_identity_across_file_and_origin_replacement() {
        use std::io::Read as _;
        let dir = link_test_kit::TempDir::new("captured-input-replacement");
        std::fs::create_dir(dir.join("origin")).unwrap();
        std::fs::create_dir(dir.join("private")).unwrap();
        std::fs::write(dir.join("origin/asset.txt"), "declared").unwrap();
        std::fs::write(dir.join("private/asset.txt"), "never captured").unwrap();
        let origin = link_files::nofollow::Dir::open_unlinked(&dir.join("origin")).unwrap();
        let mut opened = super::opened_input(&origin, std::path::Path::new("asset.txt")).unwrap();
        std::fs::rename(dir.join("origin/asset.txt"), dir.join("origin/kept.txt")).unwrap();
        std::os::unix::fs::symlink(dir.join("private/asset.txt"), dir.join("origin/asset.txt"))
            .unwrap();
        let mut captured = String::new();
        opened.read_to_string(&mut captured).unwrap();
        assert_eq!(captured, "declared");
        assert!(super::opened_input(&origin, std::path::Path::new("asset.txt")).is_err());
        std::fs::rename(dir.join("origin"), dir.join("kept-origin")).unwrap();
        std::os::unix::fs::symlink(dir.join("private"), dir.join("origin")).unwrap();
        assert_eq!(
            super::input(&origin, std::path::Path::new("kept.txt")).unwrap(),
            b"declared"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("private/asset.txt")).unwrap(),
            "never captured"
        );
    }
}
