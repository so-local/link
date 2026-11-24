//! Where memory reads what it indexes, behind one seam ([`Tree`]): the
//! agent's `memory/` and its conversations, and nothing else of its
//! files folder.
//!
//! - [`Folder`]: a folder on this machine, as on a computer or a box:
//!   walked without following a link ([`crate::folder`]), conversations
//!   kept compact or by a store driver made into their copy
//!   ([`crate::copies`]).
//! - [`Bridged`]: Home kept in a bucket, reached through the files
//!   bridge ([`link_store_contract::files`]) as the agent, with its proof; its
//!   conversations through the store bridge ([`link_store_contract::store`]). It
//!   lists only when told something changed ([`Tree::on_change`]), never
//!   on every call.

use link_format::{Foreign, sentence};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use link_harness_contract::harness::layout::{MEMORY, SKILLS};
use link_store_contract::files::HOME;
use link_store_contract::store::Agent;

use crate::{CONVERSATIONS, Kind, copies, extract, folder};

/// A file to index, as a listing found it.
#[derive(Clone, Debug)]
pub struct Found {
    pub kind: Kind,
    pub size: i64,
    /// Nanoseconds since 1970: with its size, how it's known to have
    /// changed.
    pub modified: i64,
    /// A conversation's copy memory makes, not a file: how it's kept.
    pub copy: Option<copies::Kept>,
    /// A driver's conversation's thread.
    pub thread: Option<String>,
}

/// Where a conversation's indexing left off, so the next reads only what
/// it grew by: the bytes of its copy kept as a file, or the events of
/// one memory makes from them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// Its file's first this many bytes.
    Byte(u64),
    /// Its events up to this one's number.
    Event(u64),
}

impl Since {
    /// Its kind and its number, as the index keeps it.
    pub(crate) fn kept(self) -> (&'static str, i64) {
        match self {
            Since::Byte(n) => ("byte", n as i64),
            Since::Event(n) => ("event", n as i64),
        }
    }

    /// The one the index keeps as `kind` and `number`.
    pub(crate) fn of(kind: &str, number: i64) -> Option<Since> {
        match kind {
            "byte" => Some(Since::Byte(number.max(0) as u64)),
            "event" => Some(Since::Event(number.max(0) as u64)),
            _ => None,
        }
    }
}

/// A file's bytes as [`Tree::read`] read them: all of it, or for
/// [`Tree::read_since`], what a conversation grew by.
#[derive(Debug)]
pub struct Read {
    pub bytes: Vec<u8>,
    /// For a conversation, where what was read ends: the next read starts
    /// there ([`Tree::read_since`]).
    pub since: Option<Since>,
    /// Why it couldn't be read, though it's there (a conversation whose
    /// events don't parse), its bytes none; none when it was read.
    pub unreadable: Option<String>,
    /// Whether memory made it, from what's kept (a conversation's copy
    /// made from its events): no file holds it, so the index keeps it.
    pub derived: bool,
}

impl Read {
    /// All of a file.
    pub fn whole(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            since: None,
            unreadable: None,
            derived: false,
        }
    }

    /// A conversation's copy kept as a file, read from byte `from` to
    /// its end.
    pub fn grown(bytes: Vec<u8>, from: u64) -> Self {
        Self {
            since: Some(Since::Byte(from + bytes.len() as u64)),
            ..Self::whole(bytes)
        }
    }

    /// A conversation's copy memory made, no file holding it, or what it
    /// grew by.
    pub fn made(copy: copies::Copy) -> Self {
        Self {
            derived: true,
            since: Some(Since::Event(copy.last)),
            ..Self::whole(copy.text.into_bytes())
        }
    }

    /// A file that's there but can't be read, and why.
    pub fn unreadable(why: String) -> Self {
        Self {
            bytes: Vec::new(),
            since: None,
            unreadable: Some(why),
            derived: false,
        }
    }
}

