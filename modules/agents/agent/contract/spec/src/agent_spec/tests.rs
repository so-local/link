//! The spec's tests: Link's own, a spec's values, teams and access.

mod connections;
mod teams;

/// Run by the release after it refreshes the file: whatever the
/// models' figures are now, Link's own default models are described,
/// with figures a model could have.
#[test]
fn links_own_file_parses_and_describes_its_defaults() {
    let link = crate::models::parse(crate::models::LINK_MODELS).unwrap();
    // Link's own default models are described: the spec's defaults.
    let spec = crate::agent_spec::link();
    for provider in ["anthropic", "openai", "openrouter"] {
        let default = spec.model_for(provider).default.unwrap();
        let facts = link.find(&default).unwrap_or_else(|| panic!("{default}"));
        let window = facts.window.unwrap_or_default();
        assert!(window >= 32_768, "{default}: a window of {window}");
        if let Some(answer) = facts.answer {
            assert!((1024..=window).contains(&answer), "{default}: {answer}");
        }
        if let Some(price) = facts.price {
            for value in [price.input, price.output] {
                assert!(value > 0.0 && value < 1000.0, "{default}: ${value}");
            }
        }
    }
}

use super::*;

/// `text`, a spec's keys, as a spec file: naming its schema, as every
/// spec file does, where it doesn't already.
pub(super) fn named(text: &str) -> String {
    match text.parse::<toml::Table>() {
        Ok(table) if table.contains_key("schema") => text.into(),
        _ => format!("schema = 1\n{text}"),
    }
}

/// [`super::parse`], of `text` as a spec file ([`named`]).
pub(super) fn parse(text: &str) -> anyhow::Result<Spec> {
    super::parse(&named(text))
}

/// [`super::parse_team`], of `text` as a team's file ([`named`]).
fn parse_team(text: &str) -> anyhow::Result<Team> {
    super::parse_team(&named(text))
}

#[test]
fn the_prompt_source_is_optional_and_resolves_like_other_opinions() {
    assert_eq!(parse("").unwrap().instruction_source(), Source::Link);
    for (source, expected) in [
        ("link", Source::Link),
        ("none", Source::None),
        (
            "instructions/instruction.md",
            Source::File("instructions/instruction.md".into()),
        ),
        ("instruction.md", Source::File("instruction.md".into())),
    ] {
        let own = parse(&format!("[instructions]\nsource = {source:?}\n")).unwrap();
        let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
        assert_eq!(resolved.spec.instruction_source(), expected);
        assert_eq!(resolved.from["instructions.source"], From::Yours);
        let json = serde_json::to_string(&resolved.spec).unwrap();
        let kept: Spec = serde_json::from_str(&json).unwrap();
        assert_eq!(kept.instruction_source(), expected);
    }
    for source in [
        "",
        "sections",
        "../instruction.md",
        "/etc/instruction.md",
        ".hidden/a.md",
        "instructions/a.txt",
    ] {
        let err = parse(&format!("[instructions]\nsource = {source:?}\n")).unwrap_err();
        assert!(
            format!("{err:#}").contains("[instructions] source"),
            "{err:#}"
        );
    }
}

#[test]
fn a_plain_prompt_requires_no_assembly_keys_when_extending_none() {
    for source in ["instructions/instruction.md", "none", "link"] {
        let mut own = link();
        own.extends = Some(Extends::None);
        own.instructions = Some(Instructions {
            source: Some(source.into()),
            ..Instructions::default()
        });
        let resolved = resolve(Some(&own), &[], &known("main"));
        if source == "link" {
            let err = resolved.unwrap_err();
            assert!(err.to_string().contains("instructions.base"), "{err:#}");
        } else {
            let resolved = resolved.unwrap();
            assert_eq!(
                resolved.spec.instruction_source(),
                Source::parse(source).unwrap()
            );
            assert_eq!(resolved.from["instructions.source"], From::Yours);
            assert!(!resolved.from.contains_key("instructions.base"));
        }
    }
}

fn known(name: &str) -> Known {
    Known { name: name.into() }
}

