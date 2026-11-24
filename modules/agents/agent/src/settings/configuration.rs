//! One editable agent file: setup at the root, behavior in its own table.
use super::*;
use toml_edit::{DocumentMut, Item};

/// Recreate defaults for an existing agent without restoring deleted grants or credentials.
/// Its identity stays in this machine's records, which a load reads.
pub fn recreate(name: &str) -> anyhow::Result<Settings> {
    let settings = Settings::new(name);
    let path = Settings::path_of(name);
    link_files::create_private_dir(path.parent().within("agent configuration needs a folder")?)?;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(&path) {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(link_files::saved::print(&settings)?.as_bytes())?;
            Ok(settings)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(link_files::saved::parse(&read_spec_text(&path)?)?)
        }
        Err(err) => Err(err.into()),
    }
}

/// Whether `spec`'s instructions are Link's, as a new agent's file or
/// an expanded `--spec` writes them: not its own.
pub fn shipped_instructions(spec: &link_agent_spec::Spec) -> bool {
    shipped().0.spec.instructions == spec.instructions
}

/// Link's spec alone, resolved, with its spec as TOML: made once.
fn shipped() -> &'static (link_agent_spec::Resolved, toml::Value) {
    static SHIPPED: std::sync::LazyLock<(link_agent_spec::Resolved, toml::Value)> =
        std::sync::LazyLock::new(|| {
            let known = link_agent_spec::Known { name: MAIN.into() };
            let links = link_agent_spec::resolve(None, &[], &known).expect("Link's spec resolves");
            let value = toml::Value::try_from(&links.spec).expect("a spec is TOML");
            (links, value)
        });
    &SHIPPED
}

/// Link's defaults written out, without `extends` or managed identity.
pub fn default_behavior() -> Option<link_agent_spec::Spec> {
    let mut spec = shipped().0.spec.clone();
    spec.extends = None;
    spec.name = None;
    spec.harness = None;
    Some(spec)
}

/// Gives back to Link each key the agent's own spec holds at Link's
/// default value: a default written into its file (a new
/// agent's, an expanded `--spec`'s) is not a choice of its own.
pub fn unclaim_defaults(resolved: &mut link_agent_spec::Resolved) {
    let Ok(spec) = toml::Value::try_from(&resolved.spec) else {
        return;
    };
    let at = |value: &toml::Value, key: &str| {
        key.split('.')
            .try_fold(value, |value, part| value.get(part))
            .cloned()
    };
    for (key, whose) in resolved.from.iter_mut() {
        if !matches!(whose, link_agent_spec::From::Yours) {
            continue;
        }
        let Some(own) = at(&spec, key) else {
            continue;
        };
        let (defaults, default) = shipped();
        if let Some(origin) = defaults
            .from
            .get(key)
            .filter(|origin| !matches!(origin, link_agent_spec::From::Yours))
            .filter(|_| at(default, key).as_ref() == Some(&own))
        {
            *whose = origin.clone();
        }
    }
}

/// A behavior table as a standalone spec, preserving its comments. The
/// settings file is read whole as today's shapes, its `[behavior]` the
/// agent spec's current type, so the standalone spec says that schema;
/// [`with_behavior`] takes it out again.
pub fn behavior_text(text: &str) -> anyhow::Result<Option<String>> {
    let behavior = behavior_table(text)?;
    if let Some(behavior) = &behavior {
        parse(behavior)?;
    }
    Ok(behavior)
}

/// The behavior table of an agent's settings file as its spec, parsed
/// once: what [`Settings::own_spec`] runs.
pub fn behavior_spec(text: &str) -> anyhow::Result<Option<link_agent_spec::Spec>> {
    behavior_table(text)?.as_deref().map(parse).transpose()
}

/// [`behavior_text`], unparsed.
fn behavior_table(text: &str) -> anyhow::Result<Option<String>> {
    let doc: DocumentMut = text.parse()?;
    doc.get("behavior")
        .map(|item| {
            let table = item
                .clone()
                .into_table()
                .map_err(|_| anyhow::anyhow!("[behavior] isn't a table"))?;
            // A sub-table's blank line before it, which led in the file.
            Ok(format!(
                "schema = {}\n{}",
                <link_agent_spec::Spec as link_spec::Specification>::format().current,
                DocumentMut::from(table)
                    .to_string()
                    .trim_start_matches(['\r', '\n'])
            ))
        })
        .transpose()
}

