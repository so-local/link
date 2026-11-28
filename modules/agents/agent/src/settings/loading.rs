//! Settings found and kept: by name or this machine's, their secrets in
//! the keychain, saved readable only by the user, and peeked at without keys.

use super::*;
use link_agent_identity::own::OwnSecret;
use link_format::WithinExt;

impl Settings {
    /// The agent a command acts on: `name` (`-a`), which must be here,
    /// else [`Settings::here`].
    pub fn resolve(name: Option<&str>) -> anyhow::Result<Self> {
        Self::resolve_with(name, true)
    }

    /// [`Settings::resolve`] without the keys a Mac keeps in the Keychain,
    /// for a command that uses none ([`Settings::load_without_keys`]).
    pub fn resolve_without_keys(name: Option<&str>) -> anyhow::Result<Self> {
        Self::resolve_with(name, false)
    }

    fn resolve_with(name: Option<&str>, keys: bool) -> anyhow::Result<Self> {
        let Some(name) = name else {
            return Self::here_with(keys);
        };
        // An ID is written with its kind, `agent:<id>`, which no name has.
        let named;
        let name = match name.starts_with(&format!("{}:", super::ids::KIND)) {
            true => {
                named = Self::name_of(name)?;
                named.as_str()
            }
            false => name,
        };
        link_machine::machine::check_name(name)?;
        if !Self::path_of(name).exists() && !agent_dir(name).is_dir() {
            if let Some(note) = crate::moved::of_name(name)? {
                if note.to.cloud {
                    bail!(
                        "{name} runs on {}: lnk agent move here --agent {name} brings it back",
                        note.place()
                    );
                }
                bail!(
                    "{name} runs on {}: lnk agent move here --agent {name} brings it back, \
                     lnk box run {} -- agent -a {name} ... acts on it there",
                    note.place(),
                    note.to.name
                );
            }
            bail!(
                "no agent named {name} here: lnk agent list lists them, \
                 lnk agent create {name} makes it"
            );
        }
        Self::load_with(name, keys)
    }

    /// The name of the agent whose ID is `id`, `agent:<id>`: the agent
    /// here that has it, else the one a move took from here (its moved
    /// note), so a command naming it goes where a command naming it by
    /// its name does, to its placement or the place it moved to.
    pub fn name_of(id: &str) -> anyhow::Result<String> {
        let id = super::ids::parse(id)?;
        if let Some(name) = names()?
            .into_iter()
            .find(|name| super::id_of(name).as_deref() == Some(id.as_str()))
        {
            return Ok(name);
        }
        let note = crate::moved::of_id(&id)?.within(format!(
            "no agent here has the ID {}, and none moved from here: lnk agent list lists them",
            super::ids::written(&id)
        ))?;
        // Its name reaches it only while nothing else answers to that name:
        // no agent here has it now, and no later move of another agent
        // took it. Else the name is another agent's, never this one's.
        let namesake_here = Self::path_of(&note.name).exists() || agent_dir(&note.name).is_dir();
        let latest = crate::moved::of_name(&note.name)?.and_then(|n| n.id);
        if namesake_here || latest.as_deref() != Some(id.as_str()) {
            bail!(
                "{} moved to {}, and the name {} is another agent's now: act on it there",
                super::ids::written(&id),
                note.place(),
                note.name
            );
        }
        Ok(note.name)
    }

    /// This machine's only agent: none here, `lnk agent start` creates the
    /// first; several, `--agent` names one.
    pub fn here() -> anyhow::Result<Self> {
        Self::here_with(true)
    }

    fn here_with(keys: bool) -> anyhow::Result<Self> {
        match names()?.as_slice() {
            // None here: one that moved away is still the agent meant,
            // and its moved note, read next, says where it went.
            [] => match away()?.as_slice() {
                [] => bail!("no agent here yet: lnk agent start creates your first"),
                [one] => Ok(Self::new(one)),
                many => bail!(
                    "this machine's {} agents moved away ({}): name one with --agent <name>",
                    many.len(),
                    many.join(", ")
                ),
            },
            [one] => Self::load_with(one, keys),
            many => bail!(
                "this machine has {} agents ({}): name one with --agent <name>",
                many.len(),
                many.join(", ")
            ),
        }
    }

