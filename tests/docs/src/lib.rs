//! The docs' checks that read only files: a workspace is read once, then
//! every rule runs over it at once, a file to a thread, and every
//! failure is reported, not the first. The checks that need Link's own
//! types are `link-docs-check-code`'s, beside this crate; each rule is a
//! line of the `writing-docs` skill, which writers read first.

pub mod abbreviations;
pub mod asides;
pub mod clicks;
mod commands;
pub mod doc_tests;
pub mod examples;
pub mod md;
pub mod navigation;
pub mod outlines;

mod groups;
pub mod kinds;
mod layers;
pub mod layout;
mod limits;
mod list_titles;
mod output;
pub mod pages;
pub mod priority;
pub mod prose;
pub mod references;
mod rust_names;
pub mod rust_source;
mod shape;
pub mod site;
pub mod spec_sources;
pub mod structure;
pub mod terminals;
mod words;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub use limits::{Exception, Large, Limit, Limits, check_exceptions};

/// The file at the top of an archive kept as a folder, such as each of
/// Link's agents in `agents/`: what's below it is the archive's data, an
/// agent's instructions and files, never pages, so no check reads it.
const ARCHIVE_MANIFEST: &str = "archive.toml";

/// Folders no check reads: build output, installed packages, and git
/// worktrees.
const SKIP: &[&str] = &[
    ".git",
    "target",
    "artifacts",
    "node_modules",
    ".next",
    ".turbo",
    "worktrees",
    "__pycache__",
];

/// Where a workspace keeps its list of files over their limits and of
/// folders of data, from the workspace's root.
const LARGE_FILES: &str = "tests/docs/large-files.toml";

/// The list of files over their limits that the docs under `root` are
/// held to: the `tests/docs/large-files.toml` nearest `root`, outside
/// build output and hidden folders, so a workspace's own list when its
/// docs check is deeper than its root. None when `root` holds no list,
/// and refused when two are equally near.
pub fn large_files_of(root: &Path) -> Result<Option<PathBuf>, String> {
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() {
        let found: Vec<PathBuf> = level
            .iter()
            .map(|dir| dir.join(LARGE_FILES))
            .filter(|list| list.is_file())
            .collect();
        match found.as_slice() {
            [] => {}
            [one] => return Ok(Some(one.clone())),
            several => {
                return Err(format!(
                    "{} lists of large files are equally near {}: run from one workspace's root",
                    several.len(),
                    root.display()
                ));
            }
        }
        level = level
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flat_map(|entries| entries.flatten())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                !name.starts_with('.') && !SKIP.contains(&name.as_ref())
            })
            .map(|entry| entry.path())
            .collect();
    }
    Ok(None)
}

/// Where a rule holds: in the public workspace, whose docs are the
/// website, or in every workspace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Site,
    Every,
}

/// A failure, or a warning that fails nothing.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub warning: bool,
    pub text: String,
}

pub fn fail(text: impl Into<String>) -> Finding {
    Finding {
        warning: false,
        text: text.into(),
    }
}

pub fn warn(text: impl Into<String>) -> Finding {
    Finding {
        warning: true,
        text: text.into(),
    }
}

/// How a rule reads the tree: one file at a time, or all of it at once.
#[derive(Clone, Copy)]
pub enum Check {
    Each(fn(&Tree, &Path, &str) -> Vec<Finding>),
    Whole(fn(&Tree) -> Vec<Finding>),
}

/// One rule of the docs: its id, which the `writing-docs` skill names,
/// where it holds, what to do about a failure, and its check.
#[derive(Clone, Copy)]
pub struct Rule {
    pub id: &'static str,
    pub scope: Scope,
    pub fix: &'static str,
    pub check: Check,
}

/// Every rule this crate checks.
pub fn rules() -> Vec<Rule> {
    site::RULES
        .iter()
        .chain(abbreviations::RULES)
        .chain(clicks::RULES)
        .chain(commands::RULES)
        .chain(shape::RULES)
        .chain(navigation::RULES)
        .chain(groups::RULES)
        .chain(kinds::RULES)
        .chain(prose::RULES)
        .chain(pages::RULES)
        .chain(terminals::RULES)
        .chain(layers::RULES)
        .chain(layout::RULES)
        .chain(doc_tests::RULES)
        .chain(output::RULES)
        .chain(structure::RULES)
        .chain(words::RULES)
        .chain(list_titles::RULES)
        .chain(asides::RULES)
        .chain(rust_names::RULES)
        .chain(references::RULES)
        .copied()
        .collect()
}

