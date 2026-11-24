//! A bound agent, served as its bridges reach it: bound, its index
//! fetched from its copy and written back to it.

use super::*;
use link_memory_tool_contract::TeamRef;

impl Table {
    /// Binds `agent` as `bound` says, or its session of `team`'s memory:
    /// its index opened from its cache, fetched from its copy first when
    /// there's none. Bound already with the same bridges and key, it
    /// stays as it is. Either way, only once a bridge takes a call proved
    /// with its key: an index is never served on a binding alone, and a
    /// team's never to a member its files bridge refuses the team's index.
    pub(super) fn bind(
        &self,
        agent: &str,
        bound: Bound,
        team: Option<TeamSession>,
    ) -> Result<BoundSlot> {
        check_agent(agent)?;
        link_agent_contract::identity::check_owner(&bound.owner)?;
        if let Some(team) = &team {
            check_agent(&team.id).context("a team's ID")?;
        }
        self.pinned(agent, &bound)?;
        let id = team.as_ref().map(|t| t.id.as_str());
        let key = key(&bound.owner, agent, id);
        let one = self
            .binding
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.clone())
            .or_default()
            .clone();
        let _binding = one.lock().unwrap_or_else(|e| e.into_inner());
        let reached = Reached::of(agent, &bound, team.as_ref().map(|t| &t.named))?;
        reached.prove()?;
        if let Some(own) = team.as_ref().and(self.get(&bound.owner, agent, None)) {
            // A team's memory, bound only as the agent is now.
            ensure!(
                own.bound == bound,
                "agent {agent} is bound here as another binding"
            );
        }
        if let Some(slot) = self.get(&bound.owner, agent, id) {
            let (same, as_was) = (slot.bound == bound, slot.team == team);
            match (same, as_was) {
                (true, true) => return Ok(slot),
                // The team named anew, or its grant changed: this session
                // alone opened again.
                (true, false) => self.end_one(&key),
                // Bound again, as another binding: the old one's key goes,
                // its teams' memories' with it.
                (false, _) => self.end(&bound.owner, agent),
            }
        }
        let (held, options) = self.open_bound(agent, bound, team, reached)?;
        let opened = Arc::new(BoundAgent::start(held, options, self.pool.clone()));
        self.bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, opened.clone());
        Ok(opened)
    }

    /// An error unless the bridges `bound` names are this machine's own
    /// ([`Config::files_bridge`], [`Config::storage_bridge`], or the
    /// agent's own in [`Config::bridges`]): a key is proved only to them,
    /// never to a socket whoever binds names.
    fn pinned(&self, agent: &str, bound: &Bound) -> Result<()> {
        if let Some(files) = &bound.files {
            ensure!(
                self.config.files_bridge.as_ref() == Some(files),
                "agent {agent}'s files bridge isn't this machine's"
            );
        }
        if let Some(socket) = bound.storage.as_ref().and_then(|s| s.socket.as_ref()) {
            ensure!(
                own_storage(
                    self.config.storage_bridge.as_deref(),
                    self.config.bridges.as_deref(),
                    agent,
                    socket
                ),
                "agent {agent}'s storage bridge isn't this machine's"
            );
        }
        Ok(())
    }

    /// `agent`'s index, or its team's memory's, as its bridges reach it,
    /// and how it's served.
    fn open_bound(
        &self,
        agent: &str,
        bound: Bound,
        team: Option<TeamSession>,
        reached: Reached,
    ) -> Result<(Held, mcp::Options)> {
        // A team's memory is served as no `<agents>` entry says: the
        // agent's are its own Home's.
        let named = match &team {
            None if self.agents.0.contains_key(agent) => Some(args(&self.serve_args(agent)?)?),
            None => None,
            Some(_) => None,
        };
        // Its index here: where `<agents>` keeps it, else in the cache, a
        // team's under the agent's by the team's ID (`teams/<ID>`).
        let file = match named.as_ref().and_then(|a| a.index.clone()) {
            Some(file) => file,
            None => {
                let cache = self.config.cache.as_ref().with_context(|| {
                    format!("agent {agent} isn't served here, and there's no --cache")
                })?;
                let mut dir = cache.join(&bound.owner).join(agent);
                if let Some(team) = &team {
                    dir = dir.join("teams").join(&team.id);
                }
                dir.join("index.sqlite")
            }
        };
        if let Some(dir) = file.parent() {
            link_files::create_private_dir(dir)?;
        }
        let noted = noted_of(&file);
        let copies = reached.copies.clone();
        // The cache, and the copy's version it's of; without both, the
        // copy is fetched again.
        let copy = match &copies {
            None => None,
            Some(copies) => {
                // The cache, while it's of the copy there now: else the
                // copy as it is, fetched.
                let cached = match (file.exists(), std::fs::read_to_string(&noted)) {
                    (true, Ok(version)) => Some(version.trim().to_string()),
                    _ => None,
                };
                let now = crate::cache::version(copies.as_ref())?;
                match cached {
                    Some(cached) if Some(&cached) == now.as_ref() => now,
                    _ => {
                        remove_index(&file);
                        let fetched = crate::cache::fetch(copies.as_ref(), &file)?;
                        note(&noted, fetched.as_deref())?;
                        fetched
                    }
                }
            }
        };
        let tree: Box<dyn tree::Tree> = match (&named, &reached.files) {
            _ if reached.team.is_some() => Box::new(tree::Bridged::new(
                reached.team.as_ref().expect("a team").duplicate(),
                None,
            )),
            (_, Some(files)) => Box::new(tree::Bridged::new(
                files.duplicate(),
                reached.conversations(),
            )),
            (Some(named), None) => {
                let folder = tree::Folder::new(named.folder.clone(), reached.whose.clone());
                Box::new(match reached.conversations() {
                    Some(stored) => folder.with_conversations(stored),
                    None => folder,
                })
            }
            (None, None) => match reached.conversations() {
                // Bound with the storage bridge alone: its conversations.
                Some(stored) => Box::new(tree::Bridged::conversations(stored)),
                None => {
                    bail!("agent {agent}'s Home isn't served here, and its bridges don't reach it")
                }
            },
        };
        let mut index = Index::open_tree(tree, Some(&file))?;
        if let Some(named) = &named {
            index.spend(named.opinions.clone())?;
        }
        index.for_agent(reached.whose.clone());
        // The service isn't confined, and serves every agent: a document's
        // parser runs with nothing granted.
        let lnk = std::path::PathBuf::from(link_core::lnk().get_program());
        index.extract_with(crate::extract::Extractor::for_service(
            lnk,
            link_files::try_home_dir()?,
        ));
        match &named {
            Some(named) => embed(&mut index, named)?,
            None => {
                let embeds = team
                    .as_ref()
                    .is_none_or(|team| team.writes || team.semantic);
                if let Some((url, model)) = self.config.embed.as_ref().filter(|_| embeds) {
                    index.embed(Source::Bound {
                        url: url.clone(),
                        model: model.clone(),
                        binding: link_sandbox_binding::Binding {
                            identity: link_sandbox_binding::Identity {
                                agent: agent.to_string(),
                                service: bound.service.clone(),
                                owner: bound.owner.clone(),
                                teams: Vec::new(),
                            },
                            key: bound.key.duplicate(),
                        },
                        team: team.as_ref().map(|team| {
                            link_memory_tool_contract::embedding::Team {
                                id: team.id.clone(),
                                named: team.named.clone(),
                            }
                        }),
                    })?;
                }
            }
        }
        // Its notes where it may write them: through its storage bridge
        // where its memory is kept elsewhere, else the file
        // `<agents>` names.
        let most = match (&named, &team) {
            // A team's memory writes no notes.
            (_, Some(_)) => None,
            (Some(named), None) => named.notes.as_ref().map(|_| named.notes_most),
            (None, None) => bound.notes,
        };
        let notes = match (most, &reached.store) {
            (None, _) => None,
            (Some(most), Some(store)) if reached.places.elsewhere(Collection::Memory) => Some(
                Notes::stored(Arc::new(store.duplicate()), reached.whose.clone(), most)?,
            ),
            (Some(_), _) => named
                .as_ref()
                .and_then(|named| named.notes.as_ref().map(|file| (named, file)))
                .map(|(named, file)| Notes::new(named.notes_root(), file, named.notes_most))
                .transpose()?,
        };
        let tokens = named.as_ref().map_or(mcp::tokens_max(), |n| n.tokens);
        // Unnamed, its harness is taken to be sent others' messages: only
        // what's named says otherwise.
        let from = named.as_ref().is_none_or(|n| n.from);
        let mark = index.stamp()?;
        let readers = Arc::new(RwLock::new(index.common()));
        Ok((
            Held {
                index,
                bound,
                team,
                copies,
                copy,
                file,
                written: (Instant::now(), mark),
                readers,
            },
            mcp::Options {
                tokens,
                notes,
                from,
            },
        ))
    }

    /// Writes `held`'s index back if it changed since it last was (or
    /// always, with `now`), embedding what's waiting first within
    /// [`FLUSH_EMBED`] when `now`. Refused for its version, its cache is
    /// fetched again, what it lacks indexed, and written back once more.
    pub(super) fn write_back(&self, agent: &str, held: &mut Held, now: bool) -> Result<()> {
        if held.team.as_ref().is_some_and(|team| !team.writes) {
            // A team's memory for a member that only reads its folder:
            // what's changed is listed by its writer, never written back.
            held.written.0 = Instant::now();
            return Ok(());
        }
        // At a flush, what's left to index, and a change told of, is
        // indexed at once: the agent is about to sleep.
        if now && (held.index.due() || held.index.waiting() || held.index.pending()) {
            held.index.refresh()?;
        }
        if now {
            let until = Instant::now() + FLUSH_EMBED;
            while Instant::now() < until && held.index.embed_pending(16)? > 0 {}
        }
        let Some(copies) = held.copies.clone() else {
            // Kept here: nothing to write back.
            held.written.0 = Instant::now();
            return Ok(());
        };
        let mark = held.index.stamp()?;
        if mark == held.written.1 && held.copy.is_some() {
            held.written.0 = Instant::now();
            return Ok(());
        }
        let scratch = held.file.with_extension("snapshot");
        match held
            .index
            .write_back(copies.as_ref(), held.copy.as_ref(), &scratch)
        {
            Ok(crate::cache::Written::Copy(version)) => {
                note(&noted_of(&held.file), Some(&version))?;
                held.copy = Some(version);
            }
            // Past the most kept off the machine: kept here alone, searched
            // by words, as its status says.
            Ok(crate::cache::Written::TooLarge { .. }) => {}
            Err(err) if files::is_conflict(&err) => {
                // Behind another writer: the copy as it is now, and what
                // this one lacks made again from Home and the conversations.
                remove_index(&held.file);
                let reached =
                    Reached::of(agent, &held.bound, held.team.as_ref().map(|t| &t.named))?;
                let (mut fresh, _) =
                    self.open_bound(agent, held.bound.duplicate(), held.team.clone(), reached)?;
                fresh.index.refresh()?;
                fresh.index.watch();
                let copies = fresh.copies.clone().context("its copy went")?;
                let written =
                    fresh
                        .index
                        .write_back(copies.as_ref(), fresh.copy.as_ref(), &scratch)?;
                if let crate::cache::Written::Copy(version) = written {
                    note(&noted_of(&fresh.file), Some(&version))?;
                    fresh.copy = Some(version);
                }
                // Its sessions read the index fetched again from their
                // next call.
                fresh.readers = held.readers.clone();
                *fresh.readers.write().unwrap_or_else(|e| e.into_inner()) = fresh.index.common();
                *held = fresh;
            }
            Err(err) => return Err(err),
        }
        held.written = (Instant::now(), held.index.stamp()?);
        Ok(())
    }
}

