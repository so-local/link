//! How a conversation is kept, its marks, and its turn's lease.

use super::*;

impl Files {
    /// How `thread`'s conversation is kept, and its folder, held open: as
    /// its files say, else as `agent`'s spec says a new one is (`[storage]
    /// conversations`), started that way: its folder made, with the files
    /// that say so. No folder where it's kept off this machine.
    pub(super) fn way(
        &self,
        agent: &Agent,
        thread: &Thread,
    ) -> anyhow::Result<(Way, Option<Held>)> {
        // Kept off this machine: the place decides, and nothing is
        // written here to say so.
        if let Some(place) = self.place_off(
            agent,
            link_store_contract::storage::Collection::Conversations,
        )? {
            return Ok((Way::Driver(place), None));
        }
        let folder = self.made(agent, thread)?;
        if let Some(way) = kept_in(&folder)? {
            return Ok((way, Some(folder)));
        }
        let way = self.configured(agent);
        match &way {
            Way::Compact => frames::start(&folder, thread.name())?,
            Way::Driver(name) => {
                link_store_contract::store::check_name(name)?;
                let marker = format!("{DRIVER_MARK}{name}");
                folder
                    .open(&marker, libc::O_WRONLY | libc::O_CREAT, 0o600)
                    .within(format!("writing {}", folder.at(&marker).display()))?;
            }
            Way::Log => {}
        }
        Ok((way, Some(folder)))
    }

    /// `thread`'s folder, made if it isn't there ([`Files::folder`]).
    pub(super) fn made(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<Held> {
        self.folder(agent, thread, true)?
            .within(format!("making {}'s folder", thread.name()))
    }

    /// Each folder in `agent`'s `chats/`, by its name, held open:
    /// one that's a link, or isn't text, left out.
    pub(super) fn each_folder(&self, agent: &Agent) -> Vec<(String, Held)> {
        let Ok(Some(conversations)) = self.conversations(agent, false) else {
            return Vec::new();
        };
        let Ok(names) = conversations.names() else {
            return Vec::new();
        };
        names
            .into_iter()
            .filter_map(|name| Some((name.clone(), conversations.folder(&name, false).ok()??)))
            .collect()
    }

    /// How `agent`'s spec says a new conversation is kept (`[storage]
    /// conversations`): the log when it can't be read.
    pub fn configured(&self, agent: &Agent) -> Way {
        match self.config(agent) {
            Ok(config) => Way::named(
                config
                    .opinions()
                    .storage
                    .unwrap_or_default()
                    .conversations(),
            ),
            Err(_) => Way::Log,
        }
    }

    /// The store drivers `agent`'s conversations are kept by, or its spec
    /// names: those it can, of its conversations' folders it can read.
    pub fn drivers(&self, agent: &Agent) -> Vec<String> {
        let mut names = Vec::new();
        if let Way::Driver(name) = self.configured(agent) {
            names.push(name);
        }
        names.extend(
            self.each_folder(agent)
                .iter()
                .filter_map(|(_, folder)| driver_of(folder)),
        );
        names.sort();
        names.dedup();
        names
    }

    /// `agent`'s conversation `thread` as a person reads it
    /// ([`markdown`]), from its events, wherever they're kept.
    pub fn conversation(&self, agent: &Agent, thread: &Thread) -> anyhow::Result<String> {
        let folder = self.folder(agent, thread, false)?;
        // A driver's, off this machine, has no folder here: it holds one
        // when it holds any of it.
        let (kept, events) = match self.kept(agent, folder.as_ref())? {
            Some(Way::Driver(_)) => {
                let events = self.read(agent, thread)?;
                (!events.is_empty(), events)
            }
            Some(_) => (true, self.read(agent, thread)?),
            None => (false, Vec::new()),
        };
        anyhow::ensure!(
            kept,
            "{} has no conversation {}",
            agent.name(),
            thread.name()
        );
        Ok(markdown(thread, &events))
    }

    /// Whether `agent`'s conversations are kept off this machine: then
    /// nothing of one is on its disk, its marks held in memory
    /// ([`Marks`]). Its settings unread, yes: nothing is written.
    pub(super) fn marks_held(&self, agent: &Agent) -> bool {
        self.place_off(
            agent,
            link_store_contract::storage::Collection::Conversations,
        )
        .map_or(true, |place| place.is_some())
    }

    pub(super) fn marks(&self) -> std::sync::MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lets go of any interruption asked of `thread`'s turn.
    pub(super) fn clear_interrupt(&self, agent: &Agent, thread: &Thread, folder: Option<&Held>) {
        self.marks()
            .interrupted
            .remove(&(agent.clone(), thread.name().to_string()));
        if let Some(folder) = folder {
            let _ = folder.remove(INTERRUPT);
        }
    }

    /// How the conversation in `folder` is kept: as its files say, where
    /// it has them, whatever its spec says now, as one kept before is read
    /// where it is until it's moved; else by its place's driver, where
    /// `agent`'s machine keeps its conversations off the machine.
    pub(super) fn kept(&self, agent: &Agent, folder: Option<&Held>) -> anyhow::Result<Option<Way>> {
        if let Some(way) = folder.map_or(Ok(None), kept_in)? {
            return Ok(Some(way));
        }
        Ok(self
            .place_off(
                agent,
                link_store_contract::storage::Collection::Conversations,
            )?
            .map(Way::Driver))
    }

    /// The store driver that keeps the conversation in `folder`, if one
    /// does ([`Files::kept`]).
    pub(super) fn driver_of(&self, agent: &Agent, folder: Option<&Held>) -> Option<String> {
        match self.kept(agent, folder).ok()?? {
            Way::Driver(name) => Some(name),
            _ => None,
        }
    }

    /// `thread`'s lease, as its file says; none for no turn, or a file
    /// that isn't one (taken as lapsed).
    pub(super) fn lease(&self, folder: &Held) -> Option<Lease> {
        serde_json::from_slice(&folder.read(TURN, MARK_MAX).ok()?).ok()
    }

    /// How long `agent`'s leases last without being renewed: its spec's
    /// (`[turns] lapse`), so a turn renews it far more often and one this
    /// old belongs to a process that died.
    pub(super) fn lapse(&self, agent: &Agent) -> std::time::Duration {
        match self.config(agent) {
            Ok(config) => config.lease().1,
            Err(_) => Config::default().lease().1,
        }
    }

    /// Whether the lease file in `folder` lapsed, `lapse` after its last
    /// renewal: unread, its last renewal is its file's time.
    pub(super) fn lapsed(&self, folder: &Held, lapse: std::time::Duration) -> bool {
        self.lapsed_as(folder, folder.read(TURN, MARK_MAX).ok().as_deref(), lapse)
    }

    /// [`Self::lapsed`], the lease file read as `read`.
    pub(super) fn lapsed_as(
        &self,
        folder: &Held,
        read: Option<&[u8]>,
        lapse: std::time::Duration,
    ) -> bool {
        let lease = read.and_then(|bytes| serde_json::from_slice::<Lease>(bytes).ok());
        let renewed = match lease {
            Some(lease) => lease.renewed,
            None => folder
                .modified(TURN)
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
        };
        secs().saturating_sub(renewed) >= lapse.as_secs()
    }
}