#[test]
fn links_spec_parses_and_is_whole() {
    let link = parse(LINK_SPEC).unwrap();
    let resolved = resolve(None, &[], &known("main")).unwrap();
    assert_eq!(resolved.spec.turns.as_ref().unwrap().rounds, Some(8));
    assert_eq!(resolved.from["turns.rounds"], From::Link);
    assert_eq!(resolved.from["name"], From::Yours);
    assert_eq!(
        resolved.model_for("anthropic").default.as_deref(),
        Some("claude-haiku-5-5")
    );
    assert_eq!(
        resolved.model_for("anthropic").thinking,
        Some(Thinking::Low)
    );
    assert_eq!(resolved.model_for("ollama").default, None);
    assert_eq!(resolved.tool("memory").answer.as_deref(), Some("32 KB"));
    assert_eq!(resolved.tool("gh").timeout, Some("5m".parse().unwrap()));
    assert_eq!(
        resolved.connection("slack").working.as_deref(),
        Some("eyes")
    );
    // Discord marks the message being answered with eyes, as Slack does.
    assert_eq!(
        resolved.connection("discord").working.as_deref(),
        Some("eyes")
    );
    // Each tool call's line reads `› run command: git status`.
    assert_eq!(
        resolved.connection("telegram").activity_mark.as_deref(),
        Some("›")
    );
    assert_eq!(
        resolved
            .connection("telegram")
            .activity_separator
            .as_deref(),
        Some(": ")
    );
    assert_eq!(
        resolved.connection("discord").pace,
        Some(link_actions::value::Duration::seconds(1))
    );
    assert_eq!(
        resolved.connection("telegram").activity,
        Some(crate::Activity::Replace)
    );
    assert!(link.name.is_none());
}

/// Code's servers: Link's, each key of one changed alone, a language
/// added, and an extension served twice in the union refused.
#[test]
fn code_s_servers_are_changed_key_by_key() {
    let link = resolve(None, &[], &known("main")).unwrap().tool("code");
    let servers = link.servers;
    assert_eq!(servers["rust"].command(), ["rust-analyzer"]);
    assert_eq!(servers["python"].files(), ["py", "pyi"]);
    let own = parse(
        "[tool.code.servers.rust]\ncommand = [\"/opt/ra\"]\n\
         [tool.code.servers.zig]\ncommand = [\"zls\"]\nfiles = [\"zig\"]\n",
    )
    .unwrap();
    let servers = resolve(Some(&own), &[], &known("main"))
        .unwrap()
        .tool("code")
        .servers;
    assert_eq!(servers["rust"].command(), ["/opt/ra"]);
    assert_eq!(servers["rust"].roots(), ["Cargo.toml"]);
    assert_eq!(servers["zig"].files(), ["zig"]);
    let twice = parse("[tool.code.servers.zig]\nfiles = [\"rs\"]\n").unwrap();
    let refused = resolve(Some(&twice), &[], &known("main")).unwrap_err();
    assert!(
        format!("{refused:#}").contains(".rs files are both"),
        "{refused:#}"
    );
}

/// A spec's VM keeps its shape, and is refused while this build has no
/// VM provider: an agent is never made in a sandbox in its place.
#[test]
fn a_spec_naming_a_vm_is_refused() {
    let refused = parse("[isolation]\nvm = \"vm.toml\"\n").unwrap_err();
    assert!(
        format!("{refused:#}").contains("no VM provider"),
        "{refused:#}"
    );
}

#[test]
fn yours_wins_key_by_key() {
    let own = parse("[turns]\nrounds = 3\n\n[connection.slack]\nstream = false\n").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    let turns = resolved.spec.turns.clone().unwrap();
    assert_eq!((turns.rounds, turns.calls), (Some(3), Some(16)));
    assert_eq!(resolved.from["turns.rounds"], From::Yours);
    assert_eq!(resolved.from["turns.calls"], From::Link);
    // A table of yours replaces only the keys it has.
    let slack = resolved.connection("slack");
    assert_eq!(slack.working.as_deref(), Some("eyes"));
    assert_eq!(slack.stream, Some(false));
    let printed = resolved.print().unwrap();
    assert!(printed.contains("rounds = 3  # yours"), "{printed}");
    assert!(printed.contains("calls = 16  # Link's"), "{printed}");
    // What it prints is a spec Link reads.
    let back = parse(&printed).unwrap();
    assert_eq!(back.turns, resolved.spec.turns);
}

#[test]
fn extending_link_is_leaving_extends_out() {
    let link = resolve(None, &[], &known("main")).unwrap();
    let own = parse("extends = \"link\"\n").unwrap();
    assert_eq!(link, resolve(Some(&own), &[], &known("main")).unwrap());
}

/// Every one of Link's tools starts off, and the owner's own switch
/// turns each on.
#[test]
fn links_built_in_tools_stay_off_until_enabled() {
    for tool in ["file", "scheduler", "run", "code", "mcp"] {
        let off = resolve(None, &[], &known("main")).unwrap();
        assert_eq!(off.tool(tool).enabled, Some(false), "{tool}");
        let own = parse(&super::with_tool_enabled(&named(""), tool, true).unwrap()).unwrap();
        let on = resolve(Some(&own), &[], &known("main")).unwrap();
        assert_eq!(on.tool(tool).enabled, Some(true), "{tool}");
    }
}

#[test]
fn extending_none_needs_every_key() {
    let own = parse("extends = \"none\"\n[turns]\nrounds = 3\n").unwrap();
    let err = resolve(Some(&own), &[], &known("main")).unwrap_err();
    assert!(format!("{err}").contains("model.thinking"), "{err}");
    let whole = format!("extends = \"none\"\n{LINK_SPEC}");
    let own = parse(&whole).unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    assert_eq!(resolved.from["turns.rounds"], From::Yours);
}

