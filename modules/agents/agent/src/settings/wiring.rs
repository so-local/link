//! What the harness is handed: the model's ports and key wired through
//! the proxy, sealed variables and placeholders, and its settings.

use super::*;
use link_format::WithinExt;

/// A secret its proxy adds, as its route names it, and its value, read for
/// its placeholder and for `lnk agent harness check` to look for: never
/// on the route.
pub struct Wired {
    pub named: link_secret_contract::route::Named,
    pub value: link_secretive::Secret,
}

impl Settings {
    /// The saved model's explicit ID, else its effective spec's provider
    /// default, else Link's built-in default. Naming never pins a saved model.
    pub fn model_name(model: &Model, spec: &link_agent_spec::Resolved) -> Option<String> {
        model
            .name
            .clone()
            .or_else(|| spec.model_for(&model.provider).default)
            .or_else(|| model.named())
    }

    /// The local model's localhost port, if the model runs here.
    pub fn model_port(&self) -> Option<u16> {
        local_port(self.model.as_ref()?.base_url.as_deref()?)
    }

    /// The port its harness reaches its proxy on: in the sandbox's own
    /// network on Linux; on macOS, or with no sandbox, on this machine's
    /// loopback, one fixed per agent so the harness can be set up with it
    /// before it starts: the last of its block of ports.
    pub fn proxy_port(&self) -> u16 {
        // Where an owner's agents share one sandbox, each proxy is reached
        // at a port of its own there, as on a Mac.
        match self.port {
            Some(base) if self.proxy_is_here() => base + self.block() - 1,
            _ => link_sandbox_client::PROXY_PORT,
        }
    }

    /// Whether the proxy listens on this computer's loopback network.
    pub fn proxy_is_here(&self) -> bool {
        cfg!(target_os = "macos")
            || !self.sandbox
            || crate::machine_settings::shared_harness().unwrap_or(false)
    }

    /// The hosted model whose key the proxy adds on the model route,
    /// when it does: keys on the wire, the agent's harness behind its
    /// proxy, a provider the proxy knows at its own address. Its model,
    /// else, for a model on this machine or at an address of its own
    /// ([`address_of`]), its fallback: the route holds one key.
    pub fn wired_model(&self) -> anyhow::Result<Option<&Model>> {
        let enabled = self.keys_wired()?;
        let wired = |model: &&Model| {
            !model.api_key.is_empty()
                && address_of(model).is_none()
                && link_model_contract::route::PROVIDERS.contains(&model.provider.as_str())
        };
        Ok([self.model.as_ref(), self.fallback.as_ref()]
            .into_iter()
            .flatten()
            .find(wired)
            .filter(|_| enabled))
    }

    /// [`Self::wired_model`]'s key as the proxy adds it, with the
    /// placeholder its harness holds, of this start's
    /// [`Self::placeholders`].
    pub fn wire_key(&self) -> anyhow::Result<Option<link_model_contract::route::Hosted>> {
        use link_model_contract::route::HostedKey;
        let Some(model) = self.wired_model()? else {
            return Ok(None);
        };
        // Named, never the key: its bridge reads it from your secrets.
        let of = match self.model.as_ref().is_some_and(|m| std::ptr::eq(m, model)) {
            true => HostedKey::Model,
            false => HostedKey::Fallback,
        };
        Ok(Some(link_model_contract::route::Hosted::new(
            &self.placeholders()?,
            &model.provider,
            of,
            &model.api_key,
            false,
        )))
    }

