//! How specs meet: Link's, then its teams', then yours ([`resolve`]),
//! and what the union reads back ([`Resolved`]).

use super::*;

/// What Link knows of the agent here, beneath its own file: its name.
/// Whose it is and its teams are its saved setup's and its teams' files',
/// never a spec's.
#[derive(Clone, Debug, Default)]
pub struct Known {
    pub name: String,
}

/// Managed members take their own definition directly from the canonical
/// team file. Their standalone configuration remains intact for leaving
/// the team. Permanent identity selects a definition, never a display name.
pub fn resolve_member(
    own: Option<&Spec>,
    teams: &[Team],
    known: &Known,
    agent: &str,
) -> anyhow::Result<Resolved> {
    let mut definitions = teams
        .iter()
        .filter_map(|team| {
            team.member(agent)
                .and_then(|(_, member)| member.agent.as_ref())
                .map(|spec| (team, spec))
        })
        .collect::<Vec<_>>();
    definitions.sort_by(|(a, _), (b, _)| a.id.cmp(&b.id));
    let Some((team, definition)) = definitions.first() else {
        return resolve(own, teams, known);
    };
    ensure!(
        definitions.iter().all(|(_, other)| *other == *definition),
        "two teams define this managed member differently"
    );
    let mut definition = (*definition).clone();
    // The roster's name is an address within that team, not a rename of
    // the agent's permanent identity on this machine.
    definition.name = None;
    let mut resolved = resolve(Some(&definition), teams, known)?;
    for (key, from) in &mut resolved.from {
        if matches!(from, From::Yours) && key != "name" {
            *from = From::Team(team.name.clone());
        }
    }
    Ok(resolved)
}

/// The spec agent `known` runs: Link's (unless `own` says `extends =
/// "none"`), then each of `teams` (by name), then `own`, each key the
/// last set's that holds it. An error naming the key when the union
/// lacks one of [`REQUIRED`], when two teams set one key differently
/// and `own` doesn't set it, or when `own` names another agent than Link
/// knows.
pub fn resolve(own: Option<&Spec>, teams: &[Team], known: &Known) -> anyhow::Result<Resolved> {
    let own = own.cloned().unwrap_or_default();
    if let Some(name) = &own.name {
        ensure!(
            *name == known.name,
            "the spec is {name}'s, not {}'s",
            known.name
        );
    }
    let mut sets: Vec<(From, BTreeMap<String, toml::Value>)> = Vec::new();
    if own.extends != Some(Extends::None) {
        sets.push((From::Link, leaves(&link())?));
    }
    let mut teams: Vec<&Team> = teams.iter().collect();
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    for team in &teams {
        super::teams::check_opinions(&team.settings.agent)
            .within(format!("team {}", link_format::clean(&team.name)))?;
        sets.push((From::Team(team.name.clone()), leaves(&team.settings.agent)?));
    }
    let mut mine = leaves(&own)?;
    mine.remove("extends");
    mine.insert("name".into(), toml::Value::String(known.name.clone()));
    // Two teams that disagree on a key the agent leaves to them.
    for (i, (a_from, a)) in sets.iter().enumerate() {
        for (b_from, b) in &sets[i + 1..] {
            let (From::Team(a_name), From::Team(b_name)) = (a_from, b_from) else {
                continue;
            };
            for (key, value) in a {
                if b.get(key).is_some_and(|other| other != value) && !mine.contains_key(key) {
                    bail!(
                        "teams {a_name} and {b_name} set {key} differently: set it in the \
                         agent's own spec"
                    );
                }
            }
        }
    }
    sets.push((From::Yours, mine));
    let mut union: BTreeMap<String, toml::Value> = BTreeMap::new();
    let mut from: BTreeMap<String, From> = BTreeMap::new();
    for (whose, set) in sets {
        // A general key shadows the specific ones of the sets before
        // (your `[model] default`, Link's `[model.anthropic] default`).
        union.retain(|key, _| {
            set.contains_key(key) || general(key).is_none_or(|general| !set.contains_key(&general))
        });
        from.retain(|key, _| union.contains_key(key));
        for (key, value) in set {
            union.insert(key.clone(), value);
            from.insert(key, whose.clone());
        }
    }
    let spec: Spec = toml::Value::Table(unflatten(&union))
        .try_into()
        .map_err(|e: toml::de::Error| anyhow::anyhow!("{e}"))?;
    let assembled = spec.instruction_source() == Source::Link;
    for key in REQUIRED
        .iter()
        .filter(|key| assembled || !key.starts_with("instructions."))
    {
        ensure!(
            union.contains_key(*key),
            "no spec sets {key}: the agent's own spec must, when it extends none"
        );
    }
    check(&spec)?;
    Ok(Resolved { spec, from })
}

/// What `spec` (a union) sets past what any of Link's tests run
/// ([`UNUSUAL`]), each said: allowed, as the spec's to say, but likely a
/// typo or a costly turn.
pub fn unusual(spec: &Spec) -> Vec<String> {
    let scheduler = spec.tool("scheduler");
    let turns = spec.turns.clone().unwrap_or_default();
    let values = [
        turns.rounds,
        turns.calls,
        scheduler.per_day,
        scheduler.subagents,
        scheduler.hops,
    ];
    UNUSUAL
        .iter()
        .zip(values)
        .filter_map(|((key, most), value)| {
            let value = value.filter(|value| value > most)?;
            Some(format!(
                "{key} is {value}, past the {most} Link is tested with"
            ))
        })
        .collect()
}

