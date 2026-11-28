//! Agent-specific fakes used by subject suites.
use std::path::Path;

/// A stand-in bridge for tests, listening at `socket` for good, on the
/// server every bridge runs on ([`link_sandbox_binding::serve`]): each call
/// proved as an agent of the keyring in `keys`, answered with `answer` of
/// who called and its action as one JSON line; one not proved, with the
/// error envelope.
pub fn stand_in_bridge<A, R>(
    socket: &Path,
    keys: &Path,
    answer: impl Fn(link_sandbox_binding::Identity, A) -> R + Send + Sync + 'static,
) where
    A: serde::de::DeserializeOwned + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    let listener = std::os::unix::net::UnixListener::bind(socket).expect("listening for a test");
    let keys: std::sync::Arc<dyn link_sandbox_binding::Keys> =
        std::sync::Arc::new(link_sandbox_binding::Keyring::at(keys));
    let limits = link_sandbox_binding::Limits {
        name: "the stand-in bridge",
        calls: 64,
        per_agent: 64,
        per_owner: 64,
        action_max: link_sandbox_binding::CALL_MAX,
        action_wait: std::time::Duration::from_secs(30),
        answer_wait: std::time::Duration::from_secs(30),
    };
    std::thread::spawn(move || {
        let never = std::sync::atomic::AtomicBool::new(false);
        link_sandbox_binding::serve(listener, keys, &limits, &never, move |who, action: A| {
            Ok(answer(who.clone(), action))
        })
    });
}

/// The service a fixture's agent `id` acts as, when its settings name
/// none: the fixture's stand-in for the one Link mints with an agent,
/// told from the ID alone so a test can name it (its characters
/// reversed, never the agent's own ID).
pub fn service_of(id: &str) -> String {
    id.chars().rev().collect()
}

/// The fields of an agent's settings this machine keeps in its records,
/// never in `agent.toml`.
pub const RECORDS: &[&str] = &[
    "id",
    "service",
    "owner",
    "name",
    "port",
    "block",
    "webhook_port",
    "stopped_services",
];

/// Agent `name`'s record `file` in `home` (`identity.toml`, `here.toml`),
/// as this machine keeps it, its values as JSON's for a test to compare:
/// empty when it has none.
pub fn record(home: &Path, name: &str, file: &str) -> serde_json::Map<String, serde_json::Value> {
    let path = link_files::layout::Hidden::Agents
        .in_home(home)
        .join(name)
        .join(file);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let table: toml::Table = text.parse().unwrap();
            match serde_json::to_value(table).unwrap() {
                serde_json::Value::Object(map) => map,
                _ => unreachable!("a table is an object"),
            }
        }
        Err(_) => serde_json::Map::new(),
    }
}

/// `record` written as this machine keeps it, as TOML naming its schema.
fn write_record(folder: &Path, file: &str, mut record: serde_json::Map<String, serde_json::Value>) {
    record.insert("schema".into(), 1.into());
    let table = toml::Value::try_from(serde_json::Value::Object(record)).unwrap();
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join(file), toml::to_string(&table).unwrap()).unwrap();
}

/// Sets `field` of agent `name`'s record `file` in `home` to `value`,
/// keeping the rest, as a fixture changes what this machine keeps.
pub fn set_record(home: &Path, name: &str, file: &str, field: &str, value: serde_json::Value) {
    let mut record = record(home, name, file);
    record.insert(field.into(), value);
    let folder = link_files::layout::Hidden::Agents.in_home(home).join(name);
    write_record(&folder, file, record);
}