/// A spec that extends none needn't set Link's tools' keys: each is
/// its tool's to need. A tool from outside Link has keys of its own;
/// Link's tools take only theirs.
#[test]
fn each_tool_has_its_own_table() {
    let mut links: toml::Table = toml::from_str(LINK_SPEC).unwrap();
    links.remove("tool");
    links.remove("schema");
    let mut own = format!("extends = \"none\"\n{}", toml::to_string(&links).unwrap());
    own += "\n[tool.weather]\nunits = \"metric\"\ntimeout = \"20s\"\n";
    let resolved = resolve(Some(&parse(&own).unwrap()), &[], &known("main")).unwrap();
    assert_eq!(resolved.tool("scheduler").per_day, None);
    let weather = resolved.tool("weather");
    assert_eq!(weather.others["units"].as_str(), Some("metric"));
    assert_eq!(weather.timeout, Some("20s".parse().unwrap()));
    for typo in [
        "[tool.memory]\nkeeps = \"10m\"\n",
        "[tools]\nunits = \"metric\"\n",
        "[tool.weather]\n\"Units!\" = 1\n",
    ] {
        assert!(parse(typo).is_err(), "{typo}");
    }
}

#[test]
fn a_spec_cant_claim_what_isnt_its() {
    let k = known("main");
    let other = parse("name = \"work\"\n").unwrap();
    assert!(resolve(Some(&other), &[], &k).is_err());
    // Whose it is and its teams are no spec's to say: neither is a key.
    assert!(parse("owner = \"github-9\"\n").is_err());
    assert!(parse("teams = [\"research\"]\n").is_err());
}