/// What memory indexes, wherever it is.
pub trait Tree: Send {
    /// Every file to index, by its path: `memory/...`, and
    /// `chats/<thread>/conversation.md`. `known` is what the index
    /// holds of a path (size and time), for a listing that can't reach
    /// part of it now and keeps what it had.
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> Result<BTreeMap<String, Found>>;
    /// The bytes of `path` as [`Tree::list`] found it; none when it went
    /// or grew past what's read since. A conversation's copy memory makes
    /// is made from all its events ([`copies::since`]).
    fn read(&mut self, path: &str, found: &Found) -> Result<Option<Read>>;
    /// What conversation `path` grew by since `since`: its file's bytes
    /// from there, or its copy's sections for the events after it. None
    /// when it can't say, or the conversation doesn't go on from there
    /// (it shrank, or was written again): it's read whole.
    fn read_since(&mut self, _path: &str, _found: &Found, _since: Since) -> Result<Option<Read>> {
        Ok(None)
    }
    /// What reads its files, shared with every call.
    fn files(&self) -> std::sync::Arc<dyn Files>;
    /// Whether it's listed again only when told something changed
    /// ([`crate::Index::changed`]), not before each call.
    fn on_change(&self) -> bool {
        false
    }
    /// The folder it is, as shown.
    fn shown(&self) -> &Path;
    /// The agent, as a store driver keeping its conversations names it,
    /// for a tree that runs the driver itself.
    fn for_agent(&mut self, _whose: Agent) {}
    /// The store driver the agent's spec keeps its conversations by, the
    /// only one it runs, for a tree that runs drivers itself: none, none.
    fn driven_by(&mut self, _driver: Option<String>) {}
    /// What it may spend: the folders it lists and the largest file it
    /// reads ([`crate::opinions::Opinions`]).
    fn spend(&mut self, _opinions: &crate::opinions::Opinions) {}
    /// The file at `path` as a listing would find it, for a tree that can
    /// say one at a time: none when it can't (a walk finds it then), and
    /// `Ok(None)` when nothing to index is there.
    fn found(&mut self, _path: &str) -> Option<Result<Option<Found>>> {
        None
    }
    /// Watches what it holds, noting each change in `journal`; none when
    /// it can't, and a walk finds what changes.
    fn watch(
        &self,
        _journal: std::sync::Arc<crate::index::Journal>,
    ) -> Option<crate::index::Watch> {
        None
    }
}

/// What reads the agent's files for any call: an indexed file's bytes,
/// which the index keeps only the positions of, and a skill's file,
/// which memory never indexes. It holds nothing a listing changes, so no
/// call waits on the writer.
pub trait Files: Send + Sync {
    /// The bytes of `path` (from the folder served), straight from where
    /// it's kept: none when it's gone, larger than `most`, not a regular
    /// file, or has a link on the way.
    fn read(&self, path: &str, most: u64) -> Result<Option<Vec<u8>>>;
    /// Bytes `start` to `end` of `path`, as [`Files::read`] reads it,
    /// fewer where it ends sooner: a passage, never the whole file where
    /// it's kept on this machine.
    fn read_range(&self, path: &str, start: u64, end: u64) -> Result<Option<Vec<u8>>> {
        let most = crate::limits::limits().source_max_bytes;
        Ok(self.read(path, most)?.map(|bytes| {
            let end = (end as usize).min(bytes.len());
            bytes[(start as usize).min(end)..end].to_vec()
        }))
    }
}

/// A folder's files, on this machine.
struct FolderFiles(link_agent_contract::folder::Folder);

impl Files for FolderFiles {
    fn read(&self, path: &str, most: u64) -> Result<Option<Vec<u8>>> {
        // Opened afresh, each name from its folder's handle: a link
        // anywhere under the folder is refused, never followed.
        let at = place_in(&self.0, path);
        let root = folder::Dir::open(at).with_context(|| format!("reading {}", at.display()))?;
        Ok(root.read(path, most)?)
    }

    fn read_range(&self, path: &str, start: u64, end: u64) -> Result<Option<Vec<u8>>> {
        let at = place_in(&self.0, path);
        let root = folder::Dir::open(at).with_context(|| format!("reading {}", at.display()))?;
        Ok(root.read_range(path, start, end)?)
    }
}

/// Files reached through the files bridge, or none.
struct BridgedFiles(Option<std::sync::Arc<dyn link_store_contract::files::Store>>);

