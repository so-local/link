//! Versioned TOML documents, upgraded before their current type is read:
//! specs, and every file Link saves. Each names its schema, and its owner
//! declares the steps from each older one in its `schema.toml`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use toml_edit::{DocumentMut, Item, Key, Table, TableLike};

/// The schema a format reads from unless it says otherwise.
fn first() -> u32 {
    1
}

/// A spec kind owns one declaration beside its current type.
pub trait Specification: DeserializeOwned + Serialize {
    fn format() -> &'static Format;
}

/// Implement a kind using its one built-in declaration. A spec kind, as
/// `link_spec::specification!` registers one, is `named`: its files name
/// their kind beside their schema. A kind whose steps name code says
/// `migrations`, and its `migrations/` folder registers that code
/// ([`Migrations`]). A saved file holding names that follow history says
/// `references = <its sites>` ([`references`]).
#[macro_export]
macro_rules! specification {
    (@format $ty:ty, $source:expr, $named:expr, $code:expr, $sites:expr) => {
        impl $crate::schema::Specification for $ty {
            fn format() -> &'static $crate::schema::Format {
                static FORMAT: std::sync::LazyLock<$crate::schema::Format> =
                    std::sync::LazyLock::new(|| {
                        $crate::schema::Format::parse_with($source, $code)
                            .map(|format| if $named { format.named() } else { format })
                            .map(|format| format.following($sites))
                            .expect("the built-in spec schema is valid")
                    });
                &FORMAT
            }
        }
    };
    (@format $ty:ty, $source:expr, $named:expr, $code:expr) => {
        $crate::specification!(@format $ty, $source, $named, $code, Vec::new());
    };
    ($ty:ty, $source:expr, references = $sites:expr) => {
        $crate::specification!(@format $ty, $source, false, &[], $sites);
    };
    ($ty:ty, $source:expr, named, migrations) => {
        $crate::specification!(@format $ty, $source, true, <$ty as $crate::schema::Migrations>::ALL);
    };
    ($ty:ty, $source:expr, named) => {
        $crate::specification!(@format $ty, $source, true, &[]);
    };
    ($ty:ty, $source:expr, migrations) => {
        $crate::specification!(@format $ty, $source, false, <$ty as $crate::schema::Migrations>::ALL);
    };
    ($ty:ty, $source:expr) => {
        $crate::specification!(@format $ty, $source, false, &[]);
    };
}

/// The version of the declaration format itself, the `schema` line every
/// `schema.toml` opens with.
pub const DECLARATION: u32 = 1;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    /// The declaration format's version ([`DECLARATION`]).
    pub schema: u32,
    pub kind: String,
    pub current: u32,
    /// The oldest schema it still reads, its steps from there to
    /// `current`: an older file is refused as unknown, once the steps
    /// that read it are deleted at their `until`.
    #[serde(default = "first")]
    pub oldest: u32,
    /// The command printing a file of the kind in its current schema
    /// (`lnk box spec show`): none where no module that comes with the
    /// kind answers one.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Shapes that predate Link 0.1, each refused by name with what
    /// replaced it ([`Before`]).
    #[serde(default)]
    pub before: Before,
    /// What the kind configures, the opening of its reference page.
    #[serde(default)]
    pub about: Option<String>,
    /// Whether its files hold state, which an upgrade converts, or
    /// history, which is never rewritten ([`Holds`]).
    #[serde(default)]
    pub holds: Holds,
    /// Where its files are on a machine, for an upgrade to find them
    /// ([`Place`]).
    #[serde(default)]
    pub files: Vec<Place>,
    /// The records it takes from another owner's file in an upgrade
    /// ([`Transfer`]).
    #[serde(default)]
    pub transfers: Vec<Transfer>,
    /// Whether its files name their kind, as a spec's do: a top-level
    /// `kind` beside `schema`, written when printed, refused when it names
    /// another kind, and taken as this one when left out.
    #[serde(skip)]
    pub named: bool,
    /// The code its steps name, which its owner registers: none in a
    /// format another program exported.
    #[serde(skip)]
    pub migrations: &'static [Migration],
    /// Where its files hold names that follow history, followed by the
    /// program's follower when read ([`references::follower`]).
    #[serde(skip)]
    pub references: Vec<references::Site>,
}

