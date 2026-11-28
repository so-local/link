//! Specs: a thing written down as a TOML file, read strictly, and the
//! same type a command's flags make.
//!
//! A spec's type lives in the module that owns the thing (a sandbox's in
//! the sandbox module, a box's in the box module) and derives serde's
//! `Deserialize` with `deny_unknown_fields`, so a typo is an error. This
//! module holds what every spec shares: reading a file (an empty table
//! refused, the error naming the line), its paths (`~/`, relative to the
//! spec's folder, never `..`), printing one back, and the specs Link has run
//! before.

use std::path::{Path, PathBuf};

use link_format::WithinExt;
use serde::de::DeserializeOwned;

#[cfg(test)]
mod schema_tests;

/// The largest spec Link reads: a spec is a page, not a dump.
pub const MAX_BYTES: u64 = 64 * 1024;

/// A spec read from a file: the value and the folder its relative paths
/// are from.
pub struct Read<T> {
    pub spec: T,
    /// The spec's own folder, its real path.
    pub dir: PathBuf,
    /// The file, its real path.
    pub path: PathBuf,
    /// Its text as read.
    pub text: String,
}

/// Reads the spec at `path` strictly: no unknown key (the type's
/// `deny_unknown_fields`), no empty table, at most [`MAX_BYTES`]. Read
/// where it really is: a link is followed once, here, and its `path` and
/// `dir` are the real file's, so what it names is from the real file's
/// folder, never the link's.
pub fn read<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Read<T>> {
    let link_files::fs::Trusted { path, text } = link_files::fs::read_trusted(
        path,
        link_files::fs::Trust::default(),
        MAX_BYTES,
        "a spec's",
    )?;
    let spec = parse(&text).within(format!("in {}", path.display()))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    Ok(Read {
        spec,
        dir,
        path,
        text,
    })
}

use crate::parse;
#[cfg(test)]
use crate::{path, print};

/// Read a published specification through its owning schema declaration,
/// with what its kind refuses beyond its type ([`crate::Kind::check`]).
pub fn parse_spec<T: crate::Kind>(text: &str) -> anyhow::Result<T> {
    let read = crate::follow::read::<T>(text)?;
    if read.changed {
        note_read(T::format(), read.from);
    }
    read.spec.check()?;
    Ok(read.spec)
}

/// A spec this release wrote itself, such as a module's own defaults
/// built into its program, read as [`parse_spec`] reads one but with
/// its names as written: they're this release's already, so the
/// release's history is never asked for them, and a program that can't
/// reach `lnk` still reads its own defaults.
pub fn parse_current<T: crate::Kind>(text: &str) -> anyhow::Result<T> {
    let read = T::format().read::<T>(text)?;
    read.spec.check()?;
    Ok(read.spec)
}

/// Say that a file in schema `from` was read as `format`'s current one.
pub(crate) fn note_read(format: &crate::schema::Format, from: u32) {
    use link_format::{Note, aside, sentence};
    aside(Note(sentence!(
        "Read {} schema {} as schema {}. lnk spec convert <file> rewrites it.",
        format.kind.clone(),
        u64::from(from),
        u64::from(format.current)
    )));
}

/// The spec file a person gave at `path`, read strictly: what doesn't
/// read is theirs to fix ([`link_format::Refused`]).
pub fn read_spec<T: crate::Kind>(path: &Path) -> anyhow::Result<Read<T>> {
    let read = read::<toml::Table>(path).map_err(link_format::refused)?;
    let spec = parse_spec(&read.text)
        .within(format!("in {}", read.path.display()))
        .map_err(link_format::refused)?;
    Ok(Read {
        spec,
        dir: read.dir,
        path: read.path,
        text: read.text,
    })
}

/// What converting a spec file did.
#[derive(Debug, PartialEq, Eq)]
pub struct Converted {
    /// The schema it was in.
    pub from: u32,
    /// Whether it was rewritten: not when it was current already.
    pub changed: bool,
}

