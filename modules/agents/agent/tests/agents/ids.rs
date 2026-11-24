//! An agent's ID: never another's, here or where it moved.

use super::*;

/// An ID that moved away reaches the agent that moved, where it went,
/// never another agent that has its name here now.
#[test]
fn an_id_that_moved_away_never_reaches_its_namesake() {
    let world = World::new("moved-id");
    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    let moved = link_files::layout::Hidden::Moved.in_home(&world.home);
    std::fs::create_dir_all(&moved).unwrap();
    let note = |id: &str, name: &str| {
        std::fs::write(
            moved.join(format!("{id}.toml")),
            format!(
                "schema = 1\nid = \"{id}\"\nname = \"{name}\"\nat = 1\n\n\
                 [to]\nname = \"aws-1\"\nresource = \"box:b0x1a2w3s4b5\"\n"
            ),
        )
        .unwrap();
    };
    // Gone from here, its name free: its ID goes where its name does,
    // to the box it moved to (none answers here).
    note("g0n3a1g2e3n4", "gone");
    let by_id = failed(&world.lnk(&["show", "--agent", "agent:g0n3a1g2e3n4"]));
    let by_name = failed(&world.lnk(&["show", "--agent", "gone"]));
    let first = |said: &str| said.lines().next().unwrap_or_default().to_string();
    assert_eq!(first(&by_id), first(&by_name), "{by_id}\n{by_name}");
    assert!(!by_id.contains("no agent here has the ID"), "{by_id}");
    // Its name another agent's here now: refused, never that agent.
    note("w0rka1g2e3n4", "work");
    let refused = failed(&world.lnk(&["show", "--agent", "agent:w0rka1g2e3n4"]));
    assert!(refused.contains("is another agent's now"), "{refused}");
    assert!(!refused.contains(&id_of(&world, "work")), "{refused}");
}

/// An agent deleted and created again with the same name is another
/// agent: a new ID, which no grant of the first one's names, and the
/// first one's ID retired here, never reached by `--agent` again.
#[test]
fn a_deleted_agents_id_is_never_given_again() {
    let world = World::new("retired");
    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    let first = id_of(&world, "work");
    let first_service = service_of(&world, "work");
    assert_ne!(
        first, first_service,
        "an agent's service has an ID of its own"
    );
    let named = ok(&world.lnk(&["show", "--agent", &format!("agent:{first}"), "--json"]));
    assert!(named.contains("\"work\""), "{named}");
    let store = world.home.join(".lnk/safe/secrets/secrets.toml");
    let kept = format!(
        "{}\n[entry.npmt0k3n0000]\nkind = \"named\"\nname = \"NPM_TOKEN\"\nvalue = \"npm_granted\"\n\
         to = [{{ principal = \"service:{first_service}\", action = \"secret.secret.read\", resource = \"secret:npmt0k3n0000\", workload = \"agent:{first}\", name = \"work\", as = {{ kind = \"variable\" }} }}]\n",
        std::fs::read_to_string(&store).unwrap_or_default()
    );
    link_files::write_private(&store, kept.as_bytes()).unwrap();
    ok(&world.lnk(&["delete", "work", "--yes"]));
    // A ledger names its schema, then its two sets.
    let kept = |file: link_files::layout::Hidden| -> link_id::Table {
        let mut table: toml::Table = toml::from_str(&read(&file.in_home(&world.home))).unwrap();
        assert_eq!(table.remove("schema"), Some(toml::Value::Integer(1)));
        table.try_into().unwrap()
    };
    let ledger = kept(link_files::layout::Hidden::AgentIds);
    assert!(ledger.retired.contains(&first), "{ledger:?}");
    let services = kept(link_files::layout::Hidden::AgentServices);
    assert!(services.retired.contains(&first_service), "{services:?}");

    ok(&world.start_on_owned_port("work", &["create", "work", "--harness", "fake"]));
    let second = id_of(&world, "work");
    assert_ne!(first, second);
    assert_ne!(first_service, service_of(&world, "work"));
    // What was granted to the first never reaches the second: gone with
    // it, or naming the first alone.
    let secrets: toml::Table = read(&store).parse().unwrap();
    let granted: Vec<&str> = secrets
        .get("entry")
        .and_then(|entries| entries.get("npmt0k3n0000"))
        .and_then(|entry| entry.get("to"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|grant| {
            grant
                .get("principal")
                .and_then(toml::Value::as_str)
                .unwrap()
        })
        .collect();
    let first_principal = format!("service:{first_service}");
    assert!(granted.iter().all(|w| *w == first_principal), "{secrets}");
    let gone = failed(&world.lnk(&["show", "--agent", &format!("agent:{first}")]));
    assert!(gone.contains("no agent here has the ID"), "{gone}");
    // Deleted by its ID, as by its name.
    let said = ok(&world.lnk(&["delete", &format!("agent:{second}"), "--yes"]));
    assert!(said.contains("Removed work."), "{said}");
}

/// A create refused after its ID was reserved, here at a model key it
/// has no terminal to ask for, leaves no live ID behind: the ID it
/// reserved is retired, and no settings of the agent are saved.
#[test]
fn a_refused_create_leaves_no_live_id() {
    let world = World::new("refused-id");
    let said = failed(&world.lnk(&[
        "create",
        "work",
        "--harness",
        "fake",
        "--model.default",
        "nobody/no-such-model",
    ]));
    assert!(said.contains("Couldn't ask"), "{said}");
    let ledger = link_files::layout::Hidden::AgentIds.in_home(&world.home);
    let mut table: toml::Table = toml::from_str(&read(&ledger)).unwrap();
    table.remove("schema");
    let kept: link_id::Table = table.try_into().unwrap();
    assert!(kept.live.is_empty(), "{kept:?}");
    assert_eq!(kept.retired.len(), 1, "{kept:?}");
    let settings = link_files::layout::Home::AgentSettings
        .at(&world.home.join("Link"))
        .join("work/agent.toml");
    assert!(!settings.exists(), "{}", settings.display());
}