/// One transition. Paths are table keys joined by dots, never array
/// indices, a key a bare TOML key can't spell quoted
/// (`locations."lnk.migration-journal"`, [`path`]).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub from: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rename: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remove: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub refuse: BTreeMap<String, String>,
    /// The owner's code that takes the step instead, named for its kind
    /// and two schemas ([`Migration`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<String>,
    /// For a step that runs code, the field flags of the schema before it
    /// that the code retires, each with what to write instead: they answer
    /// an old flag as `refuse` does, and are never applied to a file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flags: BTreeMap<String, String>,
    /// The release that brought the step.
    pub since: link_version::Version,
    /// The release that deletes it, after which a file in its schema is
    /// refused as unknown: never, in a format of history, whose lines
    /// are read as written however old ([`Holds::History`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<link_version::Version>,
}

/// A field a step retired: what a flag naming its old path is told
/// instead of an unknown argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retired {
    /// The field it set, in the schema before the step.
    pub path: String,
    /// The schema that retired it.
    pub since: u32,
    pub fate: Fate,
}

/// What became of a retired flag's field, by the current schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    /// It's the field at this path now, whose flag replaces it.
    Renamed(String),
    /// It's gone, for this reason.
    Removed(String),
    /// It's refused, with this instruction.
    Refused(String),
}

/// A file holding a path a schema step refuses, with what to do instead:
/// its owner's to say, never read past.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub kind: String,
    /// The schema whose step refuses it.
    pub schema: u32,
    pub path: String,
    pub why: String,
}

impl Refused {
    fn new(kind: &str, schema: u32, path: &str, why: &str) -> Refused {
        Refused {
            kind: kind.to_string(),
            schema,
            path: path.to_string(),
            why: why.to_string(),
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} schema {}: {}: {}",
            self.kind, self.schema, self.path, self.why
        )
    }
}

impl std::error::Error for Refused {}

#[derive(Debug)]
pub struct Read<T> {
    pub spec: T,
    pub text: String,
    pub from: u32,
    pub changed: bool,
}

impl Format {
    /// The format declared in `text`, which comes with its owner: a panic
    /// unless it's one, as a built-in declaration always is (its owner's
    /// tests read it). For a saved value whose type its owner can't
    /// declare with [`crate::specification!`], another crate's, held in a
    /// `LazyLock` beside the code that reads it.
    pub fn shipped(text: &str) -> Format {
        Format::parse(text).unwrap_or_else(|e| panic!("a built-in saved-file schema: {e:#}"))
    }

    /// Describe the optional metadata alongside the owning current type.
    pub fn json_schema(&self, mut schema: serde_json::Value) -> serde_json::Value {
        schema["properties"]["schema"] = serde_json::json!({
            "type": "integer", "const": self.current, "default": self.current,
            "description": "Format number. Omission means current."
        });
        if self.named {
            schema["properties"]["kind"] = serde_json::json!({
                "type": "string", "const": self.kind,
                "description": "The spec kind. Omission means this one."
            });
        }
        schema
    }
    /// It as a spec kind's, whose files name their kind ([`Format::named`]).
    pub fn named(mut self) -> Self {
        self.named = true;
        self
    }

    /// It with names at `sites` that follow history.
    pub fn following(mut self, sites: Vec<references::Site>) -> Self {
        self.references = sites;
        self
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Self::parse_with(text, &[])
    }

