//! The storage bridge an agent's bridge runs ([`StoragePlan`]).

use super::*;

/// The storage bridge an agent's bridge runs: where its machine keeps
/// each of its collections, the socket it listens at (in the harness's
/// own folder, which its sandbox reaches), the agent's own folder
/// (`agent/`, whose `chats/` its drivers keep what they keep in),
/// and the folders the agent may write (its sandbox file's `[files]
/// write`): a file of any other collection is never changed through the
/// bridge, wherever it's kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoragePlan {
    pub places: link_store_contract::storage::Places,
    pub socket: PathBuf,
    /// The agent's own folder.
    pub own: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write: Vec<String>,
    /// What the agent's memory may change besides, through its own socket
    /// ([`memory_storage`]): its `memory` collection (its index's copy),
    /// and its notes ([`NOTES`]) where they're written
    /// ([`link_harness_contract::harness::Folders::writes_notes`]), no other file of
    /// its instructions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<String>,
    /// Notes are bounded by the agent's memory spec, when it sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes_most: Option<u64>,
}

impl StoragePlan {
    /// Agent `agent`'s, run by harness `boxed`: each collection where its
    /// spec prefers, as this machine's `agents.toml` supports
    /// ([`link_store_contract::storage::Supported::resolve`]). None unless the
    /// harness keeps to the files folder's layout, writing only its
    /// folders of it: a harness that writes the whole files folder could
    /// put a link where a driver keeps what it keeps, and the driver,
    /// outside its sandbox, could follow it.
    pub fn of(agent: &Settings, boxed: &crate::adapter::Boxed) -> anyhow::Result<Option<Self>> {
        if boxed.folders.as_ref().is_none_or(|folders| folders.whole()) {
            return Ok(None);
        }
        let preferred = agent.spec()?.spec.storage.unwrap_or_default();
        let places = crate::machine_settings::storage()?.resolve(&preferred);
        Ok(Some(Self::with(places, boxed)))
    }

    /// The plan for `places`, run by harness `boxed`.
    pub fn with(
        places: link_store_contract::storage::Places,
        boxed: &crate::adapter::Boxed,
    ) -> Self {
        Self {
            places,
            notes_most: None,
            socket: boxed.dir.join(STORE_SOCKET),
            own: boxed.folder.agent.clone(),
            write: boxed
                .folders
                .as_ref()
                .map(|folders| folders.write.clone())
                .unwrap_or_default(),
            memory: boxed
                .folders
                .as_ref()
                .map(|folders| {
                    let mut memory = vec![
                        link_store_contract::storage::Collection::Memory
                            .name()
                            .to_string(),
                    ];
                    if folders.writes_notes() {
                        memory.push(NOTES.to_string());
                    }
                    memory
                })
                .unwrap_or_default(),
        }
    }

    /// The plan as the agent's memory's socket serves it: what memory may
    /// change added to what the agent's sandbox writes.
    pub fn for_memory(&self) -> Self {
        let mut plan = self.clone();
        plan.write.extend(self.memory.iter().cloned());
        plan
    }

    /// Whether the agent may change the file at `path` through the bridge:
    /// one of a folder its sandbox writes, or one named whole (memory's
    /// notes, [`NOTES`]).
    pub fn changes(&self, path: &str) -> bool {
        let folder = path.split('/').next().unwrap_or_default();
        self.write
            .iter()
            .any(|name| name == path || (path != NOTES && name == folder))
    }

    /// What its harness is handed: each collection's place, and the
    /// socket reaching them all.
    pub fn at(&self) -> link_store_contract::storage::StorageAt {
        link_store_contract::storage::StorageAt {
            places: self.places.clone(),
            socket: Some(self.socket.clone()),
        }
    }

    /// How agent `me`'s storage bridge answers each call: refused for any
    /// other agent, refused when it changes a file of a collection the
    /// agent's sandbox only reads (its key is in the sandbox), and else
    /// by the driver of the place the call's collection is kept at
    /// ([`Self::driver`]).
    pub fn driver_for(
        &self,
        me: link_sandbox_binding::Identity,
    ) -> Arc<link_store_contract::store::PlacedDriver> {
        self.driver_for_with(me, link_store_contract::store::Driver::find)
    }

    /// [`Self::driver_for`], each place's driver found by `find`.
    pub fn driver_for_with(
        &self,
        me: link_sandbox_binding::Identity,
        find: impl Fn(&str, &std::path::Path) -> anyhow::Result<link_store_contract::store::Driver>
        + Send
        + Sync
        + 'static,
    ) -> Arc<link_store_contract::store::PlacedDriver> {
        let plan = self.clone();
        Arc::new(
            move |caller: &link_sandbox_binding::Identity,
                  collection: link_store_contract::storage::Collection,
                  request: &link_store_contract::store::Request| {
                anyhow::ensure!(
                    caller.agent == me.agent && caller.owner == me.owner,
                    "this is agent {}'s storage bridge: agent {}'s calls aren't taken",
                    me.agent,
                    link_format::clean(&caller.agent)
                );
                if let Some(path) = request.changed_file() {
                    anyhow::ensure!(
                        plan.changes(path),
                        "agent {} may not change {}: refused",
                        me.agent,
                        link_format::clean(path)
                    );
                    if path == NOTES
                        && let Some(most) = plan.notes_most
                    {
                        match request {
                            link_store_contract::store::Request::FileWrite { data, .. } => {
                                anyhow::ensure!(
                                    link_store_contract::store::file_data(path, data)?.len() as u64
                                        <= most,
                                    "the agent's notes hold at most {most} bytes"
                                );
                            }
                            link_store_contract::store::Request::FileAppend { .. } => {
                                anyhow::bail!(
                                    "notes require a bounded versioned FileWrite, not FileAppend"
                                )
                            }
                            _ => {}
                        }
                    }
                }
                plan.driver_with(&me.agent, collection, &find)
            },
        )
    }

    /// The driver keeping `collection` of agent `agent`'s (its ID), found
    /// as Link finds a module ([`link_store_contract::store::Driver::find`]), run
    /// in its `chats/`, which is checked first for a link on the
    /// way ([`real_folders`]). Refused for the `file` place, which Link
    /// harness reaches itself.
    pub fn driver(
        &self,
        agent: &str,
        collection: link_store_contract::storage::Collection,
    ) -> anyhow::Result<link_store_contract::store::Driver> {
        self.driver_with(agent, collection, &link_store_contract::store::Driver::find)
    }

    /// [`Self::driver`], each place's driver found by `find` (its name,
    /// the folder it keeps what it keeps in).
    pub fn driver_with(
        &self,
        agent: &str,
        collection: link_store_contract::storage::Collection,
        find: &dyn Fn(&str, &std::path::Path) -> anyhow::Result<link_store_contract::store::Driver>,
    ) -> anyhow::Result<link_store_contract::store::Driver> {
        match self.places.of(collection) {
            // Link Harness keeps what's at `file` in its folder itself,
            // as its sandbox lets it: `lnk-file-store`, outside the sandbox and
            // rooted at the whole folder, is never run for it.
            link_store_contract::storage::FILE => anyhow::bail!(
                "{} is kept in agent {agent}'s folder, which its harness reaches itself: \
                 not through the storage bridge",
                collection.name()
            ),
            place => {
                let dir = self
                    .own
                    .join(link_harness_contract::harness::layout::CONVERSATIONS);
                real_folders(&dir, &self.own)?;
                find(place, &dir)
            }
        }
    }
}
