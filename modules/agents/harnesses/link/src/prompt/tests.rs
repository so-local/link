use super::*;

#[test]
fn a_time_as_people_read_it() {
    // 2026-10-08T12:03:07Z.
    let secs = 1_791_460_987;
    assert_eq!(
        when(secs, 7200),
        "Thursday 8 October 2026, 14:03 (UTC+02:00)"
    );
    assert_eq!(
        when(secs, -5 * 3600 - 1800),
        "Thursday 8 October 2026, 06:33 (UTC-05:30)"
    );
    assert_eq!(when(0, 0), "Thursday 1 January 1970, 00:00 (UTC+00:00)");
}

/// The owner's sections, the persona, the owner's profile and the
/// standing instructions, say what each is and who wrote it: the
/// owner alone, unless the agent may write `instructions/`.
#[test]
fn the_owners_sections_say_who_wrote_them() {
    let environment = |write: &[&str]| Environment {
        connections: Vec::new(),
        folders: Folders {
            write: write.iter().map(|n| n.to_string()).collect(),
            read: Vec::new(),
            teams: Default::default(),
        },
        commands: Vec::new(),
        teams: Vec::new(),
    };
    assert_eq!(yours("standing", " \n", None), "");
    let owners = yours(
        "standing",
        "Answer in French.",
        Some(&environment(&["work"])),
    );
    assert_eq!(
        owners,
        "# Standing instructions\n\nWritten by your owner, who alone changes them:\n\n\
         Answer in French."
    );
    assert_eq!(yours("standing", "Answer in French.", None), owners);
    let either = yours(
        "standing",
        "Answer in French.",
        Some(&environment(&["instructions", "work"])),
    );
    assert!(
        either.contains("Written last by your owner or by you"),
        "{either}"
    );
    assert_eq!(
        yours("persona", "You are Ada, dry and kind.", None),
        "# Who you are\n\nWritten by your owner, who alone changes it:\n\n\
         You are Ada, dry and kind."
    );
    assert_eq!(
        yours("owner", "Sam, a nurse in Leeds, on nights.", None),
        "# Your owner\n\nWritten by your owner, who alone changes it:\n\n\
         Sam, a nurse in Leeds, on nights."
    );
    assert_eq!(yours("owner", "", None), "");
}

/// A model's folder is its name as OpenRouter writes it.
#[test]
fn a_models_folder_is_named_as_openrouter_names_it() {
    let model = |provider: &str, name: &str| Model {
        provider: provider.into(),
        name: Some(name.into()),
        ..Model::default()
    };
    for (provider, name, folder) in [
        ("anthropic", "claude-opus-5-5", "anthropic/claude-opus-5-5"),
        ("openai", "gpt-6-luna", "openai/gpt-6-luna"),
        (
            "openrouter",
            "anthropic/claude-opus-5.5",
            "anthropic/claude-opus-5.5",
        ),
        ("ollama", "qwen3:8b", "ollama/qwen3:8b"),
    ] {
        assert_eq!(
            model_folder(&model(provider, name)).as_deref(),
            Some(format!("instructions/models/{folder}").as_str())
        );
    }
    for name in ["../notes", "a//b", ".hidden", "a/\n"] {
        assert_eq!(model_folder(&model("openrouter", name)), None, "{name}");
    }
}

/// A fallback's model section takes the first's place, whichever of
/// them is empty.
#[test]
fn a_fallback_is_sent_its_own_models_section() {
    let swap = |at: usize, first: &str, second: &str| Swap {
        at,
        first: first.into(),
        second: second.into(),
    };
    let both = "Base.\n\nFirst.\n\nRest.";
    assert_eq!(
        swap(7, "First.", "Second.").apply(both),
        "Base.\n\nSecond.\n\nRest."
    );
    assert_eq!(swap(7, "First.", "").apply(both), "Base.\n\nRest.");
    // The first has none: where it would be, after the base.
    assert_eq!(
        swap(5, "", "Second.").apply("Base.\n\nRest."),
        "Base.\n\nSecond.\n\nRest."
    );
    // No base.
    assert_eq!(
        swap(0, "First.", "Second.").apply("First.\n\nRest."),
        "Second.\n\nRest."
    );
    assert_eq!(swap(0, "", "Second.").apply("Rest."), "Second.\n\nRest.");
    // A prompt not as it was: sent as it is.
    assert_eq!(swap(7, "First.", "Second.").apply("Other."), "Other.");
}