impl Files for BridgedFiles {
    fn read(&self, path: &str, most: u64) -> Result<Option<Vec<u8>>> {
        let Some(files) = self.0.as_ref() else {
            return Ok(None);
        };
        Ok(files
            .get(&format!("{HOME}{path}"))?
            .map(|(bytes, _)| bytes)
            .filter(|bytes| bytes.len() as u64 <= most))
    }
}

/// Where `path`'s first part is in `folder`: the agent's own folder for
/// its conversations and skills, else Home.
fn place_in<'a>(folder: &'a link_agent_contract::folder::Folder, path: &str) -> &'a Path {
    match in_own(path) {
        true => &folder.agent,
        false => &folder.home,
    }
}

/// Whether `path` (from the folder served) is one memory indexes, by its
/// name: `memory/` holds documents and text, at most `most` bytes, a
/// conversation's folder its markdown copy.
pub fn wanted(path: &str, size: u64, most: u64) -> Option<Kind> {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    // A conversation of any size: indexed whole, then as it grows
    // ([`crate::tree::Tree::read_since`]).
    if path.starts_with(&format!("{CONVERSATIONS}/")) {
        return (extension == "md").then_some(Kind::Conversation);
    }
    if !path.starts_with(&format!("{MEMORY}/")) {
        return None;
    }
    let fits = match extract::format_of(path) {
        Some(_) => size <= extract::document_limits().document_max_bytes,
        None => crate::TEXT.contains(&extension.as_str()) && size <= most,
    };
    fits.then_some(Kind::File)
}

/// Whether `path` is a skill's file, which a call reads by its path: a text file
/// under `skills/`, by names that are neither empty, hidden nor `..`.
/// A skill is read by its path, as its harness's prompt says, though
/// never indexed, so never searched or listed.
pub fn unindexed(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(&format!("{SKILLS}/")) else {
        return false;
    };
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    !rest.is_empty()
        && rest
            .split('/')
            .all(|name| !name.is_empty() && !name.starts_with('.'))
        && crate::TEXT.contains(&extension.as_str())
}

/// An agent's conversations as their place's driver keeps them, reached
/// through its storage bridge: each a copy memory makes from its events.
pub struct Stored {
    store: Box<dyn link_store_contract::store::Store + Send>,
    whose: Agent,
}

impl Stored {
    /// `whose` conversations, through `store`.
    pub fn new(store: Box<dyn link_store_contract::store::Store + Send>, whose: Agent) -> Self {
        Self { store, whose }
    }

    /// Each conversation's copy, by its path, as new as its last event.
    fn list(&self, found: &mut BTreeMap<String, Found>) -> Result<()> {
        let threads = self
            .store
            .threads(&self.whose)
            .context("listing the conversations")?;
        for kept in threads {
            let path = format!(
                "{CONVERSATIONS}/{}/{}",
                kept.thread.replace(':', "-"),
                copies::COPY
            );
            found.insert(
                path,
                Found {
                    kind: Kind::Conversation,
                    size: kept.last as i64,
                    modified: kept.last as i64,
                    copy: Some(copies::Kept::Stored),
                    thread: Some(kept.thread),
                },
            );
        }
        Ok(())
    }

    /// The copy of conversation `thread`, from after event `after`
    /// ([`copies::since`]).
    fn read(&self, thread: &str, after: u64) -> Result<Option<copies::Copy>> {
        let events = self.store.read(&self.whose, thread, after.max(1))?;
        Ok(copies::since(thread, &events, after))
    }
}

/// A folder on this machine: the agent's Home, and its own folder beside
/// it, where its conversations and skills are (as they're named here,
/// `chats/`, `skills/`), or one holding both
/// ([`link_agent_contract::folder::Folder::flat`]: a team's shared folder, or an
/// agent's laid out before the two).
pub struct Folder {
    folder: link_agent_contract::folder::Folder,
    whose: Agent,
    root: Option<folder::Dir>,
    own_root: Option<folder::Dir>,
    drivers: Option<copies::Drivers>,
    /// The store driver its spec keeps conversations by, the only one
    /// run ([`link_memory_tool_contract::DRIVER_ENV`]): none, none is.
    driver: Option<String>,
    /// Its conversations, where they're reached through the storage
    /// bridge: the folder's are then never read.
    conversations: Option<Stored>,
    /// What it may spend: the folders it lists, the largest file it reads.
    opinions: crate::opinions::Opinions,
}

