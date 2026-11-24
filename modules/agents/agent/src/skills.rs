//! `lnk agent skill add`, `list` and `remove`: a skill's folder (the Agent Skills standard's,
//! [`link_harness_wire::skill`]) copied into the agent's `skills/`, from this
//! machine or a git repository, named as its `SKILL.md` says. A harness
//! that keeps to Link's layout lists it in its prompt from its next
//! message; nothing restarts.
//!
//! What's copied is checked first and copied whole or not at all: its
//! `SKILL.md` must say a name and a description, links and anything but
//! files and folders are left out, and at most [`FILES_MAX`] files and
//! [`BYTES_MAX`] bytes are taken. It's put in place by a rename, so an
//! agent never lists a skill half copied.

use link_format::WithinExt;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{bail, ensure};
use link_harness_contract::harness::layout;
use link_harness_wire::skill::{self, Skill};

use link_files::nofollow::{Copying, Dir, Limit, Links};

/// The most files a skill may hold.
pub const FILES_MAX: usize = 1000;

/// The most bytes a skill's files may hold together.
pub const BYTES_MAX: u64 = 64 << 20;

/// The most folders a skill may hold.
pub const FOLDERS_MAX: usize = 1000;

/// The deepest a folder in a skill may be.
pub const DEPTH_MAX: usize = 32;

/// The largest `SKILL.md` read.
const SKILL_FILE_MAX: u64 = 1 << 20;

/// What was added: the skill, where it is now, and what was left out
/// (links, pipes), by their paths in it.
#[derive(Debug)]
pub struct Added {
    pub skill: Skill,
    pub at: PathBuf,
    pub replaced: bool,
    pub left_out: Vec<String>,
}

/// Where a skill comes from: a folder here, or a folder in a git
/// repository.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Path(PathBuf),
    Git {
        url: String,
        /// The branch or tag; none: the repository's default.
        reference: Option<String>,
        /// The skill's folder in it; empty: its top.
        folder: String,
    },
}

impl Source {
    /// `text` as a source: an `https://` or `file://` URL is a git
    /// repository (GitHub's `/tree/<branch>/<folder>` and
    /// `/blob/<branch>/<folder>/SKILL.md` name a folder in one); anything
    /// else is a path here, to a skill's folder or its `SKILL.md`.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        if !text.contains("://") {
            let path = PathBuf::from(text);
            let path = match path.file_name().is_some_and(|n| n == skill::FILE) {
                true => path.parent().map(Path::to_path_buf).unwrap_or_default(),
                false => path,
            };
            return Ok(Self::Path(match path.as_os_str().is_empty() {
                true => PathBuf::from("."),
                false => path,
            }));
        }
        ensure!(
            text.starts_with("https://") || text.starts_with("file://"),
            "{:?}: a skill comes from a path here, or a git repository's https:// or file:// URL",
            link_format::clean(text)
        );
        ensure!(
            !text.chars().any(|c| c.is_control() || c.is_whitespace()) && text.len() <= 2048,
            "{:?} isn't a URL",
            link_format::clean(text)
        );
        let trimmed = text.trim_end_matches('/');
        // GitHub's page of a folder, or of a file in it, at a branch.
        if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 4 && (parts[2] == "tree" || parts[2] == "blob") {
                let mut folder: Vec<&str> = parts[4..].to_vec();
                if folder.last() == Some(&skill::FILE) {
                    folder.pop();
                }
                let folder = folder.join("/");
                check_folder(&folder)?;
                return Ok(Self::Git {
                    url: format!("https://github.com/{}/{}", parts[0], parts[1]),
                    reference: Some(parts[3].to_string()),
                    folder,
                });
            }
        }
        Ok(Self::Git {
            url: trimmed.to_string(),
            reference: None,
            folder: String::new(),
        })
    }
}

/// An error for a folder in a repository that could reach outside it.
fn check_folder(folder: &str) -> anyhow::Result<()> {
    ensure!(
        folder.is_empty()
            || folder
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "{:?} isn't a folder in the repository",
        link_format::clean(folder)
    );
    Ok(())
}