    /// Agent `name`'s settings; a new agent when it has none.
    pub fn load(name: &str) -> anyhow::Result<Self> {
        Self::load_with(name, true)
    }

    /// Agent `name`'s settings without the keys a Mac keeps in the
    /// Keychain (they're empty), for a command that names its bots but
    /// uses no key: no `security` run, unless its settings are from
    /// before the map of connections, moved once (a bot's id can be its
    /// token's first part).
    pub fn load_without_keys(name: &str) -> anyhow::Result<Self> {
        Self::load_with(name, false)
    }

    fn load_with(name: &str, keys: bool) -> anyhow::Result<Self> {
        link_machine::machine::check_name(name)?;
        let path = Self::path_of(name);
        let mut settings: Self = match read_spec_text(&path) {
            Ok(text) => link_files::saved::parse::<Self>(&text)
                .within(format!("reading {}", path.display()))?,
            // An existing agent's, its identity kept where no harness
            // reaches: never a folder its files make seen in a sandbox.
            Err(err) if missing(&err) && super::records::has_identity(name) => {
                let recreated = configuration::recreate(name)?;
                link_format::aside(link_format::Note(link_format::sentence!(
                    "Recreated the missing agent configuration with defaults: {}.",
                    link_format::Path::new(&path)
                )));
                recreated
            }
            Err(err) if missing(&err) => return Ok(Self::new(name)),
            Err(err) => return Err(err).within(format!("reading {}", path.display())),
        };
        settings.name = name.to_string();
        // Every agent here has its ID, reserved when it was created, in
        // this machine's records: never one made up now, which another
        // agent may have had.
        super::records::apply(&mut settings)?;
        ensure!(
            settings.id.is_some(),
            "{} has no identity here ({} is missing): lnk agent create makes an agent",
            path.display(),
            super::records::identity_path(name).display()
        );
        // A known agent's deleted sandbox file is recreated closed.
        if crate::sandbox_file::recreate(name)? {
            link_format::aside(link_format::Report::warning(link_format::sentence!(
                "Recreated the missing sandbox file with closed permissions: {}.",
                link_format::Path::new(crate::sandbox_file::path(name))
            )));
        }
        // Its keys are your secrets' (`link_secret_contract`), never this
        // file's.
        for u in settings.uses() {
            settings.set_key(&u, Secret::default());
        }
        if keys || !link_keychain::available() {
            let given = link_secret_contract::for_service(
                settings.service_id()?,
                link_secret_contract::Which::Own,
            )?;
            for given in given {
                if given.r#as != Use::Variable && settings.key_of(&given.r#as).is_some() {
                    settings.set_key(&given.r#as, given.value);
                }
            }
            settings.own_kept = settings.own();
        } else {
            settings.keys_left = true;
        }
        Ok(settings)
    }

    /// The credentials this agent holds here, as `lnk secret list` shows
    /// them, but those your secrets keep for it (its models' keys, its
    /// connections' tokens), which the secret module lists: its key on its
    /// bridges.
    pub fn held(&self) -> Vec<link_core::uninstall::Held> {
        use link_core::uninstall::Held;
        let name = &self.name;
        let mut held = Vec::new();
        let keyring = link_sandbox_binding::Keyring::here();
        let bound = keyring.dir().join(format!("{}.json", self.key()));
        if bound.exists() {
            held.push(Held {
                what: link_core::uninstall::CredentialKind::AgentKey.into(),
                of: format!("{name}'s, while it runs"),
                kept: format!(
                    "{} (only yours, hidden from every sandbox)",
                    bound.display()
                ),
                may: format!("Proves {name}'s calls to its bridges, as {name} alone"),
            });
        }
        held
    }

