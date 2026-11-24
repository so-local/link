//! An agent spec's features, as Link Harness runs them: Link's spec
//! alone gives every section of the prompt, and every feature turns off
//! where Link reads it once a spec turns it off.

use crate::common;

use link_agent_spec::{Groups, Known, parse, resolve};
use link_harness::{
    agent::{Agent, Agents},
    commands,
    config::Config,
    event::Kind,
    prompt::{self, Given},
    store::{Files, Store},
};
use link_harness_contract::harness::features::Feature;
use link_test_kit::TempDir;

/// The sections of the prompt `main` is given with its spec `own` (its
/// agent spec's file), each by where it came from, and the features
/// its spec turns on: in a repository with an `AGENTS.md`, with a
/// persona and an owner's profile.
fn sections(name: &str, own: &str) -> (Vec<(String, String)>, Vec<Feature>) {
    let (sections, config) = composed(name, own);
    let features = Feature::of_spec(&config.opinions()).into_iter().collect();
    (sections, features)
}

/// [`sections`]' prompt, and the config Link Harness read it with.
fn composed(name: &str, own: &str) -> (Vec<(String, String)>, Config) {
    let root = TempDir::new(&format!("features-{name}"));
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(files.join(".git")).unwrap();
    std::fs::create_dir_all(files.join("instructions")).unwrap();
    std::fs::create_dir_all(files.join("skills/tides")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        files.join("skills/tides/SKILL.md"),
        "---\nname: tides\ndescription: Reads the tides.\n---\nLook them up.\n",
    )
    .unwrap();
    std::fs::write(files.join("AGENTS.md"), "Run the checks.\n").unwrap();
    std::fs::write(files.join("instructions/persona.md"), "You are Ada.\n").unwrap();
    std::fs::write(files.join("instructions/owner.md"), "Sam, in Leeds.\n").unwrap();
    let agent = Agent::new("main").unwrap();
    let mut agents = Agents::default();
    agents.add(agent.clone(), home, common::flat(&files));
    let store = Files::new(agents);
    let known = Known {
        name: "main".into(),
    };
    let resolved = resolve(
        Some(&parse(&format!("schema = 1\n{own}")).unwrap()),
        &[],
        &known,
    )
    .unwrap();
    let config = Config {
        spec: Some(resolved.spec.clone()),
        defaults: resolved.links(),
        ..Config::default()
    };
    store.set_config(&agent, &config).unwrap();
    let config = store.config(&agent).unwrap();
    let given = Given {
        model: None,
        tools: &[],
        others: false,
    };
    let prompt = prompt::compose(&store, &agent, &config, &common::flat(&files), given).unwrap();
    let sections = prompt
        .sections
        .iter()
        .map(|s| (s.name.to_string(), s.from.clone()))
        .collect();
    (sections, config)
}

fn from<'a>(sections: &'a [(String, String)], name: &str) -> &'a str {
    &sections.iter().find(|(n, _)| n == name).unwrap().1
}

#[test]
fn links_spec_alone_gives_every_section() {
    let (sections, features) = sections("link", "");
    assert_eq!(from(&sections, "repository"), "repository");
    assert_eq!(from(&sections, "persona"), "instructions/persona.md");
    assert_eq!(from(&sections, "owner"), "instructions/owner.md");
    assert!(features.contains(&Feature::Commands));
    // File is on, so asked to, it changes its files, instructions and
    // skills; the other tools are off until added.
    for on in [
        Feature::FileRead,
        Feature::FileEdit,
        Feature::CodeSearch,
        Feature::Diffs,
    ] {
        assert!(features.contains(&on), "{on:?}");
    }
    for off in [
        Feature::Subagents,
        Feature::Schedules,
        Feature::Proactive,
        Feature::Shell,
        Feature::Lsp,
    ] {
        assert!(!features.contains(&off), "{off:?}");
    }
}

#[test]
fn a_spec_turns_sections_off_by_their_keys() {
    let (sections, features) = sections(
        "sections-off",
        "[instructions]\nrepository = \"none\"\npersona = \"none\"\nowner = \"none\"\n",
    );
    for section in ["repository", "persona", "owner"] {
        assert_eq!(from(&sections, section), "none", "{section}");
    }
    for off in [Feature::RepoInstructions, Feature::Persona] {
        assert!(!features.contains(&off), "{off:?}");
    }
    assert!(features.contains(&Feature::Commands));
}