    /// The format declared in `text`, with the code its steps name.
    pub fn parse_with(text: &str, migrations: &'static [Migration]) -> anyhow::Result<Self> {
        let mut format: Self = toml::from_str(text)?;
        format.migrations = migrations;
        ensure!(
            format.schema == DECLARATION,
            "a schema declaration in format {} needs this lnk to read format {DECLARATION}",
            format.schema
        );
        ensure!(
            format.current > 0
                && !format.kind.trim().is_empty()
                && format
                    .command
                    .as_deref()
                    .is_none_or(|command| command.starts_with("lnk ")),
            "a spec needs its kind, a positive schema, and a command of lnk's if it names one"
        );
        ensure!(
            format.oldest > 0 && format.oldest <= format.current,
            "{} reads from schema {}, which isn't a schema up to {}",
            format.kind,
            format.oldest,
            format.current
        );
        let mut versions = BTreeSet::new();
        for step in &format.steps {
            ensure!(
                step.from >= format.oldest
                    && step.from < format.current
                    && versions.insert(step.from),
                "invalid or duplicate schema step {}",
                step.from
            );
            for path in step
                .rename
                .keys()
                .chain(step.rename.values())
                .chain(step.remove.keys())
                .chain(step.refuse.keys())
                .chain(step.flags.keys())
            {
                ensure!(
                    path != "schema" && path != "kind" && path::check(path),
                    "invalid schema step path {path:?}"
                );
            }
            for why in step
                .remove
                .values()
                .chain(step.refuse.values())
                .chain(step.flags.values())
            {
                ensure!(
                    !why.trim().is_empty(),
                    "a removal or refusal needs its explanation"
                );
            }
            let mut touched = BTreeSet::new();
            for path in step
                .rename
                .keys()
                .chain(step.remove.keys())
                .chain(step.refuse.keys())
            {
                ensure!(touched.insert(path), "schema step edits {path} twice");
            }
            // One field, one fate in a step: no path inside another it edits.
            for a in &touched {
                for b in &touched {
                    ensure!(
                        a == b || !b.starts_with(&format!("{a}.")),
                        "schema step {} edits {b} inside {a}: give each its own step",
                        step.from
                    );
                }
            }
            if let Some(until) = &step.until {
                ensure!(
                    step.since < *until,
                    "schema step {} is deleted at {until}, before or as it came at {}",
                    step.from,
                    step.since
                );
            }
        }
        for n in format.oldest..format.current {
            ensure!(
                versions.contains(&n),
                "missing {} schema step {n}",
                format.kind
            );
        }
        format.before.check(&format.kind, format.current)?;
        upgrade::check(&format)?;
        migration::check(&format, migrations)?;
        Ok(format)
    }

    /// Every field its steps retired, each answered by what it is in the
    /// current schema: a rename followed through every later step, so the
    /// oldest path names today's.
    pub fn retired(&self) -> Vec<Retired> {
        let mut retired = Vec::new();
        for step in &self.steps {
            let fates = step
                .rename
                .iter()
                .map(|(old, new)| (old, self.fate(step.from + 1, new)))
                .chain(
                    step.remove
                        .iter()
                        .map(|(old, why)| (old, Fate::Removed(why.clone()))),
                )
                .chain(
                    step.refuse
                        .iter()
                        .chain(&step.flags)
                        .map(|(old, why)| (old, Fate::Refused(why.clone()))),
                );
            for (path, fate) in fates {
                retired.push(Retired {
                    path: path.clone(),
                    since: step.from + 1,
                    fate,
                });
            }
        }
        retired
    }

    /// What the field at `path` in schema `at` is in the current one.
    pub fn fate(&self, at: u32, path: &str) -> Fate {
        let mut path = path.to_string();
        for step in self.steps.iter().filter(|s| s.from >= at) {
            let under = |old: &str| path == old || path.starts_with(&format!("{old}."));
            if let Some((old, new)) = step.rename.iter().find(|(old, _)| under(old)) {
                path = format!("{new}{}", &path[old.len()..]);
            } else if let Some((_, why)) = step.remove.iter().find(|(old, _)| under(old)) {
                return Fate::Removed(why.clone());
            } else if let Some((_, why)) = step
                .refuse
                .iter()
                .chain(&step.flags)
                .find(|(old, _)| under(old))
            {
                return Fate::Refused(why.clone());
            }
        }
        Fate::Renamed(path)
    }