/// The general key `key` is a specific one of: `model.<provider>.<key>`'s
/// `model.<key>`, `connection.<name>.<key>`'s `connections.<key>`,
/// `tool.<name>.<key>`'s `tools.<key>`.
fn general(key: &str) -> Option<String> {
    let parts: Vec<&str> = key.split('.').collect();
    let [table, _, leaf] = parts.as_slice() else {
        return None;
    };
    let general = match *table {
        "model" => "model",
        "connection" => "connections",
        "tool" => "tools",
        _ => return None,
    };
    Some(format!("{general}.{leaf}"))
}

/// Every value of `spec`, by its dotted key (`turns.rounds`); a list is
/// one value, whole.
fn leaves(spec: &Spec) -> anyhow::Result<BTreeMap<String, toml::Value>> {
    let value = toml::Value::try_from(spec)?;
    let mut out = BTreeMap::new();
    fn walk(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, toml::Value>) {
        match value {
            toml::Value::Table(table) => {
                for (key, value) in table {
                    let path = match prefix.is_empty() {
                        true => key.clone(),
                        false => format!("{prefix}.{key}"),
                    };
                    walk(&path, value, out);
                }
            }
            other => {
                out.insert(prefix.to_string(), other.clone());
            }
        }
    }
    walk("", &value, &mut out);
    Ok(out)
}

/// `leaves` as tables again.
fn unflatten(leaves: &BTreeMap<String, toml::Value>) -> toml::Table {
    let mut root = toml::Table::new();
    for (key, value) in leaves {
        let mut table = &mut root;
        let parts: Vec<&str> = key.split('.').collect();
        for part in &parts[..parts.len() - 1] {
            table = match table
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            {
                toml::Value::Table(inner) => inner,
                _ => unreachable!("a key is a value or a table, never both"),
            };
        }
        table.insert(parts[parts.len() - 1].to_string(), value.clone());
    }
    root
}

impl Resolved {
    /// [`Spec::model_for`] of the union.
    pub fn model_for(&self, provider: &str) -> Model {
        self.spec.model_for(provider)
    }

    /// [`Spec::tool`] of the union.
    pub fn tool(&self, name: &str) -> Tool {
        self.spec.tool(name)
    }

    /// [`Spec::connection`] of the union.
    pub fn connection(&self, name: &str) -> Connection {
        self.spec.connection(name)
    }

    /// The union as `lnk agent spec show` prints it: a spec file's lines,
    /// each key's with whose it is.
    pub fn lines(&self) -> anyhow::Result<Vec<(String, Option<String>)>> {
        let text = link_spec::print_spec(&self.spec)?;
        let mut out = Vec::new();
        let mut table = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(header) = trimmed.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
                table = header.trim().replace('"', "");
                out.push((line.to_string(), None));
                continue;
            }
            let key = trimmed.split('=').next().unwrap_or_default().trim();
            let path = match table.is_empty() {
                true => key.to_string(),
                false => format!("{table}.{key}"),
            };
            let whose = match key.is_empty() || trimmed.starts_with('#') {
                true => None,
                false => self.from.get(&path).map(|whose| whose.mark().to_string()),
            };
            out.push((line.to_string(), whose));
        }
        Ok(out)
    }

    /// [`Self::lines`] as one text, a key's owner after a `#`.
    pub fn print(&self) -> anyhow::Result<String> {
        let mut out = String::new();
        for (line, whose) in self.lines()? {
            match whose {
                Some(whose) => out.push_str(&format!("{line}  # {whose}\n")),
                None => out.push_str(&format!("{line}\n")),
            }
        }
        Ok(out)
    }

    /// The spec as it runs on a machine whose places are `places`
    /// ([`crate::storage::Supported::resolve`]): each collection's place
    /// where it's kept, a key the machine overrode marked its, naming the
    /// place the agent prefers.
    pub fn on_machine(mut self, places: &crate::storage::Places) -> Self {
        use crate::storage::Collection;
        let storage = self.spec.storage.get_or_insert_with(Default::default);
        for collection in Collection::ALL {
            let preferred = storage.place(collection);
            let place = places.of(collection).to_string();
            let key = format!("storage.{}", collection.name());
            if places.0.get(&collection).is_some_and(|p| p.overridden) {
                self.from.insert(key, From::Machine(preferred));
            } else {
                // Left out of every spec: Link's place, `file`.
                self.from.entry(key).or_insert(From::Link);
            }
            let slot = match collection {
                Collection::Home => &mut storage.home,
                Collection::Instructions => &mut storage.instructions,
                Collection::Skills => &mut storage.skills,
                Collection::Memory => &mut storage.memory,
                Collection::Conversations => &mut storage.conversations,
                Collection::Schedule => &mut storage.schedule,
            };
            *slot = Some(place);
        }
        self
    }

    /// The keys that are Link's spec's: set by no
    /// team and not by you (`Settings::defaults`), so a harness tells
    /// Link's defaults from your choices.
    pub fn links(&self) -> Vec<String> {
        self.from
            .iter()
            .filter(|(_, whose)| matches!(whose, From::Link))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// The opinions a harness is handed, as JSON (`Settings::spec`).
    pub fn json(&self) -> serde_json::Value {
        serde_json::to_value(&self.spec).unwrap_or_default()
    }
}