/// Whether Link reads `feature` as on, where it reads it: the prompt's
/// sections, the commands, what a message brings, the connection's
/// switches, the fallback, and the tools Link starts (as `lnk agent
/// start` decides, `[tool.<name>] enabled`). Every feature has its arm:
/// one added later doesn't compile until it says where it's read.
fn read_on(feature: Feature, sections: &[(String, String)], config: &Config) -> bool {
    let section = |name: &str| {
        sections
            .iter()
            .find(|(n, _)| n == name)
            .is_some_and(|(_, from)| from != "none")
    };
    let command = |name: &str| commands::list(config).iter().any(|c| c.name == name);
    let opinions = config.opinions();
    let connection = opinions.connection("telegram");
    let tool = |name: &str| opinions.tool(name).enabled == Some(true);
    let file = opinions.tool("file");
    match feature {
        Feature::RepoInstructions => section("repository"),
        Feature::Persona => section("persona") || section("owner"),
        Feature::Skills => section("skills"),
        Feature::Commands => command("help"),
        Feature::Goals => command("chat:goal"),
        Feature::ModelFallback => opinions.model.as_ref().and_then(|m| m.fallback) == Some(true),
        Feature::Images => config.takes("telegram:1", Kind::Image),
        Feature::Attachments => config.takes("telegram:1", Kind::Document),
        Feature::Voice => config.takes("telegram:1", Kind::Voice),
        Feature::Groups => connection.groups.is_some_and(|g| g != Groups::Never),
        Feature::Streaming => connection.stream == Some(true),
        Feature::Activity => {
            connection.stream == Some(true)
                && connection
                    .activity
                    .is_some_and(link_agent_spec::Activity::shows)
        }
        Feature::Subagents | Feature::Schedules | Feature::Proactive => tool("scheduler"),
        Feature::Shell => tool("run"),
        Feature::FileRead | Feature::FileEdit => tool("file"),
        Feature::CodeSearch => tool("file") && file.find == Some(true),
        Feature::Diffs => tool("file") && file.diffs == Some(true),
        Feature::Lsp => tool("code"),
    }
}

/// A spec turning `feature` off: one of its groups of keys
/// ([`Feature::switches`]), the last that a spec can turn off, each key
/// false, `"none"` or `"never"`, whichever the key takes.
fn turned_off(feature: Feature) -> String {
    let valid = |spec: &str| {
        let known = Known {
            name: "main".into(),
        };
        parse(&format!("schema = 1\n{spec}"))
            .and_then(|own| resolve(Some(&own), &[], &known))
            .ok()
    };
    for group in feature.switches().iter().rev() {
        // Each key's off value, by its table: a table is written once.
        let mut tables = std::collections::BTreeMap::<&str, String>::new();
        for key in group.iter() {
            let (table, key) = key.rsplit_once('.').unwrap();
            let line = ["false", "\"none\"", "\"never\""]
                .iter()
                .map(|off| format!("{key} = {off}\n"))
                .find(|line| valid(&format!("[{table}]\n{line}")).is_some());
            match line {
                Some(line) => tables.entry(table).or_default().push_str(&line),
                None => tables.clear(),
            }
            if tables.is_empty() {
                break;
            }
        }
        if tables.is_empty() {
            continue;
        }
        let spec: String = tables
            .iter()
            .map(|(table, lines)| format!("[{table}]\n{lines}"))
            .collect();
        if valid(&spec).is_some_and(|r| !feature.on(&r.spec)) {
            return spec;
        }
    }
    panic!(
        "no spec can turn {} off: it needs a key of Link's spec",
        feature.id()
    )
}

/// Every feature is on where Link reads it with Link's spec alone, and
/// off there once a spec turns it off. A feature with no key a spec can
/// turn off fails here, as `/goal` did before `[commands] goal`.
#[test]
fn every_feature_turns_off_and_back_on() {
    let (sections, config) = composed(
        "every-on",
        "[tool.scheduler]\nenabled = true\n[tool.run]\nenabled = true\n[tool.file]\nenabled = true\n[tool.code]\nenabled = true\n",
    );
    for feature in Feature::ALL {
        assert!(
            read_on(feature, &sections, &config),
            "{} is off in Link's spec",
            feature.id()
        );
    }
    for feature in Feature::ALL {
        let off = turned_off(feature);
        let (sections, config) = composed(&format!("off-{}", feature.id()), &off);
        assert!(
            !read_on(feature, &sections, &config),
            "{} is still read as on with\n{off}",
            feature.id()
        );
        assert!(!Feature::of_spec(&config.opinions()).contains(&feature));
    }
}
