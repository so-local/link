//! A call run through the files bridge, where Home is kept in a bucket,
//! and the storage bridge, where the agent's instructions and skills are
//! kept apart from Home ([`bridged`]).

use std::io::{BufRead, Write};

use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};

use crate::{CONVERSATIONS, limits, team_paths};
use crate::{FILE_PATH, FOLDER, Query, given, lines, media_of, parts, serve_with};
use crate::{span_of, with_change};

/// Answers the messages on `input` on `output`, until `input` ends, each
/// call run through the bridges `bridges` as its agent, writing only the
/// folders `write` names ([`bridged`]).
pub fn serve_bridged(
    bridges: &Bridges<'_>,
    write: &[String],
    offer: &super::Offer,
    input: impl BufRead,
    output: impl Write,
) -> anyhow::Result<()> {
    let seen = Seen::default();
    let run = |name: &str, arguments: &Value| bridged(bridges, write, &seen, name, arguments);
    serve_with(&run, offer, input, output)
}

/// What a bridged call reaches its agent's files through: the files
/// bridge, for Home and its teams' folders; and the storage bridge, for
/// its own `instructions/` and `skills/` where they're kept apart from
/// Home, as a cloud keeps them in its store. Each call is proved with the
/// agent's key, so the storage bridge reads and writes that agent's files
/// alone.
#[derive(Clone, Copy)]
pub struct Bridges<'a> {
    pub files: &'a link_store_contract::files::Client,
    /// None: a path in `instructions/` or `skills/` is refused, as where
    /// only its teams' folders are bridged ([`crate::serve_teamed`]).
    pub store: Option<&'a link_store_contract::store::Bridged>,
}

/// The agent's own folders a bridged call reaches through the storage
/// bridge, by their names as the model sees them.
const OWN: [link_store_contract::storage::Collection; 2] = [
    link_store_contract::storage::Collection::Instructions,
    link_store_contract::storage::Collection::Skills,
];

impl Bridges<'_> {
    /// The storage bridge, and the agent its calls are for: the one its
    /// key proves.
    fn own(
        &self,
    ) -> anyhow::Result<(
        &link_store_contract::store::Bridged,
        link_store_contract::store::Agent,
    )> {
        let store = self
            .store
            .context("your instructions and skills aren't reached here")?;
        let who = &store.binding.identity;
        let agent = link_store_contract::store::Agent {
            id: who.agent.clone(),
            owner: who.owner.clone(),
        };
        Ok((store, agent))
    }

    /// What's at `at` and its version, none when there's nothing.
    fn read(
        &self,
        at: &At,
    ) -> anyhow::Result<Option<(Vec<u8>, link_store_contract::files::Version)>> {
        use link_store_contract::store::Store as _;
        match &at.kept {
            Kept::Home => self.files.read_in(None, &at.path),
            Kept::Team(team) => self.files.read_in(Some(team), &at.path),
            Kept::Own => {
                let (store, agent) = self.own()?;
                let read = store.file_read(&agent, &at.path)?;
                Ok(read.map(|(bytes, version)| (bytes, version.to_string())))
            }
        }
    }

    /// Writes `data` at `at` if it's as `expect` says, else a
    /// [`link_store_contract::files::Conflict`]: its new version.
    fn write(
        &self,
        at: &At,
        data: &[u8],
        expect: link_store_contract::files::Expect,
    ) -> anyhow::Result<link_store_contract::files::Version> {
        use link_store_contract::files::Expect;
        use link_store_contract::store::{Store as _, Written};
        match &at.kept {
            Kept::Home => self.files.write_in(None, &at.path, data, expect),
            Kept::Team(team) => self.files.write_in(Some(team), &at.path, data, expect),
            Kept::Own => {
                let (store, agent) = self.own()?;
                // The store's versions count up from 1; 0 names none.
                let version = match expect {
                    Expect::Absent => 0,
                    Expect::Version(version) => version
                        .parse()
                        .with_context(|| format!("{version:?} isn't a version of the store's"))?,
                    Expect::Any => bail!("{} is written only from a version read", at.path),
                };
                match store.file_write(&agent, &at.path, data, version)? {
                    Written::Version(version) => Ok(version.to_string()),
                    Written::Stale(_) => Err(link_store_contract::files::Conflict {
                        name: at.path.clone(),
                    }
                    .into()),
                }
            }
        }
    }

    /// The files under folder `within` (all of Home for an empty path),
    /// each by its whole path and its size, where its place says it.
    fn list(&self, within: &At) -> anyhow::Result<Vec<(String, Option<u64>)>> {
        use link_store_contract::store::Store as _;
        let folder = match within.path.is_empty() {
            true => String::new(),
            false => format!("{}/", within.path),
        };
        let sized = |files: Vec<link_store_contract::files::Object>| {
            files.into_iter().map(|f| (f.path, Some(f.size))).collect()
        };
        match &within.kept {
            Kept::Home => Ok(sized(self.files.list_in(None, &folder)?)),
            Kept::Team(team) => Ok(sized(self.files.list_in(Some(team), &folder)?)),
            Kept::Own => {
                let (store, agent) = self.own()?;
                let paths = store.file_list(&agent, &folder)?;
                Ok(paths.into_iter().map(|path| (path, None)).collect())
            }
        }
    }
}

