//! What an upgrade changes in the files Link writes, after the person's
//! yes: every file of state converted to its format's current schema,
//! and every transfer run, a record moving into the file of the owner
//! that now holds it. History is never rewritten.
//!
//! Each program of a release registers the formats it owns
//! ([`Saved`]), and `lnk upgrade` asks it in three stages
//! ([`Stage`]): what it will change, before anything is installed; then,
//! once the release is in place, its transfers; then its conversions. A
//! transfer runs before any conversion, so it takes records from the
//! source in the schema they were written in, and the source's next
//! schema then refuses each path it gave up. Each program is asked
//! stating the asker's protocol version, and each answer is written in
//! that version or the program's own where it's older ([`respond`]);
//! each request after the first carries what it was given, written in
//! the version the program answered in ([`Request`]).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, bail, ensure};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use crate::layout::map::{Entry, Roots};
use crate::schema::{Encoding, Format, Holds, Place, Specification, Transfer};

mod journal;

/// The upgrade protocol, which each request and answer states.
pub static PROTOCOL: LazyLock<link_version::Protocol> = LazyLock::new(|| {
    link_version::Protocol::shipped_upgrade(include_str!("upgrade/protocol.toml"))
});

/// The variable the core asking a program states its [`PROTOCOL`]
/// version in ([`asking`]), which the program answers in ([`asked`]).
pub const PROTOCOL_VARIABLE: &str = "LNK_UPGRADE_PROTOCOL";

/// A format a program owns, with what reads and writes its files as its
/// owner's type, so a conversion never writes what its owner can't read.
#[derive(Clone, Copy)]
pub struct Saved {
    pub format: &'static Format,
    convert: fn(&Format, Encoding, &str) -> anyhow::Result<String>,
    check: fn(&Format, &str) -> anyhow::Result<()>,
}

impl Saved {
    pub fn of<T: Specification>() -> Saved {
        Saved::shipped::<T>(T::format())
    }

    /// A `Format::shipped` format, whose value `T` is another crate's
    /// type, so it can't name its format as a [`Specification`] does.
    pub fn shipped<T: DeserializeOwned + Serialize>(format: &'static Format) -> Saved {
        Saved {
            format,
            convert: convert::<T>,
            check: |format, text| format.read_saved::<T>(text).map(drop),
        }
    }

    /// The map, which core owns.
    pub fn map() -> Saved {
        Saved::of::<crate::layout::map::Map>()
    }

    /// The upgrade's own journal, which core owns.
    pub fn journal() -> Saved {
        Saved::of::<journal::Journal>()
    }

    /// Home's marker, which core owns (`lnk location create`).
    pub fn marker() -> Saved {
        Saved::of::<crate::layout::Marker>()
    }

    /// When Link last swept its folders, which core owns
    /// (`lnk location check`).
    pub fn maintenance() -> Saved {
        Saved::of::<crate::maintenance::State>()
    }
}

/// `text`, a file of `format` written as `encoding`, in the current
/// schema, read back as `T`. A TOML file keeps its comments, key order
/// and line endings; JSON is written as its owner writes it.
fn convert<T: DeserializeOwned + Serialize>(
    format: &Format,
    encoding: Encoding,
    text: &str,
) -> anyhow::Result<String> {
    match encoding {
        Encoding::Toml => {
            let (doc, _) = format.document(text)?;
            let mut converted = doc.to_string();
            if text.contains("\r\n") {
                converted = converted.replace("\r\n", "\n").replace('\n', "\r\n");
            }
            format.read_saved::<T>(&converted)?;
            Ok(converted)
        }
        Encoding::Json => {
            let value: T = format.read_json(text.as_bytes())?.spec;
            Ok(String::from_utf8(format.print_json(&value)?)?)
        }
        Encoding::Lines => {
            let mut converted = String::new();
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let value: T = format.read_json(line.as_bytes())?.spec;
                converted.push_str(std::str::from_utf8(&format.print_json(&value)?)?);
                converted.push('\n');
            }
            Ok(converted)
        }
    }
}

/// What a program is asked, as `--link-upgrade <stage>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// What it will change, changing nothing.
    Plan,
    /// Its transfers.
    Transfer,
    /// Its conversions.
    Convert,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Plan, Stage::Transfer, Stage::Convert];

    /// Its word on the command line.
    pub fn word(self) -> &'static str {
        match self {
            Stage::Plan => "plan",
            Stage::Transfer => "transfer",
            Stage::Convert => "convert",
        }
    }

    pub fn parse(word: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|stage| stage.word() == word)
    }

    /// Whether it writes files, which only `lnk upgrade` asks for.
    pub fn writes(self) -> bool {
        self != Stage::Plan
    }
}