    /// What it uses keys for, each it has a section for: its model, its
    /// fallback, each connection's secret fields, and each webhook's secret
    /// (and the one before it, while it rolls).
    fn uses(&self) -> Vec<Use> {
        let mut owns = vec![OwnSecret::Model, OwnSecret::Fallback];
        for (connection, about) in &self.connections {
            for field in &about.about.secret_fields {
                owns.push(OwnSecret::Connection {
                    connection: connection.clone(),
                    field: field.clone(),
                });
            }
        }
        for (name, webhook) in &self.webhooks {
            owns.push(OwnSecret::Webhook {
                name: name.clone(),
                previous: false,
            });
            if webhook.previous_until.is_some() {
                owns.push(OwnSecret::Webhook {
                    name: name.clone(),
                    previous: true,
                });
            }
        }
        // A name the store's purposes can't hold has no key kept.
        let mut uses: Vec<Use> = owns.iter().filter_map(|o| o.to_use().ok()).collect();
        uses.retain(|u| self.key_of(u).is_some());
        uses
    }

    /// Its keys as they are now, each it has, by what it uses them for:
    /// what your secrets keep for it.
    fn own(&self) -> BTreeMap<Use, Secret> {
        self.uses()
            .into_iter()
            .filter_map(|u| self.key_of(&u).filter(|k| !k.is_empty()).map(|k| (u, k)))
            .collect()
    }

    /// The key it uses as `use`, where its section is.
    fn key_of(&self, r#use: &Use) -> Option<Secret> {
        match OwnSecret::of(r#use)? {
            OwnSecret::Model => self.model.as_ref().map(|m| m.api_key.duplicate()),
            OwnSecret::Fallback => self.fallback.as_ref().map(|m| m.api_key.duplicate()),
            OwnSecret::Connection { connection, field } => {
                self.connections.get(&connection).map(|c| {
                    c.settings.expose()[&field]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                        .into()
                })
            }
            OwnSecret::Webhook { name, previous } => {
                self.webhooks.get(&name).map(|w| match previous {
                    false => w.secret.duplicate(),
                    true => w.previous.duplicate(),
                })
            }
        }
    }

    /// Puts `key` where it's used as `use`, where its section is.
    fn set_key(&mut self, r#use: &Use, key: Secret) {
        let Some(own) = OwnSecret::of(r#use) else {
            return;
        };
        match own {
            OwnSecret::Model => {
                if let Some(model) = &mut self.model {
                    model.api_key = key;
                }
            }
            OwnSecret::Fallback => {
                if let Some(model) = &mut self.fallback {
                    model.api_key = key;
                }
            }
            OwnSecret::Connection { connection, field } => {
                if let Some(section) = self
                    .connections
                    .get_mut(&connection)
                    .and_then(|c| c.settings.expose_mut().as_object_mut())
                {
                    // Where the connection's module reads it.
                    section.insert(field, key.expose_owned().into());
                }
            }
            OwnSecret::Webhook { name, previous } => {
                if let Some(webhook) = self.webhooks.get_mut(&name) {
                    match previous {
                        false => webhook.secret = key,
                        true => webhook.previous = key,
                    }
                }
            }
        }
    }

    /// Takes back what your secrets keep for this agent, when it leaves
    /// this machine or is removed (`link_secret_contract::forget`): an
    /// error when they stay, for the caller to report or refuse on. An
    /// agent acting as no service yet has none.
    pub fn forget_keys(&self) -> anyhow::Result<()> {
        let Some(service) = self.service.as_deref() else {
            return Ok(());
        };
        link_secret_contract::forget(service)
    }