/// What a session saw through the bridge: the version of each file of
/// Home it last read or wrote, by its path, which its next write of the
/// file expects; and the file it last read from its first line, which
/// the `read`s going on from a later line page.
#[derive(Debug, Default)]
pub struct Seen {
    versions: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
    paged: std::cell::RefCell<Option<Paged>>,
}

/// A file as a session read it from its first line: its path as the
/// model gives it, and its text.
#[derive(Debug)]
struct Paged {
    path: String,
    text: String,
}

impl Seen {
    fn of(&self, path: &str) -> Option<String> {
        self.versions.borrow().get(path).cloned()
    }

    fn saw(&self, path: &str, version: String) {
        self.versions.borrow_mut().insert(path.to_string(), version);
    }

    fn forget(&self, path: &str) {
        self.versions.borrow_mut().remove(path);
    }

    /// File `path`'s text as last read from its first line, unless it was
    /// written since.
    fn paged(&self, path: &str) -> Option<std::cell::Ref<'_, str>> {
        std::cell::Ref::filter_map(self.paged.borrow(), |paged| match paged {
            Some(paged) if paged.path == path => Some(paged.text.as_str()),
            _ => None,
        })
        .ok()
    }

    fn page(&self, path: &str, text: String) {
        *self.paged.borrow_mut() = Some(Paged {
            path: path.to_string(),
            text,
        });
    }

    /// Forgets `path`'s pages: the session wrote it.
    fn wrote(&self, path: &str, version: String) {
        let mut paged = self.paged.borrow_mut();
        if paged.as_ref().is_some_and(|p| p.path == path) {
            *paged = None;
        }
        self.saw(path, version);
    }
}