/// One change an upgrade makes, as a program answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "lowercase", deny_unknown_fields)]
pub enum Change {
    /// A file Link writes, converted from its schema to the current one.
    Convert {
        file: PathBuf,
        kind: String,
        from: u32,
        to: u32,
    },
    /// A file Link writes kept in its schema, since a step of the
    /// current one refuses what it holds: `why` says what to do, as
    /// installing the module that now owns a record it holds.
    Kept {
        file: PathBuf,
        kind: String,
        from: u32,
        to: u32,
        why: String,
    },
    /// A file Link writes that this release can't read or convert (torn,
    /// not its kind's shape, or written by a newer release): kept as it
    /// is and named, `why` saying what's wrong, while the other files go
    /// on, so one file never stops an upgrade.
    Unread {
        file: PathBuf,
        kind: String,
        why: String,
    },
    /// A spec you wrote, behind its kind's schema: offered, never
    /// rewritten, for `lnk spec convert` to convert.
    Offer {
        file: PathBuf,
        kind: String,
        from: u32,
        to: u32,
    },
    /// Records moving from `source`, of kind `from`, into `into`, of
    /// kind `kind`: the source's dotted paths taken.
    Transfer {
        source: PathBuf,
        from: String,
        into: PathBuf,
        kind: String,
        paths: Vec<String>,
    },
}

/// A program's answer: the protocol version it's written in and its
/// changes.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub protocol: u32,
    pub changes: Vec<Change>,
}

impl Answer {
    /// `changes`, written in `asked`, the version the asker stated, by a
    /// program speaking `protocol`: its current one where the asker
    /// stated none or a newer one. An older core reads a newer program's
    /// answer this way, and a version that changes the shape keeps a
    /// writer for each it still accepts.
    pub fn stating(
        changes: Vec<Change>,
        asked: Option<link_version::Accepted>,
        protocol: &link_version::Protocol,
    ) -> Answer {
        Answer {
            protocol: protocol.answering(asked),
            changes,
        }
    }

    /// The answer `program` gave in `bytes`, its version read before its
    /// shape and refused when it's one this release doesn't read. Its
    /// `protocol` is the version the program speaks to this core, which
    /// each later [`Request`] to it is written in.
    pub fn parse(program: &str, bytes: &[u8]) -> anyhow::Result<Answer> {
        let didnt = || format!("{program} didn't answer what it will change");
        let stated: Stated = serde_json::from_slice(bytes).with_context(didnt)?;
        PROTOCOL.read(program, stated.protocol)?;
        serde_json::from_slice(bytes).with_context(didnt)
    }
}

/// The version a request or answer states, read before its shape.
#[derive(Deserialize)]
struct Stated {
    protocol: u32,
}

/// What `lnk upgrade` gives a program on its standard input with a
/// stage: for the plan, the transfers the release's programs will run,
/// so each conversion is tried on what they leave; for a stage that
/// writes, the plan the person agreed to, the only changes it makes. The
/// first plan a core asks gives none, as it doesn't yet know the
/// program's version: the answer states it, and each later request is
/// written in it ([`Request::stating`]).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub changes: Vec<Change>,
}

impl Request {
    /// `changes`, written in `spoken`, the version the program's answer
    /// stated ([`Answer::parse`] has read it, so this release writes
    /// it). A version that changes the shape keeps a writer for each it
    /// still accepts.
    pub fn stating(changes: Vec<Change>, spoken: u32) -> Request {
        Request {
            protocol: spoken,
            changes,
        }
    }
}

/// `command`, which asks a program of a release for a stage, stating
/// this release's [`PROTOCOL`] version.
pub fn asking(command: &mut std::process::Command) -> &mut std::process::Command {
    command.env(PROTOCOL_VARIABLE, PROTOCOL.current.to_string())
}

/// The version the core asking this program stated, none when a person
/// asks by hand.
pub fn asked() -> anyhow::Result<Option<u32>> {
    std::env::var(PROTOCOL_VARIABLE)
        .ok()
        .map(|stated| {
            stated.parse().map_err(|_| {
                anyhow::anyhow!("{PROTOCOL_VARIABLE} is {stated:?}, not a protocol version")
            })
        })
        .transpose()
}