impl Folder {
    /// `folder`, an agent's, whose conversations a driver keeps as
    /// `whose`.
    pub fn new(folder: link_agent_contract::folder::Folder, whose: Agent) -> Self {
        Self {
            folder,
            whose,
            root: None,
            own_root: None,
            drivers: None,
            driver: None,
            conversations: None,
            opinions: crate::opinions::Opinions::default(),
        }
    }

    /// Where `path`'s first part is: the agent's own folder for its
    /// conversations and skills, else Home.
    fn place_of(&self, path: &str) -> &Path {
        place_in(&self.folder, path)
    }

    /// The handle of the folder `path` is in, opened at the last `list`.
    fn root_of(&self, path: &str) -> Option<&folder::Dir> {
        match in_own(path) {
            true => self.own_root.as_ref(),
            false => self.root.as_ref(),
        }
    }

    /// The same, its conversations `conversations`' alone.
    pub fn with_conversations(mut self, conversations: Stored) -> Self {
        self.conversations = Some(conversations);
        self
    }
}

impl Tree for Folder {
    fn list(
        &mut self,
        known: &dyn Fn(&str) -> Option<(i64, i64)>,
    ) -> Result<BTreeMap<String, Found>> {
        let mut found = BTreeMap::new();
        let home = &self.folder.home;
        let root =
            folder::Dir::open(home).with_context(|| format!("reading {}", home.display()))?;
        // Its own folder: not there yet, it holds nothing.
        let own_root = folder::Dir::search(&self.folder.agent).ok();
        // Only its documents and its conversations: the rest of the folder
        // is read through `file`, never indexed. Its conversations kept
        // elsewhere, through the storage bridge alone.
        let chats = self.opinions.indexes(crate::opinions::Indexed::Chats);
        let mut tops = Vec::new();
        if self.opinions.indexes(crate::opinions::Indexed::Memory) {
            tops.push(MEMORY);
        }
        if chats && self.conversations.is_none() {
            tops.push(CONVERSATIONS);
        }
        for top in tops {
            let at = match in_own(top) {
                true => own_root.as_ref(),
                false => Some(&root),
            };
            if let Some(inner) = at.and_then(|at| at.folder(top).ok()) {
                walk(&inner, top, self.opinions.file, &mut found)?;
            }
        }
        if let Some(conversations) = self.conversations.as_ref().filter(|_| chats) {
            conversations.list(&mut found)?;
        }
        // A driver's conversation is as new as its last event.
        let mut drivers = copies::Drivers::new(
            self.place_of(CONVERSATIONS).join(CONVERSATIONS),
            self.whose.clone(),
        );
        let mut gone = Vec::new();
        for (path, file) in found.iter_mut() {
            let Some(copies::Kept::Driver(name)) = &file.copy else {
                continue;
            };
            // Only the driver the spec names runs: a folder the agent
            // writes never picks it.
            if self.driver.as_deref() != Some(name.as_str()) {
                gone.push(path.clone());
                continue;
            }
            let rel = path.rsplit_once('/').map_or("", |(rel, _)| rel);
            match drivers.kept(name, rel) {
                Ok(Some(kept)) => {
                    file.size = kept.last as i64;
                    file.modified = kept.last as i64;
                    file.thread = Some(kept.thread);
                }
                Ok(None) => gone.push(path.clone()),
                // Unreachable now: as it was.
                Err(err) => {
                    crate::warn(sentence!("{} isn't reached.", Foreign::new(path)), &err);
                    match known(path) {
                        Some((size, modified)) => (file.size, file.modified) = (size, modified),
                        None => gone.push(path.clone()),
                    }
                }
            }
        }
        for path in gone {
            found.remove(&path);
        }
        self.root = Some(root);
        self.own_root = own_root;
        self.drivers = Some(drivers);
        Ok(found)
    }