#[cfg(test)]
thread_local! {
    /// How many times this test's thread parsed an agent's own spec.
    pub(crate) static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// An agent's own spec, from its behavior table's text.
fn parse(text: &str) -> anyhow::Result<link_agent_spec::Spec> {
    #[cfg(test)]
    PARSES.with(|parses| parses.set(parses.get() + 1));
    link_agent_spec::parse(text)
}

/// Replace the behavior table without changing setup or connection values.
pub fn with_behavior(text: &str, behavior: &str) -> anyhow::Result<String> {
    link_agent_spec::parse(behavior)?;
    let mut doc: DocumentMut = text.parse()?;
    let mut behavior: DocumentMut = behavior.parse()?;
    // The settings file's own shapes say what its behavior's schema is.
    for key in ["schema", "kind"] {
        behavior.remove(key);
    }
    doc.insert("behavior", Item::Table(behavior.into_table()));
    stamped(&doc.to_string())
}

/// Save current setup while retaining owner-written behavior and comments.
pub fn with_setup(text: &str, setup: &toml::Value) -> anyhow::Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    let incoming: DocumentMut = toml::to_string(setup)?.parse()?;
    let keys: Vec<String> = doc.iter().map(|(key, _)| key.to_string()).collect();
    for key in keys {
        if key != "behavior" && key != "schema" && !incoming.contains_key(&key) {
            doc.remove(&key);
        }
    }
    for (key, value) in incoming.iter() {
        if key == "behavior" && doc.contains_key("behavior") {
            continue;
        }
        if doc
            .get(key)
            .is_none_or(|old| rendered(key, old) != rendered(key, value))
        {
            doc.insert(key, value.clone());
        }
    }
    stamped(&doc.to_string())
}

/// `text`, an agent's settings file, naming its current schema: the
/// `schema` line it has set to it, else one put above its first key, under
/// the comments that open the file. Every writer of the file passes
/// through here ([`with_setup`], [`with_behavior`]).
pub fn stamped(text: &str) -> anyhow::Result<String> {
    use link_files::schema::Specification;
    let current = Settings::format().current;
    let mut doc: DocumentMut = text.parse()?;
    if doc.contains_key("schema") {
        doc["schema"] = toml_edit::value(i64::from(current));
        return Ok(doc.to_string());
    }
    let line = format!("schema = {current}\n");
    let mut out = String::with_capacity(text.len() + line.len());
    let mut placed = false;
    for piece in text.split_inclusive('\n') {
        let trimmed = piece.trim_start();
        if !placed && !trimmed.is_empty() && !trimmed.starts_with('#') {
            out.push_str(&line);
            placed = true;
        }
        out.push_str(piece);
    }
    if !placed {
        out.push_str(&line);
    }
    Ok(out)
}

/// `item` as a file holding it alone writes it: a table of only tables
/// (`[webhooks.deploy]`) with them, which its own text leaves out.
fn rendered(key: &str, item: &Item) -> String {
    let mut doc = DocumentMut::new();
    doc.insert(key, item.clone());
    doc.to_string()
}

