//! Root's copies of module programs (`root_programs.toml`), which `lnk
//! upgrade` brings to the new build: a new client calling an old keeper
//! breaks every call it makes.

use std::io::Read;

use link_files::layout::System;
use link_format::{Note, Question};
use serde::Deserialize;

use super::*;

/// A program a module copies into root's exec folder, and the command
/// that copies it again.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootProgram {
    location: System,
    command: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootPrograms {
    program: Vec<RootProgram>,
}

fn root_programs() -> Vec<RootProgram> {
    toml::from_str::<RootPrograms>(include_str!("root_programs.toml"))
        .expect("the built-in root programs are valid")
        .program
}

impl RootProgram {
    /// The program's name, the same in root's folder and in yours.
    fn name(&self) -> String {
        self.location
            .path()
            .file_name()
            .expect("a root program's location names a file")
            .to_string_lossy()
            .into_owned()
    }

    /// The command that copies it again, as typed.
    fn typed(&self) -> Command {
        lnk_command(&self.command)
    }
}

/// Whether root's copy at `root` is another build than yours at `yours`:
/// both there, and their bytes differ. A missing copy is nothing to bring
/// along.
fn behind(root: &Path, yours: &Path) -> anyhow::Result<bool> {
    let (Ok(mut root), Ok(mut yours)) = (std::fs::File::open(root), std::fs::File::open(yours))
    else {
        return Ok(false);
    };
    if root.metadata()?.len() != yours.metadata()?.len() {
        return Ok(true);
    }
    let (mut a, mut b) = (vec![0; 1 << 16], vec![0; 1 << 16]);
    loop {
        let read = root.read(&mut a)?;
        if read == 0 {
            return Ok(false);
        }
        yours.read_exact(&mut b[..read])?;
        if a[..read] != b[..read] {
            return Ok(true);
        }
    }
}

/// Brings root's copies in `dir`'s programs' places to the new build, by
/// each one's command run with the new `lnk` at `exe`: asked first in a
/// terminal, and run without asking with `yes`. What's left to run is
/// returned, for the upgrade to name as its next steps.
pub(super) fn follow(
    out: &mut Out,
    exe: &Path,
    dir: &Path,
    yes: bool,
) -> Result<Vec<Command>, Report> {
    let mut stale = Vec::new();
    for program in root_programs() {
        // A copy that can't be compared is copied again: the same build
        // copied once more harms nothing.
        if behind(&program.location.path(), &dir.join(program.name())).unwrap_or(true) {
            stale.push(program);
        }
    }
    if stale.is_empty() {
        return Ok(Vec::new());
    }
    let names = Value::List(
        stale
            .iter()
            .map(|p| Product::new(p.name()).into())
            .collect(),
    );
    out.block(Note(sentence!(
        "Root's copies of {} in {} are still the old build.",
        names,
        Shown::new(link_files::layout::SystemRoot::Exec.path())
    )));
    if !yes && !out.can_ask() {
        return Ok(stale.iter().map(RootProgram::typed).collect());
    }
    let agreed = out.ask(
        Question::confirm(sentence!("Bring them to the new build?"), true)
            .why(prose!(
                "Each is copied again with sudo, by {}.",
                Value::List(stale.iter().map(|p| p.typed().into()).collect())
            ))
            .yes(yes),
    )?;
    if !agreed {
        return Ok(stale.iter().map(RootProgram::typed).collect());
    }
    let mut left = Vec::new();
    for program in stale {
        // sudo asks for a password on the terminal, and its lines are its own.
        let _ = out.hand_over();
        let ran = link_secretive::process::command(exe)
            .args(&program.command)
            .status()
            .within(format!("running lnk {}", program.command.join(" ")));
        match ran {
            Ok(status) if status.success() => out.block(Note(sentence!(
                "Root's {} is the new build.",
                Product::new(program.name())
            ))),
            _ => {
                out.block(Note(sentence!(
                    "Root's {} is still the old build.",
                    Product::new(program.name())
                )));
                left.push(program.typed());
            }
        }
    }
    Ok(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_programs_are_root_exec_files_of_modules() {
        let programs = root_programs();
        assert!(!programs.is_empty());
        for program in programs {
            let path = program.location.path();
            assert_eq!(
                path.parent(),
                Some(link_files::layout::SystemRoot::Exec.path().as_path()),
                "{path:?} is in root's exec folder"
            );
            let name = program.name();
            assert!(
                registry::REGISTRY
                    .values()
                    .any(|module| module.component.program == name),
                "{name} is a module's program"
            );
            assert!(!program.command.is_empty());
        }
    }

    #[test]
    fn a_copy_is_behind_only_when_both_are_there_and_differ() {
        let dir = link_test_kit::TempDir::new("root-programs");
        let (root, yours) = (dir.join("root"), dir.join("yours"));
        assert!(!behind(&root, &yours).unwrap(), "neither is there");
        std::fs::write(&yours, b"new build").unwrap();
        assert!(!behind(&root, &yours).unwrap(), "root has no copy");
        std::fs::write(&root, b"old build").unwrap();
        assert!(behind(&root, &yours).unwrap(), "same length, other bytes");
        std::fs::write(&root, b"older build").unwrap();
        assert!(behind(&root, &yours).unwrap(), "another length");
        std::fs::write(&root, b"new build").unwrap();
        assert!(!behind(&root, &yours).unwrap(), "the same build");
    }
}