    /// Apply only declared edits, preserving document decoration and order.
    pub fn document(&self, text: &str) -> anyhow::Result<(DocumentMut, u32)> {
        let mut doc: DocumentMut = text.parse()?;
        if self.named {
            match doc.get("kind").map(|k| k.as_str()) {
                None => {}
                Some(Some(kind)) if kind == self.kind => {}
                Some(Some(kind)) => bail!("this is a {kind:?} spec, not a {} spec", self.kind),
                Some(None) => bail!("kind names a spec kind, as kind = \"{}\"", self.kind),
            }
        }
        let from = match doc.get("schema") {
            None if self.named => bail!(
                "{} specs name their schema: add schema = {} for this release's, or the \
                 schema it was written in",
                self.kind,
                self.current
            ),
            None => self.current,
            Some(item) => item
                .as_integer()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .context("schema must be a positive integer")?,
        };
        self.check_readable(from)?;
        for version in from..self.current {
            let step = self.step_from(version)?;
            if let Some(named) = &step.migration {
                self.run_document(named, version, &mut doc)?;
                continue;
            }
            for (key, why) in &step.refuse {
                if lookup(doc.as_table(), key).is_some() {
                    return Err(Refused::new(&self.kind, version, key, why).into());
                }
            }
            for (old, new) in &step.rename {
                if lookup(doc.as_table(), old).is_none() {
                    continue;
                }
                ensure!(
                    lookup(doc.as_table(), new).is_none(),
                    "{} schema {version}: both {old} and {new} are present; resolve the ambiguity",
                    self.kind
                );
                // A key renamed within its table keeps its place there.
                fn parent(p: &str) -> Option<&str> {
                    path::split_last(p).and_then(|(head, _)| head)
                }
                let place = (parent(old) == parent(new))
                    .then(|| {
                        let table = match parent(old) {
                            Some(head) => table_at(doc.as_table(), head)?,
                            None => doc.as_table(),
                        };
                        let (_, leaf) = path::split_last(old)?;
                        table.iter().position(|(key, _)| key == leaf)
                    })
                    .flatten();
                let item = take(doc.as_table_mut(), old).context("reading the renamed key")?;
                put(doc.as_table_mut(), new, item)?;
                if let Some(at) = place
                    && let Some((_, leaf)) = path::split_last(new)
                {
                    match parent(new) {
                        Some(head) => {
                            if let Some(table) = table_at_mut(doc.as_table_mut(), head) {
                                reorder(table, leaf, at);
                            }
                        }
                        None => reorder(doc.as_table_mut(), leaf, at),
                    }
                }
            }
            for key in step.remove.keys() {
                take(doc.as_table_mut(), key);
            }
        }
        if !self.before.keys.is_empty() {
            let table: toml::Table = toml::from_str(&doc.to_string())?;
            self.before
                .keys(&self.kind, &serde_json::to_value(&table)?)?;
        }
        let decor = doc
            .get("schema")
            .and_then(Item::as_value)
            .map(|v| v.decor().clone());
        doc["schema"] = toml_edit::value(i64::from(self.current));
        if let Some(decor) = decor {
            *doc["schema"]
                .as_value_mut()
                .expect("schema is an integer")
                .decor_mut() = decor;
        }
        Ok((doc, from))
    }

    pub fn read<T: DeserializeOwned>(&self, text: &str) -> anyhow::Result<Read<T>> {
        self.read_by(text, parse::<T>, None)
    }

    /// [`Format::read`], with the references at `sites` followed `by`
    /// history once the steps are taken: an old name read as the current
    /// one, a removed one refused ([`references`]).
    pub fn read_following<T: DeserializeOwned>(
        &self,
        text: &str,
        sites: &[references::Site],
        by: &dyn references::Follow,
    ) -> anyhow::Result<Read<T>> {
        self.read_by(text, parse::<T>, Some((sites, by)))
    }

