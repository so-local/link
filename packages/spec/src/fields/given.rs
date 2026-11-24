//! Field flags taken from a command line ([`split`]) as one TOML table,
//! and read over a spec file through the kind's strict reader
//! ([`read_with`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use link_format::WithinExt;

use super::{At, Binding, flag_of, resolve};
use crate::Kind;
use crate::constants::{RESERVED, SPEC_OWN, TOP};

/// The field flags one command line gave: one table, by key path, laid
/// over the spec file's key by key. Command-line defaults never enter
/// it, so only the spec type supplies defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldFlags {
    table: toml::Table,
    /// The paths of lists given, whose later flags add an element.
    lists: Vec<Vec<String>>,
    /// The paths flags set, whose values replace the file's whole, a
    /// table's too: only the tables a dotted path passes through merge.
    set: Vec<Vec<String>>,
    /// The flags given, as their messages name them.
    given: Vec<String>,
}

impl FieldFlags {
    /// Whether no field flag was given.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// The flags as the table they lay over a file.
    pub fn table(&self) -> &toml::Table {
        &self.table
    }

    /// The value a flag set at the key path `path`, where one did.
    pub fn get(&self, path: &[&str]) -> Option<&toml::Value> {
        let (last, parents) = path.split_last()?;
        let mut table = &self.table;
        for key in parents {
            table = table.get(*key)?.as_table()?;
        }
        table.get(*last)
    }

    /// The flags as a command line gives them again, one `--<path>=<TOML
    /// value>` each, for a program that runs the same command elsewhere.
    pub fn args(&self) -> Vec<OsString> {
        self.set
            .iter()
            .filter_map(|path| {
                let keys: Vec<&str> = path.iter().map(String::as_str).collect();
                let value = self.get(&keys)?;
                Some(OsString::from(format!("--{}={value}", flag_of(path))))
            })
            .collect()
    }

    /// Set `path` to `value`, or add it to a list given before.
    fn put(&mut self, path: &[String], value: toml::Value, element: bool) -> anyhow::Result<()> {
        let shown = flag_of(path);
        // A table given whole and a field inside it are refused in either
        // order, never merged in one and refused in the other.
        if let Some(other) = self
            .set
            .iter()
            .find(|set| set.as_slice() != path && (set.starts_with(path) || path.starts_with(set)))
        {
            bail!(
                "--{shown} and --{} set the same field, one inside the other; give one",
                flag_of(other)
            );
        }
        let (last, parents) = path.split_last().context("a field has a path")?;
        let mut table = &mut self.table;
        for key in parents {
            let next = table
                .entry(key.clone())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            table = match next {
                toml::Value::Table(inner) => inner,
                _ => bail!("--{shown}: a flag before it set {key} to a value, not a table"),
            };
        }
        if !self.set.iter().any(|s| s == path) {
            self.set.push(path.to_vec());
            self.given.push(format!("--{shown}"));
        }
        let given = self.lists.iter().any(|l| l == path);
        match (table.get_mut(last), element) {
            (Some(toml::Value::Array(items)), true) if given => items.push(value),
            (Some(_), _) => bail!("--{shown} is given twice; give it once"),
            (None, true) => {
                table.insert(last.clone(), toml::Value::Array(vec![value]));
                self.lists.push(path.to_vec());
            }
            (None, false) => {
                if matches!(value, toml::Value::Array(_)) {
                    self.lists.push(path.to_vec());
                }
                table.insert(last.clone(), value);
            }
        }
        Ok(())
    }
}

/// `spec` as the field flags that make it, in the words a command line
/// takes: `--<key path>` and its value, a list's each element repeated,
/// a switch that's true alone, text bare and any other value as TOML. A
/// list of tables gives each element whole as an inline table, and an
/// empty list `=[]`. Reading these flags over an empty spec gives `spec`
/// back.
pub fn flags_of<T: Kind>(spec: &T) -> anyhow::Result<Vec<String>> {
    let table = match toml::Value::try_from(spec)? {
        toml::Value::Table(table) => table,
        other => bail!("a spec is a table, not {other}"),
    };
    let mut words = Vec::new();
    walk(&crate::schema::<T>(), &table, &mut Vec::new(), &mut words);
    Ok(words)
}

