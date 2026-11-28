use super::*;

fn args(s: &str) -> Vec<OsString> {
    s.split_whitespace().map(Into::into).collect()
}

/// A command that became a group is answered only where a word after
/// it is no subcommand of the group.
#[test]
fn a_command_that_became_a_group_is_answered_before_a_word_alone() {
    let history = History::parse(
        "[[gone]]\nwas = \"task spec\"\nnow = \"lnk task spec show\"\nsince = \"1.0.0\"\nuntil = \"2.0.0\"\n",
    )
    .unwrap();
    let surface = BTreeMap::from([
        ("task spec".to_string(), BTreeMap::new()),
        ("task spec show".to_string(), BTreeMap::new()),
    ]);
    history
        .validate(&surface, &surface, &[], &Version::new(1, 0, 0))
        .unwrap();
    assert!(history.find(&args("task spec 3f9a"), &surface).is_some());
    assert!(
        history
            .find(&args("task spec show 3f9a"), &surface)
            .is_none()
    );
    assert!(history.find(&args("task spec"), &surface).is_none());
    assert!(history.find(&args("task spec --help"), &surface).is_none());
}
#[test]
fn an_entry_is_spelled_with_its_namespace_first() {
    let history = History::parse(
        "[[gone]]\nwas = \"tool serve --zone\"\nwhy = \"x\"\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n\
         [[gone]]\nwas = \"LNK_TOOL_ZONE\"\nwhy = \"x\"\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n",
    )
    .unwrap();
    assert!(history.outside("tool").is_none());
    let bare = History::parse(
        "[[gone]]\nwas = \"serve --zone\"\nwhy = \"x\"\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n",
    )
    .unwrap();
    assert_eq!(
        bare.outside("tool").map(|g| g.was.as_str()),
        Some("serve --zone")
    );
    // A namespace's prefix alone isn't its namespace.
    let longer = History::parse(
        "[[gone]]\nwas = \"toolbox serve\"\nwhy = \"x\"\nsince = \"0.0.1\"\nuntil = \"0.0.2\"\n",
    )
    .unwrap();
    assert!(longer.outside("tool").is_some());
}

#[test]
fn history_matches_syntax_and_preserves_argument_values() {
    let history = History::parse(
        r#"
[[gone]]
was = "box create --zone"
now = "lnk box create --region"
since = "1.0.0"
until = "2.0.0"
[[gone]]
was = "old"
now = "lnk box create"
since = "1.0.0"
until = "2.0.0"
"#,
    )
    .unwrap();
    let surface = BTreeMap::from([(
        "box create".into(),
        BTreeMap::from([("--name".into(), true), ("--region".into(), true)]),
    )]);
    history
        .validate(&surface, &surface, &[], &Version::new(1, 0, 0))
        .unwrap();
    for (changed, expected) in [
        ("box create --zone west", "box create --zone"),
        ("box create --zone=west", "box create --zone"),
        ("old --name web", "old"),
    ] {
        let (gone, _) = history
            .find(&args(changed), &surface)
            .expect("retired syntax");
        assert_eq!(gone.was, expected);
        assert!(gone.message().contains("Use lnk box create"));
    }
    for untouched in [
        "box create --name --zone",
        "box create -- --zone",
        "older",
        "box creates --zone",
    ] {
        assert!(
            history.find(&args(untouched), &surface).is_none(),
            "{untouched}"
        );
    }
}
/// A command that became a group answers its old flag given right
/// after it, and leaves the same flag of its subcommand alone.
#[test]
fn a_flag_of_a_command_now_a_group_is_answered_only_before_a_subcommand() {
    let history = History::parse(
        r#"
[[gone]]
was = "box spec --spec"
now = "lnk box spec show --spec"
since = "1.0.0"
until = "2.0.0"
"#,
    )
    .unwrap();
    let surface = BTreeMap::from([
        ("box spec".into(), BTreeMap::new()),
        (
            "box spec show".into(),
            BTreeMap::from([("--spec".into(), true), ("--json".into(), false)]),
        ),
    ]);
    history
        .validate(&surface, &surface, &[], &Version::new(1, 0, 0))
        .unwrap();
    for changed in [
        "box spec --spec box.toml",
        "box spec --json --spec=box.toml",
    ] {
        let (gone, _) = history.find(&args(changed), &surface).expect(changed);
        assert!(gone.message().contains("Use lnk box spec show --spec"));
    }
    for current in [
        "box spec show --spec box.toml",
        "box spec show web --spec x",
    ] {
        assert!(
            history.find(&args(current), &surface).is_none(),
            "{current}"
        );
    }
}