#[test]
fn the_tools_are_listed_whole_within_their_most() {
    assert_eq!(tools(&[], 8192), (String::new(), false));
    let said = [
        (
            "memory".to_string(),
            "Search before you answer.".to_string(),
        ),
        (
            "web".to_string(),
            "Fetch only what you're asked to.".to_string(),
        ),
    ];
    let (text, cut) = tools(&said, 8192);
    assert!(!cut);
    assert_eq!(
        text,
        "# Your tools\n\nWhat each of your tools says of itself:\n\n## memory\n\n\
         Search before you answer.\n\n## web\n\nFetch only what you're asked to.\n"
    );
    let (fewer, cut) = tools(&said, text.len() - 1);
    assert!(cut);
    assert!(fewer.ends_with("Search before you answer.\n"), "{fewer}");
    assert_eq!(tools(&said, 10), (String::new(), true));
}

#[test]
fn a_section_is_cut_where_a_character_starts() {
    assert_eq!(cut_at("héllo", 2), ("h", true));
    assert_eq!(cut_at("hello", 5), ("hello", false));
}

#[test]
fn the_skills_are_listed_whole_within_their_most() {
    let skill = |name: &str, description: &str| Skill {
        name: name.into(),
        description: description.into(),
    };
    assert_eq!(skills(&[], 8192), (String::new(), false));
    let all = [
        skill("pdf", "Read PDF files."),
        skill("release-notes", "Write release notes from a changelog."),
    ];
    let (text, cut) = skills(&all, 8192);
    assert!(!cut);
    assert!(text.starts_with("# Skills\n"), "{text}");
    assert!(
        text.contains(
            "file__read with file_path, or memory__read with path, skills/<name>/SKILL.md"
        ),
        "{text}"
    );
    assert!(
        text.contains("Use file__read when Memory is not configured"),
        "{text}"
    );
    assert!(text.contains("Read the full skill"), "{text}");
    assert!(
        text.ends_with(
            "- pdf: Read PDF files.\n- release-notes: Write release notes from a changelog.\n"
        ),
        "{text}"
    );
    // Room for the first only: the second left out whole.
    let (text, cut) = skills(&all, text.len() - 10);
    assert!(cut);
    assert!(text.ends_with("- pdf: Read PDF files.\n"), "{text}");
}

/// A section's spans: each skill or tool in it by its name, the rest by
/// the section's, every byte of it once, as the bridge counts them.
#[test]
fn each_skill_and_tool_is_spanned_by_name() {
    let found = [
        Skill {
            name: "pdf".into(),
            description: "Read PDF files.".into(),
        },
        Skill {
            name: "tides".into(),
            description: "Read the tides.".into(),
        },
    ];
    let (text, _) = skills(&found, 8192);
    let text = text.trim();
    let items: Vec<(String, String)> = found
        .iter()
        .map(|s| (format!("skills/{}", s.name), skill_line(s)))
        .collect();
    let spans = section_spans("skills", text, 100, &items);
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["skills", "skills/pdf", "skills", "skills/tides"]);
    assert_eq!(spans[0].start, 100);
    assert_eq!(spans.last().unwrap().end, 100 + text.len() as u64);
    assert!(spans.windows(2).all(|w| w[0].end == w[1].start));
    let said = [("memory".to_string(), "Search first.".to_string())];
    let (text, _) = tools(&said, 8192);
    let items = [(
        "tools/memory".to_string(),
        tool_part("memory", "Search first."),
    )];
    let spans = section_spans("tools", text.trim(), 0, &items);
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["tools", "tools/memory"]);
}