    /// A file Link saved, read as [`Format::read`] reads a spec but for
    /// the spec rules a person's file keeps: an empty table is as good as
    /// a missing one.
    pub fn read_saved<T: DeserializeOwned>(&self, text: &str) -> anyhow::Result<Read<T>> {
        self.read_by(
            text,
            |text| toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}")),
            None,
        )
    }

    fn read_by<T>(
        &self,
        text: &str,
        parse: impl FnOnce(&str) -> anyhow::Result<T>,
        following: Option<(&[references::Site], &dyn references::Follow)>,
    ) -> anyhow::Result<Read<T>> {
        let (doc, from) = self.document(text)?;
        let current = doc.to_string();
        let keys: &[&str] = if self.named {
            &["schema", "kind"]
        } else {
            &["schema"]
        };
        // Its references followed, where it holds any: a name changed,
        // the document is read as followed.
        let own = (!self.references.is_empty())
            .then(|| (self.references.as_slice(), references::follower()));
        let followed = match following.or(own).filter(|(sites, _)| !sites.is_empty()) {
            Some((sites, by)) => {
                let mut table: toml::Table = toml::from_str(&current)?;
                for key in keys {
                    table.remove(*key);
                }
                references::follow(&mut table, sites, by)?
                    .then(|| toml::to_string(&table))
                    .transpose()?
            }
            None => None,
        };
        // Unchanged, the person's own text is read with its schema and
        // kind blanked where they stand, so an error names the line of
        // their file; a migrated document has no such lines to keep.
        let body = followed.unwrap_or_else(|| {
            (from == self.current)
                .then(|| blanked(text, keys))
                .flatten()
                .unwrap_or_else(|| {
                    let mut body = doc;
                    for key in keys {
                        body.remove(key);
                    }
                    body.to_string()
                })
        });
        let spec = parse(&body).map_err(|error| {
            if !text.lines().any(|l| l.trim_start().starts_with("schema")) {
                let table: toml::Table = toml::from_str(text).unwrap_or_default();
                for step in &self.steps {
                    for key in step.rename.keys().chain(step.remove.keys()) {
                        if value_at(&table, key).is_some() {
                            return anyhow::anyhow!("{error}; {key} changed in {} schema {}; add schema = {} to read that format", self.kind, step.from + 1, step.from);
                        }
                    }
                }
            }
            error
        })?;
        Ok(Read {
            spec,
            text: current,
            from,
            changed: from != self.current,
        })
    }

    /// A saved JSON document, its steps applied to its object as to a TOML
    /// document's tables: `schema` names its version, missing as current
    /// but in a spec, which is refused without one.
    pub fn read_json<T: DeserializeOwned>(&self, bytes: &[u8]) -> anyhow::Result<Read<T>> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
        let object = value
            .as_object_mut()
            .with_context(|| format!("{} isn't a JSON object", self.kind))?;
        if self.named {
            match object.remove("kind") {
                None => {}
                Some(serde_json::Value::String(kind)) if kind == self.kind => {}
                Some(serde_json::Value::String(kind)) => {
                    bail!("this is a {kind:?} spec, not a {} spec", self.kind)
                }
                Some(_) => bail!("kind names a spec kind, as \"kind\": \"{}\"", self.kind),
            }
        }
        let from = match object.remove("schema") {
            None if self.named => bail!(
                "{} specs name their schema: add \"schema\": {} for this release's, or the \
                 schema it was written in",
                self.kind,
                self.current
            ),
            None => self.current,
            Some(schema) => schema
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .context("schema must be a positive integer")?,
        };
        self.check_readable(from)?;
        for version in from..self.current {
            let step = self.step_from(version)?;
            if let Some(named) = &step.migration {
                self.run_json(named, version, object)?;
                continue;
            }
            for (key, why) in &step.refuse {
                if json::lookup(object, key).is_some() {
                    return Err(Refused::new(&self.kind, version, key, why).into());
                }
            }
            for (old, new) in &step.rename {
                let Some(item) = json::take(object, old) else {
                    continue;
                };
                ensure!(
                    json::lookup(object, new).is_none(),
                    "{} schema {version}: both {old} and {new} are present; resolve the ambiguity",
                    self.kind
                );
                json::put(object, new, item)?;
            }
            for key in step.remove.keys() {
                json::take(object, key);
            }
        }
        self.before.keys(&self.kind, &value)?;
        let spec = serde_json::from_value(value.clone())?;
        let mut text = value;
        let object = text.as_object_mut().expect("an object");
        if self.named {
            object.insert("kind".into(), self.kind.clone().into());
        }
        object.insert("schema".into(), self.current.into());
        Ok(Read {
            spec,
            text: text.to_string(),
            from,
            changed: from != self.current,
        })
    }

    /// `spec` as JSON naming the current schema.
    /// `spec` as JSON on one line, its fields in its own order and
    /// `schema` last, so a reader that finds a line by its first field
    /// still does.
    pub fn print_json<T: Serialize>(&self, spec: &T) -> anyhow::Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(spec)?;
        ensure!(
            bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}'),
            "{} isn't a JSON object",
            self.kind
        );
        bytes.pop();
        if bytes.len() > 1 {
            bytes.push(b',');
        }
        if self.named {
            bytes.extend(
                format!("\"kind\":{},", serde_json::Value::from(self.kind.clone())).into_bytes(),
            );
        }
        bytes.extend(format!("\"schema\":{}}}", self.current).into_bytes());
        Ok(bytes)
    }

    fn check_readable(&self, from: u32) -> anyhow::Result<()> {
        self.before.schema(&self.kind, from)?;
        ensure!(
            from >= self.oldest,
            "{} schema {from} is no longer read: this lnk reads schema {} to {}; write the file \
             in the current schema",
            self.kind,
            self.oldest,
            self.current
        );
        ensure!(
            from <= self.current,
            "{} schema {from} needs a newer lnk; run lnk upgrade (this lnk reads schema {})",
            self.kind,
            self.current
        );
        Ok(())
    }

    fn step_from(&self, version: u32) -> anyhow::Result<&Step> {
        self.steps
            .iter()
            .find(|s| s.from == version)
            .context("missing schema step")
    }

    pub fn print<T: Serialize>(&self, spec: &T) -> anyhow::Result<String> {
        let text = print(spec)?;
        let mut header = DocumentMut::new();
        if self.named {
            header["kind"] = toml_edit::value(self.kind.clone());
        }
        header["schema"] = toml_edit::value(i64::from(self.current));
        Ok(format!("{header}{text}"))
    }
}