fn walk(
    schema: &serde_json::Value,
    table: &toml::Table,
    path: &mut Vec<String>,
    words: &mut Vec<String>,
) {
    for (key, value) in table {
        if path.is_empty() && (key == "kind" || key == "schema") {
            continue;
        }
        path.push(key.clone());
        let flag = format!("--{}", flag_of(path));
        match value {
            toml::Value::Table(inner) if !inner.is_empty() => walk(schema, inner, path, words),
            toml::Value::Array(items) if items.is_empty() => words.push(format!("{flag}=[]")),
            toml::Value::Array(_)
                if resolve(schema, path)
                    .ok()
                    .and_then(|at| at.element())
                    .is_some_and(|element| element.element().is_some()) =>
            {
                words.push(format!("{flag}={value}"));
            }
            toml::Value::Array(items) => {
                for item in items {
                    push_value(&flag, item, words);
                }
            }
            toml::Value::Boolean(true) if resolve(schema, path).is_ok_and(|at| at.switch()) => {
                words.push(flag)
            }
            value => {
                push_value(&flag, value, words);
            }
        }
        path.pop();
    }
}

fn push_value(flag: &str, value: &toml::Value, words: &mut Vec<String>) {
    let value = word(value);
    if value.starts_with('-') {
        words.push(format!("{flag}={value}"));
    } else {
        words.extend([flag.to_string(), value]);
    }
}