#[test]
fn values_are_checked() {
    for bad in [
        "[turns]\nrounds = 0",
        "[turns]\ncalls = 0",
        "[window]\nmax = 10",
        "[tools]\nanswer = \"lots\"",
        "[tools]\ntimeout = \"500ms\"",
        "[tool.gh]\ntimeout = \"soon\"",
        "[tools]\nkeep = \"10m\"",
        "[tool.web]\ntokens = 100",
        "[tool.web]\nfetch = \"2 MB\"",
        "[tool.web]\nfetch = \"10 B\"",
        "[tool.memory]\nfetch = \"24 KB\"",
        "[tool.memory]\nhosts = [\"docs.rs\"]",
        "[tool.web]\nhosts = [\"not a host\"]",
        "[tools]\nfetch = \"24 KB\"",
        "[tool.memory]\nkeep = \"soon\"",
        "[connections]\npace = \"0s\"",
        "[window]\nmargin = 0.6",
        "[window]\nanswer = 1.0",
        "[window]\ncompact_at = 1.0",
        "[window]\ncompact_at = 0",
        "[window]\ncompact_at = \"soon\"",
        "[window]\nsave_notes = \"yes\"",
        "[turns]\nrenew = \"10s\"\nlapse = \"15s\"",
        "[tool.scheduler]\nzone = \"../etc\"",
        "[tool.scheduler]\nfiring = \"0s\"",
        "[tool.scheduler]\nfiring = \"2d\"",
        "[tool.memory]\nfiring = \"30m\"",
        "[tool.memory]\nask = true",
        "[tool.memory]\noutput = \"16 KB\"",
        "[tool.run]\noutput = \"10 B\"",
        "[tool.run]\nbackground = 65",
        "[tool.run]\nbackground_timeout = \"8d\"",
        "[tool.run]\nbackground_output = \"2 GB\"",
        "[tool.run]\nbackground_keep = \"10s\"",
        "[tool.memory]\nbackground = 1",
        "[tool.run.servers.rust]\ncommand = [\"rust-analyzer\"]",
        "[tool.code.servers.Rust]\nfiles = [\"rs\"]",
        "[tool.code.servers.rust]\ncommand = [\" \"]",
        "[tool.code.servers.rust]\nfiles = [\".rs\"]",
        "[tool.code.servers.rust]\nroots = [\"../Cargo.toml\"]",
        "[tool.code.servers.rust]\nhelp = \"x\"",
        "[tool.code.servers.zig]\nfiles = [\"rs\"]\n[tool.code.servers.rust]\nfiles = [\"rs\"]",
        "[tools]\nbackground_keep = \"1d\"",
        "[connection.slack]\nworking = \"no way\"",
        "[connection.telegram]\nworking = \"eyes\"",
        "[connection.signal]\nsteered = \"eyes\"",
        "[window]\nmargin = 0.4\nanswer = 0.6",
        "[turns]\nrounds = 70000",
        "[tool.scheduler]\nhops = 0",
        "[model]\nthinking = \"hard\"",
        "[model.anthropic.openai]\ndefault = \"x\"",
        "extends = \"another\"",
        "[turns]\nrounds = 3\nbusy = \"x\"",
        "[instructions]\nbase = \"Be a cat.\"",
        "[instructions]\nbase = \"../etc/passwd\"",
        "[instructions]\nbase = \"/etc/passwd\"",
        "[instructions]\nbase = \".hidden/base.md\"",
        "[instructions]\nstanding = \"link\"",
        "[instructions]\npersona = \"link\"",
        "[instructions]\nowner = \"link\"",
        "[instructions.max]\nowner = \"1 MB\"",
        "[instructions]\nnow = \"instructions/now.md\"",
        "[instructions]\nnotes = \"link\"",
        "[instructions.max]\nnotes = \"1 MB\"",
        "[tool.memory]\nnotes = \"1 MB\"",
        "[tool.memory]\nnotes = \"many\"",
        "[tool.scheduler]\nnotes = \"4 KB\"",
        "[instructions.max]\nstanding = \"1 MB\"",
        "[instructions.max]\nnow = \"10 B\"",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    // A section's source: Link's, none, or a file of the agent's.
    let own = parse(
        "[instructions]\nbase = \"instructions/base.md\"\nenvironment = \"none\"\n\
         [instructions.max]\nstanding = \"1 KB\"",
    )
    .unwrap();
    let instructions = own.instructions.unwrap();
    assert_eq!(
        instructions.section("base"),
        (Some(Source::File("instructions/base.md".into())), None)
    );
    assert_eq!(instructions.section("environment").0, Some(Source::None));
    assert_eq!(instructions.section("standing").1, Some(1024));
    // The notes: the file memory's remember writes, as large as it may.
    let links = link();
    assert_eq!(
        links.instructions.as_ref().unwrap().section("notes"),
        (
            Some(Source::File(crate::harness::layout::NOTES.into())),
            Some(4096)
        )
    );
    assert_eq!(links.tool("memory").notes.as_deref(), Some("4 KB"));
    // Who it is and who its owner is: files of yours, apart from the
    // standing instructions.
    for (name, path) in [
        ("persona", "instructions/persona.md"),
        ("owner", "instructions/owner.md"),
    ] {
        assert_eq!(
            links.instructions.as_ref().unwrap().section(name),
            (Some(Source::File(path.into())), Some(8192))
        );
    }
    let old = parse("[instructions]\nnotes = \"instructions/notes.md\"")
        .unwrap()
        .instructions
        .unwrap();
    assert_eq!(
        old.section("notes").0,
        Some(Source::File(crate::harness::layout::NOTES.into()))
    );
    assert_eq!(old.notes.as_deref(), Some("instructions/notes.md"));
    assert_eq!(size("32 KB").unwrap(), 32 << 10);
    assert_eq!(size("1.5MB").unwrap(), 3 << 19);
    assert_eq!(duration_ms("10m").unwrap(), 600_000);
    assert_eq!(duration_ms("250ms").unwrap(), 250);
    assert!(duration_ms("1y").is_err());
    // Past what Link is tested with: taken, and said.
    let own = parse("[turns]\nrounds = 200\n[tool.scheduler]\nhops = 65").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    assert_eq!(unusual(&resolved.spec).len(), 2);
    assert!(unusual(&link()).is_empty());
}

/// What wakes the scheduler: the bridge by Link's spec, the calendar
/// or the machine's address by yours; nothing else.
#[test]
fn the_schedulers_driver_is_one_of_three() {
    assert_eq!(
        link().tool("scheduler").driver,
        Some(SchedulerDriver::Service)
    );
    for (text, driver) in [
        ("service", SchedulerDriver::Service),
        ("cron", SchedulerDriver::Cron),
        ("address", SchedulerDriver::Address),
    ] {
        let own = parse(&format!("[tool.scheduler]\ndriver = \"{text}\"\n")).unwrap();
        let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
        assert_eq!(resolved.spec.tool("scheduler").driver, Some(driver));
    }
    for bad in [
        "[tool.scheduler]\ndriver = \"socket\"",
        "[tool.scheduler]\ndriver = \"Address\"",
        "[tool.memory]\ndriver = \"address\"",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

/// Whether the agent's recurring schedules ask: `ask` by Link's spec,
/// `allow` by yours, from its file or its flag alike; nothing else, and
/// only in `[tool.scheduler]`.
#[test]
fn a_recurring_schedule_asks_unless_the_spec_allows_it() {
    assert_eq!(
        link().tool("scheduler").recurring,
        Some(SchedulerRecurring::Ask)
    );
    for (text, recurring) in [
        ("ask", SchedulerRecurring::Ask),
        ("allow", SchedulerRecurring::Allow),
    ] {
        let own = parse(&format!("[tool.scheduler]\nrecurring = \"{text}\"\n")).unwrap();
        let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
        assert_eq!(resolved.spec.tool("scheduler").recurring, Some(recurring));
        let binding = link_spec::Binding::of::<Spec>();
        let args = ["lnk", "--tool.scheduler.recurring", text].map(std::ffi::OsString::from);
        let (flags, rest) = link_spec::split(args, Some(&binding), None).unwrap();
        assert_eq!(rest, ["lnk"]);
        let flagged = link_spec::read_text_with::<Spec>("", &flags).unwrap();
        assert_eq!(flagged.tool("scheduler").recurring, Some(recurring));
    }
    for bad in [
        "[tool.scheduler]\nrecurring = \"never\"",
        "[tool.scheduler]\nrecurring = true",
        "[tool.memory]\nrecurring = \"allow\"",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    let binding = link_spec::Binding::of::<Spec>();
    let args = ["lnk", "--tool.scheduler.recurring", "never"].map(std::ffi::OsString::from);
    let refused = link_spec::split(args, Some(&binding), None)
        .and_then(|(flags, _)| link_spec::read_text_with::<Spec>("", &flags));
    assert!(refused.is_err(), "the flag takes what the file does");
}

/// A general key of yours shadows the specific ones of the sets
/// before it; an empty reaction is none; a file's owner isn't kept.
#[test]
fn an_agent_stops_when_idle_as_its_spec_says() {
    let link = resolve(None, &[], &known("main")).unwrap();
    let run = link.spec.run.clone().unwrap();
    assert_eq!(run.idle_after(), Some(std::time::Duration::from_secs(300)));
    let never = parse("[run]\nidle_after = \"never\"\n").unwrap();
    assert_eq!(never.run.unwrap().idle_after(), None);
    for bad in ["0s", "500ms", "soon"] {
        assert!(
            parse(&format!("[run]\nidle_after = \"{bad}\"\n")).is_err(),
            "{bad}"
        );
    }
    assert!(parse("[run]\nidle_after = \"1s\"\n").is_ok());
}

/// A quiet chat on a connection compacts as what it costs chooses, its
/// cache too, and a conversation of `lnk agent chat` (`link`) never;
/// yours wins, a connection's own over `[connections]`; a duration less
/// than a minute, or none, is refused, and so are pauses that aren't.
#[test]
fn a_quiet_chat_compacts_as_its_connection_says() {
    use std::time::Duration;
    let link = resolve(None, &[], &known("main")).unwrap().spec;
    assert_eq!(link.compact_after("telegram"), Some(Quiet::Auto));
    assert_eq!(link.compact_after("link"), None);
    assert_eq!(link.cache_for("telegram"), None);
    let minute = Duration::from_secs(60);
    assert_eq!(
        link.pauses("telegram"),
        [minute, minute, minute * 20, minute * 60 * 8]
    );
    let own = parse(
        "[connections]\ncompact_after = \"never\"\ncache = \"1h\"\n[connection.link]\ncompact_after = \"1m\"\n",
    )
    .unwrap();
    let yours = resolve(Some(&own), &[], &known("main")).unwrap().spec;
    assert_eq!(yours.compact_after("telegram"), None);
    assert_eq!(yours.compact_after("link"), Some(Quiet::After(minute)));
    assert_eq!(yours.cache_for("telegram").as_deref(), Some("1h"));
    for (key, bad) in [
        ("compact_after", "\"59s\""),
        ("compact_after", "\"soon\""),
        ("cache", "\"0s\""),
        ("cache", "\"always\""),
        ("pauses", "[]"),
        ("pauses", "[\"0s\"]"),
        ("pauses", "[\"later\"]"),
    ] {
        for table in ["connections", "connection.telegram"] {
            assert!(
                parse(&format!("[{table}]\n{key} = {bad}\n")).is_err(),
                "[{table}] {key} = {bad}"
            );
        }
    }
}

#[test]
fn your_general_key_beats_links_specific_one() {
    let own =
        parse("[model]\ndefault = \"claude-opus-5-5\"\n[connections]\nworking = \"\"\n").unwrap();
    let nobodys = known("main");
    let resolved = resolve(Some(&own), &[], &nobodys).unwrap();
    assert_eq!(
        resolved.model_for("anthropic").default.as_deref(),
        Some("claude-opus-5-5")
    );
    assert_eq!(resolved.from["model.default"], From::Yours);
    assert!(!resolved.from.contains_key("model.anthropic.default"));
    assert_eq!(resolved.connection("slack").working, None);
    // Your specific key still wins over your general one.
    let own = parse(
        "[model]\ndefault = \"anthropic/claude-sonnet-5-5\"\n[model.openai]\ndefault = \"b\"\n",
    )
    .unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    assert_eq!(resolved.model_for("openai").default.as_deref(), Some("b"));
    assert_eq!(
        resolved.model_for("anthropic").default.as_deref(),
        Some("claude-sonnet-5-5")
    );
}

/// `[model] default` says who serves it: it's the default of that
/// provider alone, as that provider's own id.
#[test]
fn a_routed_default_is_its_own_providers_alone() {
    let own = parse("[model]\ndefault = \"openrouter/deepseek/deepseek-v4\"\n").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    assert_eq!(
        resolved.model_for("openrouter").default.as_deref(),
        Some("deepseek/deepseek-v4")
    );
    let own = parse("[model]\ndefault = \"local/qwen3:8b\"\n").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    assert_eq!(
        resolved.model_for("ollama").default.as_deref(),
        Some("qwen3:8b")
    );
    assert_ne!(
        resolved.model_for("openai").default.as_deref(),
        Some("qwen3:8b")
    );
    // A bare name with no owner Link knows: here's, else OpenRouter's.
    let own = parse("[model]\ndefault = \"qwen3:8b\"\n").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    for provider in ["ollama", "openrouter"] {
        assert_eq!(
            resolved.model_for(provider).default.as_deref(),
            Some("qwen3:8b")
        );
    }
    assert_ne!(
        resolved.model_for("anthropic").default.as_deref(),
        Some("qwen3:8b")
    );
}

/// `lnk agent spec show` on a machine: each collection where it's kept, a
/// place the machine overrode marked its, naming the agent's.
#[test]
fn a_spec_on_a_machine_marks_what_the_machine_overrode() {
    let mut spec = link();
    spec.storage = Some(Storage {
        conversations: Some("bucket".into()),
        skills: Some("sqlite".into()),
        ..Storage::default()
    });
    let resolved = Resolved {
        spec,
        from: [("storage.conversations".to_string(), From::Yours)].into(),
    };
    let machine: crate::storage::Supported =
        toml::from_str("conversations = [\"file\", \"sqlite\"]").unwrap();
    let on = resolved.on_machine(&machine.resolve(&resolved_storage()));
    let storage = on.spec.storage.clone().unwrap();
    assert_eq!(storage.conversations.as_deref(), Some("file"));
    assert_eq!(storage.skills.as_deref(), Some("sqlite"));
    assert_eq!(storage.home.as_deref(), Some("file"));
    assert_eq!(
        on.from["storage.conversations"],
        From::Machine("bucket".into())
    );
    assert_eq!(on.from["storage.home"], From::Link);
    let printed = on.print().unwrap();
    assert!(
        printed.contains("conversations = \"file\"  # this machine's: it doesn't support bucket"),
        "{printed}"
    );

    fn resolved_storage() -> Storage {
        Storage {
            conversations: Some("bucket".into()),
            skills: Some("sqlite".into()),
            ..Storage::default()
        }
    }
}

/// The features' switches: each table takes only its own, and Link's
/// spec turns every one on.
#[test]
fn each_switch_is_its_tables_own() {
    for (text, says) in [
        (
            "[tool.memory]\nenabled = false\n",
            "enabled: only [tool.<name>] of",
        ),
        (
            "[tools]\nenabled = false\n",
            "enabled: only [tool.<name>] of",
        ),
        ("[tool.run]\nfind = false\n", "find: only [tool.file]"),
        ("[tool.code]\ndiffs = false\n", "diffs: only [tool.file]"),
        (
            "[model.anthropic]\nfallback = false\n",
            "unknown field `fallback`",
        ),
        (
            "[instructions]\nrepository = \"AGENTS.md\"\n",
            "repository is link or none",
        ),
        ("[commands]\ninit = true\n", "unknown field"),
    ] {
        let err = parse(text).unwrap_err();
        assert!(format!("{err:#}").contains(says), "{text}: {err:#}");
    }
    for table in SWITCHED {
        parse(&format!("[tool.{table}]\nenabled = false\n")).unwrap();
    }
    parse("[tool.file]\nfind = false\ndiffs = false\n[connection.slack]\nimages = false\n")
        .unwrap();
    let link = resolve(None, &[], &known("main")).unwrap();
    for table in SWITCHED {
        assert_eq!(link.tool(table).enabled, Some(false), "{table}");
    }
    let file = link.tool("file");
    assert_eq!((file.find, file.diffs), (Some(true), Some(true)));
    let slack = link.connection("slack");
    assert_eq!((slack.images, slack.documents), (Some(true), Some(true)));
    assert_eq!(link.spec.model.as_ref().unwrap().fallback, Some(true));
    assert_eq!(link.spec.commands.as_ref().unwrap().create, Some(true));
    let instructions = link.spec.instructions.as_ref().unwrap();
    assert_eq!(instructions.repository.as_deref(), Some("none"));
    assert_eq!(
        instructions.section("repository"),
        (Some(Source::None), Some(16 << 10))
    );
}

#[test]
fn start_settings_have_spec_keys_and_resolve_before_starting() {
    let own = parse(
        r#"
[model]
fallback_model = "gpt-6.1-sol"
[run]
files = "/project"
let_sleep = true
"#,
    )
    .unwrap();
    let resolved = resolve(
        Some(&own),
        &[],
        &Known {
            name: "main".into(),
        },
    )
    .unwrap();
    let model = resolved.spec.model.unwrap();
    assert_eq!(model.fallback_model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(model.fallback, Some(true));
    let run = resolved.spec.run.unwrap();
    assert_eq!(run.files.as_deref(), Some(std::path::Path::new("/project")));
    assert_eq!(run.let_sleep, Some(true));
    assert!(parse("[model.openai]\nfallback_model = 'gpt-5'").is_err());
    assert!(parse("[model]\nfallback_model = ''").is_err());
}

/// A fallback is a hosted model: one named on this machine is refused,
/// by its name or an alias, even with falling back switched off.
#[test]
fn a_fallback_on_this_machine_is_refused() {
    for text in [
        "[model]\nfallback_model = 'local/qwen3'\n",
        "[model]\nfallback = false\nfallback_model = 'local/qwen3'\n",
        "[model]\nfallback_model = 'near'\n[model.aliases]\nnear = 'local/qwen3:8b'\n",
    ] {
        let err = parse(text).unwrap_err();
        assert!(
            format!("{err:#}").contains("runs on this machine"),
            "{text}: {err:#}"
        );
    }
    parse("[model]\nfallback_model = 'openrouter/qwen/qwen3-32b'\n").unwrap();
}

/// `[model.aliases]` names models: read by [`Spec::model_named`], only
/// in `[model]`, each a word for a model.
#[test]
fn aliases_name_models() {
    let spec = parse("[model.aliases]\nhigh = 'claude-opus-5-5'\n").unwrap();
    assert_eq!(spec.model_named("high"), "claude-opus-5-5");
    assert_eq!(spec.model_named("gpt-6.1-sol"), "gpt-6.1-sol");
    assert_eq!(parse("").unwrap().model_named("high"), "high");
    assert!(parse("[model.openai.aliases]\nhigh = 'gpt-5'").is_err());
    assert!(parse("[model.aliases]\nhigh = ''").is_err());
    assert!(parse("[model.aliases]\n'two words' = 'gpt-5'").is_err());
}

/// A spec with a `[channels]` table, which Link has no longer: refused, never
/// read as `[connections]`.
#[test]
fn a_spec_with_a_channels_table_is_refused() {
    let refused =
        crate::agent_spec::parse("schema = 1\n[channels]\nmax_chars = 100\n").unwrap_err();
    assert!(format!("{refused:#}").contains("channels"), "{refused:#}");
}

#[test]
fn interaction_opinions_inherit_without_replacing_the_existing_wait_default() {
    use link_interactions_contract::{DomainId, Fallback};
    let own = parse("[approvals.domains.question]\nwait_secs = 45\nchoice = 'use_recommended'\nallow_request_overrides = false\n").unwrap();
    let resolved = resolve(Some(&own), &[], &known("main")).unwrap();
    let question = &resolved.spec.approvals.unwrap().domains[&DomainId::new("question").unwrap()];
    assert_eq!(question.wait_secs, Some(45));
    assert_eq!(question.choice, Some(Fallback::UseRecommended));
    assert_eq!(
        resolved.from["approvals.domains.question.wait_secs"],
        From::Yours
    );
    assert_eq!(
        resolved.spec.connections.unwrap().ask_wait,
        link().connections.unwrap().ask_wait
    );
    for invalid in [
        "wait_secs = 0",
        "wait_secs = 86401",
        "choice = 'allow'",
        "unrecognized = true",
    ] {
        assert!(
            parse(&format!("[approvals.domains.question]\n{invalid}\n")).is_err(),
            "{invalid}"
        );
    }
}

/// A path field's flag is made absolute from the folder the command runs
/// in, so the spec it makes means the same wherever it's read.
#[test]
fn a_path_fields_flag_is_absolute() {
    let binding = link_spec::Binding::of::<Spec>();
    let args = ["lnk", "--run.files", "work", "--env", "env.toml"].map(std::ffi::OsString::from);
    let (flags, _) = link_spec::split(args, Some(&binding), None).unwrap();
    for path in [["run", "files"].as_slice(), &["env"]] {
        let value = flags.get(path).and_then(|v| v.as_str()).unwrap();
        assert!(
            std::path::Path::new(value).is_absolute(),
            "{path:?}: {value}"
        );
    }
}

/// `[tool.web] hosts` names the hosts web fetches, as `[internet] hosts`
/// names them, and Link's spec names none, so web fetches any public host.
#[test]
fn web_hosts_are_the_specs() {
    let spec =
        parse("[tool.web]\nhosts = [\"docs.rs\", \"*.example.org\", \"10.0.0.2\"]\n").unwrap();
    assert_eq!(
        spec.tool("web").hosts.unwrap(),
        ["docs.rs", "*.example.org", "10.0.0.2"]
    );
    assert_eq!(crate::link().tool("web").hosts, None);
}

/// `[tool.browser] program` names the browser's program by its absolute
/// path, only in the browser's table, and its flag holds it to the same
/// checks; Link's spec names none, so the browser finds one.
#[test]
fn a_browser_program_is_the_browsers_and_absolute() {
    let spec = parse("[tool.browser]\nprogram = \"/opt/chromium/chrome\"\n").unwrap();
    assert_eq!(
        spec.tool("browser").program.as_deref(),
        Some("/opt/chromium/chrome")
    );
    assert_eq!(crate::link().tool("browser").program, None);
    for (bad, said) in [
        (
            "[tool.browser]\nprogram = \"chrome\"",
            "isn't an absolute path",
        ),
        (
            "[tool.web]\nprogram = \"/opt/chromium/chrome\"",
            "only [tool.browser] takes it",
        ),
        (
            "[tools]\nprogram = \"/opt/chromium/chrome\"",
            "only [tool.browser] takes it",
        ),
    ] {
        let refused = parse(bad).expect_err(bad);
        assert!(format!("{refused:#}").contains(said), "{bad}: {refused:#}");
    }
    let binding = link_spec::Binding::of::<Spec>();
    for (value, absolute) in [("/opt/chromium/chrome", true), ("chrome", false)] {
        let args = ["--tool.browser.program", value].map(std::ffi::OsString::from);
        let (flags, rest) = link_spec::split(args, Some(&binding), None).unwrap();
        assert!(rest.is_empty());
        let read = link_spec::read_text_with::<Spec>(&named(""), &flags);
        match absolute {
            true => assert_eq!(
                read.unwrap().tool("browser").program.as_deref(),
                Some(value)
            ),
            false => {
                let refused = read.expect_err("the flag is held to the file's checks");
                assert!(
                    format!("{refused:#}").contains("[tool.browser] program"),
                    "{refused:#}"
                );
            }
        }
    }
}

/// Each tool's own table is keyed by its name, which follows history
/// (`x-link-ref`), in the spec and in an agent's `[behavior]`.
#[test]
fn a_tools_key_follows_history() {
    let sites = link_spec::follow::sites(&link_spec::schema::<crate::Spec>());
    assert!(
        sites.iter().any(|site| site.shown() == "tool" && site.keys),
        "{sites:?}"
    );
    let under = link_spec::follow::sites_under("behavior", &link_spec::schema::<crate::Spec>());
    assert!(
        under
            .iter()
            .any(|site| site.shown() == "behavior.tool" && site.keys)
    );
}

/// Each ready-made agent's spec fits how it runs: a run or code tool's
/// own timeout is within the `[tools]` timeout every call waits for, its
/// own or Link's, which the agent refuses to serve the tool past; and an
/// awake one with the scheduler lets its recurring schedules through.
#[test]
fn every_ready_made_agents_spec_fits_how_it_runs() {
    let agents =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../agents/ready-made");
    let link = crate::link();
    let calls = |spec: &Spec| spec.tools.as_ref().and_then(|tools| tools.timeout);
    let mut seen = 0;
    for agent in std::fs::read_dir(&agents).unwrap() {
        let Ok(owners) = std::fs::read_dir(agent.unwrap().path().join("Safe/Agents")) else {
            continue;
        };
        for owner in owners {
            let path = owner.unwrap().path().join("agent.toml");
            let mut file: toml::Table =
                toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut behavior = match file.remove("behavior") {
                Some(toml::Value::Table(behavior)) => behavior,
                _ => toml::Table::new(),
            };
            behavior.insert("schema".into(), 1.into());
            let spec = parse(&toml::to_string(&behavior).unwrap())
                .unwrap_or_else(|err| panic!("{}: {err:#}", path.display()));
            let all = calls(&spec).or(calls(&link)).unwrap();
            for name in ["run", "code"] {
                let own = spec
                    .tool
                    .get(name)
                    .and_then(|tool| tool.timeout)
                    .or(link.tool(name).timeout);
                if let Some(own) = own {
                    assert!(
                        millis(&own) <= millis(&all),
                        "{}: [tool.{name}] timeout {own} is above [tools] timeout {all}",
                        path.display()
                    );
                }
            }
            // An awake one with the scheduler makes its recurring
            // schedules itself (a coach's check-ins, a morning brief),
            // often with no one there to answer: they start without asking.
            let scheduler = spec.tool("scheduler");
            if file.get("awake").and_then(toml::Value::as_bool) == Some(true)
                && scheduler.enabled == Some(true)
            {
                assert_eq!(
                    scheduler.recurring,
                    Some(SchedulerRecurring::Allow),
                    "{}: its recurring schedules would ask, with no one to answer",
                    path.display()
                );
            }
            seen += 1;
        }
    }
    assert!(
        seen >= 4,
        "{seen} ready-made agents found in {}",
        agents.display()
    );
}