/// Runs call `name` with `arguments` on Home through the files bridge of
/// `bridges`, and on the agent's own `instructions/` and `skills/`
/// through its storage bridge, writing only the folders `write` names
/// (`.` for all of Home but its conversations and its own folders). A `write` of a file the session read
/// (or wrote) names the version it last saw ([`Seen`]), and an `edit`
/// the version it reads, so a file changed meanwhile, by another of the
/// agent's units, is refused and read again, never overwritten unseen.
/// A `write` of a file it never read replaces what's there. A `read` from
/// a later line than the first goes on in the text the session read from
/// the first line, so a file's pages are of one version and the file is
/// never read whole again for each; a `read` from the first line reads it
/// anew. A path under
/// `teams/<team>/` (`teams/<account>/<team>/` for another account's team)
/// is that team's folder, reached as the bridge says at each call
/// ([`link_store_contract::files::TeamRef`]); a `glob` or `grep` with no
/// `path` looks in Home alone.
pub fn bridged(
    bridges: &Bridges<'_>,
    write: &[String],
    seen: &Seen,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    use link_store_contract::files::Expect;
    let text = |key: &str| -> anyhow::Result<&str> {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("{name} needs '{key}', a string"))
    };
    // A path as Home has it, or a team's folder: relative, no `..`, none
    // hidden.
    let home_path = |path: &str| -> anyhow::Result<At> {
        let at = at(&parts(path)?)?;
        link_store_contract::files::check_path(&at.path)?;
        Ok(at)
    };
    // What's there and its version, at most `file_max` ([`Limits`]).
    let current_bytes = |at: &At| -> anyhow::Result<Option<(Vec<u8>, String)>> {
        let path = &at.path;
        let Some((bytes, version)) = bridges.read(at)? else {
            return Ok(None);
        };
        ensure!(
            bytes.len() as u64 <= limits().file_max,
            "{path} is over {} MiB, larger than file reads",
            limits().file_max >> 20
        );
        Ok(Some((bytes, version)))
    };
    // As text.
    let current = |at: &At| -> anyhow::Result<Option<(String, String)>> {
        let Some((bytes, version)) = current_bytes(at)? else {
            return Ok(None);
        };
        let path = &at.path;
        let text =
            String::from_utf8(bytes).with_context(|| format!("{path} isn't text (UTF-8)"))?;
        Ok(Some((text, version)))
    };
    // Home's folders as its grants say, the same check the local tool
    // makes; a team's, as the bridge does.
    let writable = |at: &At| -> anyhow::Result<()> {
        if let Kept::Team(_) = at.kept {
            return Ok(());
        }
        let parts: Vec<String> = at.path.split('/').map(str::to_string).collect();
        crate::home_writable(write, &parts, &at.path)
    };
    let changed =
        |err: anyhow::Error, path: &str| match link_store_contract::files::is_conflict(&err) {
            true => anyhow::anyhow!("{path} changed since it was read: read it again, then write"),
            false => err,
        };
    match name {
        "read" => {
            let at = home_path(text(FILE_PATH)?)?;
            let path = at.shown();
            let span = span_of(arguments)?;
            if span.offset > 1
                && let Some(text) = seen.paged(&path)
            {
                return lines(&text, &path, span);
            }
            let Some((bytes, version)) = current_bytes(&at)? else {
                // Nothing seen: a file removed since is written anew.
                seen.forget(&path);
                bail!("no file {path}");
            };
            seen.saw(&path, version);
            // An image or a PDF, whole; text, paged.
            if media_of(&bytes).is_some() {
                return given(bytes, &path, span);
            }
            let content =
                String::from_utf8(bytes).with_context(|| format!("{path} isn't text (UTF-8)"))?;
            let read = lines(&content, &path, span);
            if span.offset == 1 {
                seen.page(&path, content);
            }
            read
        }
        "write" => {
            let at = home_path(text(FILE_PATH)?)?;
            let path = at.shown();
            let content = text("content")?;
            writable(&at)?;
            ensure!(
                content.len() as u64 <= limits().file_max,
                "a file written is at most {} MiB",
                limits().file_max >> 20
            );
            // What's there, to show the change; the version last seen, else
            // what's there, never read. Written, it's what the version names.
            let there = bridges.read(&at)?;
            let expect = match (seen.of(&path), &there) {
                (Some(version), _) => Expect::Version(version),
                (None, Some((_, version))) => Expect::Version(version.clone()),
                (None, None) => Expect::Absent,
            };
            let created = expect == Expect::Absent;
            // Unknown if it isn't text, or is larger than a file read.
            let before = match there {
                None => Some(None),
                Some((bytes, _)) => (bytes.len() as u64 <= limits().file_max)
                    .then(|| String::from_utf8(bytes).ok().map(Some))
                    .flatten(),
            };
            let version = bridges
                .write(&at, content.as_bytes(), expect)
                .map_err(|err| changed(err, &path))?;
            seen.wrote(&path, version);
            Ok(with_change(
                json!({"wrote": path, "bytes": content.len(), "created": created}),
                before.map(|before| crate::diff::unified(&path, before.as_deref(), Some(content))),
                content,
            ))
        }
        "edit" => {
            let at = home_path(text(FILE_PATH)?)?;
            let path = at.shown();
            let (old, new) = (text("old_string")?, text("new_string")?);
            ensure!(
                !old.is_empty(),
                "'old_string' is the text to replace: it can't be empty"
            );
            let all = arguments
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            writable(&at)?;
            let (content, version) = current(&at)?.with_context(|| format!("no file {path}"))?;
            let found = content.matches(old).count();
            ensure!(
                found > 0,
                "{path} doesn't hold 'old_string': read it, and give its text exactly"
            );
            ensure!(
                found == 1 || all,
                "{path} holds 'old_string' {found} times: give more of the text around it, \
                 or 'replace_all'"
            );
            let size = content.len() as u128 - (found * old.len()) as u128
                + found as u128 * new.len() as u128;
            ensure!(
                size <= limits().file_max as u128,
                "replaced, {path} would be over {} MiB, larger than a file written",
                limits().file_max >> 20
            );
            let after = content.replace(old, new);
            // The version last seen, as a `write` expects it: changed
            // since, the edit is refused; never read, what's there.
            let version = bridges
                .write(
                    &at,
                    after.as_bytes(),
                    Expect::Version(seen.of(&path).unwrap_or(version)),
                )
                .map_err(|err| changed(err, &path))?;
            seen.wrote(&path, version);
            Ok(with_change(
                json!({"replaced": found, "path": path}),
                Some(crate::diff::unified(&path, Some(&content), Some(&after))),
                &after,
            ))
        }
        "glob" | "grep" => find_in(bridges, name, arguments, limits().find_time_max()),
        other => bail!("no call {other:?}"),
    }
}

