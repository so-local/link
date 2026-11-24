//! What the generators write, and its freshness: a checked-in file is
//! refused when it is absent, stale, or edited by hand, and a file a person
//! wrote is never overwritten.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};

/// Generated files by their path from a root, and the folders the
/// generators own whole, where a generated file nothing writes any more is
/// removed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Files {
    files: BTreeMap<PathBuf, String>,
    owned: BTreeSet<PathBuf>,
}

/// Why a checked-in file isn't what its generator writes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// Nothing is there.
    Absent(PathBuf),
    /// It differs from what the generator writes now: stale, or edited by
    /// hand.
    Stale(PathBuf),
    /// A file a person wrote is where a generated one goes.
    Authored(PathBuf),
    /// A generated file in an owned folder that nothing writes any more.
    Orphan(PathBuf),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent(path) => write!(f, "{}: absent; regenerate it", path.display()),
            Self::Stale(path) => write!(
                f,
                "{}: stale or edited by hand; change the declarations and regenerate",
                path.display()
            ),
            Self::Authored(path) => write!(
                f,
                "{}: a file a person wrote is where a generated one goes; move it",
                path.display()
            ),
            Self::Orphan(path) => write!(
                f,
                "{}: generated, but nothing writes it any more; regenerate to remove it",
                path.display()
            ),
        }
    }
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file, refusing a second at the same path.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: String) -> anyhow::Result<()> {
        let path = path.into();
        if !generated(&path, &text) {
            bail!("{} carries no generator marker", path.display());
        }
        if self.files.insert(path.clone(), text).is_some() {
            bail!("two generated files at {}", path.display());
        }
        Ok(())
    }

    /// Marks `folder` as the generators' own.
    pub fn own(&mut self, folder: impl Into<PathBuf>) {
        self.owned.insert(folder.into());
    }

    /// Adds every file and owned folder of `other` under `prefix`.
    pub fn extend(&mut self, prefix: &Path, other: Files) -> anyhow::Result<()> {
        for (path, text) in other.files {
            self.insert(prefix.join(path), text)?;
        }
        self.owned
            .extend(other.owned.into_iter().map(|f| prefix.join(f)));
        Ok(())
    }

    pub fn get(&self, path: impl AsRef<Path>) -> Option<&str> {
        self.files.get(path.as_ref()).map(String::as_str)
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// What differs under `root` from what the generators write, sorted.
    pub fn problems(&self, root: &Path) -> anyhow::Result<Vec<Problem>> {
        let mut out = Vec::new();
        for (path, text) in &self.files {
            let full = root.join(path);
            match read(&full)? {
                None => out.push(Problem::Absent(path.clone())),
                Some(old) if !generated(&full, &old) => out.push(Problem::Authored(path.clone())),
                Some(old) if &old != text => out.push(Problem::Stale(path.clone())),
                Some(_) => {}
            }
        }
        for path in self.orphans(root)? {
            out.push(Problem::Orphan(path));
        }
        out.sort();
        Ok(out)
    }

    /// Writes every absent or stale file under `root` and removes each
    /// orphan, refusing to overwrite a file a person wrote. Answers how
    /// many files it changed.
    pub fn write(&self, root: &Path) -> anyhow::Result<usize> {
        let problems = self.problems(root)?;
        if let Some(Problem::Authored(path)) =
            problems.iter().find(|p| matches!(p, Problem::Authored(_)))
        {
            bail!("{}", Problem::Authored(path.clone()));
        }
        for problem in &problems {
            match problem {
                Problem::Absent(path) | Problem::Stale(path) => {
                    let full = root.join(path);
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .with_context(|| parent.display().to_string())?;
                    }
                    std::fs::write(&full, &self.files[path])
                        .with_context(|| full.display().to_string())?;
                }
                Problem::Orphan(path) => {
                    let full = root.join(path);
                    std::fs::remove_file(&full).with_context(|| full.display().to_string())?;
                }
                Problem::Authored(_) => unreachable!("refused above"),
            }
        }
        Ok(problems.len())
    }

    /// Generated files in owned folders that nothing writes any more.
    fn orphans(&self, root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for folder in &self.owned {
            let mut todo = vec![root.join(folder)];
            while let Some(dir) = todo.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e).with_context(|| dir.display().to_string()),
                };
                for entry in entries {
                    let full = entry?.path();
                    if full.is_dir() {
                        todo.push(full);
                        continue;
                    }
                    let path = full.strip_prefix(root).unwrap_or(&full).to_path_buf();
                    if self.files.contains_key(&path) {
                        continue;
                    }
                    if read(&full)?.is_some_and(|text| generated(&full, &text)) {
                        out.push(path);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Whether `text` carries the marker where a generator puts it: as its
/// top-level `x-link-generator` in JSON, on its first line otherwise. A
/// person's file quoting the marker elsewhere is never taken for one.
fn generated(path: &Path, text: &str) -> bool {
    if path.extension().is_some_and(|e| e == "json") {
        return serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v.get(key())?.as_str().map(|m| m == crate::marker()))
            .unwrap_or(false);
    }
    text.lines()
        .next()
        .is_some_and(|line| line.contains(crate::marker()))
}

fn key() -> &'static str {
    &crate::config::SETTINGS.generator.json_key
}