/// A folder of ours, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The skills in `own`'s `skills/` (the agent's own folder), by name: each folder whose
/// `SKILL.md` says one, links passed over.
pub fn list(own: &Path) -> anyhow::Result<Vec<Skill>> {
    let skills = own.join(layout::SKILLS);
    let Ok(entries) = std::fs::read_dir(&skills) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let file = entry.path().join(skill::FILE);
        let Ok(meta) = std::fs::symlink_metadata(&file) else {
            continue;
        };
        if !meta.is_file() || meta.len() > SKILL_FILE_MAX {
            continue;
        }
        if let Ok(skill) = std::fs::read_to_string(&file)
            .map_err(anyhow::Error::from)
            .and_then(|text| skill::parse(&text))
        {
            found.push(skill);
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Removes skill `name` from `own`'s `skills/`: where it was. Its name
/// as the standard takes it, and a folder, never a link.
pub fn remove(own: &Path, name: &str) -> anyhow::Result<PathBuf> {
    skill::check_name(name)?;
    let at = own.join(layout::SKILLS).join(name);
    let meta = std::fs::symlink_metadata(&at)
        .map_err(|_| anyhow::anyhow!("no skill {name}: lnk agent skill list lists them"))?;
    ensure!(
        meta.is_dir(),
        "{} isn't a skill's folder: not removing it",
        at.display()
    );
    std::fs::remove_dir_all(&at).within(format!("removing {}", at.display()))?;
    Ok(at)
}

/// Copies the skill at `source` into `own`'s `skills/`, as
/// `skills/<name>/`: refused when one of that name is there, unless
/// `replace`.
///
/// The harness writes `skills/` while this runs, outside its sandbox, so
/// a repository is cloned into a private folder of the system's, never
/// there, and every file is read and written through its folder's
/// handle, never following a link: one swapped in for a file or a
/// folder, in what's copied or where it's copied to, is refused, never
/// followed. The copy is put in place by a rename.
pub fn add(own: &Path, source: &Source, replace: bool) -> anyhow::Result<Added> {
    let skills = own.join(layout::SKILLS);
    if std::fs::symlink_metadata(&skills).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("{} is a link: not following it", skills.display());
    }
    std::fs::create_dir_all(&skills).within(format!("creating {}", skills.display()))?;
    let skills_dir = Dir::open(&skills).within(format!("opening {}", skills.display()))?;
    let (_clone, from) = match source {
        Source::Path(path) => {
            let meta =
                std::fs::symlink_metadata(path).within(format!("reading {}", path.display()))?;
            ensure!(
                meta.is_dir(),
                "{} isn't a skill's folder: a folder holding {}",
                path.display(),
                skill::FILE
            );
            let dir = Dir::open(path).within(format!("opening {}", path.display()))?;
            (None, dir)
        }
        Source::Git {
            url,
            reference,
            folder,
        } => {
            let clone = Scratch(
                link_files::private_temp_dir("skill").within("making a folder to clone into")?,
            );
            let into = clone.0.join("repository");
            git_clone(url, reference.as_deref(), &into)?;
            if !into.join(folder).join(skill::FILE).is_file() {
                let found = skills_in(&into);
                if !found.is_empty() {
                    bail!(
                        "{url} holds no {} at {}; it holds skills in {}: name one, as \
                         lnk agent skill add <url>/tree/<branch>/<folder> on GitHub",
                        skill::FILE,
                        match folder.is_empty() {
                            true => "its top",
                            false => folder,
                        },
                        found.join(", ")
                    );
                }
            }
            // Each folder on the way opened without following a link: a
            // committed link can't lead out of the clone.
            let mut dir = Dir::open(&into).within(format!("opening {}", into.display()))?;
            for part in folder.split('/').filter(|p| !p.is_empty()) {
                dir = dir.folder(OsStr::new(part)).within(format!(
                    "{url}'s {folder} isn't a folder in it (a link is refused)"
                ))?;
            }
            (Some(clone), dir)
        }
    };
    let shown = match source {
        Source::Path(path) => path.display().to_string(),
        Source::Git { url, folder, .. } => format!("{url} {folder}").trim_end().to_string(),
    };
    let found = read_skill(&from, &shown)?;
    let name = OsStr::new(&found.name);
    let at = skills.join(&found.name);
    let there = skills_dir.stat(name).is_ok();
    ensure!(
        !there || replace,
        "your agent has a skill {} already, at {}: --replace replaces it",
        found.name,
        at.display()
    );
    // Copied whole beside it, then put in place.
    let stage = OsString::from(format!("{}.lnk-{}", found.name, std::process::id()));
    let _ = std::fs::remove_dir_all(skills.join(&stage));
    let _staged = Scratch(skills.join(&stage));
    let into = skills_dir
        .create_new_folder(&stage, 0o755)
        .within(format!("creating {}", skills.join(&stage).display()))?;
    let left_out = copy_skill(&from, &into)?;
    if there {
        let old = OsString::from(format!("{}.old.lnk-{}", found.name, std::process::id()));
        let _ = std::fs::remove_dir_all(skills.join(&old));
        let _old = Scratch(skills.join(&old));
        skills_dir
            .rename(name, &old)
            .within(format!("moving {}", at.display()))?;
        skills_dir
            .rename(&stage, name)
            .within(format!("writing {}", at.display()))?;
    } else {
        skills_dir
            .rename(&stage, name)
            .within(format!("writing {}", at.display()))?;
    }
    Ok(Added {
        skill: found,
        at,
        replaced: there,
        left_out,
    })
}

/// The skill folder `dir` holds, by its `SKILL.md`; `shown` names the
/// folder.
fn read_skill(dir: &Dir, shown: &str) -> anyhow::Result<Skill> {
    let file = dir.open_entry(OsStr::new(skill::FILE)).within(format!(
        "{shown} isn't a skill's folder: it holds no {}",
        skill::FILE
    ))?;
    ensure!(
        file.metadata()?.is_file(),
        "{shown}'s {} isn't a file",
        skill::FILE
    );
    let mut text = String::new();
    file.take(SKILL_FILE_MAX)
        .read_to_string(&mut text)
        .within(format!("reading {shown}'s {}", skill::FILE))?;
    skill::parse(&text).within(format!("reading {shown}'s {}", skill::FILE))
}

/// Copies the skill's folder `from` into `to` (made already): files and
/// folders only, never `.git`, a link or a pipe (what it left out, by
/// their paths in it); an error past [`FILES_MAX`], [`BYTES_MAX`],
/// [`FOLDERS_MAX`] or [`DEPTH_MAX`], or for
/// a link swapped in meanwhile, on either side ([`Dir::copy_into`]).
fn copy_skill(from: &Dir, to: &Dir) -> anyhow::Result<Vec<String>> {
    let copied = from.copy_into(
        to,
        &Copying {
            skip: &|path| path.file_name().is_some_and(|name| name == ".git"),
            links: Links::LeaveOut,
            limit: Some(Limit {
                files: FILES_MAX,
                bytes: BYTES_MAX,
                folders: FOLDERS_MAX,
                depth: DEPTH_MAX,
                what: "the skill",
            }),
            private: false,
        },
    )?;
    Ok(copied
        .left_out
        .iter()
        .map(|path| path.display().to_string())
        .collect())
}

/// The folders under `root` holding a `SKILL.md`, by their paths in it,
/// three deep at most, at most ten.
fn skills_in(root: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, depth: usize, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(root.join(at)) else {
            return;
        };
        let mut names: Vec<_> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name())
            .filter(|n| !n.to_string_lossy().starts_with('.'))
            .collect();
        names.sort();
        for name in names {
            let inside = at.join(&name);
            if root.join(&inside).join(skill::FILE).is_file() {
                found.push(inside.display().to_string());
            } else if depth < 3 {
                walk(root, &inside, depth + 1, found);
            }
            if found.len() >= 10 {
                return;
            }
        }
    }
    let mut found = Vec::new();
    walk(root, Path::new(""), 1, &mut found);
    found
}

