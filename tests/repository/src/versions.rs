//! No Link version past the workspace's own: a tracked file outside test
//! code never names a release of Link beyond the line the workspace's
//! version is on, its major and minor numbers, as a floor, a minimum, a
//! step's `since`, an example or a fact. Versions are reset or bumped in
//! one place, the workspace's `Cargo.toml`, and whatever named a release
//! above it would then require one that doesn't exist. A version is
//! Link's where its context says so ([`CONTEXTS`]): `lnk` or a module
//! saying its version, Link and a release, the install's version flag
//! or variable, a release's minimum, a staging, dev or prod build's
//! stamp, a step's `since`, a harness's or relay's image, a
//! `Version::new` and a release named as one. A test's versions are its
//! own inputs, as a far-future release standing in for the next, and a
//! test that leans on the workspace's version fails by itself, so test
//! code is left out ([`is_test`]). A file that must say one anyway, a
//! third party's version said the same way, is listed in its
//! workspace's `versions.toml` with how many such lines it has, and the
//! list may shrink, never grow. Each workspace runs it on its own root.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Where a version is Link's, each with the version's major and minor
/// numbers as its first two groups.
pub const CONTEXTS: [&str; 8] = [
    // `lnk` or a module saying its version.
    r"\blnk(?:-[a-z][a-z-]*)? v?(\d+)\.(\d+)\.\d+",
    // Link and a release.
    r"\b(?:Local )?Link v?(\d+)\.(\d+)\b",
    // The version an install, an upgrade or a minimum names.
    r"(?:LNK_VERSION|--lnk\.version|lnk upgrade|MIN_VERSION|minimum_release|minimum_version|RELEASE_MINIMUM|min-relay-version|lnk-release-minimum)[^0-9A-Za-z\n]{1,4}v?(\d+)\.(\d+)",
    // A staging, dev or prod build's stamp.
    r"(?:^|[^\w.])v?(\d+)\.(\d+)\.\d+-(?:staging|dev|prod)\.",
    // The release a schema step, a transfer or a history entry comes at.
    r#"\bsince\s*=\s*\\?["'](\d+)\.(\d+)"#,
    // A harness's or the relay's image.
    r"\b(?:harness|relay)@(\d+)\.(\d+)",
    // A version made in code.
    r"\bVersion::new\((\d+),\s*(\d+),",
    // A release named as one.
    r"\b[Rr]elease (?:candidate at )?v?(\d+)\.(\d+)\b",
];

static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    CONTEXTS
        .iter()
        .map(|context| Regex::new(context).expect("a context is a regex"))
        .collect()
});

/// A workspace's line: the major and minor numbers of the version its
/// root `Cargo.toml` gives under `[workspace.package]`.
pub fn line(manifest: &Path) -> (u64, u64) {
    let text =
        std::fs::read_to_string(manifest).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
    let value: toml::Value = toml::from_str(&text).expect("a Cargo.toml");
    let version = value["workspace"]["package"]["version"]
        .as_str()
        .expect("[workspace.package] version");
    let mut numbers = version.split('.').map(|n| n.parse::<u64>());
    match (numbers.next(), numbers.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor),
        _ => panic!("{version:?} isn't a version"),
    }
}

/// Whether `file`, a path from the root, is test code or a test's input:
/// in a `tests` folder, a `tests.rs`, a `.test.` script, a Python
/// `test_` module, or, Markdown aside, in a `fixtures` or `cases` folder.
pub fn is_test(file: &str) -> bool {
    let path = Path::new(file);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let folders: Vec<String> = path
        .parent()
        .map(|dir| {
            dir.components()
                .map(|part| part.as_os_str().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    folders.iter().any(|folder| folder == "tests")
        || name == "tests.rs"
        || name.contains(".test.")
        || (name.starts_with("test_") && name.ends_with(".py"))
        || (!name.ends_with(".md")
            && folders
                .iter()
                .any(|folder| folder == "fixtures" || folder == "cases"))
}

/// Whether `file` is left out as another's or as history: a lock file,
/// a session's frozen record, or research into another company's product.
pub fn skipped(file: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml"
    ) || file.contains("data/logs/")
        || file.contains("docs/discovery/")
}

/// The lines of `text` that name a Link version past `line`, each with
/// its number and the words naming it. In Rust, the lines from the first
/// `#[cfg(test)]` on are a test's and left out.
pub fn ahead(text: &str, rust: bool, line: (u64, u64)) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (at, said) in text.lines().enumerate() {
        if rust && said.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        for pattern in PATTERNS.iter() {
            for named in pattern.captures_iter(said) {
                let number = |group: usize| named[group].parse::<u64>().unwrap_or(u64::MAX);
                if (number(1), number(2)) > line {
                    found.push((at + 1, named[0].trim().to_string()));
                }
            }
        }
    }
    found
}

/// Each tracked file under `root`, outside test code, that names a Link
/// version past `line`, by its path from `root`, with the lines naming one.
pub fn found(root: &Path, line: (u64, u64)) -> BTreeMap<String, Vec<(usize, String)>> {
    crate::credentials::tracked(root)
        .into_iter()
        .filter(|file| !is_test(file) && !skipped(file))
        .filter_map(|file| {
            let text = std::fs::read_to_string(root.join(&file)).ok()?;
            let named = ahead(&text, file.ends_with(".rs"), line);
            (!named.is_empty()).then_some((file, named))
        })
        .collect()
}

/// The problems of `found` against `listed`, a `versions.toml`'s
/// `[lines]`: none when every file is listed at exactly as many lines as
/// it has, since the list may shrink, never grow.
pub fn compare(
    found: &BTreeMap<String, Vec<(usize, String)>>,
    listed: &BTreeMap<String, usize>,
    line: (u64, u64),
) -> Vec<String> {
    let (major, minor) = line;
    let mut problems = Vec::new();
    for (file, named) in found {
        let n = named.len();
        match listed.get(file) {
            None => problems.extend(named.iter().map(|(at, said)| {
                format!(
                    "{file}:{at}: {said:?} names a Link release past this workspace's \
                     {major}.{minor}: name one on its line, or say it without a version; \
                     versions.toml takes no new file"
                )
            })),
            Some(&was) if n > was => problems.push(format!(
                "{file}: {n} lines name a Link release past {major}.{minor}, up from {was}: \
                 take the new one out ({named:?})"
            )),
            Some(&was) if n < was => problems.push(format!(
                "{file}: {n} lines name a Link release past {major}.{minor}, down from {was}: \
                 lower it in versions.toml"
            )),
            Some(_) => {}
        }
    }
    for file in listed.keys().filter(|file| !found.contains_key(*file)) {
        problems.push(format!(
            "{file}: names no Link release past {major}.{minor} any more: take it out of \
             versions.toml"
        ));
    }
    problems
}

/// The check: the tracked files under `root` against `list`, a
/// `versions.toml`, at the line of `manifest`, the workspace's
/// `Cargo.toml`.
pub fn check(root: &Path, manifest: &Path, list: &Path) -> Vec<String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct List {
        #[serde(default)]
        lines: BTreeMap<String, usize>,
    }
    let text = std::fs::read_to_string(list).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let listed: List = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let line = line(manifest);
    compare(&found(root, line), &listed.lines, line)
}