/// `value` as a flag's value: text bare, anything else as TOML.
fn word(value: &toml::Value) -> String {
    match value {
        // A quoted string, number, array, or other TOML-looking text
        // would otherwise be parsed as that value in an opaque field.
        toml::Value::String(text) if toml_value(text).is_some() => value.to_string(),
        toml::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Take every field flag out of `args`, a whole command line, before
/// clap reads the rest: each `--<key path>` with its value, checked
/// against the shape of the spec the command binds, or refused where it
/// binds none. A field's old flag is answered from the step that retired
/// it. Nothing after `--` is a flag. Relative paths in flags resolve from
/// the current folder.
///
/// `trailing`, for a command that takes the rest of its line as its own
/// (`sandbox run <program> ...`), says where the program starts: from the
/// first word after its name that is neither a flag nor a runtime flag's
/// value, nothing is a field flag, so a program's own dotted flags stay
/// its own.
pub fn split(
    args: impl IntoIterator<Item = OsString>,
    binding: Option<&Binding>,
    trailing: Option<Trailing>,
) -> anyhow::Result<(FieldFlags, Vec<OsString>)> {
    split_claimed(args, binding, trailing, &[])
}

/// [`split`], where the command's runtime flags are `runtime`, its own and
/// the shared ones, by their long names: a flag one of them names stays
/// the command's, and one sharing a field's name is refused, since a
/// runtime flag never shares a field's.
pub fn split_claimed(
    args: impl IntoIterator<Item = OsString>,
    binding: Option<&Binding>,
    trailing: Option<Trailing>,
    runtime: &[String],
) -> anyhow::Result<(FieldFlags, Vec<OsString>)> {
    let cwd = std::env::current_dir().context("reading the current folder")?;
    split_from(
        args,
        binding,
        trailing,
        runtime,
        &cwd,
        &link_files::home_dir(),
    )
}

/// The runtime flags of `runtime` sharing the name of a top-level field
/// of `schema`, each a declaration to fix: none on a command that binds
/// its spec rightly.
pub fn clashes(schema: &serde_json::Value, runtime: &[String]) -> Vec<String> {
    let fields = At::top(schema).keys();
    runtime
        .iter()
        .filter(|flag| short_field(&fields, flag))
        .map(|flag| format!("--{flag} is a runtime flag and a field's flag: rename one"))
        .collect()
}

/// Whether `name`, undotted, is a top-level field's flag among `fields`.
fn short_field(fields: &std::collections::BTreeSet<&str>, name: &str) -> bool {
    fields.contains(name) && !SPEC_OWN.contains(&name) && !RESERVED.contains(&name)
}

/// Where a command that takes the rest of its line as its own starts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trailing {
    /// How many words name the command, the program's first.
    pub words: usize,
    /// Its runtime flags that take a value (`upstream`), whose value is
    /// no program's word.
    pub valued: Vec<String>,
}

impl From<usize> for Trailing {
    fn from(words: usize) -> Self {
        Trailing {
            words,
            valued: Vec::new(),
        }
    }
}

pub(crate) fn split_from(
    args: impl IntoIterator<Item = OsString>,
    binding: Option<&Binding>,
    trailing: Option<Trailing>,
    runtime: &[String],
    cwd: &Path,
    home: &Path,
) -> anyhow::Result<(FieldFlags, Vec<OsString>)> {
    if let Some(binding) = binding
        && let Some(clash) = clashes(&binding.schema, runtime).into_iter().next()
    {
        bail!("{clash}");
    }
    let top = binding
        .map(|b| At::top(&b.schema).keys())
        .unwrap_or_default();
    // A flag with no dot is a field's when its word is a top-level field
    // the command's runtime flags don't claim, or names one the format
    // keeps for itself or retired, so it's answered by name.
    let short = |name: &str| {
        !runtime.iter().any(|r| r == name)
            && (short_field(&top, name)
                || RESERVED.contains(&name)
                || binding.is_some_and(|b| b.retired(&[name.to_string()]).is_some()))
    };
    let mut flags = FieldFlags::default();
    let mut rest = Vec::new();
    let mut args = args.into_iter().peekable();
    let mut at = 0;
    // Whether this word is a runtime flag's value.
    let mut value_of_flag = false;
    while let Some(arg) = args.next() {
        at += 1;
        let program_words = trailing.as_ref().is_some_and(|t| at > t.words)
            && !value_of_flag
            && arg.to_str().is_none_or(|a| !a.starts_with('-'));
        value_of_flag = trailing.as_ref().is_some_and(|t| {
            arg.to_str()
                .and_then(|a| a.strip_prefix("--"))
                .is_some_and(|name| t.valued.iter().any(|v| v == name))
        });
        if arg == "--" || program_words {
            rest.push(arg);
            rest.extend(args);
            break;
        }
        let Some((name, value)) = arg.to_str().and_then(|a| field_flag(a, &short)) else {
            rest.push(arg);
            continue;
        };
        let mut path = path_of(name)?;
        let Some(binding) = binding else {
            bail!("--{name}: this command takes no field flags; a runtime flag never holds a dot");
        };
        let mut at = match resolve(&binding.schema, &path) {
            Ok(at) => at,
            Err(unknown) => match binding.retired(&path) {
                Some(retired) => return Err(retired.into()),
                None => bail!("{unknown}"),
            },
        };
        // A secret's value on the command line is in the shell's history
        // and every process listing: the spec file names it instead.
        if at.sensitive() {
            bail!(
                "--{name} is a secret's: a field flag never takes one, so write it in the spec file, --spec"
            );
        }
        let text = match value {
            Some(text) => text.to_string(),
            // A switch alone is true, and takes only a word saying so.
            None if at.switch() => match args.next_if(|next| next == "true" || next == "false") {
                Some(word) => word.to_string_lossy().into_owned(),
                None => "true".into(),
            },
            None => match args.next_if(|next| next != "--" && !starts_flag(next)) {
                Some(next) => next
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("--{name}: its value isn't text"))?,
                None => bail!("--{name} needs a value: --{name} VALUE, or --{name}=VALUE"),
            },
        };
        // A bare value on a record's own flag is its shorthand's: only a
        // whole table, `{ … }`, sets the record itself.
        if !at.value()
            && let Some((key, short)) = at.shorthand()
            && !matches!(toml_value(&text), Some(toml::Value::Table(_)))
        {
            path.push(key.to_string());
            at = short;
        }
        let (value, element) = value_at(&at, &text)?;
        let value = match (element, at.element()) {
            (true, Some(each)) => absolute(&each, value, cwd, home)?,
            _ => absolute(&at, value, cwd, home)?,
        };
        flags.put(&path, value, element)?;
    }
    Ok((flags, rest))
}

