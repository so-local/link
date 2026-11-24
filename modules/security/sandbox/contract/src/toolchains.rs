//! What each of the sandbox's toolchains lets run, declared in
//! `toolchains.toml`. A toolchain is a name a spec's `[commands]
//! toolchains` gives in place of programs it would otherwise list one by
//! one, standing for the programs it names, so a grant always says what it
//! lets run. Its names and what each means are the spec's own type,
//! [`Toolchain`], declared once in the module's `types.toml`; this file
//! holds only what each runs.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;

pub use crate::spec::Toolchain;

/// What the toolchains run, as declared.
const TOOLCHAINS: &str = include_str!("toolchains.toml");

/// What one toolchain runs, as `toolchains.toml` declares it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    /// Its programs, by their names on `PATH`.
    pub programs: Vec<String>,
    /// Where its programs keep those they run themselves, each a folder
    /// (`~` for the home folder).
    pub folders: Vec<String>,
    /// Folders a build writes its own programs to, each beside the
    /// project file that makes it, in a folder the sandbox writes.
    pub builds: Vec<Build>,
}

/// A folder a build writes its own programs to, and the project file
/// beside it that says the build is there.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// The folder, relative, ending `/`.
    pub folder: String,
    /// The file beside it in a project of its kind (`Cargo.toml`).
    pub beside: String,
}

/// What every toolchain runs, by the toolchain, in declared order.
pub fn all() -> &'static BTreeMap<Toolchain, Declared> {
    static ALL: LazyLock<BTreeMap<Toolchain, Declared>> =
        LazyLock::new(|| toml::from_str(TOOLCHAINS).expect("the sandbox's toolchains.toml"));
    &ALL
}

/// What `toolchain` runs: `toolchains.toml` has a table for each one, as
/// a test here proves.
pub fn named(toolchain: Toolchain) -> &'static Declared {
    all()
        .get(&toolchain)
        .expect("toolchains.toml declares what every toolchain runs")
}

/// What a toolchain lets run on this machine: each of its programs found
/// on `path` and each of its folders that's here (a program or folder
/// this machine lacks left out), and its build folders in each of
/// `writes` that holds the project file beside them, and what in the
/// home folder it reads to run them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Here {
    pub programs: Vec<PathBuf>,
    pub builds: Vec<PathBuf>,
    /// Where in `home` its programs are found and its folders are, read
    /// only, since a sandbox's home folder holds only what it's granted:
    /// rustup's proxies in `~/.cargo/bin`, each a link `PATH` finds by its
    /// own name, and its toolchains in `~/.rustup/toolchains`.
    pub read: Vec<PathBuf>,
}

impl Declared {
    /// What it lets run here, its folders' `~` being `home`.
    pub fn here(&self, home: &Path, path: Option<&OsStr>, writes: &[PathBuf]) -> Here {
        let mut read: Vec<PathBuf> = Vec::new();
        let mut in_home = |dir: PathBuf| {
            if dir.starts_with(home) && dir != home && !read.contains(&dir) {
                read.push(dir);
            }
        };
        let mut programs: Vec<PathBuf> = Vec::new();
        for name in &self.programs {
            let found = match path {
                Some(path) => link_files::find_program(name, path),
                None => Some(PathBuf::from(name)),
            };
            let Some(found) = found else { continue };
            if let Some(dir) = found.parent().filter(|_| path.is_some()) {
                in_home(dir.to_path_buf());
            }
            match path {
                Some(_) => programs.extend(found.canonicalize().ok()),
                None => programs.push(found),
            }
        }
        for folder in &self.folders {
            let folder = match folder.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => PathBuf::from(folder),
            };
            if folder.is_dir() {
                in_home(folder.clone());
                programs.push(folder);
            }
        }
        let mut builds: Vec<PathBuf> = Vec::new();
        for write in writes {
            for build in &self.builds {
                let folder = write.join(&build.folder);
                if write.join(&build.beside).is_file() && !builds.contains(&folder) {
                    builds.push(folder);
                }
            }
        }
        Here {
            programs,
            builds,
            read,
        }
    }
}

#[cfg(test)]
mod tests {
    use link_actions::value::Words;

    /// The shell toolchain holds what an installer script runs for its
    /// scratch folder (`mktemp`), a random name (`od` on `/dev/urandom`)
    /// a download's checksum (`sha256sum`) and a folder it emptied
    /// (`rmdir`).
    #[test]
    fn the_shell_runs_what_an_installer_script_runs() {
        let shell = super::named(super::Toolchain::Shell);
        for program in ["mktemp", "od", "sha256sum", "rmdir"] {
            assert!(shell.programs.iter().any(|p| p == program), "{program}");
        }
    }