/// Do `stage` for `saved` in the folders of `roots`, as the asker
/// stating `asked` sent `request`, answered in its version. A stage that
/// writes is `lnk upgrade`'s alone, so it's refused unless the asker
/// states a version this release accepts; a plan asked by hand, or the
/// first plan a core asks, which gives nothing, is answered as though no
/// transfer ran. A request states its own version, read before its shape
/// and refused when it's one this release doesn't read: the core writes
/// it in the version this program's answer stated.
pub fn respond(
    stage: Stage,
    saved: &[Saved],
    roots: &Roots,
    asked: Option<u32>,
    request: &[u8],
) -> anyhow::Result<Answer> {
    let refused = || {
        format!(
            "only lnk upgrade asks a program to {} its files: run lnk upgrade",
            stage.word()
        )
    };
    let (accepted, given) = match asked {
        None => {
            ensure!(!stage.writes(), refused());
            (None, Vec::new())
        }
        Some(version) => {
            let accepted = PROTOCOL.accept("the lnk upgrading", version)?;
            let given = match request.iter().all(u8::is_ascii_whitespace) {
                true => {
                    ensure!(!stage.writes(), refused());
                    Vec::new()
                }
                false => {
                    let reading = "reading what lnk upgrade gave";
                    let stated: Stated = serde_json::from_slice(request).context(reading)?;
                    PROTOCOL.read("the lnk upgrading", stated.protocol)?;
                    serde_json::from_slice::<Request>(request)
                        .context(reading)?
                        .changes
                }
            };
            (Some(accepted), given)
        }
    };
    Ok(Answer::stating(
        run(stage, saved, roots, &given)?,
        accepted,
        &PROTOCOL,
    ))
}

/// Whether an earlier upgrade left a transfer under way in `roots`: then
/// a rerun has the release's programs do their stages though it plans no
/// change, finishing each, or dropping one whose records are gone. The
/// person agreed to it when it started.
pub fn under_way(roots: &Roots) -> anyhow::Result<bool> {
    journal::under_way(roots)
}

/// The roots of this user's map: their home folder and `~/.lnk`.
pub fn roots() -> anyhow::Result<Roots> {
    Ok(Roots {
        user_home: crate::try_home_dir()?,
        hidden: crate::try_installation_dir()?,
    })
}

/// Do `stage` for `saved`, the formats a program owns, in the folders of
/// `roots`, given `given`: for the plan, the transfers the release will
/// run; for a stage that writes, the agreed plan. What it changes, or
/// would.
pub fn run(
    stage: Stage,
    saved: &[Saved],
    roots: &Roots,
    given: &[Change],
) -> anyhow::Result<Vec<Change>> {
    let mut changes = Vec::new();
    for owned in saved {
        match stage {
            Stage::Plan => {
                changes.extend(transfers(owned, roots, None)?);
                changes.extend(conversions(owned, roots, given, false)?);
            }
            Stage::Transfer => changes.extend(transfers(owned, roots, Some(given))?),
            Stage::Convert => changes.extend(conversions(owned, roots, given, true)?),
        }
    }
    Ok(changes)
}

/// Where the location named `name` is, by this machine's map, checked.
fn location(name: &str, roots: &Roots) -> anyhow::Result<PathBuf> {
    use crate::layout::Hidden;
    // The map itself is at its fixed path, never mapped.
    if name == Entry::Hidden(Hidden::Map).name() {
        return Ok(Hidden::Map.at_default(&roots.hidden));
    }
    let key = Entry::parse(name)?;
    Ok(crate::layout::map::load(&roots.user_home, &roots.hidden)?.path(key, roots))
}

