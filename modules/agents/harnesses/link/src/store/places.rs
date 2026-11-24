//! Where a conversation is kept off this machine, and the driver keeping it.

use super::*;

impl Files {
    /// The place `agent`'s `collection` is kept at, when its machine keeps
    /// it off this machine (reached through the storage bridge its
    /// settings hand it, as `lnk agent start` does on every machine); none
    /// when it's in its files folder, or it has no settings yet. Its
    /// settings unreadable, an error: never guessed as here.
    pub(super) fn place_off(
        &self,
        agent: &Agent,
        collection: link_store_contract::storage::Collection,
    ) -> anyhow::Result<Option<String>> {
        let config = match self.config(agent) {
            Ok(config) => config,
            // No settings at all: nothing says it's off this machine.
            Err(err)
                if err
                    .root_cause()
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(err),
        };
        let Some(storage) = config.storage else {
            return Ok(None);
        };
        let places = storage.places;
        Ok(places
            .elsewhere(collection)
            .then(|| places.of(collection).to_string()))
    }

    /// The driver keeping `agent`'s conversations off this machine, and
    /// `agent` as it names it: none when they're in its files folder.
    pub(super) fn conversations_off(
        &self,
        agent: &Agent,
    ) -> anyhow::Result<
        Option<(
            Box<dyn link_store_contract::store::Store>,
            link_store_contract::store::Agent,
        )>,
    > {
        match self.place_off(
            agent,
            link_store_contract::storage::Collection::Conversations,
        )? {
            Some(place) => self.driver(agent, &place).map(Some),
            None => Ok(None),
        }
    }

    /// The storage bridge where `agent`'s `collection` is kept off this
    /// machine, and `agent` as it names it; none where it's in its files
    /// folder.
    pub(super) fn placed(
        &self,
        agent: &Agent,
        collection: link_store_contract::storage::Collection,
    ) -> anyhow::Result<
        Option<(
            link_store_contract::store::Bridged,
            link_store_contract::store::Agent,
        )>,
    > {
        if self.place_off(agent, collection)?.is_none() {
            return Ok(None);
        }
        let config = self.config(agent)?;
        let socket = config
            .storage
            .and_then(|s| s.socket)
            .within("its storage is off this machine, and it was given no bridge to it")?;
        let key = config
            .endpoint_key
            .within("no key to prove its storage calls with")?;
        let whose = link_store_contract::store::Agent {
            id: agent.id().to_string(),
            owner: agent.owner().to_string(),
        };
        let binding = link_sandbox_binding::Binding {
            identity: link_sandbox_binding::Identity {
                agent: whose.id.clone(),
                service: agent.service().to_string(),
                owner: whose.owner.clone(),
                teams: Vec::new(),
            },
            key,
        };
        Ok(Some((
            link_store_contract::store::Bridged { socket, binding },
            whose,
        )))
    }

    /// Store driver `name` (`lnk-<name>`), keeping what it keeps of
    /// `agent`'s in its `chats/`, and `agent` as the driver names
    /// it: through the agent's store bridge when Link runs one for that
    /// driver, each call proved with the agent's key, else run here.
    pub fn driver(
        &self,
        agent: &Agent,
        name: &str,
    ) -> anyhow::Result<(
        Box<dyn link_store_contract::store::Store>,
        link_store_contract::store::Agent,
    )> {
        let whose = link_store_contract::store::Agent {
            id: agent.id().to_string(),
            owner: agent.owner().to_string(),
        };
        // Its settings unread, it can't know whether a bridge keeps this
        // driver outside its sandbox: never guessed as none.
        let config = match self.bridged {
            true => Some(self.config(agent).within(
                "reading the agent's settings, which say whether its store bridge runs its driver",
            )?),
            false => None,
        };
        // Its conversations kept elsewhere, through the storage bridge it
        // was handed: the bridge runs the place's driver, the one asked
        // for or none.
        if let Some(config) = &config
            && config.storage.as_ref().is_some_and(|s| s.socket.is_some())
            && let Some(place) = self.place_off(
                agent,
                link_store_contract::storage::Collection::Conversations,
            )?
            && let Some((bridged, whose)) = self.placed(
                agent,
                link_store_contract::storage::Collection::Conversations,
            )?
        {
            anyhow::ensure!(
                place == name,
                "driver {} asked for, but its conversations are kept at {place}",
                link_format::clean(name)
            );
            return Ok((Box::new(bridged), whose));
        }
        if let Some(config) = config
            && let Some(at) = config.store.filter(|at| at.driver == name)
        {
            let key = config
                .endpoint_key
                .within("no key to prove its store calls with: lnk agent start gives one")?;
            let binding = link_sandbox_binding::Binding {
                identity: link_sandbox_binding::Identity {
                    agent: whose.id.clone(),
                    service: agent.service().to_string(),
                    owner: whose.owner.clone(),
                    teams: Vec::new(),
                },
                key,
            };
            let bridged = link_store_contract::store::Bridged {
                socket: at.socket,
                binding,
            };
            return Ok((Box::new(bridged), whose));
        }
        let paths = self.agents.paths(agent)?;
        let driver = link_store_contract::store::Driver::find(name, &paths.conversations)?;
        Ok((Box::new(driver), whose))
    }
}