/// Rewrite the spec at `path` in its kind's current schema, by the kind's
/// steps alone, as its owner exports them: no code of the owner's runs.
/// The result names its kind, keeps the file's comments, key order and
/// permissions, and is checked against `shape` before it's written; a
/// file changed while it was read is refused.
pub fn convert(
    format: &crate::schema::Format,
    shape: &serde_json::Value,
    path: &Path,
) -> anyhow::Result<Converted> {
    let original = read::<toml::Table>(path)?;
    let (document, from) = format
        .document(&original.text)
        .within(format!("in {}", original.path.display()))?;
    let document = named(document, &format.kind);
    let mut text = document.to_string();
    if original.text.contains("\r\n") {
        text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    let table: toml::Table = toml::from_str(&text)?;
    let problems = crate::fields::check_value(shape, &table);
    anyhow::ensure!(
        problems.is_empty(),
        "{} in schema {} isn't a {} spec: {}",
        original.path.display(),
        format.current,
        format.kind,
        problems.join("; ")
    );
    if text == original.text {
        return Ok(Converted {
            from,
            changed: false,
        });
    }
    let check = read::<toml::Table>(path)?;
    anyhow::ensure!(
        check.text == original.text && check.path == original.path,
        "the spec changed while it was read; convert it again"
    );
    let permissions = std::fs::metadata(&original.path)?.permissions();
    link_files::write_private(&original.path, text.as_bytes())?;
    std::fs::set_permissions(&original.path, permissions)?;
    Ok(Converted {
        from,
        changed: true,
    })
}

/// `document` with `kind` and `schema` its first keys, taking the leading
/// comments of what came first, so a file's opening comment stays at its
/// top.
fn named(document: toml_edit::DocumentMut, kind: &str) -> toml_edit::DocumentMut {
    // The file's opening comment, on whatever came first.
    let mut items: Vec<(toml_edit::Key, toml_edit::Item)> = document
        .as_table()
        .iter()
        .map(|(name, item)| {
            let key = document
                .as_table()
                .key(name)
                .cloned()
                .unwrap_or_else(|| toml_edit::Key::new(name));
            (key, item.clone())
        })
        .collect();
    let opening = items.first_mut().and_then(|(key, item)| {
        let decor = match item {
            toml_edit::Item::Table(table) => table.decor_mut(),
            _ => key.leaf_decor_mut(),
        };
        let prefix = decor.prefix().cloned();
        decor.set_prefix("");
        prefix
    });
    let take = |items: &mut Vec<(toml_edit::Key, toml_edit::Item)>, name: &str| {
        let at = items.iter().position(|(key, _)| key.get() == name)?;
        Some(items.remove(at))
    };
    let mut named = toml_edit::DocumentMut::new();
    let (kind_key, kind_item) = take(&mut items, "kind")
        .unwrap_or_else(|| (toml_edit::Key::new("kind"), toml_edit::value(kind)));
    named.insert_formatted(&kind_key, kind_item);
    if let Some((key, item)) = take(&mut items, "schema") {
        named.insert_formatted(&key, item);
    }
    for (key, item) in items {
        named.insert_formatted(&key, item);
    }
    if let (Some(prefix), Some(mut kind)) = (opening, named.key_mut("kind")) {
        kind.leaf_decor_mut().set_prefix(prefix);
    }
    named.set_trailing(document.trailing().as_str().unwrap_or_default());
    named
}

/// The specs this user has run, by path, with a hash of what they
/// opened, in Link's settings: a spec not run before, or one that opens
/// something else since, is shown before it runs. What a spec opens is
/// its module's to write out (every path resolved, every program found),
/// so a link moved under a path it names counts as a change.
pub struct Seen {
    file: PathBuf,
    hashes: Hashes,
}

/// The hash of what each spec opened when it last ran, by its path, as
/// `seen.toml` keeps them, naming its schema.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Hashes(std::collections::BTreeMap<String, String>);

link_files::specification!(Hashes, include_str!("io/seen.schema.toml"));

/// The specifications seen, which an upgrade converts.
pub fn saved_seen() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Hashes>()
}

impl Seen {
    /// The specs its owner `kind` (`sandboxes`, `boxes`) has seen before.
    pub fn load(kind: &str) -> anyhow::Result<Self> {
        Self::load_at(link_files::layout::seen_specs(kind)?)
    }

    /// [`Seen::load`] of the file at `file`.
    fn load_at(file: PathBuf) -> anyhow::Result<Self> {
        let hashes = match std::fs::read_to_string(&file) {
            // Refused by name, never read as empty: the next save would
            // forget every spec it knew.
            Ok(text) => {
                link_files::saved::parse(&text).within(format!("reading {}", file.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Hashes::default(),
            Err(e) => return Err(e).within(format!("reading {}", file.display())),
        };
        Ok(Self { file, hashes })
    }

    /// Whether the spec at `path` was run before opening `opens`.
    pub fn knows(&self, path: &Path, opens: &str) -> bool {
        self.hashes.0.get(&key(path)) == Some(&hash(opens))
    }

    /// Remembers that the spec at `path` ran opening `opens`.
    pub fn remember(&mut self, path: &Path, opens: &str) -> anyhow::Result<()> {
        self.hashes.0.insert(key(path), hash(opens));
        if let Some(dir) = self.file.parent() {
            link_files::create_private_dir(dir)?;
        }
        link_files::saved::write(&self.file, &self.hashes)
            .within(format!("writing {}", self.file.display()))
    }
}

fn key(path: &Path) -> String {
    path.display().to_string()
}

fn hash(text: &str) -> String {
    link_digest::sha256(text.as_bytes())
}

/// A name in a spec: lowercase letters, digits and dashes, starting
/// with a letter, at most 30.
pub fn check_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 30
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && !name.ends_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "{:?} can't be a name: lowercase letters, digits and dashes, starting with a letter, at \
         most 30",
        link_format::clean(name)
    );
    Ok(())
}

/// Variables Link sets, which a spec can't.
const OWN: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "LANG", "TZ",
];