#[test]
fn history_refuses_invalid_entries_and_bad_targets() {
    assert!(History::parse("[[gone]]\nwas = 'old'\nsince = '1.0.0'\nuntil = '2.0.0'\n").is_err());
    let history = History::parse(
        "[[gone]]\nwas = 'old'\nsince = '1.0.0'\nuntil = '2.0.0'\nnow = 'lnk absent'\n",
    )
    .unwrap();
    assert!(
        history
            .validate(
                &Surface::new(),
                &Surface::new(),
                &[],
                &Version::new(1, 0, 0)
            )
            .is_err()
    );
}

fn renames() -> History {
    History::parse(
        r#"
[[renamed]]
module = "memory"
now = "memory-store"
since = "1.0.0"
until = "2.0.0"
[[renamed]]
module = "memory-store"
now = "memory-tool"
since = "1.1.0"
until = "2.0.0"
"#,
    )
    .unwrap()
}

#[test]
fn a_renamed_modules_old_name_is_answered_with_its_name_today() {
    let history = renames();
    let surface = BTreeMap::from([
        (
            "module add".into(),
            BTreeMap::from([("--url".into(), true)]),
        ),
        ("module remove".into(), BTreeMap::new()),
    ]);
    history
        .validate(&surface, &surface, &["memory-tool"], &Version::new(1, 1, 0))
        .unwrap();
    for (old, module) in [
        ("memory start", "memory"),
        ("module add web memory", "memory"),
        ("module add memory-store", "memory-store"),
    ] {
        let renamed = history.find_renamed(&args(old), &surface).expect(old);
        assert_eq!(renamed.module, module);
        let now = history.name(&renamed.module).unwrap();
        assert_eq!(
            renamed.message(now),
            format!(
                "The {module} module is named memory-tool since {}.",
                renamed.since
            )
        );
    }
    for untouched in [
        "module add memory-tool",
        "module add --url memory",
        "module add -- memory",
        "box memory",
        "module list memory",
        // The core's own `module remove` answers it, taking a leftover.
        "module remove memory-store",
    ] {
        assert!(
            history.find_renamed(&args(untouched), &surface).is_none(),
            "{untouched}"
        );
    }
}

#[test]
fn renames_refuse_loops_twins_and_targets_that_arent_modules() {
    let entry = |module: &str, now: &str| {
        format!(
            "[[renamed]]\nmodule = '{module}'\nnow = '{now}'\nsince = '1.0.0'\nuntil = '2.0.0'\n"
        )
    };
    assert!(History::parse(&entry("a", "a")).is_err());
    assert!(History::parse(&(entry("a", "b") + &entry("a", "c"))).is_err());
    assert!(History::parse(&(entry("a", "b") + &entry("b", "a"))).is_err());
    assert!(History::parse(&entry("A", "b")).is_err());
    let history = History::parse(&entry("a", "b")).unwrap();
    let (surface, version) = (Surface::new(), Version::new(1, 0, 0));
    assert!(
        history
            .validate(&surface, &surface, &["b"], &version)
            .is_ok()
    );
    assert!(
        history
            .validate(&surface, &surface, &["c"], &version)
            .is_err()
    );
    assert!(
        history
            .validate(&surface, &surface, &["a", "b"], &version)
            .is_err()
    );
    assert!(
        history
            .validate(&surface, &surface, &["b"], &Version::new(0, 9, 0))
            .is_err()
    );
}

/// A staging build of the release that renamed a module, removed one or
/// retired a spelling runs: it's a build of that release, though its
/// prerelease orders before it, and a build of an earlier one still
/// refuses the entry as future.
#[test]
fn a_staging_build_holds_its_own_releases_entries() {
    let history = History::parse(
        "[[renamed]]\nmodule = 'a'\nnow = 'b'\nsince = '0.0.2'\nuntil = '0.0.4'\n\
         [[removed]]\nmodule = 'c'\nwhy = 'Use b.'\nsince = '0.0.2'\nuntil = '0.0.4'\n\
         [[gone]]\nwas = 'b old'\nwhy = 'Gone.'\nsince = '0.0.2'\nuntil = '0.0.4'\n",
    )
    .unwrap();
    let surface = Surface::new();
    let staging: Version = "0.0.2-staging.202611170000.g1a2b3c4d5e6f".parse().unwrap();
    history
        .validate(&surface, &surface, &["b"], &staging)
        .unwrap();
    let earlier: Version = "0.0.1-staging.202611170000.g1a2b3c4d5e6f".parse().unwrap();
    assert!(
        history
            .validate(&surface, &surface, &["b"], &earlier)
            .is_err()
    );
}