    fn read(&mut self, path: &str, file: &Found) -> Result<Option<Read>> {
        let Some(root) = self.root_of(path) else {
            return Ok(None);
        };
        let rel = path.rsplit_once('/').map_or("", |(rel, _)| rel);
        let max = match file.kind {
            Kind::File if extract::format_of(path).is_some() => {
                extract::document_limits().document_max_bytes
            }
            Kind::File => self.opinions.file,
            Kind::Conversation => crate::limits::limits().source_max_bytes,
        };
        Ok(match (&file.copy, &file.thread, &self.drivers) {
            (None, _, _) if file.kind == Kind::Conversation => root
                .read(path, max)
                .ok()
                .flatten()
                .map(|bytes| Read::grown(bytes, 0)),
            (None, _, _) => root.read(path, max).ok().flatten().map(Read::whole),
            (Some(copies::Kept::Compact), _, _) => match copies::compact(root, rel, 0) {
                Ok(copy) => copy.map(Read::made),
                // There, but its events don't read: kept by its name, with
                // why, until it changes.
                Err(err) => Some(Read::unreadable(format!("{err:#}"))),
            },
            (Some(copies::Kept::Driver(name)), Some(thread), Some(drivers)) => {
                drivers.copy(name, thread, 0).ok().flatten().map(Read::made)
            }
            (Some(copies::Kept::Driver(_)), _, _) => None,
            (Some(copies::Kept::Stored), Some(thread), _) => match &self.conversations {
                Some(conversations) => conversations.read(thread, 0).ok().flatten().map(Read::made),
                None => None,
            },
            (Some(copies::Kept::Stored), None, _) => None,
        })
    }

    fn read_since(&mut self, path: &str, file: &Found, since: Since) -> Result<Option<Read>> {
        let Some(root) = self.root_of(path) else {
            return Ok(None);
        };
        let rel = path.rsplit_once('/').map_or("", |(rel, _)| rel);
        Ok(match (since, &file.copy, &file.thread, &self.drivers) {
            // Shorter than what was read: written again.
            (Since::Byte(from), None, _, _) if file.size as u64 >= from => root
                .read_range(path, from, file.size as u64)?
                .map(|bytes| Read::grown(bytes, from)),
            (Since::Event(after), Some(copies::Kept::Compact), _, _) => {
                copies::compact(root, rel, after)?.map(Read::made)
            }
            (
                Since::Event(after),
                Some(copies::Kept::Driver(name)),
                Some(thread),
                Some(drivers),
            ) => drivers.copy(name, thread, after)?.map(Read::made),
            (Since::Event(after), Some(copies::Kept::Stored), Some(thread), _) => {
                match &self.conversations {
                    Some(conversations) => conversations.read(thread, after)?.map(Read::made),
                    None => None,
                }
            }
            _ => None,
        })
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        std::sync::Arc::new(FolderFiles(self.folder.clone()))
    }

    fn shown(&self) -> &Path {
        &self.folder.home
    }

    fn for_agent(&mut self, whose: Agent) {
        self.whose = whose;
    }

    fn driven_by(&mut self, driver: Option<String>) {
        self.driver = driver;
    }

    fn spend(&mut self, opinions: &crate::opinions::Opinions) {
        self.opinions = opinions.clone();
    }