/// A variable's name: capital letters, digits and `_`, not starting with
/// a digit, not Link's (`LNK_...`) nor one it sets.
pub fn check_variable(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 64
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
        "{:?} can't be a variable's name: capital letters, digits and _",
        link_format::clean(name)
    );
    anyhow::ensure!(
        !name.starts_with("LNK_") && !OWN.contains(&name),
        "{name} is a variable Link sets"
    );
    Ok(())
}

/// Words a field is never named by, unless serde skips it: a secret a
/// command takes as a flag is never part of a file, nor printed, and
/// never a value on its command line.
pub const SECRET_WORDS: &[&str] = &[
    "token",
    "key",
    "secret",
    "password",
    "pass",
    "auth",
    "apikey",
    "header",
    "cookie",
    "credential",
    "credentials",
    "jitconfig",
];

/// Whether a field's name is a secret's: one of its words is one of
/// [`SECRET_WORDS`] (`token`, `key_id`, `auth`).
pub fn named_like_a_secret(name: &str) -> bool {
    name.split(['_', '-'])
        .any(|word| SECRET_WORDS.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing list of the specs seen is empty; one that can't be read
    /// is refused by name, never read as empty.
    #[test]
    fn an_unreadable_seen_list_is_refused_by_name() {
        let dir = link_test_kit::TempDir::new("seen-specs");
        let file = dir.join("seen.toml");
        assert!(Seen::load_at(file.clone()).unwrap().hashes.0.is_empty());
        std::fs::write(&file, "schema = 1\n\"/a\" = \n").unwrap();
        let refused = Seen::load_at(file).err().unwrap();
        assert!(format!("{refused:#}").contains("seen.toml"), "{refused:#}");
    }

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Spec {
        name: Option<String>,
        files: Option<Files>,
    }

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Files {
        #[serde(default)]
        write: Vec<String>,
    }

    #[test]
    fn a_spec_is_read_strictly() {
        let spec: Spec = parse("name = \"a\"\n[files]\nwrite = [\".\"]\n").unwrap();
        assert_eq!(spec.files.unwrap().write, ["."]);
        let typo = parse::<Spec>("[files]\nwirte = [\".\"]\n").unwrap_err();
        assert!(typo.to_string().contains("wirte"), "{typo}");
        assert!(typo.to_string().contains("line 2"), "{typo}");
        let empty = parse::<Spec>("name = \"a\"\n\n[files]\n").unwrap_err();
        assert!(
            empty.to_string().contains("line 3: [files] is empty"),
            "{empty}"
        );
        let inline = parse::<Spec>("files = {}\n").unwrap_err();
        assert!(inline.to_string().contains("line 1"), "{inline}");
        // An empty file is the least spec.
        assert_eq!(
            parse::<Spec>("").unwrap(),
            Spec {
                name: None,
                files: None
            }
        );
    }

    #[test]
    fn paths_are_from_the_specs_folder_and_never_climb() {
        let (dir, home) = (Path::new("/specs"), Path::new("/home/me"));
        let at = |p: &str| path(Path::new(p), dir, home).unwrap();
        assert_eq!(at("."), Path::new("/specs"));
        assert_eq!(at("./app"), Path::new("/specs/app"));
        assert_eq!(at("app/"), Path::new("/specs/app"));
        assert_eq!(at("~"), Path::new("/home/me"));
        assert_eq!(at("~/code"), Path::new("/home/me/code"));
        assert_eq!(at("/opt/x"), Path::new("/opt/x"));
        for climbing in ["..", "../x", "./a/../../b", "~/../other", "/opt/../etc"] {
            assert!(path(Path::new(climbing), dir, home).is_err(), "{climbing}");
        }
    }

    #[test]
    fn a_spec_too_large_is_refused() {
        let dir = link_files::private_temp_dir("lnk-spec-test").unwrap();
        let big = dir.join("big.toml");
        std::fs::write(&big, format!("name = \"{}\"", "a".repeat(70_000))).unwrap();
        assert!(read::<Spec>(&big).is_err());
        let small = dir.join("small.toml");
        std::fs::write(&small, "name = \"a\"\n").unwrap();
        let read = read::<Spec>(&small).unwrap();
        assert_eq!(read.dir, dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn printing_goes_through_the_serializer() {
        let spec = Spec {
            name: Some("a\"\n[evil]".into()),
            files: None,
        };
        let text = print(&spec).unwrap();
        assert_eq!(parse::<Spec>(&text).unwrap(), spec);
    }
}