/// Whether `arg` starts another flag, so isn't a value: a dash before
/// anything but a number.
fn starts_flag(arg: &OsString) -> bool {
    // A lone `-` is a value: standard input, as `--x.file -` names it.
    arg.to_str().is_some_and(|a| {
        a.len() > 1
            && a.starts_with('-')
            && !a[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.')
    })
}

/// `arg`'s name and any `=value`, when it's a field flag: a long flag
/// whose name holds a dot outside quotes, or one `short` says is a
/// top-level field's.
fn field_flag<'a>(
    arg: &'a str,
    short: &dyn Fn(&str) -> bool,
) -> Option<(&'a str, Option<&'a str>)> {
    let body = arg.strip_prefix("--")?;
    let (word, value) = match body.split_once('=') {
        Some((word, value)) => (word, Some(value)),
        None => (body, None),
    };
    if !word.is_empty() && short(word) {
        return Some((word, value));
    }
    let mut quote = None;
    let mut escaped = false;
    let mut dotted = false;
    for (i, c) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (Some('"'), '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '.') => dotted = true,
            (None, '=') => return dotted.then(|| (&body[..i], Some(&body[i + 1..]))),
            _ => {}
        }
    }
    dotted.then_some((body, None))
}

/// The key path a flag's name spells: TOML's dotted keys, `spec.` before
/// a top-level key.
fn path_of(name: &str) -> anyhow::Result<Vec<String>> {
    let keys: Vec<String> = toml_edit::Key::parse(name)
        .map_err(|e| anyhow::anyhow!("--{name} isn't a key path: {e}"))?
        .into_iter()
        .map(|k| k.get().to_string())
        .collect();
    match keys.as_slice() {
        [top, rest @ ..] if top == TOP => match rest {
            [only] if SPEC_OWN.contains(&only.as_str()) => Ok(vec![only.clone()]),
            _ => bail!(
                "--{name}: {TOP}. comes only before the spec's own {}; a field's flag is its \
                 own key path, --{}",
                SPEC_OWN.join(", "),
                rest.join(".")
            ),
        },
        _ => Ok(keys),
    }
}

/// The value `text` gives at `at`, and whether it's one list element:
/// the TOML value the file would hold, or the text itself where a
/// string is wanted. A list takes a whole list or one element.
fn value_at(at: &At<'_>, text: &str) -> anyhow::Result<(toml::Value, bool)> {
    let parsed = toml_value(text);
    if let Some(element) = at.element() {
        // Its element types distinguish a whole nested list from one
        // element. An empty list always clears the whole list.
        if let Some(list @ toml::Value::Array(_)) = &parsed
            && at.accepts(list)
        {
            return Ok((list.clone(), false));
        }
        return Ok((one(&element, text, parsed)?, true));
    }
    Ok((one(at, text, parsed)?, false))
}

fn one(at: &At<'_>, text: &str, parsed: Option<toml::Value>) -> anyhow::Result<toml::Value> {
    match parsed {
        Some(value) if at.accepts(&value) => Ok(value),
        _ if at.takes_text(text) => Ok(toml::Value::String(text.to_string())),
        // What the reader refuses, it refuses as it would in a file.
        Some(value) => Ok(value),
        None => Ok(toml::Value::String(text.to_string())),
    }
}