/// Whether `err` is only that the file isn't there.
fn absent(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// Each file at `place`, with what each `*` stood for.
fn found(place: &Place, roots: &Roots) -> anyhow::Result<Vec<(PathBuf, Vec<String>)>> {
    let mut at = vec![(location(&place.location, roots)?, Vec::new())];
    for segment in place.segments() {
        let mut next = Vec::new();
        for (dir, names) in at {
            let Some((before, after)) = segment.split_once('*') else {
                next.push((dir.join(segment), names));
                continue;
            };
            let reading = || format!("reading {}", dir.display());
            let entries = match std::fs::read_dir(&dir) {
                Err(err) if absent(&err) => continue,
                entries => entries.with_context(reading)?,
            };
            let mut entries = entries
                .collect::<Result<Vec<_>, _>>()
                .with_context(reading)?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                // Never a link, a hidden name or one being written.
                if name.starts_with('.')
                    || crate::layout::is_temporary(&name)
                    || entry.file_type().with_context(reading)?.is_symlink()
                {
                    continue;
                }
                let Some(stood) = name
                    .strip_prefix(before)
                    .and_then(|rest| rest.strip_suffix(after))
                    .filter(|stood| !stood.is_empty())
                else {
                    continue;
                };
                let mut names = names.clone();
                names.push(stood.to_string());
                next.push((dir.join(name), names));
            }
        }
        at = next;
    }
    let mut files = Vec::new();
    for (path, names) in at {
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() => files.push((path, names)),
            Ok(_) => {}
            Err(err) if absent(&err) => {}
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        }
    }
    Ok(files)
}

/// `place`'s file for what each `*` stands for.
fn at(place: &Place, roots: &Roots, names: &[String]) -> anyhow::Result<PathBuf> {
    let mut path = location(&place.location, roots)?;
    let mut names = names.iter();
    for segment in place.segments() {
        match segment.split_once('*') {
            Some((before, after)) => {
                let stood = names.next().context("a name for each *")?;
                path.push(format!("{before}{stood}{after}"));
            }
            None => path.push(segment),
        }
    }
    Ok(path)
}

/// A control file's text, trusted as settings are.
fn text(path: &Path) -> anyhow::Result<Option<String>> {
    crate::settings::text_optional(path)
}