/// `value`, a JSON object, marked as generated.
pub(crate) fn stamp(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(object) = value.as_object_mut() {
        object.insert(key().into(), crate::marker().into());
    }
    value
}

fn read(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Files, Problem};
    use link_test_kit::temp::TempDir;
    use std::path::PathBuf;

    fn generated(body: &str) -> String {
        format!("// {}\n{body}\n", crate::marker())
    }

    #[test]
    fn absent_stale_authored_and_orphaned_files_are_refused_then_written() {
        let root = TempDir::new("surfaces-files");
        let mut files = Files::new();
        files.own("pages");
        files.insert("pages/a.ts", generated("a")).unwrap();
        files.insert("pages/b.ts", generated("b")).unwrap();
        files.insert("authored.ts", generated("c")).unwrap();
        std::fs::create_dir_all(root.join("pages")).unwrap();
        std::fs::write(root.join("pages/b.ts"), generated("edited by hand")).unwrap();
        std::fs::write(root.join("pages/gone.ts"), generated("gone")).unwrap();
        std::fs::write(root.join("pages/notes.md"), "a person's notes").unwrap();
        std::fs::write(root.join("authored.ts"), "a person's file").unwrap();
        assert_eq!(
            files.problems(&root).unwrap(),
            vec![
                Problem::Absent(PathBuf::from("pages/a.ts")),
                Problem::Stale(PathBuf::from("pages/b.ts")),
                Problem::Authored(PathBuf::from("authored.ts")),
                Problem::Orphan(PathBuf::from("pages/gone.ts")),
            ]
        );
        let refused = files.write(&root).unwrap_err().to_string();
        assert!(refused.contains("authored.ts"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(root.join("authored.ts")).unwrap(),
            "a person's file",
            "a person's file is never overwritten"
        );

        std::fs::remove_file(root.join("authored.ts")).unwrap();
        files.write(&root).unwrap();
        assert_eq!(files.problems(&root).unwrap(), vec![]);
        assert!(!root.join("pages/gone.ts").exists());
        assert!(
            root.join("pages/notes.md").exists(),
            "only generated files are removed"
        );
    }

    #[test]
    fn a_persons_file_quoting_the_marker_is_never_taken_for_generated() {
        let root = TempDir::new("surfaces-quoted");
        let mut files = Files::new();
        files.own("pages");
        std::fs::create_dir_all(root.join("pages")).unwrap();
        let quoted = format!("# Notes\n\nGenerated files say: {}\n", crate::marker());
        std::fs::write(root.join("pages/notes.md"), &quoted).unwrap();
        files.write(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("pages/notes.md")).unwrap(),
            quoted
        );
    }

    #[test]
    fn a_file_with_no_marker_or_a_second_at_one_path_is_refused() {
        let mut files = Files::new();
        assert!(files.insert("a.ts", "no marker".into()).is_err());
        files.insert("a.ts", generated("a")).unwrap();
        assert!(files.insert("a.ts", generated("a")).is_err());
    }
}