/// Clones `url` at `reference` into `into`, shallowly, asking nothing.
fn git_clone(url: &str, reference: Option<&str>, into: &Path) -> anyhow::Result<()> {
    let mut git = link_secretive::process::command("git");
    git.args(["clone", "--quiet", "--depth", "1", "--no-tags"]);
    if let Some(reference) = reference {
        git.args(["--branch", reference]);
    }
    git.arg("--")
        .arg(url)
        .arg(into)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    let output = match git.output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => bail!(
            "adding a skill from a URL needs git: install it, or download the skill's folder and \
             add it by its path"
        ),
        Err(err) => return Err(err).within("running git"),
    };
    ensure!(
        output.status.success(),
        "couldn't clone {url}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lnk-skills-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn skill_at(folder: &Path, name: &str, about: &str) {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {about}\n---\n# {name}\n"),
        )
        .unwrap();
    }

    #[test]
    fn a_source_is_a_path_or_a_repository() {
        assert_eq!(
            Source::parse("./pdf").unwrap(),
            Source::Path("./pdf".into())
        );
        assert_eq!(
            Source::parse("pdf/SKILL.md").unwrap(),
            Source::Path("pdf".into())
        );
        assert_eq!(Source::parse("SKILL.md").unwrap(), Source::Path(".".into()));
        assert_eq!(
            Source::parse("https://github.com/someone/skills/tree/main/skills/pdf/").unwrap(),
            Source::Git {
                url: "https://github.com/someone/skills".into(),
                reference: Some("main".into()),
                folder: "skills/pdf".into(),
            }
        );
        assert_eq!(
            Source::parse("https://github.com/someone/skills/blob/v2/pdf/SKILL.md").unwrap(),
            Source::Git {
                url: "https://github.com/someone/skills".into(),
                reference: Some("v2".into()),
                folder: "pdf".into(),
            }
        );
        assert_eq!(
            Source::parse("https://example.com/pdf.git").unwrap(),
            Source::Git {
                url: "https://example.com/pdf.git".into(),
                reference: None,
                folder: String::new(),
            }
        );
        for refused in [
            "http://example.com/pdf.git",
            "ssh://example.com/pdf.git",
            "https://github.com/a/b/tree/main/../../etc",
            "https://example.com/a b",
        ] {
            assert!(Source::parse(refused).is_err(), "{refused}");
        }
    }

    /// A link where a file or folder is copied to, as a harness writing
    /// `skills/` could plant one, is refused, never written through.
    #[test]
    fn a_copy_never_writes_through_a_planted_link() {
        let root = root("planted");
        let (from, to, outside) = (root.join("from"), root.join("to"), root.join("outside"));
        skill_at(&from, "pdf", "Read PDF files.");
        std::fs::create_dir_all(from.join("scripts")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(outside.join("SKILL.md"), to.join("SKILL.md")).unwrap();
        let copied = copy_skill(&Dir::open(&from).unwrap(), &Dir::open(&to).unwrap());
        assert!(copied.is_err());
        assert!(
            !outside.join("SKILL.md").exists(),
            "written through the link"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A repository's skill folder that's a committed link, out of the
    /// clone, is refused; the clone is never in the agent's files.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_cloned_skill_cant_lead_out_of_its_clone() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = root("clone-link");
        let (repo, files, outside) = (root.join("repo"), root.join("files"), root.join("outside"));
        skill_at(&outside, "secret", "Not the repository's.");
        std::fs::create_dir_all(&repo).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("pdf")).unwrap();
        let git = |args: &[&str]| {
            let done = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .unwrap();
            assert!(done.status.success(), "{done:?}");
        };
        git(&["init", "--quiet"]);
        git(&["add", "-A"]);
        git(&["commit", "--quiet", "-m", "x"]);
        let source = Source::Git {
            url: format!("file://{}", repo.display()),
            reference: None,
            folder: "pdf".into(),
        };
        let err = add(&files, &source, false).unwrap_err();
        assert!(
            format!("{err:#}").contains("isn't a folder in it"),
            "{err:#}"
        );
        assert!(!files.join("skills/secret").exists());
        let names: Vec<_> = std::fs::read_dir(files.join("skills"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(names.is_empty(), "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_skill_is_copied_whole_by_its_name() {
        let root = root("copy");
        let (from, files) = (root.join("downloads/pdf-helper"), root.join("files"));
        skill_at(&from, "pdf", "Read PDF files.");
        std::fs::create_dir_all(from.join("scripts")).unwrap();
        std::fs::write(from.join("scripts/extract.py"), "print('hi')\n").unwrap();
        std::fs::create_dir_all(from.join(".git")).unwrap();
        std::fs::write(from.join(".git/HEAD"), "ref").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", from.join("passwd")).unwrap();
        let added = add(&files, &Source::Path(from.clone()), false).unwrap();
        assert_eq!(added.skill.name, "pdf");
        assert_eq!(added.at, files.join("skills/pdf"));
        assert!(!added.replaced);
        assert_eq!(added.left_out, ["passwd"]);
        assert!(files.join("skills/pdf/SKILL.md").is_file());
        assert!(files.join("skills/pdf/scripts/extract.py").is_file());
        assert!(!files.join("skills/pdf/.git").exists());
        assert!(!files.join("skills/pdf/passwd").exists());
        // Nothing of ours left beside it.
        let names: Vec<_> = std::fs::read_dir(files.join("skills"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["pdf"]);

        // Again: refused, unless replaced, and then only what's new.
        skill_at(&from, "pdf", "Read and fill PDF files.");
        std::fs::remove_dir_all(from.join("scripts")).unwrap();
        let err = add(&files, &Source::Path(from.clone()), false).unwrap_err();
        assert!(format!("{err:#}").contains("--replace"), "{err:#}");
        let added = add(&files, &Source::Path(from.clone()), true).unwrap();
        assert!(added.replaced);
        assert!(!files.join("skills/pdf/scripts").exists());
        let names: Vec<_> = std::fs::read_dir(files.join("skills"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["pdf"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_isnt_a_skill_is_refused() {
        let root = root("refused");
        let files = root.join("files");
        // No SKILL.md, no front matter, a name the standard doesn't take.
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::create_dir_all(root.join("plain")).unwrap();
        std::fs::write(root.join("plain/SKILL.md"), "# Plain\n").unwrap();
        skill_at(&root.join("big"), "Big", "No.");
        for name in ["empty", "plain", "big", "missing"] {
            assert!(
                add(&files, &Source::Path(root.join(name)), false).is_err(),
                "{name}"
            );
        }
        // Too many files.
        let many = root.join("many");
        skill_at(&many, "many", "Too many.");
        for n in 0..FILES_MAX {
            std::fs::write(many.join(format!("{n}.txt")), "").unwrap();
        }
        let err = add(&files, &Source::Path(many), false).unwrap_err();
        assert!(
            format!("{err:#}").contains("more than 1000 files"),
            "{err:#}"
        );
        // Nothing was added, nor left half copied.
        let names: Vec<_> = std::fs::read_dir(files.join("skills"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(names.is_empty(), "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_skill_comes_from_a_repository() {
        let have_git = Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !have_git {
            return;
        }
        let root = root("git");
        let (repository, files) = (root.join("repository"), root.join("files"));
        skill_at(
            &repository.join("skills/tides"),
            "tides",
            "Read a tide table.",
        );
        skill_at(&repository.join("skills/bread"), "bread", "Bake bread.");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "init.defaultBranch=main",
                ])
                .args(args)
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "skills"]);
        let url = format!("file://{}", repository.display());
        // Its top holds none: it says which folders do.
        let err = add(&files, &Source::parse(&url).unwrap(), false).unwrap_err();
        assert!(
            format!("{err:#}").contains("skills/bread, skills/tides"),
            "{err:#}"
        );
        let source = Source::Git {
            url,
            reference: Some("main".into()),
            folder: "skills/tides".into(),
        };
        let added = add(&files, &source, false).unwrap();
        assert_eq!(added.skill.name, "tides");
        assert!(files.join("skills/tides/SKILL.md").is_file());
        let names: Vec<_> = std::fs::read_dir(files.join("skills"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["tides"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