pub fn parse_spec<T: Specification>(text: &str) -> anyhow::Result<T> {
    Ok(T::format().read(text)?.spec)
}

pub fn print_spec<T: Specification>(spec: &T) -> anyhow::Result<String> {
    T::format().print(spec)
}

/// `text` with each top-level key of `keys` and its value written over
/// with spaces, every line where it was: none when a key's place isn't
/// known.
fn blanked(text: &str, keys: &[&str]) -> Option<String> {
    let doc = toml_edit::Document::parse(text).ok()?;
    let mut bytes = text.as_bytes().to_vec();
    for key in keys {
        let Some((name, item)) = doc.as_table().get_key_value(key) else {
            continue;
        };
        let at = name.span()?.start..item.span()?.end;
        for byte in bytes.get_mut(at)? {
            if !matches!(byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn lookup<'a>(table: &'a dyn TableLike, path: &str) -> Option<&'a Item> {
    let (head, rest) = path::split_first(path)?;
    let item = table.get(head)?;
    match rest {
        None => Some(item),
        Some(rest) => lookup(item.as_table_like()?, rest),
    }
}

/// The table at `path` under `table`.
fn table_at<'a>(table: &'a dyn TableLike, path: &str) -> Option<&'a dyn TableLike> {
    path::keys(path)?
        .into_iter()
        .try_fold(table, |table, key| table.get(key)?.as_table_like())
}

fn table_at_mut<'a>(table: &'a mut dyn TableLike, path: &str) -> Option<&'a mut dyn TableLike> {
    path::keys(path)?
        .into_iter()
        .try_fold(table, |table, key| table.get_mut(key)?.as_table_like_mut())
}

/// `table` with its key `leaf` moved to place `at`, the rest in order.
fn reorder(table: &mut dyn TableLike, leaf: &str, at: usize) {
    let order: Vec<String> = table.iter().map(|(key, _)| key.to_string()).collect();
    let mut wanted: Vec<String> = order.iter().filter(|k| *k != leaf).cloned().collect();
    wanted.insert(at.min(wanted.len()), leaf.to_string());
    if wanted == order {
        return;
    }
    let mut entries: Vec<(String, Key, Item)> = order
        .iter()
        .filter_map(|name| {
            let key = table.key(name)?.clone();
            let item = table.remove(name)?;
            Some((name.clone(), key, item))
        })
        .collect();
    for name in &wanted {
        if let Some(i) = entries.iter().position(|(n, _, _)| n == name) {
            let (_, key, item) = entries.remove(i);
            table.entry_format(&key).or_insert(item);
        }
    }
}

pub(crate) fn take(table: &mut dyn TableLike, path: &str) -> Option<(Key, Item)> {
    match path::split_first(path)? {
        (leaf, None) => {
            let key = table.key(leaf)?.clone();
            Some((key, table.remove(leaf)?))
        }
        (head, Some(rest)) => {
            let inner = table.get_mut(head)?.as_table_like_mut()?;
            let mut item = take(inner, rest);
            if inner.is_empty() {
                let key = table.key(head)?.clone();
                let removed = table.remove(head)?;
                if let Some(moved) = &mut item {
                    carry_comments(&key, &removed, moved);
                }
            }
            item
        }
    }
}