/// Writes agent `name`'s settings `text` in `home` as Link keeps them:
/// its keys (its model's and fallback's `api_key`, each connection's secret
/// fields)
/// taken out into your secrets (`~/.lnk/safe/secrets/secrets.toml`), each
/// under an ID of its own and granted to the service the agent acts as,
/// `service:<id>` (its settings', else [`service_of`] its ID), its ID
/// `agent:<id>` (a new one, when `text` names none),
/// its records (`id`, `service`, `owner`, `name`, its ports and
/// `stopped_services`, when `text` gives them) in this machine's
/// `~/.lnk/safe/agents/<name>/`, and the rest in
/// `~/Link/Safe/Agents/<name>/agent.toml` as written.
pub fn write_agent(home: &Path, name: &str, text: &str) {
    use link_agent_identity::own::OwnSecret;
    use link_secret_contract::Use;
    let mut settings: toml::Table = text.parse().unwrap();
    // Every agent's settings name its ID, `agent:<id>`: the one `text`
    // gives (with its kind or without), else a new one.
    let id = match settings.get("id").and_then(|i| i.as_str()) {
        Some(id) => id.strip_prefix("agent:").unwrap_or(id).to_string(),
        None => link_id::candidate().unwrap(),
    };
    let service = match settings.get("service").and_then(|s| s.as_str()) {
        Some(service) => service
            .strip_prefix("service:")
            .unwrap_or(service)
            .to_string(),
        None => service_of(&id),
    };
    // This machine's records, never `agent.toml`'s: its identity, and
    // what this machine gave it.
    let mut identity = serde_json::Map::new();
    identity.insert("id".into(), id.clone().into());
    identity.insert("service".into(), service.clone().into());
    // Its owner as `text` writes it, saved by its kind.
    if let Some(owner) = settings.get("owner").and_then(|o| o.as_str()) {
        let owner = link_machine::owner::Owner::of(owner).unwrap();
        identity.insert("owner".into(), serde_json::to_value(owner).unwrap());
    }
    let mut here = serde_json::Map::new();
    for field in ["port", "block", "webhook_port"] {
        if let Some(port) = settings.get(field).and_then(|p| p.as_integer()) {
            here.insert(field.into(), port.into());
        }
    }
    if let Some(kept) = settings.get("stopped_services").and_then(|k| k.as_array()) {
        let kept: Vec<serde_json::Value> = kept
            .iter()
            .filter_map(|k| k.as_str().map(|k| k.into()))
            .collect();
        here.insert("stopped_services".into(), kept.into());
    }
    for field in RECORDS {
        settings.remove(*field);
    }
    // Saved as Link saves it, naming its schema when `text` names none.
    settings
        .entry("schema")
        .or_insert_with(|| toml::Value::Integer(1));
    let records = link_files::layout::Hidden::Agents.in_home(home).join(name);
    for (file, record) in [("identity.toml", identity), ("here.toml", here)] {
        if file == "here.toml" && record.is_empty() {
            continue;
        }
        write_record(&records, file, record);
    }
    let mut keys: Vec<(Use, String)> = Vec::new();
    let mut take = |table: Option<&mut toml::Value>, field: &str, r#use: Use| {
        if let Some(value) = table
            .and_then(|t| t.as_table_mut())
            .and_then(|t| t.remove(field))
            .and_then(|v| v.as_str().map(String::from))
            .filter(|v| !v.is_empty())
        {
            keys.push((r#use, value));
        }
    };
    take(
        settings.get_mut("model"),
        "api_key",
        OwnSecret::Model.to_use().unwrap(),
    );
    take(
        settings.get_mut("fallback"),
        "api_key",
        OwnSecret::Fallback.to_use().unwrap(),
    );
    let connections: Vec<(String, Vec<String>)> = settings
        .get("connections")
        .and_then(|c| c.as_table())
        .into_iter()
        .flatten()
        .map(|(connection, c)| {
            let fields = c["about"]["secret_fields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            (connection.clone(), fields)
        })
        .collect();
    for (connection, fields) in connections {
        for field in fields {
            let section = settings
                .get_mut("connections")
                .and_then(|c| c.get_mut(&connection))
                .and_then(|c| c.get_mut("settings"));
            let r#use = OwnSecret::Connection {
                connection: connection.clone(),
                field: field.clone(),
            }
            .to_use()
            .unwrap();
            take(section, &field, r#use);
        }
    }
    let folder = link_files::layout::Home::AgentSettings
        .at(&home.join("Link"))
        .join(name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("agent.toml"),
        toml::to_string(&settings).unwrap(),
    )
    .unwrap();
    std::fs::write(
        folder.join("sandbox.toml"),
        crate::agent_sandbox::AgentSandbox::link()
            .text(name)
            .unwrap(),
    )
    .unwrap();
    let store = link_files::layout::Hidden::Secrets.in_home(home);
    let mut kept: toml::Table = std::fs::read_to_string(&store)
        .map(|t| t.parse().unwrap())
        .unwrap_or_default();
    let entries = kept
        .entry("entry")
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .unwrap();
    for (r#use, value) in keys {
        let agent =
            link_secret_contract::Grantee::new(&format!("agent:{id}"), name, &service).unwrap();
        let secret = link_id::candidate().unwrap();
        let grant = link_secret_contract::Grant::new(&agent, r#use, &secret, false).unwrap();
        let mut entry = toml::Table::new();
        entry.insert("kind".into(), "own".into());
        entry.insert("value".into(), value.into());
        entry.insert(
            "to".into(),
            toml::Value::Array(vec![toml::Value::try_from(grant).unwrap()]),
        );
        entries.insert(secret, entry.into());
    }
    kept.entry("schema")
        .or_insert_with(|| toml::Value::Integer(1));
    link_files::write_private(&store, toml::to_string(&kept).unwrap().as_bytes()).unwrap();
}

/// The value your secrets in `home` keep for agent `name`'s `use`, if any:
/// the entry granted to an agent by that name, for that use, or retained
/// for it, whether or not its settings are here.
pub fn kept_key(home: &Path, name: &str, r#use: &link_secret_contract::Use) -> Option<String> {
    let store = std::fs::read_to_string(link_files::layout::Hidden::Secrets.in_home(home)).ok()?;
    let kept: toml::Table = store.parse().unwrap();
    kept.get("entry")?
        .as_table()?
        .values()
        .find(|entry| {
            ["to", "retained"]
                .into_iter()
                .filter_map(|key| entry.get(key).and_then(toml::Value::as_array))
                .flatten()
                .filter_map(|g| g.clone().try_into::<link_secret_contract::Grant>().ok())
                .any(|g| g.name == name && g.r#as == *r#use)
        })?
        .get("value")?
        .as_str()
        .map(String::from)
}

/// Your secrets in `home` that are the agent whose ID is `id`'s own for
/// `use`, by the service it acts as ([`service_of`]): each entry kept for
/// it (granted it, or retained), then each left of it elsewhere, by the
/// secret's ID, as the store writes them.
pub fn own_secrets(
    home: &Path,
    id: &str,
    r#use: &link_secret_contract::Use,
) -> Vec<(String, toml::Table)> {
    let Ok(store) = std::fs::read_to_string(link_files::layout::Hidden::Secrets.in_home(home))
    else {
        return Vec::new();
    };
    let store: toml::Table = store.parse().unwrap();
    let principal = format!("service:{}", service_of(id));
    let is_use = |value: Option<&toml::Value>| {
        value
            .and_then(|v| v.clone().try_into::<link_secret_contract::Use>().ok())
            .as_ref()
            == Some(r#use)
    };
    let table = |key: &str| {
        store
            .get(key)
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default()
    };
    let mut found = Vec::new();
    for (secret, entry) in table("entry") {
        let entry = entry.as_table().cloned().unwrap_or_default();
        let grants = ["to", "retained"]
            .into_iter()
            .filter_map(|key| entry.get(key).and_then(toml::Value::as_array))
            .flatten();
        let own = grants.clone().any(|g| {
            g.get("principal").and_then(toml::Value::as_str) == Some(&principal)
                && is_use(g.get("as"))
        });
        if own {
            found.push((secret, entry));
        }
    }
    for (secret, left) in table("left") {
        let left = left.as_table().cloned().unwrap_or_default();
        if left.get("principal").and_then(toml::Value::as_str) == Some(&principal)
            && is_use(left.get("as"))
        {
            found.push((secret, left));
        }
    }
    found
}

/// A connected connection as an agent's settings keep it (`[connections.<name>]`),
/// as its module's `connect` made it: `settings` is its section of what a
/// harness is handed, `bot` its bot, `secret_fields` the settings' secret
/// ones.
pub fn connection(name: &str, secret_fields: &[&str], settings: &str, bot: &str) -> String {
    let title = name[..1].to_uppercase() + &name[1..];
    let linked = name == "signal";
    let mut bot = bot.to_string();
    for field in ["id", "name"] {
        if !bot.contains(&format!("{field} =")) {
            bot.push_str(&format!("\n{field} = \"\""));
        }
    }
    format!(
        "\n[connections.{name}.settings]\n{settings}\n\n[connections.{name}.bot]\n{bot}\n\n\
         [connections.{name}.about]\ntitle = \"{title}\"\nsecret_fields = {secret_fields:?}\n\
         linked = {linked}\nbridged = true\ncli = {linked}\n"
    )
}