/// Store a spec in this agent's configuration; its setup remains canonical.
/// A missing file starts with a new agent's defaults; its identity is in
/// this machine's records, kept first, so a write cut short still leaves
/// an agent with its identity.
pub fn write_behavior(agent: &Settings, behavior: &str) -> anyhow::Result<()> {
    let path = Settings::path_of(&agent.name);
    if agent.id.is_some() {
        super::records::keep(agent)?;
    }
    let before = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Its defaults alone: never its keys, which are your secrets'.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            link_files::saved::print(&Settings::new(&agent.name))?
        }
        Err(err) => return Err(err.into()),
    };
    let written = with_behavior(&before, behavior)?;
    let _: Settings = link_files::saved::parse(&written)?;
    link_files::write_private(&path, written.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_and_behavior_share_one_file_without_overwriting_each_other() {
        let original = "# My agent\nharness = \"link\"\n[model]\nprovider = \"ollama\"\nname = \"qwen3:8b\"\n[behavior]\ncommand_response = \"Harness\"\n# Keep this choice\n[behavior.tool.file]\nenabled = false # off deliberately\n";
        let behavior = behavior_text(original).unwrap().unwrap();
        let enabled = link_agent_spec::with_tool_enabled(&behavior, "file", true).unwrap();
        let written = with_behavior(original, &enabled).unwrap();
        let mut setup: Settings = link_files::saved::parse(&written).unwrap();
        setup.model.as_mut().unwrap().name = Some("another-model".into());
        let written = with_setup(&written, &toml::Value::try_from(&setup).unwrap()).unwrap();
        let actual: Settings = link_files::saved::parse(&written).unwrap();
        assert_eq!(actual.model.unwrap().name.as_deref(), Some("another-model"));
        assert_eq!(actual.behavior.unwrap().tool("file").enabled, Some(true));
        assert!(
            written.contains("# My agent")
                && written.contains("# Keep this choice")
                && written.contains("# off deliberately")
        );
        assert!(!written.contains("extends ="));
    }

    /// A table of only tables, as each webhook is, gains one saved.
    #[test]
    fn a_table_of_tables_saved_gains_its_new_one() {
        let original = "[webhooks.deploy]\nsender = \"link\"\n";
        let mut setup: toml::Value = toml::from_str(original).unwrap();
        let webhooks = setup["webhooks"].as_table_mut().unwrap();
        webhooks.insert(
            "repo".into(),
            toml::from_str("sender = \"github\"").unwrap(),
        );
        let written = with_setup(original, &setup).unwrap();
        assert!(
            written.contains("[webhooks.deploy]") && written.contains("[webhooks.repo]"),
            "{written}"
        );
    }

    #[test]
    fn recreated_behavior_defaults_contain_no_extends_and_enable_only_file() {
        let spec = default_behavior().unwrap();
        assert!(spec.extends.is_none());
        for tool in ["scheduler", "run", "code"] {
            assert_eq!(spec.tool(tool).enabled, Some(false), "{tool}");
        }
        // File, so asked to, it changes its instructions and skills.
        assert_eq!(spec.tool("file").enabled, Some(true));
    }

    #[test]
    fn expanded_defaults_stay_links() {
        let own = default_behavior().unwrap();
        let mut resolved = link_agent_spec::resolve(
            Some(&own),
            &[],
            &link_agent_spec::Known { name: MAIN.into() },
        )
        .unwrap();
        unclaim_defaults(&mut resolved);
        for key in [
            "instructions.repository",
            "connections.activity",
            "turns.rounds",
        ] {
            assert_eq!(resolved.from[key], link_agent_spec::From::Link, "{key}");
        }
    }

    /// Which agent spec schema each settings schema's `[behavior]` holds,
    /// one pair a settings schema. A settings file writes its behavior at
    /// the agent spec's current schema without a line of its own
    /// ([`behavior_text`]), so when the agent spec's schema moves, the
    /// settings file takes a step of its own and records the pair here.
    const BEHAVIOR_OF: [(u32, u32); 1] = [(1, 1)];

    #[test]
    fn the_behavior_moves_with_the_agent_spec() {
        use link_files::schema::Specification;
        let settings = Settings::format().current;
        let spec = <link_agent_spec::Spec as link_spec::Specification>::format().current;
        assert_eq!(
            BEHAVIOR_OF.last(),
            Some(&(settings, spec)),
            "the agent spec is at schema {spec}: the settings file needs a step to schema \
             {} of its own, holding it, recorded in BEHAVIOR_OF",
            settings + 1
        );
    }

    /// Every write names the file's schema: above its first key, under
    /// the comments opening it, or the line it has set to the current one.
    #[test]
    fn every_write_names_the_schema() {
        let written = stamped("# My agent\n\nharness = \"link\"\n[behavior]\n").unwrap();
        assert_eq!(
            written,
            "# My agent\n\nschema = 1\nharness = \"link\"\n[behavior]\n"
        );
        assert_eq!(stamped(&written).unwrap(), written);
        assert_eq!(stamped("").unwrap(), "schema = 1\n");
        let setup = with_setup(
            "harness = \"link\"\n",
            &toml::Value::try_from(Settings::new("a")).unwrap(),
        )
        .unwrap();
        assert!(setup.starts_with("schema = 1\n"), "{setup}");
        let read: Settings = link_files::saved::parse(&setup).unwrap();
        assert_eq!(read.harness, Settings::new("a").harness);
        let behavior = with_behavior(&setup, "schema = 1\n").unwrap();
        assert_eq!(behavior.matches("schema = 1").count(), 1, "{behavior}");
    }
}