    /// The keys of its model and fallback at addresses of their own
    /// ([`address_of`]), as its proxy adds them, when it adds keys: each
    /// a hosted route of its own, to that one host and its API's
    /// inference alone, its key read from your secrets as the agent's own
    /// at each call. Never the provider's route, which would send it to
    /// its provider's address. An error for an address the proxy can't
    /// hold a key to.
    pub fn address_keys(&self) -> anyhow::Result<Vec<link_model_contract::route::Hosted>> {
        use link_model_contract::route::{At, Hosted, HostedKey};
        if !self.keys_wired()? {
            return Ok(Vec::new());
        }
        let placeholders = self.placeholders()?;
        let mut keys = Vec::new();
        for (model, of) in [
            (self.model.as_ref(), HostedKey::Model),
            (self.fallback.as_ref(), HostedKey::Fallback),
        ] {
            let Some(model) = model.filter(|m| address_of(m).is_some()) else {
                continue;
            };
            if model.api_key.is_empty() {
                continue;
            }
            // The API it speaks there: a declared provider's, else its own.
            let speaks = link_model_contract::providers::declared(&model.provider)
                .map_or(model.provider.as_str(), |declared| declared.speaks.as_str());
            let address = address_of(model).expect("filtered on it");
            let at = At::new(speaks, address)
                .within(format!("the base_url of its {} model", model.provider))?;
            keys.push(Hosted::new(&placeholders, speaks, of, &model.api_key, false).at(at));
        }
        Ok(keys)
    }

    /// `model` as its harness is handed it at an address of its own, when
    /// its proxy adds its key ([`Self::address_keys`]): its route's
    /// address on the proxy in place of its own, and the placeholder.
    fn at_address_for_harness(
        &self,
        model: &mut Model,
        of: link_model_contract::route::HostedKey,
    ) -> anyhow::Result<()> {
        let Some(key) = self.address_keys()?.into_iter().find(|key| key.key == of) else {
            return Ok(());
        };
        model.base_url = key.url(self.proxy_port());
        model.api_key = key.placeholder;
        Ok(())
    }

    /// Typed model actions share the proved backend and the spec's model allow list.
    pub fn model_api(
        &self,
        model: &Model,
        fallback: bool,
    ) -> anyhow::Result<Option<link_model_contract::route::actions::Api>> {
        self.model_api_in(model, fallback, None)
    }

    fn model_api_in(
        &self,
        model: &Model,
        fallback: bool,
        spec: Option<&link_agent_spec::Resolved>,
    ) -> anyhow::Result<Option<link_model_contract::route::actions::Api>> {
        use link_model_contract::route::actions::{Api, Backend};
        if self.proxy_of(self.harness()).is_none() {
            return Ok(None);
        }
        let backend = if model.is_local() {
            let Some(port) = model.base_url.as_deref().and_then(local_port) else {
                return Ok(None);
            };
            Backend::Local { port }
        } else {
            if address_of(model).is_some()
                || !self.wired_model()?.is_some_and(|wired| {
                    wired.provider == model.provider && wired.api_key == model.api_key
                })
            {
                return Ok(None);
            }
            let Some(hosted) = self.wire_key()? else {
                return Ok(None);
            };
            Backend::Hosted { hosted }
        };
        let own;
        let spec = match spec {
            Some(spec) => spec,
            None => {
                own = self.spec()?;
                &own
            }
        };
        let opinions = spec.model_for(&model.provider);
        let Some(name) = Self::model_name(model, spec) else {
            return Ok(None);
        };
        let facts = link_model_contract::known(&name)?
            .map(|known| known.facts)
            .unwrap_or_default();
        Ok(Some(Api {
            transcribe: opinions.transcribe,
            window: spec
                .spec
                .window
                .as_ref()
                .and_then(|window| window.max)
                .or(facts.window),
            answer: opinions.answer.or(facts.answer),
            backend,
            name,
            allow: opinions.allow,
            fallback,
        }))
    }