    fn found(&mut self, path: &str) -> Option<Result<Option<Found>>> {
        let mut parts = path.split('/');
        let top = parts.next()?;
        // Read since the last walk only: a walk opens the folders first.
        let root = self.root_of(path)?;
        if path.split('/').any(|name| name.starts_with('.')) {
            return Some(Ok(None));
        }
        let indexed = |folder| self.opinions.indexes(folder);
        if top == MEMORY {
            if !indexed(crate::opinions::Indexed::Memory) {
                return Some(Ok(None));
            }
            return match root.entry(path) {
                Err(err) => Some(Err(err.into())),
                Ok(None) => Some(Ok(None)),
                // A folder: what's in it is found by a walk.
                Ok(Some(entry)) if entry.kind == folder::Kind::Folder => None,
                Ok(Some(entry)) if entry.kind != folder::Kind::File => Some(Ok(None)),
                Ok(Some(entry)) => Some(Ok(wanted(path, entry.size, self.opinions.file).map(
                    |kind| Found {
                        kind,
                        size: entry.size as i64,
                        modified: entry.modified,
                        copy: None,
                        thread: None,
                    },
                ))),
            };
        }
        // A conversation's copy, from its folder alone: one a store keeps
        // (a driver's, or reached through the storage bridge) is found by a
        // walk.
        let thread = parts.next()?;
        if top != CONVERSATIONS
            || self.conversations.is_some()
            || !indexed(crate::opinions::Indexed::Chats)
        {
            return None;
        }
        let rel = format!("{CONVERSATIONS}/{thread}");
        // Each folder opened from its parent's handle: a link is refused.
        let dir = match root
            .folder(CONVERSATIONS)
            .and_then(|chats| chats.folder(thread))
        {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Ok(None)),
            // A link, or not a folder: nothing to index there.
            Err(_) => return Some(Ok(None)),
        };
        let mut found = BTreeMap::new();
        if let Err(err) = walk(&dir, &rel, self.opinions.file, &mut found) {
            return Some(Err(err));
        }
        match found.remove(path) {
            Some(file) if matches!(file.copy, Some(copies::Kept::Driver(_))) => None,
            file => Some(Ok(file)),
        }
    }

    fn watch(&self, journal: std::sync::Arc<crate::index::Journal>) -> Option<crate::index::Watch> {
        let mut roots = Vec::new();
        if self.opinions.indexes(crate::opinions::Indexed::Memory) {
            roots.push(crate::index::Root {
                base: self.folder.home.clone(),
                top: MEMORY,
            });
        }
        let chats = self.opinions.indexes(crate::opinions::Indexed::Chats);
        if chats && self.conversations.is_none() {
            roots.push(crate::index::Root {
                base: self.folder.agent.clone(),
                top: CONVERSATIONS,
            });
        }
        let watch = crate::index::watch(roots, journal.clone());
        // Conversations a store keeps change where no watch here sees.
        if watch.is_some() && chats && (self.driver.is_some() || self.conversations.is_some()) {
            journal.watching(Some(
                "the conversations a store keeps change where no watch here sees".into(),
            ));
        }
        watch
    }
}

/// Whether `path` is of the agent's own folder, as the agent sees it
/// ([`link_agent_contract::folder::seen_collection`]): its conversations or its
/// skills, here.
fn in_own(path: &str) -> bool {
    link_agent_contract::folder::seen_collection(path).is_some()
}

/// Adds the files to index in `dir` (`rel` from the folder) to `found`:
/// no hidden names, no symbolic links, nothing too large. Each folder is
/// opened from its parent's handle, never followed if it's a link.
fn walk(
    dir: &folder::Dir,
    rel: &str,
    most: u64,
    found: &mut BTreeMap<String, Found>,
) -> Result<()> {
    let entries = match dir.entries() {
        Ok(entries) => entries,
        // A folder may go meanwhile.
        Err(_) => return Ok(()),
    };
    // A conversation's folder: one without a copy gets memory's own.
    let conversation = rel
        .strip_prefix(&format!("{CONVERSATIONS}/"))
        .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'));
    if conversation {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        if let Some(kept) = copies::kept(&names) {
            // As new as its files, for a compact one: its frames and tail.
            let files = entries.iter().filter(|e| {
                [
                    link_store_contract::store::compact::FRAMES,
                    link_store_contract::store::compact::TAIL,
                ]
                .contains(&e.name.as_str())
            });
            let (size, modified) = files.fold((0, 0), |(size, modified), e| {
                (size + e.size as i64, modified.max(e.modified))
            });
            found.insert(
                format!("{rel}/{}", copies::COPY),
                Found {
                    kind: Kind::Conversation,
                    size,
                    modified,
                    copy: Some(kept),
                    thread: None,
                },
            );
        }
    }
    for entry in entries {
        let name = entry.name;
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{rel}/{name}");
        if entry.kind == folder::Kind::Folder {
            // Gone, or swapped for a link, since it was listed: skipped.
            if let Ok(inner) = dir.folder(&name) {
                walk(&inner, &path, most, found)?;
            }
            continue;
        }
        if entry.kind != folder::Kind::File {
            continue;
        }
        let Some(kind) = wanted(&path, entry.size, most) else {
            continue;
        };
        found.insert(
            path,
            Found {
                kind,
                size: entry.size as i64,
                modified: entry.modified,
                copy: None,
                thread: None,
            },
        );
    }
    Ok(())
}