/// `text` as one TOML value, or nothing where it's no value alone.
fn toml_value(text: &str) -> Option<toml::Value> {
    let mut table: toml::Table = toml::from_str(&format!("v = {text}")).ok()?;
    match table.len() {
        1 => table.remove("v"),
        _ => None,
    }
}

/// `value` with its paths, where `at` says it's one, made absolute from
/// `dir`: a list's elements alike.
fn absolute(
    at: &At<'_>,
    value: toml::Value,
    dir: &Path,
    home: &Path,
) -> anyhow::Result<toml::Value> {
    transform_paths(at, value, &|at, text| resolved_path(at, text, dir, home))
}

fn resolved_path(at: &At<'_>, text: String, dir: &Path, home: &Path) -> anyhow::Result<String> {
    if !at.path() {
        return Ok(text);
    }
    let resolved = crate::path(Path::new(&text), dir, home);
    let resolved = if at.confined() {
        resolved.context("write a confined input path without ..")?
    } else {
        resolved?
    };
    Ok(resolved.display().to_string())
}

fn absolute_table(
    at: &At<'_>,
    table: toml::Table,
    dir: &Path,
    home: &Path,
) -> anyhow::Result<toml::Table> {
    transform_table(at, table, &|at, text| resolved_path(at, text, dir, home))
}