    /// [`Settings::forget_keys`], what stays said as a warning: for a
    /// removal that goes on whatever its keys do.
    pub fn forget_keys_or_warn(&self) {
        if let Err(err) = self.forget_keys() {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "{}'s keys stay in your secrets.",
                    link_format::Name::new("agent", &self.name)
                ))
                .why(link_format::Sentence::adopt(&format!("{err:#}"))),
            );
        }
    }

    /// Saves the settings. A new agent here gets its ID then (unless it
    /// came with one), a block of ports of its own, and this machine's
    /// default route unless it has one ([`Defaults`]).
    pub fn save(&mut self) -> anyhow::Result<()> {
        link_settings::folders::need()?;
        let path = self.path();
        let known = path.exists() && self.id.is_some();
        if !known {
            crate::sandbox_file::initialize(&self.name)?;
        }
        if !known {
            // Its ID was reserved when it was created (`lnk agent create`),
            // imported or migrated.
            ensure!(
                self.id.is_some(),
                "agent {} has no ID: lnk agent create {} makes it",
                self.name,
                self.name
            );
            // Every first save holds its ID in the ledger, however it came
            // (created, defined in a team, migrated, moved, restored):
            // refused when it's retired here or another agent's.
            if let Some(id) = self.agent_id() {
                super::ids::arrive(&self.name, id)?;
            }
            // Its service is minted with it, or comes with it.
            match &self.service {
                Some(service) => super::ids::SERVICES.arrive(&self.name, service)?,
                None => self.service = Some(super::ids::SERVICES.reserve()?),
            }
            if self.owner.is_none() {
                self.owner = Some(link_core::owner::current_owner()?);
            }
            self.take_ports_here()?;
            if self.upstream.is_none() {
                self.upstream = Defaults::load()?.upstream;
            }
        }
        let mut text = toml::Value::try_from(&*self)?;
        // Its keys are your secrets', never this file's.
        for r#use in self.uses() {
            let (table, field) = match OwnSecret::of(&r#use) {
                Some(OwnSecret::Model) => (text.get_mut("model"), "api_key".to_string()),
                Some(OwnSecret::Fallback) => (text.get_mut("fallback"), "api_key".to_string()),
                Some(OwnSecret::Connection { connection, field }) => (
                    text.get_mut("connections")
                        .and_then(|t| t.get_mut(&connection))
                        .and_then(|t| t.get_mut("settings")),
                    field,
                ),
                // Never in this file: its secrets are your secrets' alone.
                Some(OwnSecret::Webhook { .. }) | None => continue,
            };
            if let Some(table) = table.and_then(|t| t.as_table_mut()) {
                table.remove(&field);
            }
        }
        // Loaded without them, its keys stay as your secrets hold them,
        // but for what it no longer has (a connection disconnected).
        let agent = self.secret_agent()?;
        if self.keys_left {
            let kept = self.uses().into_iter().map(|u| (u, None)).collect();
            link_secret_contract::keep_own(&agent, kept)?;
        } else {
            let own = self.own();
            if own != self.own_kept {
                let kept = own
                    .iter()
                    .map(|(u, k)| (u.clone(), Some(k.duplicate())))
                    .collect();
                link_secret_contract::keep_own(&agent, kept)?;
                self.own_kept = own;
            }
        }
        let old = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err.into()),
        };
        // Its records first, so a save cut short leaves an agent with its
        // identity, whose missing settings are recreated.
        super::records::keep(self)?;
        let written = configuration::with_setup(&old, &text)?;
        link_files::write_private(&path, written.as_bytes())
            .within(format!("writing {}", path.display()))?;
        // Made here now: laid out as two folders from the start.
        if !known {
            crate::layout::lay_out(self)?;
        }
        Ok(())
    }
}