/// A workspace, read once: every text file under it, and the folders
/// its docs are in.
pub struct Tree {
    /// The workspace's folder.
    pub root: PathBuf,
    /// The checkout's root, which holds `.github/` and `.agents/`: the
    /// workspace's folder once it's a repository of its own, the one
    /// above it before.
    pub checkout: PathBuf,
    /// Every text file read, by its full path.
    pub files: BTreeMap<PathBuf, String>,
    /// Every folder read.
    pub dirs: BTreeSet<PathBuf>,
    /// The folders whose `docs/` make the websites, from
    /// `scripts/documentation-index.json`: none in a workspace without one.
    pub folders: Vec<PathBuf>,
    /// The websites the index names, each with its folders.
    pub websites: Vec<Website>,
    /// Every page of the websites by its English address
    /// (`https://docs.local.link/agents`, `https://www.local.link` for the
    /// home page), to its files: a path in a folder's `docs/` is its
    /// URL on its website, and more than one file at an address fails the
    /// `pages` rule.
    pub pages: BTreeMap<String, Vec<PathBuf>>,
    pub limits: Limits,
    /// The files over their line limit when the limit came in.
    pub large: Large,
}

/// Whether `text` is a reference `link-actions-surfaces` wrote from the
/// modules' declarations, a spec page or a command reference, its marker
/// on its first line: its entries are their declarations' help, said
/// there, and its tree is as deep as they make it.
pub fn generated_reference(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.contains(link_actions_surfaces::marker()))
}

impl Tree {
    /// Whether `rule` doesn't hold `file`, a page `link-actions-surfaces`
    /// wrote in one of its folders, as `limits.toml`'s `[spec-pages]` and
    /// `[reference-pages]` declare.
    pub fn generated_exempt(&self, rule: &str, file: &Path, text: &str) -> bool {
        let within = |folder: &str| {
            let folder: Vec<_> = Path::new(folder).components().collect();
            let path: Vec<_> = file
                .parent()
                .into_iter()
                .flat_map(Path::components)
                .collect();
            path.windows(folder.len())
                .any(|run| run == folder.as_slice())
        };
        generated_reference(text)
            && [&self.limits.spec_pages, &self.limits.reference_pages]
                .into_iter()
                .any(|pages| pages.exempt.iter().any(|r| r == rule) && within(&pages.folder))
    }

    /// Reads the workspace at `root`, the checkout's `.github/`, `.agents/`
    /// and `.claude/` with it when `site`, and `large`, its list of files
    /// over their limit.
    pub fn read(root: &Path, site: bool, large: &Path) -> Tree {
        let root = root.canonicalize().expect("the workspace's folder");
        let checkout = if root.join(".github").exists() {
            root.clone()
        } else {
            root.parent().expect("a folder above").to_path_buf()
        };
        let mut found = Found {
            within: root.clone(),
            ..Found::default()
        };
        found.walk(&root);
        if site && checkout != root {
            found.within.clone_from(&checkout);
            for extra in [".github", ".agents", ".claude"] {
                found.walk(&checkout.join(extra));
            }
        }
        let (folders, websites) = outlines::sites(&root, &found.files)
            .expect("valid documentation indexes rooted in their workspace");
        let limits = Limits::read();
        let large = Large::read(large);
        let mut tree = Tree {
            root,
            checkout,
            files: found.files,
            dirs: found.dirs,
            folders,
            websites,
            pages: BTreeMap::new(),
            limits,
            large,
        };
        tree.pages = tree.find_pages();
        tree
    }

    /// A file's text, when it was read.
    pub fn text(&self, path: &Path) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// A file's path from the workspace (`../.github/...` for the
    /// checkout's).
    pub fn shown(&self, path: &Path) -> String {
        if let Ok(rel) = path.strip_prefix(&self.root) {
            rel.display().to_string()
        } else if let Ok(rel) = path.strip_prefix(&self.checkout) {
            format!("../{}", rel.display())
        } else {
            path.display().to_string()
        }
    }