/// A team: its member's place, its teammates and how to reach them,
/// and only what of its folder the agent may reach.
#[test]
fn the_environment_says_the_teams() {
    let mut folders = Folders::default();
    folders.teams.insert(
        "research".into(),
        TeamFolders {
            write: vec!["drafts".into()],
            read: vec!["sources".into()],
        },
    );
    let environment = Environment {
        folders,
        teams: vec![
            TeamPlace {
                name: "research".into(),
                id: "t4m9q2x7k1ab".into(),
                member: "writer".into(),
                purpose: Some("drafts the report".into()),
                teammates: vec![link_harness_contract::harness::Teammate {
                    name: "checker".into(),
                    id: "p3q4r5s6t7u8".into(),
                    purpose: Some("checks every claim".into()),
                }],
                shared: Some("/home/me/Link/Teams/research/shared".into()),
            },
            TeamPlace {
                name: "press".into(),
                id: "t5n0r3y8l2cd".into(),
                member: "writer".into(),
                shared: Some("/home/me/Link/Teams/press/shared".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let text = super::environment(Some(&environment), &main_folder());
    for said in [
        "You're writer on team research: drafts the report. Your teammates:\n\
         - checker (agent p3q4r5s6t7u8): checks every claim\n",
        &format!(
            "agent__send, {} one's name or research for all of them",
            super::code("to")
        ),
        &format!(
            "The team shares a folder. With file, you may change {}; you may read {}, \
             and nothing else of it. Your owner finds it at /home/me/Link/Teams/research/shared.",
            super::code("teams/research/drafts/"),
            super::code("teams/research/sources/")
        ),
        "You're writer on team press. No one else is on it yet.",
    ] {
        assert!(text.contains(said), "{said:?} in {text}");
    }
    assert!(!text.contains("scheduler__send"), "{text}");
    // Press grants it nothing of its folder: not named.
    assert!(!text.contains("press/shared"), "{text}");
    assert!(!text.contains("teams/press/"), "{text}");
    // Never the host's path joined to the file tool's.
    assert!(!text.contains("teams/research/shared"), "{text}");
}

/// A team's whole folder granted: named as the file tool takes it,
/// `teams/<team>/`, never by its host path joined to it.
#[test]
fn the_environment_names_a_whole_team_folder_as_file_takes_it() {
    let mut folders = Folders::default();
    folders.teams.insert(
        "life".into(),
        TeamFolders {
            write: vec![link_harness_contract::harness::WHOLE.into()],
            read: Vec::new(),
        },
    );
    let environment = Environment {
        folders,
        teams: vec![TeamPlace {
            name: "life".into(),
            id: "t6p1s4z9m3ef".into(),
            member: "coach".into(),
            shared: Some("/home/me/Link/Teams/life/shared".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let text = super::environment(Some(&environment), &main_folder());
    let said = format!(
        "With file, you may change {}, and nothing else of it.",
        super::code("teams/life/")
    );
    assert!(text.contains(&said), "{said:?} in {text}");
    assert!(!text.contains("teams/life/shared"), "{text}");
}

/// An agent's folder, its Home and its own beside it.
fn main_folder() -> Folder {
    Folder::new("/f/main".as_ref(), None)
}

/// An agent in an environment writes its whole Home, the repository;
/// its conversations, in its own folder, it reads.
#[test]
fn the_environment_says_a_repository_is_written() {
    let environment = Environment {
        folders: Folders {
            write: vec![".".into()],
            read: Vec::new(),
            teams: Default::default(),
        },
        ..Default::default()
    };
    let text = super::environment(
        Some(&environment),
        &Folder::new("/f/main".as_ref(), Some("/home/me/app".as_ref())),
    );
    for said in [
        "- work/: anything else you make; you may change it",
        "- chats/: your conversations, a folder each, kept by Link; read only",
        "- the rest: the repository you work in; you may change it",
    ] {
        assert!(text.contains(said), "{said:?} in {text}");
    }
    assert!(!text.contains("- ./"), "{text}");
}

#[test]
fn the_environment_says_the_folders_and_connections() {
    let environment = Environment {
        connections: vec!["telegram".into(), "slack".into()],
        folders: Folders {
            write: vec!["work".into()],
            read: vec!["instructions".into(), "chats".into()],
            teams: Default::default(),
        },
        commands: vec!["sh".into(), "python3".into()],
        teams: Vec::new(),
    };
    let text = super::environment(Some(&environment), &main_folder());
    assert!(
        text.contains(&format!(
            "You may run sh, python3 ({}), each in a sandbox of its own.",
            super::code("run")
        )),
        "{text}"
    );
    assert!(text.contains("Your files are in /f/main/home:"), "{text}");
    assert!(
        text.contains(
            "Your own folder, /f/main/agent, holds (as instructions/, skills/, \
             chats/ to your file tool):"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "- instructions/: what you're told to be (instruction.md), who you are \
             (persona.md), and who your owner is (owner.md); read only"
        ),
        "{text}"
    );
    assert!(
        text.contains("- work/: anything else you make; you may change it"),
        "{text}"
    );
    assert!(text.contains("on Telegram, Slack."), "{text}");
    assert!(!text.contains("can't run programs"), "{text}");

    // With no `run`, it's told, so it never offers to.
    let none = Environment {
        commands: Vec::new(),
        ..environment
    };
    let text = super::environment(Some(&none), &main_folder());
    assert!(
        text.contains(&format!(
            "You can't run programs here: you have no {}.",
            super::code("run")
        )),
        "{text}"
    );
}