fn carry_comments(key: &Key, item: &Item, moved: &mut (Key, Item)) {
    let decor = item
        .as_table()
        .map(Table::decor)
        .or_else(|| item.as_value().map(|value| value.decor()));
    let comments: String = std::iter::once(key.leaf_decor())
        .chain(decor)
        .flat_map(|decor| [decor.prefix(), decor.suffix()])
        .flatten()
        .filter_map(|raw| raw.as_str())
        .filter(|text| text.contains('#'))
        .map(|text| format!("{}\n", text.trim_end()))
        .collect();
    if !comments.is_empty() {
        let target = match &mut moved.1 {
            Item::Table(table) => table.decor_mut(),
            _ => moved.0.leaf_decor_mut(),
        };
        let prefix = target
            .prefix()
            .and_then(|raw| raw.as_str())
            .unwrap_or_default();
        target.set_prefix(format!("{comments}{prefix}"));
    }
}

pub(crate) fn put(
    table: &mut dyn TableLike,
    path: &str,
    (old, item): (Key, Item),
) -> anyhow::Result<()> {
    match path::split_first(path).with_context(|| format!("{path} isn't a path"))? {
        (leaf, None) => {
            table.insert(leaf, item);
            if let Some(mut key) = table.key_mut(leaf) {
                *key.leaf_decor_mut() = old.leaf_decor().clone();
            }
        }
        (head, Some(rest)) => {
            if !table.contains_key(head) {
                let mut inner = Table::new();
                inner.set_implicit(true);
                table.insert(head, Item::Table(inner));
            }
            let Some(inner) = table.get_mut(head).and_then(Item::as_table_like_mut) else {
                bail!("{head} is not a table");
            };
            put(inner, rest, (old, item))?;
        }
    }
    Ok(())
}

fn value_at<'a>(table: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let (key, rest) = path::split_first(path)?;
    let value = table.get(key)?;
    match rest {
        None => Some(value),
        Some(rest) => value_at(value.as_table()?, rest),
    }
}

mod before;
mod coverage;
mod json;
mod migration;
pub mod path;
pub mod references;
mod upgrade;

pub use before::Before;
pub use coverage::field_paths;
pub use migration::{Body, Migration, Migrations};
pub(crate) use migration::{item, plain};
pub use upgrade::{Encoding, Holds, Place, Transfer, name as transfer_name};

#[cfg(test)]
mod tests;

/// Parses a spec strictly, refusing unknown fields and empty tables.
pub fn parse<T: DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    let table: toml::Table = toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(key) = empty_table(&table, "") {
        match line_of(text, &key) {
            Some(line) => bail!("line {line}: [{key}] is empty: give it its key, or leave it out"),
            None => bail!("[{key}] is empty: give it its key, or leave it out"),
        }
    }
    toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))
}

/// The first empty table in `table`, by its dotted name.
fn empty_table(table: &toml::Table, prefix: &str) -> Option<String> {
    for (key, value) in table {
        let name = match prefix {
            "" => key.clone(),
            _ => format!("{prefix}.{key}"),
        };
        if let toml::Value::Table(inner) = value {
            if inner.is_empty() {
                return Some(name);
            }
            if let Some(found) = empty_table(inner, &name) {
                return Some(found);
            }
        }
    }
    None
}

/// The line `[key]` or `key = {}` is on, counting from 1.
fn line_of(text: &str, key: &str) -> Option<usize> {
    let last = key.rsplit('.').next().unwrap_or(key);
    text.lines()
        .position(|line| {
            let line = line.trim();
            line.strip_prefix('[')
                .and_then(|l| l.split(']').next())
                .is_some_and(|l| l.trim() == key)
                || line.split('=').next().is_some_and(|l| l.trim() == last)
        })
        .map(|i| i + 1)
}

/// `spec` as TOML, by the serializer, never by hand: a name from
/// elsewhere can't add a line.
pub fn print<T: Serialize>(spec: &T) -> anyhow::Result<String> {
    toml::to_string_pretty(spec).context("writing the spec")
}