/// Home kept in a bucket, through the files bridge, and conversations
/// through the storage bridge: either, or both. Or a team's folder,
/// through the files bridge as a member ([`link_store_contract::files::InTeam`]):
/// its `memory/` as Home's is.
pub struct Bridged {
    /// Its Home, or the team's folder, where it's kept in a bucket: a
    /// store named as Home's is (`home/<path>`).
    files: Option<std::sync::Arc<dyn link_store_contract::files::Store>>,
    /// Its conversations, when they're reached through the storage
    /// bridge.
    conversations: Option<Stored>,
    shown: PathBuf,
    /// What it may spend: the folders it lists, the largest file it reads.
    opinions: crate::opinions::Opinions,
}

impl Bridged {
    /// `files`, a Home's or a team's folder's objects through the files
    /// bridge, and `conversations`.
    pub fn new(
        files: impl link_store_contract::files::Store + 'static,
        conversations: Option<Stored>,
    ) -> Self {
        Self {
            files: Some(std::sync::Arc::new(files)),
            conversations,
            shown: PathBuf::from("home"),
            opinions: crate::opinions::Opinions::default(),
        }
    }

    /// Its conversations alone, its Home reached by no bridge here: an
    /// agent bound with the storage bridge only.
    pub fn conversations(conversations: Stored) -> Self {
        Self {
            files: None,
            conversations: Some(conversations),
            shown: PathBuf::from("home"),
            opinions: crate::opinions::Opinions::default(),
        }
    }

    /// File `path` of the folder, through the bridge.
    fn file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match &self.files {
            Some(files) => Ok(files.get(&format!("{HOME}{path}"))?.map(|(bytes, _)| bytes)),
            None => Ok(None),
        }
    }
}

impl Tree for Bridged {
    fn list(&mut self, _: &dyn Fn(&str) -> Option<(i64, i64)>) -> Result<BTreeMap<String, Found>> {
        let mut found = BTreeMap::new();
        let documents = match &self.files {
            Some(files) if self.opinions.indexes(crate::opinions::Indexed::Memory) => {
                files.list(&format!("{HOME}{MEMORY}/"))?
            }
            _ => Vec::new(),
        };
        for mut object in documents {
            let Some(path) = object.path.strip_prefix(HOME) else {
                continue;
            };
            object.path = path.to_string();
            // Hidden files and anything outside `memory/` aren't Home's
            // documents, whatever the bridge says.
            if object.path.split('/').any(|p| p.starts_with('.')) {
                continue;
            }
            let Some(kind) = wanted(&object.path, object.size, self.opinions.file) else {
                continue;
            };
            if kind != Kind::File {
                continue;
            }
            found.insert(
                object.path,
                Found {
                    kind,
                    size: object.size as i64,
                    modified: object.modified,
                    copy: None,
                    thread: None,
                },
            );
        }
        // Unreachable now: the index stays as it was, until the next
        // listing.
        let chats = self.opinions.indexes(crate::opinions::Indexed::Chats);
        if let Some(conversations) = self.conversations.as_ref().filter(|_| chats) {
            conversations.list(&mut found)?;
        }
        Ok(found)
    }

    fn read(&mut self, path: &str, file: &Found) -> Result<Option<Read>> {
        match (&file.thread, &self.conversations) {
            (Some(thread), Some(conversations)) => {
                Ok(conversations.read(thread, 0)?.map(Read::made))
            }
            (Some(_), None) => Ok(None),
            (None, _) => Ok(self.file(path)?.map(Read::whole)),
        }
    }

    fn read_since(&mut self, _path: &str, file: &Found, since: Since) -> Result<Option<Read>> {
        match (since, &file.thread, &self.conversations) {
            (Since::Event(after), Some(thread), Some(conversations)) => {
                Ok(conversations.read(thread, after)?.map(Read::made))
            }
            _ => Ok(None),
        }
    }

    fn files(&self) -> std::sync::Arc<dyn Files> {
        std::sync::Arc::new(BridgedFiles(self.files.clone()))
    }

    fn on_change(&self) -> bool {
        true
    }

    fn spend(&mut self, opinions: &crate::opinions::Opinions) {
        self.opinions = opinions.clone();
    }

    fn shown(&self) -> &Path {
        &self.shown
    }
}