    /// Whether a path belongs to a declared whole-input or record tree.
    /// Paths remain in `files` for raw inventories and file checks.
    pub fn is_data(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root).is_ok_and(|relative| {
            self.large
                .data
                .iter()
                .any(|data| relative.starts_with(Path::new(data)))
        })
    }

    /// The Markdown people and agents read: every folder's docs, its
    /// `.github/`, and its own `README.md` and `AGENTS.md`, and the
    /// checkout's `.github/`.
    pub fn docs(&self) -> impl Iterator<Item = (&PathBuf, &String)> {
        self.files.iter().filter(|(path, _)| self.is_doc(path))
    }

    pub fn is_doc(&self, path: &Path) -> bool {
        if path.extension().is_none_or(|e| e != "md")
            || self.folders.is_empty()
            || self.is_data(path)
        {
            return false;
        }
        if path.starts_with(self.checkout.join(".github")) {
            return true;
        }
        self.folders.iter().any(|folder| {
            path.starts_with(folder.join("docs"))
                || path.starts_with(folder.join(".github"))
                || ["README.md", "AGENTS.md", "CLAUDE.md"]
                    .iter()
                    .any(|top| path == folder.join(top))
        })
    }

    /// The folder of `folders` a file is in: the outermost that holds it.
    /// The workspace's own root is one, for its `docs/`: a file there has
    /// that `docs/` as its folder, and any other file the folder below.
    pub fn folder_of(&self, path: &Path) -> Option<PathBuf> {
        let below = self
            .folders
            .iter()
            .filter(|folder| **folder != self.root && path.starts_with(folder))
            .min_by_key(|folder| folder.as_os_str().len());
        let docs = self.root.join("docs");
        match below {
            Some(folder) => Some(folder.clone()),
            None if self.folders.contains(&self.root) && path.starts_with(&docs) => Some(docs),
            None => None,
        }
    }

    fn find_pages(&self) -> BTreeMap<String, Vec<PathBuf>> {
        let mut pages: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for website in &self.websites {
            if let Some(homepage) = &website.homepage {
                pages
                    .entry(website.url.clone())
                    .or_default()
                    .push(homepage.clone());
            }
            for (address, file) in &website.shared_pages {
                pages
                    .entry(format!("{}/{address}", website.url))
                    .or_default()
                    .push(file.clone());
            }
        }
        let packages = shape::packages(self);
        for (website, folder) in self
            .websites
            .iter()
            .flat_map(|w| w.folders.iter().map(move |f| (w, f)))
        {
            let docs = folder.join("docs");
            for path in self.files.keys().filter(|p| p.starts_with(&docs)) {
                if !navigation::selected(self, &packages, path)
                    || outlines::metadata(self, path)
                    || navigation::translation(path).is_some()
                {
                    continue;
                }
                let rel = path
                    .strip_prefix(&docs)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let url = rel.trim_end_matches(".md");
                let url = url
                    .strip_suffix("README")
                    .unwrap_or(url)
                    .trim_end_matches('/');
                let address = match url {
                    "" => website.url.clone(),
                    url => format!("{}/{url}", website.url),
                };
                pages.entry(address).or_default().push(path.clone());
            }
        }
        pages
    }

    /// The websites whose app's `navigation.md` is here.
    pub fn web(&self) -> impl Iterator<Item = &Website> {
        self.websites
            .iter()
            .filter(|w| self.files.contains_key(&w.app.join("docs/navigation.md")))
    }

    /// A link to a page of one of the websites, by its address: the
    /// address without its anchor, and the website. Another host, the bare
    /// `local.link` or a tunnel's among them, is no page.
    pub fn site_page<'a>(&'a self, target: &'a str) -> Option<(&'a Website, &'a str)> {
        let website = self.websites.iter().find(|w| {
            target
                .strip_prefix(w.url.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '#']))
        })?;
        let address = target.split(['#', '?']).next().unwrap_or_default();
        Some((website, address.trim_end_matches('/')))
    }
}

/// One of the index's websites: its address, its app's folder, and the
/// folders whose `docs/` are its pages, the app's first.
#[derive(Debug)]
pub struct Website {
    /// Pages of another workspace this site shows, by their address under
    /// its URL, from the index's `sharedPages`.
    pub shared_pages: Vec<(String, PathBuf)>,
    pub workspace: PathBuf,
    pub shared: bool,
    pub name: String,
    pub homepage: Option<PathBuf>,
    pub url: String,
    pub app: PathBuf,
    pub folders: Vec<PathBuf>,
}

#[derive(Default)]
struct Found {
    files: BTreeMap<PathBuf, String>,
    dirs: BTreeSet<PathBuf>,
    /// The folder a link's file must be in to be read: one elsewhere is
    /// another workspace's, checked there. Empty, every file is read.
    within: PathBuf,
}