/// `path` written whole with `text`, keeping its permissions where it
/// was there.
fn replace(path: &Path, text: &str) -> anyhow::Result<()> {
    let permissions = std::fs::metadata(path).ok().map(|m| m.permissions());
    crate::write_private(path, text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    if let Some(permissions) = permissions {
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// The oldest schema a file of `format` written as `encoding` holds.
fn schema_of(format: &Format, encoding: Encoding, text: &str) -> anyhow::Result<u32> {
    let number = |value: Option<&serde_json::Value>| -> anyhow::Result<u32> {
        match value {
            None => Ok(format.current),
            Some(value) => value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .context("schema must be a positive integer"),
        }
    };
    let from = match encoding {
        // Its number alone: a step may refuse what a transfer takes first.
        // An error names the line, never what's there.
        Encoding::Toml => number(
            crate::fs::from_toml::<toml::Table>(text, Path::new("the file"))?
                .get("schema")
                .map(serde_json::to_value)
                .transpose()?
                .as_ref(),
        )?,
        Encoding::Json => number(serde_json::from_str::<serde_json::Value>(text)?.get("schema"))?,
        Encoding::Lines => {
            let mut oldest = format.current;
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let value: serde_json::Value = serde_json::from_str(line)?;
                oldest = oldest.min(number(value.get("schema"))?);
            }
            oldest
        }
    };
    ensure!(
        from >= format.oldest,
        "{} schema {from} is no longer read: this lnk reads schema {} to {}",
        format.kind,
        format.oldest,
        format.current
    );
    ensure!(
        from <= format.current,
        "{} schema {from} needs a newer lnk; run lnk upgrade (this lnk reads schema {})",
        format.kind,
        format.current
    );
    Ok(from)
}

/// `file`'s writer lock, as its encoding's writers hold it.
fn lock(file: &Path, encoding: Encoding) -> anyhow::Result<crate::saved::Held> {
    match encoding {
        Encoding::Toml => crate::settings::lock(file),
        Encoding::Json | Encoding::Lines => crate::saved::lock(file),
    }
}

/// `text`, a TOML file, without the paths each of `given`'s transfers
/// from `file` takes: what the transfers leave it holding.
fn left(file: &Path, text: &str, given: &[Change]) -> anyhow::Result<String> {
    let taken: Vec<&String> = given
        .iter()
        .filter_map(|change| match change {
            Change::Transfer { source, paths, .. } if source == file => Some(paths),
            _ => None,
        })
        .flatten()
        .collect();
    if taken.is_empty() {
        return Ok(text.to_string());
    }
    let mut doc: DocumentMut = text.parse()?;
    for path in taken {
        crate::schema::take(doc.as_table_mut(), path);
    }
    Ok(doc.to_string())
}

/// What a step of the current schema refuses in `err`, if that's why a
/// conversion failed.
fn refused(err: &anyhow::Error) -> Option<&crate::schema::Refused> {
    err.chain().find_map(|cause| cause.downcast_ref())
}

/// Each file of `owned` behind its current schema, converted in memory:
/// written, when `apply` and `given`, the agreed plan, names it, else
/// only named. For the plan, each is tried as `given`'s transfers leave
/// it, so one that won't convert is named before the yes. A file a step
/// refuses is kept and named, as is one this release can't read or
/// convert, and the rest go on. A spec, which a person writes, is only
/// ever offered.
fn conversions(
    owned: &Saved,
    roots: &Roots,
    given: &[Change],
    apply: bool,
) -> anyhow::Result<Vec<Change>> {
    let format = owned.format;
    if format.holds == Holds::History {
        return Ok(Vec::new());
    }
    let mut changes = Vec::new();
    let unread = |file: PathBuf, err: anyhow::Error| Change::Unread {
        file,
        kind: format.kind.clone(),
        why: format!("{err:#}"),
    };
    for place in &format.files {
        for (file, _) in found(place, roots)? {
            // Read under its writers' lock, and held until it's written.
            let _lock = match apply && !format.named {
                true => match lock(&file, place.encoding) {
                    Ok(held) => Some(held),
                    Err(err) => {
                        changes.push(unread(file, err));
                        continue;
                    }
                },
                false => None,
            };
            let Some(original) = text(&file)? else {
                continue;
            };
            if format.named {
                // A spec this release can't read is its commands' to say.
                if let Ok(from) = schema_of(format, place.encoding, &original)
                    && from < format.current
                {
                    changes.push(Change::Offer {
                        file,
                        kind: format.kind.clone(),
                        from,
                        to: format.current,
                    });
                }
                continue;
            }
            let from = match schema_of(format, place.encoding, &original) {
                Ok(from) => from,
                Err(err) => {
                    changes.push(unread(file, err));
                    continue;
                }
            };
            if from == format.current {
                continue;
            }
            let tried = match apply || place.encoding != Encoding::Toml {
                true => original.clone(),
                false => left(&file, &original, given)?,
            };
            let converted = match (owned.convert)(format, place.encoding, &tried) {
                Ok(converted) => converted,
                Err(err) => match refused(&err) {
                    Some(refused) => {
                        changes.push(Change::Kept {
                            file,
                            kind: format.kind.clone(),
                            from,
                            to: format.current,
                            why: refused.to_string(),
                        });
                        continue;
                    }
                    None => {
                        changes.push(unread(file, err));
                        continue;
                    }
                },
            };
            let change = Change::Convert {
                file: file.clone(),
                kind: format.kind.clone(),
                from,
                to: format.current,
            };
            if apply {
                ensure!(
                    given.contains(&change),
                    "{} isn't as it was when you agreed to the upgrade: run lnk upgrade again",
                    file.display()
                );
                ensure!(
                    text(&file)?.as_deref() == Some(original.as_str()),
                    "{} changed while it was converted; run lnk upgrade again",
                    file.display()
                );
                replace(&file, &converted)?;
            }
            changes.push(change);
        }
    }
    Ok(changes)
}

/// What a transfer makes of one source file: the destination's text with
/// the records taken, and the source paths it took.
struct Taken {
    destination: String,
    paths: Vec<String>,
}

/// The records `transfer` takes from `source`, as `into` (the
/// destination file's text, or none) holds them: none when it's taken
/// them already. A record the destination holds otherwise is refused,
/// never overwritten.
fn take(
    owned: &Saved,
    transfer: &Transfer,
    source: &str,
    into: Option<&str>,
) -> anyhow::Result<Option<Taken>> {
    let format = owned.format;
    let mut source: DocumentMut = source.parse()?;
    let schema = source
        .get("schema")
        .with_context(|| {
            format!(
                "this {} file names no schema, and every file Link saves names the one it was \
                 written in: it isn't one Link wrote",
                transfer.from
            )
        })?
        .as_integer()
        .and_then(|n| u32::try_from(n).ok())
        .context("schema must be a positive integer")?;
    if schema > transfer.schema {
        return Ok(None);
    }
    ensure!(
        schema == transfer.schema,
        "{} schema {schema} gives its records to {} at schema {}: upgrade through the release \
         that converts it first",
        transfer.from,
        format.kind,
        transfer.schema
    );
    let mut destination = match into {
        Some(text) => format.document(text)?.0,
        None => format.empty(),
    };
    let mut paths = Vec::new();
    let mut taken = crate::schema::Body::new();
    for (old, new) in &transfer.take {
        let Some(item) = crate::schema::take(source.as_table_mut(), old) else {
            continue;
        };
        paths.push(old.clone());
        if transfer.migration.is_some() {
            let value = crate::schema::plain(&item.1).context("reading a taken record")?;
            insert_json(&mut taken, new, serde_json::to_value(value)?);
            continue;
        }
        match crate::schema::lookup(destination.as_table(), new) {
            Some(held) if crate::schema::plain(held) == crate::schema::plain(&item.1) => {}
            Some(_) => bail!(
                "{} holds {new} already, which {} would give it from {old}: keep one and run \
                 lnk upgrade again",
                format.kind,
                transfer.from
            ),
            None => crate::schema::put(destination.as_table_mut(), new, item)?,
        }
    }
    if paths.is_empty() {
        return Ok(None);
    }
    if let Some(named) = &transfer.migration {
        let made = format.run_transfer(named, taken)?;
        let made = toml::Value::try_from(serde_json::Value::Object(made))?;
        let toml::Value::Table(made) = made else {
            bail!("{named} made no table");
        };
        add(destination.as_table_mut(), &made, &format.kind, "")?;
    }
    let destination = destination.to_string();
    (owned.check)(format, &destination).with_context(|| {
        format!(
            "{} with the records of {} isn't one this release reads",
            format.kind, transfer.from
        )
    })?;
    Ok(Some(Taken { destination, paths }))
}

/// `value` put at dotted `path` in `body`.
fn insert_json(body: &mut crate::schema::Body, path: &str, value: serde_json::Value) {
    match crate::schema::path::split_first(path) {
        None => {}
        Some((leaf, None)) => {
            body.insert(leaf.to_string(), value);
        }
        Some((head, Some(rest))) => {
            let inner = body
                .entry(head.to_string())
                .or_insert_with(|| serde_json::Value::Object(Default::default()));
            if !inner.is_object() {
                *inner = serde_json::Value::Object(Default::default());
            }
            if let Some(inner) = inner.as_object_mut() {
                insert_json(inner, rest, value);
            }
        }
    }
}

/// Every key of `made` added to `table`, a value it holds already
/// otherwise refused.
fn add(
    table: &mut dyn toml_edit::TableLike,
    made: &toml::Table,
    kind: &str,
    at: &str,
) -> anyhow::Result<()> {
    for (key, value) in made {
        let path = if at.is_empty() {
            key.clone()
        } else {
            format!("{at}.{key}")
        };
        if let (Some(inner), toml::Value::Table(next)) = (
            table
                .get_mut(key)
                .and_then(toml_edit::Item::as_table_like_mut),
            value,
        ) {
            add(inner, next, kind, &path)?;
            continue;
        }
        match table.get(key) {
            Some(held) if crate::schema::plain(held).as_ref() == Some(value) => {}
            Some(_) => bail!("{kind} holds {path} already: keep one and run lnk upgrade again"),
            None => {
                table.insert(key, crate::schema::item(key, value)?);
            }
        }
    }
    Ok(())
}

/// Each transfer of `owned` with records left in its source: run, when
/// given `agreed`, the agreed plan, else only named.
fn transfers(
    owned: &Saved,
    roots: &Roots,
    agreed: Option<&[Change]>,
) -> anyhow::Result<Vec<Change>> {
    let format = owned.format;
    let mut changes = Vec::new();
    for transfer in &format.transfers {
        let place = format
            .files
            .first()
            .context("a format that takes records lists its file")?;
        for (source, names) in found(&transfer.source, roots)? {
            let into = at(place, roots, &names)?;
            ensure!(
                source != into,
                "{} takes from {} in its own file, {}: the map puts both at one path",
                format.kind,
                transfer.from,
                into.display()
            );
            let paths = match agreed {
                Some(agreed) => journal::run(owned, transfer, &names, roots, agreed)?,
                None => {
                    let Some(original) = text(&source)? else {
                        continue;
                    };
                    let held = text(&into)?;
                    take(owned, transfer, &original, held.as_deref())
                        .with_context(|| format!("taking from {}", source.display()))?
                        .map(|taken| taken.paths)
                        .unwrap_or_default()
                }
            };
            if paths.is_empty() {
                continue;
            }
            changes.push(Change::Transfer {
                source,
                from: transfer.from.clone(),
                into,
                kind: format.kind.clone(),
                paths,
            });
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