    /// `toolchains.toml` has a table for each toolchain `types.toml`
    /// declares, and none for a name it doesn't, so the two never drift;
    /// and each declared toolchain says what it is.
    #[test]
    fn every_toolchain_has_one_table() {
        let tables: toml::Table = toml::from_str(super::TOOLCHAINS).unwrap();
        let declared: Vec<&str> = super::Toolchain::WORDS.iter().map(|(_, w)| *w).collect();
        for key in tables.keys() {
            assert!(declared.contains(&key.as_str()), "{key}: not in types.toml");
        }
        for word in &declared {
            assert!(
                tables.contains_key(*word),
                "{word}: no table in toolchains.toml"
            );
        }
        for toolchain in super::Toolchain::all() {
            assert!(
                toolchain.meaning().is_some_and(|m| !m.is_empty()),
                "{}: what it is",
                toolchain.word()
            );
        }
        assert_eq!(super::all().len(), declared.len());
    }

    /// Each toolchain names programs by their plain names, folders as
    /// absolute or home paths ending `/`, and builds as relative folders.
    #[test]
    fn every_toolchain_is_well_formed() {
        assert!(!super::all().is_empty());
        for (toolchain, declared) in super::all() {
            let name = toolchain.word();
            let toolchain = declared;
            assert!(!toolchain.programs.is_empty(), "{name} names programs");
            for program in &toolchain.programs {
                assert!(
                    !program.contains('/') && program != "*",
                    "{name}: {program} is a name on PATH"
                );
            }
            for folder in &toolchain.folders {
                assert!(
                    (folder.starts_with('/') || folder.starts_with("~/")) && folder.ends_with('/'),
                    "{name}: {folder} is an absolute or home folder ending /"
                );
            }
            for build in &toolchain.builds {
                let (folder, beside) = (&build.folder, &build.beside);
                assert!(
                    !folder.starts_with('/') && !folder.contains("..") && folder.ends_with('/'),
                    "{name}: {folder} is a relative folder ending /"
                );
                assert!(
                    !beside.is_empty() && !beside.contains('/'),
                    "{name}: {beside} is a file's name"
                );
            }
        }
    }

    /// A toolchain the sandbox doesn't declare is refused as the spec is
    /// read, by its name, with those there are.
    #[test]
    fn an_unknown_toolchain_names_those_there_are() {
        let error = toml::from_str::<crate::spec::Commands>("toolchains = [\"cobol\"]")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("cobol") && error.contains("rust") && error.contains("node"),
            "{error}"
        );
    }

    /// A build folder is in each folder written that holds its project
    /// file, and nowhere else; a program this machine lacks is left out.
    #[test]
    fn a_toolchain_here_is_what_this_machine_has() {
        let root = link_test_kit::TempDir::new("toolchain");
        let (project, other) = (root.join("project"), root.join("other"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(project.join("Cargo.toml"), "").unwrap();
        let toolchain = super::Declared {
            programs: vec!["sh".into(), "no-such-program-here".into()],
            folders: vec!["/".into(), "/no/such/folder/".into()],
            builds: vec![super::Build {
                folder: "target/".into(),
                beside: "Cargo.toml".into(),
            }],
        };
        let here = toolchain.here(
            std::path::Path::new("/home/x"),
            Some(std::ffi::OsStr::new("/usr/bin:/bin")),
            &[project.clone(), other],
        );
        assert_eq!(here.programs.len(), 2, "{here:?}");
        assert_eq!(here.programs[1], std::path::PathBuf::from("/"));
        assert_eq!(here.builds, [project.join("target/")]);
        assert!(here.read.is_empty(), "nothing here is in /home/x: {here:?}");
    }

    /// A toolchain installed in the home folder, as rustup installs Rust,
    /// is read where it is: the folder `PATH` finds its proxy in, which
    /// runs the toolchain by the name it's linked as, and its toolchains'
    /// folder. Only the resolved program runs.
    #[cfg(unix)]
    #[test]
    fn a_toolchain_in_the_home_folder_is_read_where_it_is() {
        let home = link_test_kit::TempDir::new("toolchain-home");
        let bin = home.join(".cargo/bin");
        let toolchains = home.join(".rustup/toolchains");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&toolchains).unwrap();
        link_test_kit::write_program(&bin.join("rustup"), "#!/bin/sh\n").unwrap();
        std::os::unix::fs::symlink("rustup", bin.join("cargo")).unwrap();
        let toolchain = super::Declared {
            programs: vec!["cargo".into(), "sh".into()],
            folders: vec!["~/.rustup/toolchains/".into()],
            builds: vec![],
        };
        let path = std::env::join_paths([bin.clone(), "/usr/bin".into(), "/bin".into()]).unwrap();
        let here = toolchain.here(&home, Some(&path), &[]);
        assert_eq!(here.read, [bin.clone(), toolchains.clone()], "{here:?}");
        assert_eq!(
            here.programs[0],
            bin.join("rustup").canonicalize().unwrap(),
            "{here:?}"
        );
        assert!(here.programs.contains(&toolchains), "{here:?}");
    }
}