/// `glob` and `grep` on the bridge (`call`): as the local tool finds,
/// each file's read a round trip to the bucket, looking at most `time_max` (`find_time_max_ms`,
/// [`crate::Limits`]) before it answers with what it found and says it
/// stopped.
fn find_in(
    bridges: &Bridges<'_>,
    call: &str,
    arguments: &Value,
    time_max: std::time::Duration,
) -> anyhow::Result<Value> {
    let query = Query::of(call, arguments)?;
    // Home's, or a team's folder (the whole of it for none under it).
    // Left out, empty or `.`, Home, as the local tool has it.
    let named = match arguments.get(FOLDER).and_then(Value::as_str) {
        Some(folder) if !folder.is_empty() => Some(parts(folder)?),
        _ => None,
    };
    let within = match named.filter(|parts| !parts.is_empty()) {
        Some(parts) => {
            let at = at(&parts)?;
            if !at.path.is_empty() {
                link_store_contract::files::check_path(&at.path)?;
            }
            at
        }
        _ => At {
            kept: Kept::Home,
            path: String::new(),
        },
    };
    let folder = match within.path.is_empty() {
        true => String::new(),
        false => format!("{}/", within.path),
    };
    let shown = |path: &str| At {
        kept: within.kept.clone(),
        path: path.to_string(),
    };
    // As long as the local tool looks, each file's read a round
    // trip to the bucket.
    let started = std::time::Instant::now();
    let mut files = bridges.list(&within)?;
    // Left out, Home but its conversations and the agent's own folders, as
    // the local tool looks in Home alone; a conversation's own folder is
    // found when named.
    if within.kept == Kept::Home && folder.is_empty() {
        files.retain(|(path, _)| {
            let top = path.split('/').next().unwrap_or_default();
            top != CONVERSATIONS
                && link_store_contract::storage::Collection::named_by(top, &OWN).is_none()
        });
    }
    // In the order of their paths.
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut found = Vec::new();
    let mut stopped = (files.len() > limits().files_max).then_some("looked");
    let mut skipped = crate::find::Skipped::default();
    'files: for (path, size) in files.iter().take(limits().files_max) {
        if started.elapsed() >= time_max {
            stopped = Some("time");
            break;
        }
        let below = path.strip_prefix(&folder).unwrap_or(path);
        if !query.names(below) {
            continue;
        }
        let at = shown(path);
        if !query.reads() {
            found.push(json!({"path": at.shown()}));
            if found.len() >= limits().found_max {
                stopped = Some("found");
                break;
            }
            continue;
        }
        if size.is_some_and(|size| size > limits().searched_max) {
            skipped.large += 1;
            continue;
        }
        let Some((bytes, _)) = bridges.read(&at)? else {
            continue;
        };
        // A size its place didn't say, known once read.
        if bytes.len() as u64 > limits().searched_max {
            skipped.large += 1;
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            skipped.binary += 1;
            continue;
        };
        if query.found_in(&at.shown(), &content, &mut found) {
            stopped = Some("found");
            break 'files;
        }
    }
    let mut answer = json!({"found": found, "more": stopped.is_some()});
    skipped.said(&mut answer);
    if let Some(stopped) = stopped {
        answer["stopped"] = stopped.into();
    }
    Ok(answer)
}

/// Where a bridged call's path is: where it's kept, and `path` there.
struct At {
    kept: Kept,
    path: String,
}