/// A retired command whose words are a current command's again is
/// named with its positional: refused with that word given, never
/// without, so the current command still runs.
#[test]
fn a_retired_spelling_naming_a_positional_leaves_the_current_command_alone() {
    let history = History::parse(
        "[[gone]]\nwas = 'model uninstall <model>'\nnow = 'lnk model delete'\n\
         since = '1.0.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let surface = Surface::from([
        ("model uninstall".to_string(), BTreeMap::new()),
        ("model delete".to_string(), BTreeMap::new()),
    ]);
    assert!(
        history
            .validate(&surface, &surface, &[], &Version::new(1, 0, 0))
            .is_ok()
    );
    let args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
    assert!(
        history
            .find(&args(&["model", "uninstall"]), &surface)
            .is_none()
    );
    assert!(
        history
            .find(&args(&["model", "uninstall", "--parts"]), &surface)
            .is_none()
    );
    let (gone, at) = history
        .find(&args(&["model", "uninstall", "llama3"]), &surface)
        .unwrap();
    assert_eq!((gone.was.as_str(), at), ("model uninstall <model>", 3));
    // Without its positional, the same words are both current and gone.
    let bare = History::parse(
        "[[gone]]\nwas = 'model uninstall'\nnow = 'lnk model delete'\n\
         since = '1.0.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    assert!(
        bare.validate(&surface, &surface, &[], &Version::new(1, 0, 0))
            .is_err()
    );
}

#[test]
fn an_entry_past_its_until_fails_and_until_follows_since() {
    let entry = |since: &str, until: &str| {
        format!("[[gone]]\nwas = 'old'\nwhy = 'Gone.'\nsince = '{since}'\nuntil = '{until}'\n")
    };
    let history = History::parse(&entry("1.0.0", "1.2.0")).unwrap();
    assert!(history.check_retirement(&Version::new(1, 1, 0)).is_empty());
    let reached = history.check_retirement(&Version::new(1, 2, 0));
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert!(History::parse(&entry("1.2.0", "1.2.0")).is_err());
}

#[test]
fn an_upgrade_replaces_only_an_installed_old_program() {
    let dir = link_test_kit::TempDir::new("history-renamed");
    link_test_kit::write_program(&dir.join("lnk-memory"), "#!/bin/sh\necho 'lnk 1.0.0'\n").unwrap();
    let history = renames();
    assert_eq!(
        history.renamed_in(&dir, &["memory-tool"]).unwrap(),
        [("lnk-memory".to_string(), "memory-tool")]
    );
    // Not when the new name isn't in the release, nor when the old
    // name is a current module there.
    assert!(history.renamed_in(&dir, &["other"]).unwrap().is_empty());
    assert!(
        history
            .renamed_in(&dir, &["memory", "memory-tool"])
            .unwrap()
            .is_empty()
    );
}

