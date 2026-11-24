//! The placeholder stage: what it holds, and the command line that runs
//! it (`lnk-sandbox placeholders`), which the sandbox program reads with
//! its generated parser.
use super::Wanted;
use anyhow::bail;
use std::path::{Component, Path, PathBuf};

/// What the placeholder stage is given: where it records what it holds,
/// each placeholder as a kind, a folder and a path, each path it removes
/// when the command ends if a program made it (`swept`, in the same
/// triples), and the command it runs while they're held.
pub struct Placeholders {
    pub dir: PathBuf,
    pub wanted: Vec<String>,
    pub swept: Vec<String>,
    pub command: Vec<String>,
}

/// What [`Placeholders`] holds, read: where it records, the placeholders,
/// what it sweeps, and the command.
pub struct Parts {
    pub dir: PathBuf,
    pub wanted: Vec<Wanted>,
    pub swept: Vec<Wanted>,
    pub command: Vec<String>,
}

impl Placeholders {
    pub fn parts(self) -> anyhow::Result<Parts> {
        Ok(Parts {
            dir: self.dir,
            wanted: triples(&self.wanted, "wanted")?,
            swept: triples(&self.swept, "swept")?,
            command: self.command,
        })
    }
}

/// `words` read as kind, absolute folder and plain relative path, in
/// threes (`flag` names them in an error).
fn triples(words: &[String], flag: &str) -> anyhow::Result<Vec<Wanted>> {
    anyhow::ensure!(
        words.len().is_multiple_of(3),
        "sandbox placeholders --{flag}: kind, folder, path"
    );
    let mut out = Vec::new();
    for triple in words.chunks(3) {
        let file = match triple[0].as_str() {
            "file" => true,
            "folder" => false,
            other => bail!("sandbox placeholders --{flag}: {other:?} isn't file or folder"),
        };
        let (folder, rel) = (PathBuf::from(&triple[1]), PathBuf::from(&triple[2]));
        anyhow::ensure!(
            folder.is_absolute()
                && !rel.as_os_str().is_empty()
                && rel.components().all(|c| matches!(c, Component::Normal(_))),
            "sandbox placeholders --{flag}: {} in {}",
            rel.display(),
            folder.display()
        );
        out.push(Wanted { folder, rel, file });
    }
    Ok(out)
}

/// The command line that runs `command` behind the placeholder stage:
/// `wanted` held while it runs, `swept` removed after where made.
pub fn wrapping(
    exe: &Path,
    dir: &Path,
    wanted: &[Wanted],
    swept: &[Wanted],
    command: Vec<String>,
) -> Vec<String> {
    let mut outer = vec![exe.display().to_string(), "placeholders".into()];
    outer.extend(["--dir".into(), dir.display().to_string()]);
    for (flag, list) in [("--wanted", wanted), ("--swept", swept)] {
        for want in list {
            let kind = if want.file { "file" } else { "folder" };
            for word in [
                kind.to_string(),
                want.folder.display().to_string(),
                want.rel.display().to_string(),
            ] {
                outer.extend([flag.into(), word]);
            }
        }
    }
    outer.push("--".into());
    outer.extend(command);
    outer
}