fn transform_paths(
    at: &At<'_>,
    value: toml::Value,
    transform: &dyn Fn(&At<'_>, String) -> anyhow::Result<String>,
) -> anyhow::Result<toml::Value> {
    Ok(match value {
        toml::Value::String(text) => toml::Value::String(transform(at, text)?),
        toml::Value::Array(items) => match at.element() {
            Some(element) => toml::Value::Array(
                items
                    .into_iter()
                    .map(|value| transform_paths(&element, value, transform))
                    .collect::<anyhow::Result<_>>()?,
            ),
            None => toml::Value::Array(items),
        },
        toml::Value::Table(table) => toml::Value::Table(transform_table(at, table, transform)?),
        other => other,
    })
}

fn transform_table(
    at: &At<'_>,
    table: toml::Table,
    transform: &dyn Fn(&At<'_>, String) -> anyhow::Result<String>,
) -> anyhow::Result<toml::Table> {
    table
        .into_iter()
        .map(|(key, value)| {
            let value = match at.child(&key) {
                Some(child) => transform_paths(&child, value, transform)?,
                None => value,
            };
            Ok((key, value))
        })
        .collect()
}

fn confine<T: Kind>(table: toml::Table, root: &Path) -> anyhow::Result<toml::Table> {
    transform_table(&At::top(&crate::schema::<T>()), table, &|at, text| {
        if !at.confined() {
            return Ok(text);
        }
        let absolute = crate::path(Path::new(&text), root, &link_files::home_dir())?;
        let child = absolute.strip_prefix(root).with_context(|| {
            format!(
                "{} is outside the spec's folder {}",
                absolute.display(),
                root.display()
            )
        })?;
        anyhow::ensure!(
            !child.as_os_str().is_empty(),
            "a confined source path names a child of the spec's folder"
        );
        Ok(child.display().to_string())
    })
}

/// `over` laid on `base` key by key: a table a dotted flag passes
/// through merges into the file's, and a value a flag set replaces what
/// was there, a list or a table included, so `{}` empties a table.
fn merge(base: &mut toml::Table, over: toml::Table, path: &mut Vec<String>, set: &[Vec<String>]) {
    for (key, value) in over {
        path.push(key.clone());
        let whole = set.iter().any(|s| s == path);
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(more)) if !whole => {
                merge(inner, more, path, set)
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
        path.pop();
    }
}

/// A spec made from a file, a resource or flags alone, with the flags
/// laid over it.
#[derive(Debug)]
pub struct Made<T> {
    pub spec: T,
    /// The folder its file's relative paths were from, or the current
    /// folder when no file was given.
    pub dir: PathBuf,
    /// The file, its real path, where one was given.
    pub file: Option<PathBuf>,
}

/// Read the spec of kind `T` from `file`, if given, or standard input
/// for `-`, with `flags` over it, once, through the kind's strict reader: file and flags share its
/// validation and defaults. Relative paths resolve from the file's
/// folder; the flags' resolved when they were split.
pub fn read_with<T: Kind>(file: Option<&Path>, flags: &FieldFlags) -> anyhow::Result<Made<T>> {
    made_with(file, flags).map_err(link_format::refused)
}

fn made_with<T: Kind>(file: Option<&Path>, flags: &FieldFlags) -> anyhow::Result<Made<T>> {
    let cwd = std::env::current_dir().context("reading the current folder")?;
    let (base, dir, file) = match file {
        Some(path) => {
            let (table, read) = file_table::<T>(path, flags.is_empty())?;
            (table, read.dir, Some(read.path))
        }
        None => (toml::Table::new(), cwd, None),
    };
    let spec = overlay_at::<T>(base, flags, file.as_deref(), Some(&dir))?;
    Ok(Made { spec, dir, file })
}

/// Read text with a known source folder. Host paths resolve from that
/// folder, while field flags retain their invocation source. Confined
/// host paths must remain inside the source folder and are stored as
/// child-relative names. No file is opened or written by this function.
pub fn read_text_at<T: Kind>(text: &str, dir: &Path, flags: &FieldFlags) -> anyhow::Result<T> {
    anyhow::ensure!(dir.is_absolute(), "a spec's source folder must be absolute");
    let base = table_at::<T>(text, dir, flags.is_empty())?;
    overlay_at::<T>(base, flags, None, Some(dir)).map_err(link_format::refused)
}

/// Refuses a file's empty text as the kind's own reader does: a file
/// names its schema, empty or not, and only a spec made from flags alone
/// is current without one. A caller reading a file's text itself checks
/// it here before [`read_text_at`] or [`read_text_with`], which take
/// empty text for flags alone.
pub fn named_schema<T: Kind>(text: &str) -> anyhow::Result<()> {
    if !text.trim().is_empty() {
        return Ok(());
    }
    Err(crate::parse_spec::<T>(text)
        .err()
        .unwrap_or_else(|| anyhow::anyhow!("an empty spec names no schema")))
}

/// [`read_with`] for a spec's text read elsewhere, such as from standard
/// input: `text` in the current schema, or empty for flags alone, with
/// `flags` over it, once, through the kind's strict reader. Its relative
/// paths are left as written, for the caller to resolve.
pub fn read_text_with<T: Kind>(text: &str, flags: &FieldFlags) -> anyhow::Result<T> {
    text_with(text, flags).map_err(link_format::refused)
}

fn text_with<T: Kind>(text: &str, flags: &FieldFlags) -> anyhow::Result<T> {
    if text.trim().is_empty() {
        return overlay::<T>(toml::Table::new(), flags, None);
    }
    if flags.is_empty() {
        let spec = crate::parse_spec::<T>(text)?;
        <T as Kind>::check(&spec)?;
        return Ok(spec);
    }
    let (document, from) = T::format().document(text)?;
    if from != T::format().current {
        crate::io::note_read(T::format(), from);
    }
    let mut table: toml::Table = toml::from_str(&document.to_string())?;
    table.remove("schema");
    overlay::<T>(table, flags, None)
}

/// `base`, a spec already made, such as a thing's own, with `flags` over
/// it, read once through the kind's strict reader.
pub fn over<T: Kind>(base: &T, flags: &FieldFlags) -> anyhow::Result<T> {
    match toml::Value::try_from(base)? {
        toml::Value::Table(table) => overlay::<T>(table, flags, None).map_err(link_format::refused),
        other => bail!("a spec is a table, not {other}"),
    }
}

/// The path that names standard input, `--spec -`.
pub const STDIN: &str = "-";

/// A spec on standard input, `--spec -`, bounded as a file is, its
/// relative paths from the current folder.
fn from_stdin() -> anyhow::Result<crate::Read<toml::Table>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(crate::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("reading the spec on standard input")?;
    anyhow::ensure!(
        bytes.len() as u64 <= crate::MAX_BYTES,
        "the spec on standard input is over {} bytes",
        crate::MAX_BYTES
    );
    let text = String::from_utf8(bytes).context("the spec on standard input isn't text")?;
    let spec = crate::parse(&text).within("in the spec on standard input")?;
    Ok(crate::Read {
        spec,
        dir: std::env::current_dir().context("reading the current folder")?,
        path: PathBuf::from(STDIN),
        text,
    })
}

/// A spec file's table in the current schema, its paths absolute. With
/// no flags to follow, the file is first read alone, so its own errors
/// name its lines.
pub(super) fn file_table<T: Kind>(
    path: &Path,
    alone: bool,
) -> anyhow::Result<(toml::Table, crate::Read<toml::Table>)> {
    let read = match path == Path::new(STDIN) {
        true => from_stdin()?,
        false => crate::read::<toml::Table>(path)?,
    };
    named_schema::<T>(&read.text).within(format!("in {}", read.path.display()))?;
    let table = table_at::<T>(&read.text, &read.dir, alone)
        .within(format!("in {}", read.path.display()))?;
    Ok((table, read))
}

fn table_at<T: Kind>(text: &str, dir: &Path, alone: bool) -> anyhow::Result<toml::Table> {
    // No text given in place of a file: flags alone, over an empty spec.
    // A file's own empty text is refused before ([`file_table`]).
    if text.trim().is_empty() {
        return Ok(toml::Table::new());
    }
    if alone {
        crate::parse_spec::<T>(text)?;
    }
    let (document, from) = T::format().document(text)?;
    if !alone && from != T::format().current {
        crate::io::note_read(T::format(), from);
    }
    let mut table: toml::Table = toml::from_str(&document.to_string())?;
    table.remove("schema");
    absolute_table(
        &At::top(&crate::schema::<T>()),
        table,
        dir,
        &link_files::home_dir(),
    )
}

/// `flags` over `base`, read once by `T`'s reader.
pub(super) fn overlay<T: Kind>(
    base: toml::Table,
    flags: &FieldFlags,
    file: Option<&Path>,
) -> anyhow::Result<T> {
    overlay_at::<T>(base, flags, file, None)
}

pub(super) fn overlay_at<T: Kind>(
    mut base: toml::Table,
    flags: &FieldFlags,
    file: Option<&Path>,
    root: Option<&Path>,
) -> anyhow::Result<T> {
    merge(&mut base, flags.table.clone(), &mut Vec::new(), &flags.set);
    if let Some(root) = root {
        base = confine::<T>(base, root)?;
    }
    // The current schema, said, so no old key is taken for an old file's.
    base.insert(
        "schema".into(),
        toml::Value::Integer(i64::from(T::format().current)),
    );
    let text = toml::to_string(&base)?;
    let read = crate::follow::read::<T>(&text)
        .and_then(|read| <T as Kind>::check(&read.spec).map(|()| read));
    let given = flags.given.join(", ");
    match (read, file) {
        (Ok(read), _) => Ok(read.spec),
        // The file's own error names its line, where the file alone fails.
        (Err(e), Some(file)) if !flags.is_empty() && file != Path::new(STDIN) => {
            match crate::read_spec::<T>(file) {
                Err(own) => Err(own),
                Ok(_) => Err(e).within(format!("with {given} over {}", file.display())),
            }
        }
        (Err(e), Some(file)) => Err(e).within(format!("in {}", file.display())),
        (Err(e), None) if !flags.is_empty() => Err(e).within(format!("in {given}")),
        (Err(e), None) => Err(e).within("in an empty spec"),
    }
}