/// Whether `socket` is this machine's storage bridge for `agent`: the one
/// serving every agent here (`storage_bridge`), or the agent's own in the
/// folder memory's service was told (`bridges`), where its bridge listens
/// ([`link_files::layout::MemoryBridges`]).
fn own_storage(
    storage_bridge: Option<&Path>,
    bridges: Option<&Path>,
    agent: &str,
    socket: &Path,
) -> bool {
    storage_bridge == Some(socket)
        || bridges.is_some_and(|bridges| {
            link_files::layout::MemoryBridges::at(bridges.to_path_buf()).socket(agent) == socket
        })
}

/// How a bound agent's bridges reach what memory reads and writes.
struct Reached {
    whose: link_store_contract::store::Agent,
    /// The files bridge, where its Home is kept in a bucket.
    files: Option<files::Client>,
    /// The storage bridge, and where each collection is kept.
    store: Option<link_store_contract::store::Bridged>,
    places: link_store_contract::storage::Places,
    /// Where its index's copy is kept; none, here.
    copies: Option<Arc<dyn files::Store>>,
    /// The team's folder, for a session of a team's memory: its
    /// `memory/` read and its index's copy kept beside it, through the
    /// files bridge as the agent.
    team: Option<files::InTeam>,
}

impl Reached {
    fn of(agent: &str, bound: &Bound, team: Option<&TeamRef>) -> Result<Self> {
        let binding = Binding {
            identity: Identity {
                agent: agent.to_string(),
                service: bound.service.clone(),
                owner: bound.owner.clone(),
                teams: Vec::new(),
            },
            key: bound.key.duplicate(),
        };
        let whose = link_store_contract::store::Agent {
            id: agent.to_string(),
            owner: bound.owner.clone(),
        };
        let files = bound.files.clone().map(|socket| files::Client {
            socket,
            binding: binding.duplicate(),
        });
        if let Some(team) = team {
            // A team's folder, through the files bridge alone: never the
            // agent's conversations or notes.
            let client = files.context(
                "a team's memory is reached through the files bridge, and none was supplied",
            )?;
            let folder = files::InTeam {
                client: client.duplicate(),
                team: team.clone(),
            };
            return Ok(Self {
                whose,
                files: Some(client),
                store: None,
                places: Default::default(),
                copies: Some(Arc::new(folder.duplicate())),
                team: Some(folder),
            });
        }
        let (store, places) = match &bound.storage {
            Some(storage) => (
                storage
                    .socket
                    .clone()
                    .map(|socket| link_store_contract::store::Bridged {
                        socket,
                        binding: binding.duplicate(),
                    }),
                storage.places.clone(),
            ),
            None => (None, Default::default()),
        };
        ensure!(
            files.is_some() || store.is_some(),
            "agent {agent} was bound with no bridge to reach it through"
        );
        // Its copy with its memory where that's elsewhere; else beside its
        // Home where Home is in a bucket; else none: its index is here.
        let copies: Option<Arc<dyn files::Store>> = match (&store, &files) {
            (_, Some(files)) if bound.index == link_memory_tool_contract::IndexAt::Home => {
                Some(Arc::new(files.duplicate()))
            }
            (_, None) if bound.index == link_memory_tool_contract::IndexAt::Home => {
                anyhow::bail!("the index is kept beside Home, but no files bridge was supplied")
            }
            (Some(store), _) if places.elsewhere(Collection::Memory) => Some(Arc::new(
                link_store_contract::store::memory_files::MemoryFiles::new(
                    store.duplicate(),
                    whose.clone(),
                ),
            )),
            (_, Some(files)) => Some(Arc::new(files.duplicate())),
            _ => None,
        };
        Ok(Self {
            whose,
            files,
            store,
            places,
            copies,
            team: None,
        })
    }