impl Found {
    fn walk(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        self.dirs.insert(dir.to_path_buf());
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !SKIP.contains(&entry.file_name().to_string_lossy().as_ref())
                    && !path.join(ARCHIVE_MANIFEST).is_file()
                {
                    self.walk(&path);
                }
            } else if (!kind.is_symlink() || self.reads_link(&path))
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                self.files.insert(path, text);
            }
        }
    }

    /// Whether a link is read: a file, inside [`Found::within`].
    fn reads_link(&self, path: &Path) -> bool {
        path.is_file()
            && path
                .canonicalize()
                .is_ok_and(|target| target.starts_with(&self.within))
    }
}

#[cfg(test)]
mod discovery_tests {
    /// A workspace's own list is the one nearest its root, however deep
    /// its docs check is; none is found where there is none, and two
    /// equally near are refused.
    #[test]
    fn the_nearest_list_of_large_files_is_the_roots() {
        let dir = link_test_kit::TempDir::new("docs-large-files");
        assert_eq!(super::large_files_of(&dir).unwrap(), None);
        let deep = dir.join("source/one/tests/docs");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("large-files.toml"), "").unwrap();
        let built = dir.join("target/tests/docs");
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join("large-files.toml"), "").unwrap();
        assert_eq!(
            super::large_files_of(&dir).unwrap(),
            Some(deep.join("large-files.toml"))
        );
        let other = dir.join("source/two/tests/docs");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("large-files.toml"), "").unwrap();
        assert!(super::large_files_of(&dir).is_err());
    }

    /// A link to a file of the workspace is read; one to a file of
    /// another workspace is that workspace's, checked there.
    #[test]
    fn a_link_out_of_the_workspace_is_not_read() {
        let dir = link_test_kit::TempDir::new("docs-links");
        let (workspace, other) = (dir.join("workspace"), dir.join("other"));
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(workspace.join("README.md"), "# Here\n").unwrap();
        std::fs::write(other.join("main.rs"), "fn main() {}\n").unwrap();
        std::os::unix::fs::symlink(workspace.join("README.md"), workspace.join("same.md")).unwrap();
        std::os::unix::fs::symlink(other.join("main.rs"), workspace.join("main.rs")).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let mut found = super::Found {
            within: workspace.clone(),
            ..super::Found::default()
        };
        found.walk(&workspace);
        assert_eq!(
            found.files.keys().collect::<Vec<_>>(),
            [&workspace.join("README.md"), &workspace.join("same.md")]
        );
    }

    #[test]
    fn generated_artifacts_do_not_become_source_documentation() {
        let dir = link_test_kit::TempDir::new("docs-artifacts");
        std::fs::write(dir.join("README.md"), "# Source\n").unwrap();
        std::fs::create_dir_all(dir.join("artifacts/run")).unwrap();
        std::fs::write(dir.join("artifacts/run/results.md"), "# Generated run\n").unwrap();
        let mut found = super::Found::default();
        found.walk(&dir);
        assert_eq!(
            found.files.keys().collect::<Vec<_>>(),
            [&dir.join("README.md")]
        );
        assert!(!found.dirs.contains(&dir.join("artifacts")));
    }

    #[test]
    fn an_archive_kept_as_a_folder_is_data_not_documentation() {
        let dir = link_test_kit::TempDir::new("docs-archives");
        std::fs::write(dir.join("README.md"), "# Agents\n").unwrap();
        std::fs::create_dir_all(dir.join("coach/Agents/coach/home")).unwrap();
        std::fs::write(dir.join("coach").join(super::ARCHIVE_MANIFEST), "").unwrap();
        std::fs::write(dir.join("coach/Agents/coach/home/README.md"), "# Notes\n").unwrap();
        let mut found = super::Found::default();
        found.walk(&dir);
        assert_eq!(
            found.files.keys().collect::<Vec<_>>(),
            [&dir.join("README.md")]
        );
    }
}

/// What a run found, by rule.
#[derive(Default)]
pub struct Report {
    pub failures: BTreeMap<&'static str, Vec<String>>,
    pub warnings: BTreeMap<&'static str, Vec<String>>,
    fixes: BTreeMap<&'static str, &'static str>,
}