    /// What its placeholders are made with for this start
    /// (`link_sandbox_route::Placeholders`): kept in its settings (600)
    /// so its harness's configuration and its proxy agree, and made anew
    /// at each start ([`Self::new_placeholders`]), so no placeholder
    /// outlives the run it was made for.
    pub fn placeholders(&self) -> anyhow::Result<link_sandbox_route::Placeholders> {
        let file = self.wires().join("placeholders.secret");
        match std::fs::File::open(&file) {
            Ok(mut opened) => {
                let kept =
                    link_secretive::Secret::read_line(&mut std::io::BufReader::new(&mut opened))
                        .within(format!("reading {}", file.display()))?
                        .filter(|kept| kept.expose().len() == 64)
                        .within(format!("{} isn't one Link made", file.display()))?;
                Ok(link_sandbox_route::Placeholders::from_kept(
                    link_secretive::OpaqueSecret::from_text(kept),
                ))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => self.new_placeholders(),
            Err(err) => Err(err).within(format!("reading {}", file.display())),
        }
    }

    /// New placeholders for a start, kept for [`Self::placeholders`].
    pub fn new_placeholders(&self) -> anyhow::Result<link_sandbox_route::Placeholders> {
        let made = link_sandbox_route::Placeholders::new()?;
        let file = self.wires().join("placeholders.secret");
        let kept = made.kept().keep().within("placeholders kept as text")?;
        link_files::create_private_dir(&self.wires())?;
        link_files::write_private(&file, kept.expose().as_bytes())
            .within(format!("writing {}", file.display()))?;
        Ok(made)
    }

    /// Why its fallback can't be handed to its harness, if it can't: with
    /// keys on the wire the proxy holds one key, so a fallback's must be
    /// that one (its model on this machine, or a hosted model of the
    /// same provider and key).
    pub fn fallback_unwired(&self) -> anyhow::Result<Option<String>> {
        let Some(fallback) = self.fallback.as_ref().filter(|f| address_of(f).is_none()) else {
            return Ok(None);
        };
        let Some(held) = self.wired_model()? else {
            return Ok(None);
        };
        Ok(
            (held.provider != fallback.provider || held.api_key != fallback.api_key).then(|| {
                format!(
                    "its fallback's key can't be kept on the wire beside its model's: its proxy \
                 holds one key. Fall back to a model of {}'s with the same key, or keep keys \
                 in the harness (lnk agent keys agent)",
                    held.provider
                )
            }),
        )
    }

    /// Its fallback as its harness is handed it: named as `model` is, and
    /// with keys on the wire, the proxy's address and a placeholder key.
    fn fallback_for_harness(
        &self,
        spec: &link_agent_spec::Resolved,
    ) -> anyhow::Result<Option<Model>> {
        let Some(mut fallback) = self.fallback.duplicate() else {
            return Ok(None);
        };
        fallback.actions_url = None;
        if let Some(why) = self.fallback_unwired()? {
            bail!("{why}");
        }
        let opinions = spec.model_for(&fallback.provider);
        fallback.name = Self::model_name(&fallback, spec);
        if let (Some(allow), Some(name)) = (&opinions.allow, &fallback.name) {
            ensure!(
                allow.contains(name),
                "its spec allows only {}: not its fallback {name} (lnk agent spec show)",
                allow.join(", ")
            );
        }
        speaking(&mut fallback);
        if address_of(&fallback).is_some() {
            self.at_address_for_harness(
                &mut fallback,
                link_model_contract::route::HostedKey::Fallback,
            )?;
        } else if let Some(key) = self.wire_key()? {
            fallback.base_url =
                link_model_contract::route::url(&fallback.provider, self.proxy_port());
            fallback.api_key = key.placeholder;
        }
        if self
            .model_api_in(
                self.fallback.as_ref().expect("fallback exists"),
                true,
                Some(spec),
            )?
            .is_some()
        {
            fallback.actions_url = Some(link_model_contract::route::actions::url(
                self.proxy_port(),
                &self.url_secret()?,
                true,
            ));
        }
        Ok(Some(fallback))
    }

    /// Whether the proxy adds the active harness's secrets: keys on the
    /// wire, and the harness behind its proxy.
    pub fn keys_wired(&self) -> anyhow::Result<bool> {
        let wired = match self.wire_keys {
            Some(wired) => wired,
            None => takes_proxy(self.harness())?,
        };
        Ok(wired && self.proxy_of(self.harness()).is_some())
    }

    /// The connections' secrets the proxy adds, when it does (keys on the
    /// wire, the harness behind its proxy), each by its connection's `rule`
    /// (`lnk-<connection> rule --json`, in `rules`; a connection without one,
    /// the harness holds its secret). An error for a connection whose rule
    /// wasn't asked: its secret never goes to the harness unasked. None
    /// for the connections in `carried`, which Link carries for the harness
    /// (`bridge::carried`): it's never handed them, so its proxy never
    /// holds their secrets.
    /// What `lnk secret add --to` gave it, as its proxy adds
    /// each (sealed, for its host alone): by its name, the proxy's secret
    /// with this start's placeholder. One kept plain is refused: an agent
    /// is given a secret sealed only, since what it runs is the model's to
    /// choose.
    pub fn sealed_variables(&self) -> anyhow::Result<Vec<(String, Wired)>> {
        let given = link_secret_contract::for_service(
            self.service_id()?,
            link_secret_contract::Which::Variables,
        )?;
        let placeholders = self.placeholders()?;
        let mut sealed = Vec::new();
        for given in given.into_iter().filter(|g| g.r#as == Use::Variable) {
            // A variable is one `lnk secret create` named.
            let Some(name) = given.name.clone() else {
                continue;
            };
            let Some(sealing) = &given.sealed else {
                bail!(
                    "{} is granted to {} plain, and an agent is given a secret sealed only: \
                     lnk secret create {} --internet-host <its host> seals it, then lnk secret add {} --to agent:{}",
                    name,
                    self.name,
                    name,
                    name,
                    self.name
                );
            };
            let proxy = link_secret_contract::proxy_name(&name);
            link_secret_contract::route::check_secret_name(&proxy)?;
            ensure!(
                !self.connections.contains_key(&proxy),
                "{} is called {proxy} on {}'s proxy, which its {proxy} connection is: name it \
                 another way (lnk secret create)",
                name,
                self.name
            );
            let named = link_secret_contract::route::Named::new(
                &placeholders,
                &proxy,
                link_secret_contract::route::Kept::Variable(name.clone()),
                &given.value,
                sealing.rule(),
            );
            sealed.push((
                name.clone(),
                Wired {
                    named,
                    value: given.value,
                },
            ));
        }
        Ok(sealed)
    }

    pub fn wire_secrets(&self, rules: &Rules, carried: &[String]) -> anyhow::Result<Vec<Wired>> {
        if !self.keys_wired()? {
            return Ok(Vec::new());
        }
        let mut secrets = Vec::new();
        let placeholders = self.placeholders()?;
        for (connection, entry) in &self.connections {
            if carried.contains(connection) {
                continue;
            }
            let section = &entry.settings;
            let Some(rule) = rules
                .get(connection)
                .within(format!("lnk-{connection} wasn't asked for its rule"))?
            else {
                continue;
            };
            let Some(value) = section.expose()[&rule.field]
                .as_str()
                .filter(|v| !v.is_empty())
            else {
                continue;
            };
            let value: link_secretive::Secret = value.to_string().into();
            let kept = link_secret_contract::route::Kept::Own(
                link_agent_identity::own::OwnSecret::Connection {
                    connection: connection.clone(),
                    field: rule.field.clone(),
                }
                .to_use()?,
            );
            secrets.push(Wired {
                named: link_secret_contract::route::Named::new(
                    &placeholders,
                    connection,
                    kept,
                    &value,
                    rule.wire.clone(),
                ),
                value,
            });
        }
        Ok(secrets)
    }

    /// What the active harness is given: with keys on the wire, a
    /// placeholder key and the proxy's address for its provider, and for
    /// each connection whose secret the proxy adds ([`Self::wire_secrets`]),
    /// a placeholder and the proxy's address for the connection.
    pub fn for_harness(
        &self,
        rules: &Rules,
    ) -> anyhow::Result<link_harness_contract::harness::Settings> {
        let Some(mut model) = self.model.duplicate() else {
            bail!("no model key yet: run lnk agent start");
        };
        // Named for every harness: its spec's default, never the
        // harness's own, and one its spec allows.
        model.actions_url = None;
        let spec = self.spec()?;
        let opinions = spec.model_for(&model.provider);
        model.name = Self::model_name(&model, &spec);
        if let (Some(allow), Some(name)) = (&opinions.allow, &model.name) {
            ensure!(
                allow.contains(name),
                "its spec allows only {}: not {name} (lnk agent spec show)",
                allow.join(", ")
            );
        }
        speaking(&mut model);
        // The key the proxy holds may be its fallback's, for a model here.
        let wired = address_of(&model).is_none()
            && self
                .wired_model()?
                .is_some_and(|w| w.provider == model.provider && w.api_key == model.api_key);
        if let Some(key) = self.wire_key()?.filter(|_| wired) {
            model.base_url = link_model_contract::route::url(&model.provider, self.proxy_port());
            model.api_key = key.placeholder;
        }
        // One at an address of its own reaches it on its own route.
        if address_of(&model).is_some() {
            self.at_address_for_harness(&mut model, link_model_contract::route::HostedKey::Model)?;
        }
        // A local model behind the proxy on macOS: on the proxy, for
        // inference only (Seatbelt can't filter the runtime's port; Linux
        // relays it in, filtered, at the same address). On Linux too where
        // an owner's agents share one sandbox, whose network is none of
        // theirs: each reaches it through its own proxy.
        if let (Some(port), Some(_), true) = (
            self.model_port(),
            self.proxy_of(self.harness()),
            cfg!(target_os = "macos") || crate::machine_settings::shared_harness().unwrap_or(false),
        ) {
            model.base_url = Some(link_model_contract::route::local_url(
                self.proxy_port(),
                port,
                &self.url_secret()?,
            ));
        }
        if self
            .model
            .as_ref()
            .map(|raw| self.model_api_in(raw, false, Some(&spec)))
            .transpose()?
            .flatten()
            .is_some()
        {
            model.actions_url = Some(link_model_contract::route::actions::url(
                self.proxy_port(),
                &self.url_secret()?,
                false,
            ));
        }
        let mut sections = Self::sections_of(&self.connections);
        for Wired { named: secret, .. } in self.wire_secrets(rules, &[])? {
            let Some(Some(rule)) = rules.get(&secret.name) else {
                continue;
            };
            let connection = secret.name.as_str();
            let section = &mut sections.expose_mut()[connection];
            section[&rule.field] = secret.placeholder.into_exposed().into();
            section[&rule.root] =
                link_secret_contract::route::url(self.proxy_port(), connection).into();
        }
        sections.expose_mut()["files"] = serde_json::to_value(self.files())?;
        sections.expose_mut()["model"] = serde_json::to_value(&model)?;
        let mut handed: link_harness_contract::harness::Settings =
            sections.expose().read().within("a connection's settings")?;
        handed.memory = self.memory_url();
        handed.agent = Some(self.folder().agent);
        // What Link and you know of the model, for any harness to read.
        if let Some(name) = &handed.model.name {
            handed.model_facts = link_model_contract::known(name)?;
        }
        handed.fallback = self.fallback_for_harness(&spec)?;
        if let Some(name) = handed.fallback.as_ref().and_then(|f| f.name.as_ref()) {
            handed.fallback_facts = link_model_contract::known(name)?;
        }
        handed.defaults = spec.links();
        // Who it is, for a harness keeping it in files of its own.
        handed.identity = Some(crate::identity::of(&spec.spec, &self.folder().agent)?);
        // What it remembers, for a harness writing it into its own files.
        handed.remembered = Some(crate::identity::remembered(&self.folder().agent)?);
        handed.spec = Some(spec.spec);
        // What it runs with here: its connections, and what its sandbox
        // file lets it do in its folder.
        handed.environment = Some(link_harness_contract::harness::Environment {
            connections: self.connections(),
            folders: crate::sandbox_file::of(&self.name).folders(),
            // Its `run`'s programs, named where it's defined.
            commands: Vec::new(),
            teams: self.team_places()?,
        });
        Ok(handed)
    }

    /// What a harness's `needs` reads (`link_harness_contract::harness::Needs`): its
    /// settings as `configure` gets them, but without the model's key or
    /// any connection's secret, since `needs` runs outside the sandbox.
    pub fn for_needs(&self) -> anyhow::Result<link_harness_contract::harness::Settings> {
        let mut model = self.model.duplicate().unwrap_or_default();
        let spec = self
            .model
            .as_ref()
            .filter(|model| model.name.is_none())
            .map(|_| self.spec())
            .transpose()?;
        if let Some(spec) = &spec {
            model.name = Self::model_name(&model, spec);
        }
        model.api_key = Secret::default();
        speaking(&mut model);
        if self
            .model
            .as_ref()
            .map(|raw| self.model_api_in(raw, false, spec.as_ref()))
            .transpose()?
            .flatten()
            .is_some()
        {
            model.actions_url = Some(link_model_contract::route::actions::url(
                self.proxy_port(),
                &self.url_secret()?,
                false,
            ));
        }
        let mut sections = Self::sections_of(&self.connections);
        for (connection, entry) in &self.connections {
            let section = &mut sections.expose_mut()[connection.as_str()];
            for field in &entry.about.secret_fields {
                if let Some(value) = section.get_mut(field) {
                    *value = serde_json::Value::String(String::new());
                }
            }
        }
        sections.expose_mut()["files"] = serde_json::to_value(self.files())?;
        sections.expose_mut()["model"] = serde_json::to_value(&model)?;
        sections.expose().read().within("a connection's settings")
    }

    /// What a harness is handed of `connections`, by name: each connection's
    /// settings, its token among them, held whole.
    fn sections_of(
        connections: &BTreeMap<String, Connection>,
    ) -> link_secretive::Secret<link_secretive::Json> {
        let mut sections: link_secretive::Secret<link_secretive::Json> =
            serde_json::Value::Object(Default::default()).into();
        for (connection, entry) in connections {
            sections.expose_mut()[connection.as_str()] =
                entry.settings.duplicate().into_exposed().0;
        }
        sections
    }

    /// [`Self::for_harness`] with keys off the wire and a model (tests).
    #[cfg(test)]
    pub fn for_harness_unwired(&mut self) -> link_harness_contract::harness::Settings {
        self.wire_keys = Some(false);
        self.model.get_or_insert_with(Model::default);
        self.for_harness(&Rules::new()).unwrap()
    }

    /// Where its harness reaches its memory: the memory's tool on the
    /// proxy, when it has a memory and its harness runs behind the proxy.
    pub fn memory_url(&self) -> Option<String> {
        let tool = self.memory_tool()?;
        self.proxy_of(self.harness())?;
        let secret = self.url_secret().ok()?;
        Some(link_sandbox_client::routes::tool_url(
            self.proxy_port(),
            &tool,
            &secret,
        ))
    }

    /// The secret its proxy's tool and local model URLs carry
    /// (`link_sandbox_client::Proxy::secret`): the agent's own, kept in
    /// its settings (600), made new at each start ([`Self::new_url_secret`])
    /// so the harness's configuration and its proxy agree for that start,
    /// and a URL a program read before is refused after.
    pub fn url_secret(&self) -> anyhow::Result<String> {
        let file = self.wires().join("url.secret");
        if let Ok(secret) = std::fs::read_to_string(&file)
            && link_sandbox_route::is_url_secret(secret.trim())
        {
            return Ok(secret.trim().to_string());
        }
        self.new_url_secret()
    }

    /// A new [`Self::url_secret`], in place of the one before: at each
    /// start, before its proxy and its harness's configuration are made.
    pub fn new_url_secret(&self) -> anyhow::Result<String> {
        let file = self.wires().join("url.secret");
        let secret = link_sandbox_route::url_secret()?;
        link_files::write_private(&file, secret.as_bytes())
            .within(format!("writing {}", file.display()))?;
        Ok(secret)
    }
}

/// The address of its own a hosted model is at, when it is at one: its
/// provider's declared address (`link`'s,
/// [`link_model_contract::providers`]), else its `base_url`, neither
/// empty nor on this machine.
pub fn address_of(model: &Model) -> Option<&str> {
    if let Some(declared) = link_model_contract::providers::declared(&model.provider) {
        return Some(&declared.base_url);
    }
    model
        .base_url
        .as_deref()
        .filter(|url| !url.is_empty() && !model.is_local() && local_port(url).is_none())
}

/// `model` as a harness speaks to it: one of a provider at an address of
/// its own (`link`) as the API it speaks there, at that address, so any
/// harness that speaks OpenAI's speaks to it.
fn speaking(model: &mut Model) {
    if let Some(declared) = link_model_contract::providers::declared(&model.provider) {
        model.provider = declared.speaks.clone();
        model.base_url = Some(declared.base_url.clone());
    }
}