/// A command moved to another module names its new home: the
/// replacement is checked against the release's whole surface, never
/// only its owner's commands.
#[test]
fn a_replacement_may_name_another_modules_command() {
    let history = History::parse(
        "[[gone]]\nwas = 'memory export'\nnow = 'lnk notes export'\n\
         since = '1.0.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let own = Surface::from([("memory list".to_string(), BTreeMap::new())]);
    let mut catalog = own.clone();
    let version = Version::new(1, 0, 0);
    assert!(history.validate(&own, &catalog, &[], &version).is_err());
    catalog.insert("notes export".to_string(), BTreeMap::new());
    history.validate(&own, &catalog, &[], &version).unwrap();
    // Still never current where it was.
    let mut both = own.clone();
    both.insert("memory export".to_string(), BTreeMap::new());
    assert!(history.validate(&both, &catalog, &[], &version).is_err());
}

fn removal() -> History {
    History::parse(
        "[[removed]]\nmodule = 'webhook'\nwhy = 'Use lnk webhook-connection.'\n\
         since = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap()
}

/// A removed module's name is answered with why, as a command or given
/// to `lnk module add` or `remove`, and nothing else is taken for it.
#[test]
fn a_removed_modules_name_is_answered_with_why() {
    let history = removal();
    let surface = BTreeMap::from([
        (
            "module add".into(),
            BTreeMap::from([("--url".into(), true)]),
        ),
        ("module remove".into(), BTreeMap::new()),
    ]);
    history
        .validate(&surface, &surface, &["other"], &Version::new(1, 1, 0))
        .unwrap();
    for old in ["webhook start", "module add webhook"] {
        let removed = history.find_removed(&args(old), &surface).expect(old);
        assert_eq!(
            removed.message(),
            "The webhook module is removed since 1.1.0. Use lnk webhook-connection."
        );
    }
    for untouched in [
        "module add --url webhook",
        "module add -- webhook",
        "module remove webhook",
        "box webhook",
        "webhook-connection start",
    ] {
        assert!(
            history.find_removed(&args(untouched), &surface).is_none(),
            "{untouched}"
        );
    }
    assert!(history.find_renamed(&args("webhook"), &surface).is_none());
}

#[test]
fn a_removal_is_never_current_renamed_or_without_why() {
    let history = removal();
    let (surface, version) = (Surface::new(), Version::new(1, 1, 0));
    assert!(
        history
            .validate(&surface, &surface, &["webhook"], &version)
            .is_err()
    );
    let current = Surface::from([("webhook".to_string(), BTreeMap::new())]);
    assert!(history.validate(&current, &current, &[], &version).is_err());
    assert!(
        history
            .validate(&surface, &surface, &[], &Version::new(1, 0, 0))
            .is_err()
    );
    let entry = |module: &str, why: &str| {
        format!(
            "[[removed]]\nmodule = '{module}'\nwhy = '{why}'\nsince = '1.0.0'\nuntil = '2.0.0'\n"
        )
    };
    let rename = |module: &str, now: &str| {
        format!(
            "[[renamed]]\nmodule = '{module}'\nnow = '{now}'\nsince = '1.0.0'\nuntil = '2.0.0'\n"
        )
    };
    assert!(History::parse(&entry("a", " ")).is_err());
    assert!(History::parse(&entry("A", "Gone.")).is_err());
    assert!(History::parse(&(entry("a", "Gone.") + &entry("a", "Gone."))).is_err());
    assert!(History::parse(&(entry("a", "Gone.") + &rename("a", "b"))).is_err());
    // A rename may lead to a module removed since, which removes its old
    // name too: `a` is answered with `b`'s removal.
    let led = History::parse(&(entry("b", "Gone.") + &rename("a", "b"))).unwrap();
    assert_eq!(led.removal("a").map(|r| r.why.as_str()), Some("Gone."));
    assert!(led.removal("c").is_none());
    let surface = Surface::from([("module add".to_string(), BTreeMap::new())]);
    assert!(led.find_renamed(&args("module add a"), &surface).is_none());
    assert_eq!(
        led.find_removed(&args("module add a"), &surface)
            .map(|r| r.module.as_str()),
        Some("b")
    );
    led.validate(
        &Surface::new(),
        &Surface::new(),
        &[],
        &Version::new(1, 1, 0),
    )
    .unwrap();
    let reached = History::parse(&entry("a", "Gone."))
        .unwrap()
        .check_retirement(&Version::new(2, 0, 0));
    assert_eq!(reached.len(), 1, "{reached:?}");
}

/// An upgrade uninstalls a removed module's program only where it's
/// installed and the release doesn't include the name again, and the wire
/// carries the removal to it, written only when there is one.
#[test]
fn an_upgrade_uninstalls_only_an_installed_removed_program() {
    let dir = link_test_kit::TempDir::new("history-removed");
    let wire: Wire =
        serde_json::from_str(&serde_json::to_string(&removal().wire()).unwrap()).unwrap();
    assert!(wire.removed_in(&dir, &[]).is_empty());
    link_test_kit::write_program(&dir.join("lnk-webhook"), "#!/bin/sh\necho 'lnk 1.0.0'\n")
        .unwrap();
    let found = wire.removed_in(&dir, &["other"]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "lnk-webhook");
    assert_eq!(found[0].1.why, "Use lnk webhook-connection.");
    assert!(wire.removed_in(&dir, &["webhook"]).is_empty());
    // A module renamed to one removed since is uninstalled by its old
    // name, with that removal.
    let led = History::parse(
        "[[renamed]]\nmodule = 'hooks'\nnow = 'webhook'\nsince = '1.0.0'\nuntil = '2.0.0'\n\
         [[removed]]\nmodule = 'webhook'\nwhy = 'Use lnk webhook-connection.'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap()
    .wire();
    link_test_kit::write_program(&dir.join("lnk-hooks"), "#!/bin/sh\necho 'lnk 1.0.0'\n").unwrap();
    let mut found: Vec<_> = led
        .removed_in(&dir, &["other"])
        .into_iter()
        .map(|(program, removed)| (program, removed.module.clone()))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("lnk-hooks".to_string(), "webhook".to_string()),
            ("lnk-webhook".to_string(), "webhook".to_string())
        ]
    );
    assert!(
        !serde_json::to_string(&renames().wire())
            .unwrap()
            .contains("removed")
    );
}