    /// An error unless a bridge takes a call proved with the agent's key.
    fn prove(&self) -> Result<()> {
        use link_store_contract::store::Store as _;
        match (&self.copies, &self.store) {
            (Some(copies), _) => crate::cache::version(copies.as_ref()).map(drop),
            (None, Some(store)) => store.threads(&self.whose).map(drop),
            (None, None) => bail!("no bridge to prove the agent's key with"),
        }
    }

    /// Its conversations, where they're kept elsewhere and reached
    /// through the storage bridge.
    fn conversations(&self) -> Option<tree::Stored> {
        let store = self.store.as_ref()?;
        self.places
            .elsewhere(Collection::Conversations)
            .then(|| tree::Stored::new(Box::new(store.duplicate()), self.whose.clone()))
    }
}

/// Where the copy's version an index file is of is noted: beside it.
fn noted_of(file: &Path) -> PathBuf {
    file.with_extension("copy")
}

/// Notes the copy's version the cache is of (none: there's no copy).
fn note(file: &Path, version: Option<&str>) -> Result<()> {
    link_files::write_private(file, version.unwrap_or("").as_bytes())
        .with_context(|| format!("writing {}", file.display()))
}

/// An index file gone, with what's beside it: fetched again.
fn remove_index(file: &Path) {
    for gone in ["sqlite-wal", "sqlite-shm", "copy"] {
        let _ = std::fs::remove_file(file.with_extension(gone));
    }
    for gone in ["snapshot", "fetching"] {
        let _ = std::fs::remove_dir_all(file.with_extension(gone));
    }
    crate::cache::remove_parts(file);
    let _ = std::fs::remove_file(file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_files::layout::MemoryBridges;

    /// The socket an agent's bridge listens at for its memory is the one
    /// its owner's service, told its folder, takes; another agent's,
    /// another owner's, or any with no folder told is refused.
    #[test]
    fn the_service_takes_the_socket_its_agents_bridge_makes() {
        let root = Path::new("/run/user/1000/lnk");
        let owner = format!("{}@{}", "o".repeat(40), "m".repeat(23));
        let told = MemoryBridges::in_runtime(root, &owner);
        let made = MemoryBridges::in_runtime(root, &owner).socket("m4i9n2x7k1ab");
        assert!(own_storage(
            None,
            Some(told.folder()),
            "m4i9n2x7k1ab",
            &made
        ));
        assert!(!own_storage(
            None,
            Some(told.folder()),
            "07t3h5e1r2cd",
            &made
        ));
        let theirs = MemoryBridges::in_runtime(root, "you@here").socket("m4i9n2x7k1ab");
        assert!(!own_storage(
            None,
            Some(told.folder()),
            "m4i9n2x7k1ab",
            &theirs
        ));
        assert!(!own_storage(None, None, "m4i9n2x7k1ab", &made));
        let shared = Path::new("/run/storage.sock");
        assert!(own_storage(Some(shared), None, "m4i9n2x7k1ab", shared));
    }
}