impl Report {
    /// Adds a failure found outside the rules, under `id` and what to do
    /// about it, so a test that checks more than the rules still fails
    /// once, with everything it found.
    pub fn fail(&mut self, id: &'static str, fix: &'static str, text: impl Into<String>) {
        self.fixes.insert(id, fix);
        self.failures.entry(id).or_default().push(text.into());
    }

    /// Prints the warnings, and fails with every failure, each rule's
    /// under what to do about it.
    pub fn assert(self) {
        for (id, warnings) in &self.warnings {
            eprintln!("warnings ({id}):\n{}\n", warnings.join("\n"));
        }
        let failed: Vec<String> = self
            .failures
            .iter()
            .map(|(id, found)| format!("{id}: {}\n{}", self.fixes[id], found.join("\n")))
            .collect();
        assert!(failed.is_empty(), "\n{}", failed.join("\n\n"));
    }
}

/// A check's findings, or one failure naming `at` when the check panics,
/// so one rule's panic never hides every other rule's findings.
fn guarded(at: &str, check: impl FnOnce() -> Vec<Finding>) -> Vec<Finding> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)).unwrap_or_else(|panic| {
        let said = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        vec![fail(format!("{at}: the check panicked: {said}"))]
    })
}

/// Runs `rules` over `tree`, those `Each` a file at a time and those
/// `Whole` once, on as many threads as the machine has, at most eight.
pub fn run(tree: &Tree, rules: &[Rule]) -> Report {
    let whole: Vec<&Rule> = rules
        .iter()
        .filter(|r| matches!(r.check, Check::Whole(_)))
        .collect();
    let each: Vec<&Rule> = rules
        .iter()
        .filter(|r| matches!(r.check, Check::Each(_)))
        .collect();
    let files: Vec<(&PathBuf, &String)> = tree.files.iter().collect();
    let jobs = whole.len() + files.len();
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let found: Vec<(&'static str, Finding)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let job = next.fetch_add(1, Ordering::Relaxed);
                        if job >= jobs {
                            break;
                        }
                        if let Some(rule) = whole.get(job) {
                            let Check::Whole(check) = rule.check else {
                                unreachable!()
                            };
                            let found = guarded("the whole tree", || check(tree));
                            mine.extend(found.into_iter().map(|f| (rule.id, f)));
                            continue;
                        }
                        let (path, text) = files[job - whole.len()];
                        for rule in &each {
                            let Check::Each(check) = rule.check else {
                                unreachable!()
                            };
                            let at = path.display().to_string();
                            let found = guarded(&at, || check(tree, path, text));
                            mine.extend(found.into_iter().map(|f| (rule.id, f)));
                        }
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a check panicked"))
            .collect()
    });
    let mut report = Report::default();
    for rule in rules {
        report.fixes.insert(rule.id, rule.fix);
    }
    for (id, finding) in found {
        let list = match finding.warning {
            true => report.warnings.entry(id).or_default(),
            false => report.failures.entry(id).or_default(),
        };
        list.push(finding.text);
    }
    for list in report
        .failures
        .values_mut()
        .chain(report.warnings.values_mut())
    {
        list.sort();
    }
    report
}

/// The Docs Rules page, from the workspace's root.
pub const RULES: &str = "tests/docs/docs/more/developing/documentation/docs-rules.md";

/// The Docs Rules page ([`RULES`]): one line per rule, naming its id in
/// backticks, with its limit's number where it has one. A rule without
/// its line, or a line with another number, fails, so no rule or limit
/// lands without the writers seeing it first.
pub fn rules_page(tree: &Tree, rules: &[Rule]) -> Vec<Finding> {
    let path = tree.root.join(RULES);
    let Some(text) = tree.text(&path) else {
        return vec![fail(format!("{} is missing", tree.shown(&path)))];
    };
    let mut found = Vec::new();
    for rule in rules {
        let tag = format!("(`{}`", rule.id);
        let Some(line) = text.lines().find(|l| l.contains(&tag)) else {
            found.push(fail(format!(
                "{}: no line names `{}`",
                tree.shown(&path),
                rule.id
            )));
            continue;
        };
        if let Some(limit) = tree.limits.limit.get(rule.id) {
            let number = limit.number();
            if !line.contains(&number) {
                found.push(fail(format!(
                    "{}: `{}` says another number than limits.toml's {number}",
                    tree.shown(&path),
                    rule.id
                )));
            }
        }
    }
    found
}

#[cfg(test)]
link_test_kit::program!(system);