impl Settings {
    /// An agent whose files arrived here in its folders (an archive's
    /// import) adopted as a new agent: a new ID and service, this
    /// machine's owner, a block of ports and route, and its records kept,
    /// from files alone. Its Home becomes its own folder's, since a Home
    /// elsewhere isn't carried. What to tell you of it.
    pub fn adopt(name: &str, was: &str) -> anyhow::Result<(Self, Vec<String>)> {
        link_machine::machine::check_name(name)?;
        check_new_name("agents", name)?;
        ensure!(
            !super::records::has_identity(name),
            "an agent named {name} is here already"
        );
        let path = Self::path_of(name);
        let text = read_spec_text(&path)?;
        let mut settings: Self = link_files::saved::parse::<Self>(&text)
            .within(format!("reading {}", path.display()))?;
        settings.name = name.to_string();
        let mut notes = Vec::new();
        let own = home(name);
        let theirs = settings.files.clone();
        let was_own = |files: &Path| {
            files.ends_with(Path::new(was).join("home"))
                && files
                    .parent()
                    .and_then(Path::parent)
                    .is_some_and(|agents| agents.ends_with("Agents"))
        };
        if let Some(files) = theirs.filter(|files| !was_own(files)) {
            notes.push(format!(
                "Its Home was {} where it came from, which an archive doesn't carry: it's {} \
                 until lnk agent start --files names another",
                files.display(),
                own.display()
            ));
        }
        settings.files = Some(own);
        // Its sandbox is on here, whatever it was where it came from.
        if !settings.sandbox {
            settings.sandbox = true;
            notes.push(format!(
                "It ran without its sandbox where it came from: it runs in it here unless lnk \
                 agent start --agent {name} --no-sandbox turns it off"
            ));
        }
        // Where its traffic went is the machine's it came from: here, it's
        // this machine's default.
        settings.upstream = Defaults::load()?.upstream;
        // A webhook's secret never comes with it, so its webhooks are made
        // anew here.
        for webhook in std::mem::take(&mut settings.webhooks).into_keys() {
            notes.push(format!(
                "Its webhook {webhook} wasn't carried, since its secret never is: lnk agent \
                 webhook create {webhook} --agent {name} makes it here"
            ));
        }
        let written = configuration::with_setup(&text, &toml::Value::try_from(&settings)?)?;
        link_files::write_private(&path, written.as_bytes())
            .within(format!("writing {}", path.display()))?;
        crate::layout::lay_out(&settings)?;
        // Its records last: until then it isn't an agent here, so one that
        // failed is no ghost in the list.
        let id = super::ids::reserve()?;
        super::ids::arrive(name, &id)?;
        settings.id = Some(super::ids::workload(&id)?);
        settings.service = Some(super::ids::SERVICES.reserve()?);
        settings.owner = Some(link_core::owner::current_owner()?);
        settings.take_ports_here()?;
        super::records::keep(&settings)?;
        Ok((settings, notes))
    }
}

fn missing(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|err| err.kind() == std::io::ErrorKind::NotFound)
    })
}

/// What of an agent's settings needs no key, read without the Keychain.
#[derive(Default)]
pub struct Peek {
    /// [`Settings::block`].
    pub block: Option<u16>,
    /// [`Settings::port`].
    pub port: Option<u16>,
    /// [`Settings::webhook_port`].
    pub webhook_port: Option<u16>,
}

/// Agent `name`'s [`Peek`]; nothing when it has no records here.
pub fn peek(name: &str) -> anyhow::Result<Peek> {
    link_machine::machine::check_name(name)?;
    let here = super::records::here(name)?;
    Ok(Peek {
        block: here.block,
        port: here.port,
        webhook_port: here.webhook_port,
    })
}

/// The names of this machine's agents (each folder in the Link folder's
/// `Agents` holding settings), sorted.
/// The agents that moved away from this machine, by name, none of them
/// here again: each one's latest moved note says where it went.
pub fn away() -> anyhow::Result<Vec<String>> {
    let here = names()?;
    let mut away: Vec<String> = crate::moved::notes()?
        .into_iter()
        .map(|note| note.name)
        .filter(|name| !here.contains(name))
        .collect();
    away.sort();
    away.dedup();
    Ok(away)
}

pub fn names() -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for dir in [agents(), link_files::layout::Home::AgentSettings.path()] {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).within(format!("reading {}", dir.display())),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir()
            && (configuration_of(&entry.file_name().to_string_lossy()).join(SETTINGS_FILE).is_file()
                || path.join("home").is_dir()
                || path.file_name().and_then(|name| name.to_str()).is_some_and(super::records::has_identity))
            && let Some(name) = path.file_name().and_then(|s| s.to_str())
            // One named before Link's words were refused (`home`) too: its
            // ports and ID are its own still.
            && link_machine::machine::check_shape(name).is_ok()
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