/// Which of the agent's places a bridged call's path is in.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kept {
    /// Home, through the files bridge.
    Home,
    /// Under `teams/`, a team's folder, through the files bridge.
    Team(link_store_contract::files::TeamRef),
    /// Its own `instructions/` and `skills/` ([`OWN`]), through the
    /// storage bridge, by the same paths.
    Own,
}

impl At {
    /// As the model gives it.
    fn shown(&self) -> String {
        match &self.kept {
            Kept::Team(team) => format!("{}/{team}/{}", team_paths(), self.path),
            Kept::Home | Kept::Own => self.path.clone(),
        }
    }
}

/// `parts` of a bridged call's path, as [`At`]: `teams/<team>/...` a team
/// of the agent's owner's, `teams/<account>/<team>/...` another account's
/// (an account's ID is never a team's name), `instructions/...` and
/// `skills/...` the agent's own, the rest Home's.
fn at(parts: &[String]) -> anyhow::Result<At> {
    let rest = |n: usize| parts.get(n..).unwrap_or_default().join("/");
    let named = |text: &str| {
        link_store_contract::files::TeamRef::parse(text)
            .with_context(|| format!("{}/{text} isn't a team's folder", team_paths()))
    };
    match parts {
        [top, ..] if link_store_contract::storage::Collection::named_by(top, &OWN).is_some() => {
            Ok(At {
                kept: Kept::Own,
                path: parts.join("/"),
            })
        }
        [top, ..] if *top != team_paths() => Ok(At {
            kept: Kept::Home,
            path: parts.join("/"),
        }),
        [_, first, second, ..] if link_store_contract::files::TeamRef::parse(first).is_err() => {
            Ok(At {
                kept: Kept::Team(named(&format!("{first}/{second}"))?),
                path: rest(3),
            })
        }
        [_, first, ..] => Ok(At {
            kept: Kept::Team(named(first)?),
            path: rest(2),
        }),
        _ => bail!("name a team's folder: {}/<team>/...", team_paths()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use link_store_contract::bridge::{Identity, Keyring};
    use link_store_contract::files::{self, DirStore, Home, Homes, Store};
    use serde_json::json;

    use super::*;

    /// Every agent's Home, one bucket's, its `work` written.
    struct Bucket(Arc<dyn Store>);

    impl Homes for Bucket {
        fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
            Ok(Home {
                store: self.0.clone(),
                cap: None,
                write: vec!["work".into()],
            })
        }
    }

    /// `grep` and `glob` on the bridge stop at their time,
    /// `find_time_max_ms`, as the local tool's do: given none, at the
    /// first file, saying so; given its own, each looks through.
    #[test]
    fn find_stops_at_its_time() {
        let dir = link_test_kit::TempDir::short("file-bridged-time");
        let binding = link_store_contract::testing::bound(&dir.join("keys"), "agent0000001");
        let socket = dir.join("files.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let keys = Arc::new(Keyring::at(&dir.join("keys")));
        let bucket = Arc::new(Bucket(Arc::new(
            DirStore::new(&dir.join("bucket")).unwrap(),
        )));
        std::thread::spawn(move || files::serve(listener, keys, bucket));
        let client = files::Client { socket, binding };
        let bridges = Bridges {
            files: &client,
            store: None,
        };
        let write = ["work".to_string()];
        let wrote = bridged(
            &bridges,
            &write,
            &Seen::default(),
            "write",
            &json!({"file_path": "work/notes.md", "content": "needle\n"}),
        );
        wrote.unwrap();
        for (call, asked) in [
            ("grep", json!({"pattern": "needle"})),
            ("glob", json!({"pattern": "*.md", "path": "work"})),
        ] {
            let stopped = find_in(&bridges, call, &asked, std::time::Duration::ZERO).unwrap();
            assert_eq!(stopped["stopped"], "time", "{call}: {stopped}");
            assert_eq!(stopped["more"], true, "{call}: {stopped}");
            assert_eq!(stopped["found"], json!([]), "{call}: {stopped}");
            let through = find_in(&bridges, call, &asked, limits().find_time_max()).unwrap();
            assert!(through.get("stopped").is_none(), "{call}: {through}");
            assert_eq!(
                through["found"][0]["path"], "work/notes.md",
                "{call}: {through}"
            );
        }
    }
}
