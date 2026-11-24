//! Teams and access as a spec holds them: a team's opinions between
//! Link's and yours, strangers' tools, who may message an agent, and a
//! team's file.

use super::*;

fn team(name: &str, id: &str, member: &str, opinions: &str) -> Team {
    parse_team(&format!(
        "schema = 1\nid = \"{id}\"\nname = \"{name}\"\n\n[member.scout]\nid = \"{member}\"\n\
         purpose = \"finds sources\"\n\n{opinions}"
    ))
    .unwrap()
}

#[test]
fn teams_sit_between_link_and_you() {
    let research = team(
        "research",
        "t4m9q2x7k1ab",
        "a1b2c3d4e5f6",
        "[settings.agent.turns]\nrounds = 4\n",
    );
    assert!(research.has("a1b2c3d4e5f6"));
    let k = Known { ..known("scout") };
    let resolved = resolve(None, std::slice::from_ref(&research), &k).unwrap();
    assert_eq!(resolved.spec.turns.as_ref().unwrap().rounds, Some(4));
    assert_eq!(resolved.from["turns.rounds"], From::Team("research".into()));
    let own = parse("[turns]\nrounds = 2\n").unwrap();
    let resolved = resolve(Some(&own), std::slice::from_ref(&research), &k).unwrap();
    assert_eq!(resolved.spec.turns.unwrap().rounds, Some(2));

    // Two teams that disagree refuse, unless yours settles it.
    let writers = team(
        "writers",
        "w4m9q2x7k1ab",
        "a1b2c3d4e5f6",
        "[settings.agent.turns]\nrounds = 6\n",
    );
    let both = [research.clone(), writers.clone()];
    let err = resolve(None, &both, &k).unwrap_err();
    assert!(format!("{err}").contains("turns.rounds"), "{err}");
    assert!(resolve(Some(&own), &both, &k).is_ok());
    // Agreeing is fine.
    let same = team(
        "same",
        "s4m9q2x7k1ab",
        "a1b2c3d4e5f6",
        "[settings.agent.turns]\nrounds = 4\n",
    );
    assert!(resolve(None, &[research, same], &k).is_ok());
}

#[test]
fn a_team_holds_opinions_only() {
    for bad in [
        "name = \"x\"",
        "owner = \"github-1\"",
        "[harness]\nname = \"link\"",
        "[access]\nmessage = [\"chat:slack:C0TEAM\"]",
    ] {
        assert!(
            parse_team(&format!(
                "schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"t\"\n{bad}\n"
            ))
            .is_err(),
            "{bad}"
        );
    }
    assert!(parse_team("schema = 1\nid = \"nope\"\nname = \"t\"\n").is_err());
    assert!(
        parse_team("schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"t\"\n[turns]\nloops = 1\n")
            .is_err()
    );
}

/// `[tools] strangers`: tools' names, in `[tools]` alone; left out,
/// none.
#[test]
fn strangers_get_the_tools_tools_names() {
    let own = parse("[tools]\nstrangers = [\"web__fetch\"]\n").unwrap();
    assert_eq!(own.strangers(), ["web__fetch"]);
    assert!(link().strangers().is_empty());
    assert!(parse("[tool.web]\nstrangers = [\"web__fetch\"]\n").is_err());
    assert!(parse("[tools]\nstrangers = [\"web fetch\"]\n").is_err());
    // Never a tool that acts as the agent itself.
    for acting in ["scheduler", "scheduler__schedule", "run__command", "run"] {
        assert!(
            parse(&format!("[tools]\nstrangers = [\"{acting}\"]\n")).is_err(),
            "{acting}"
        );
    }
    assert!(parse("[tools]\nstrangers = [\"runner__go\"]\n").is_ok());
    // An agent's own: never a team's.
    assert!(
        parse_team("schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"t\"\n[tools]\nstrangers = [\"web__fetch\"]\n")
            .is_err()
    );
    let k = Known {
        name: "main".into(),
    };
    let resolved = resolve(Some(&own), &[], &k).unwrap();
    assert_eq!(resolved.spec.strangers(), ["web__fetch"]);
}

/// Canonical wrappers keep the same stranger boundary as their providers.
#[test]
fn strangers_keep_their_standing_through_canonical_namespaces() {
    for acting in [
        "task",
        "task__run",
        "task__agent",
        "task__send",
        "cron__create",
        "task__list",
        "task__read",
        "task__stop",
        "task__plan",
        "task__reconcile",
        "file__definition",
        "file__references",
        "file__diagnostics",
        "agent__send",
    ] {
        let error = parse(&format!("[tools]\nstrangers = [\"{acting}\"]\n")).unwrap_err();
        assert!(
            error.to_string().contains("acts as the agent itself"),
            "{acting}: {error:#}"
        );
    }
    for delegated in [
        "file__read",
        "file__glob",
        "file__grep",
        "file__write",
        "file__edit",
        "agent__show",
        "web__fetch",
        "tasker__run",
        "file__definition_extra",
    ] {
        let spec = parse(&format!("[tools]\nstrangers = [\"{delegated}\"]\n")).unwrap();
        assert_eq!(spec.strangers(), [delegated]);
    }
}

/// `[access]`: chats alone may message an agent, each a group its
/// connection keeps, never a principal; it's the agent's own, kept through its union.
#[test]
fn access_names_the_chats_that_may_message_it() {
    let own =
        parse("[access]\nmessage = [\"chat:slack:C0TEAM\", \"chat:telegram:-1001234\"]\n").unwrap();
    let k = Known {
        name: "main".into(),
    };
    let resolved = resolve(Some(&own), &[], &k).unwrap();
    let access = resolved.spec.message_access();
    assert_eq!(access.chats("slack").collect::<Vec<_>>(), ["C0TEAM"]);
    assert_eq!(resolved.from["access.message"], From::Yours);
    for bad in [
        "message = [\"member:slack:T0ACME/U0ALICE\"]",
        "message = [\"service:a1b2c3d4e5f6\"]",
        "message = [\"C0TEAM\"]",
        "member = [\"chat:slack:C0TEAM\"]",
        "member = [\"service:k3m4n5p6q7r8\"]",
    ] {
        assert!(parse(&format!("[access]\n{bad}\n")).is_err(), "{bad}");
    }
    assert!(link().access.is_none());
}

/// Sharing writes `[access] message` alone: the rest of the file, its
/// comments too, as it was; none left, no `[access]`.
#[test]
fn sharing_rewrites_access_alone() {
    let own = "# mine\nschema = 1\nname = \"main\"\n\n[turns]\nrounds = 9 # enough\n";
    let team = crate::access::chat("slack", "C0TEAM").unwrap();
    let two = crate::access::chat("telegram", "-1001234").unwrap();
    let shared = with_message_access(own, &[team.clone(), two.clone()]).unwrap();
    assert!(shared.starts_with(own), "{shared}");
    let spec = parse(&shared).unwrap();
    assert_eq!(spec.message_access().message, [team.clone(), two]);
    let one = with_message_access(&shared, &[team]).unwrap();
    assert!(one.contains("chat:slack:C0TEAM") && !one.contains("telegram"));
    assert_eq!(with_message_access(&one, &[]).unwrap().trim(), own.trim());
    // Inline, too, and nothing of a spec that isn't one.
    let inline = &named("access = { message = [\"chat:slack:C0TEAM\"] }\n");
    assert!(
        parse(&with_message_access(inline, &[]).unwrap())
            .unwrap()
            .access
            .is_none()
    );
    assert!(with_message_access(&named("[turns]\nloops = 1\n"), &[]).is_err());
    // Each chat once: nothing else is written.
    let chat = crate::access::chat("slack", "C0TEAM").unwrap();
    assert!(with_message_access(own, &[chat.clone(), chat]).is_err());
}

/// Another account's agent on a team: a member with its owner and the
/// folders it reaches, and in `[access] member`, both or neither.
#[test]
fn another_accounts_agent_is_named_in_access_and_members_both() {
    let head = "id = \"t4m9q2x7k1ab\"\nname = \"design\"\n[member.lead]\nid = \"a1b2c3d4e5f6\"\n";
    let reviewer = "[member.reviewer]\nid = \"k3m4n5p6q7r8\"\nowner = \"acct-b7\"\n\
                    purpose = \"reviews drafts\"\nread = [\".\"]\n";
    let access = "[access]\nmember = [\"service:k3m4n5p6q7r8\"]\n";
    let team = parse_team(&format!("schema = 1\n{head}{reviewer}{access}")).unwrap();
    let (name, member) = team.member("k3m4n5p6q7r8").unwrap();
    assert_eq!(name, "reviewer");
    assert_eq!(member.owner.as_deref(), Some("acct-b7"));
    assert_eq!(member.folders().read, ["."]);
    assert_eq!(team.access.member[0].as_str(), "service:k3m4n5p6q7r8");
    assert!(team.members["lead"].owner.is_none());
    // Named in one place only, either way round.
    assert!(parse_team(&format!("schema = 1\n{head}{reviewer}")).is_err());
    assert!(parse_team(&format!("schema = 1\n{head}{access}")).is_err());
    // Another account's agent than [access] names.
    let other = reviewer.replace("k3m4n5p6q7r8", "p3q4r5s6t7u8");
    assert!(parse_team(&format!("schema = 1\n{head}{other}{access}")).is_err());
    for bad in [
        // The team owner's own agent: its folders are its sandbox's.
        "[member.lead]\nid = \"a1b2c3d4e5f6\"\nwrite = [\"notes\"]\n",
        // A local owner is no account.
        "[member.x]\nid = \"p3q4r5s6t7u8\"\nowner = \"me@laptop\"\n",
        // Folders the sandbox couldn't keep.
        "[member.x]\nid = \"p3q4r5s6t7u8\"\nowner = \"acct-b7\"\nwrite = [\".\"]\nread = [\"notes\"]\n\
         [access]\nmember = [\"service:p3q4r5s6t7u8\"]\n",
        // A chat isn't a member, nor is message a team's verb.
        "[access]\nmember = [\"chat:slack:C0TEAM\"]\n",
        "[access]\nmessage = [\"chat:slack:C0TEAM\"]\n",
    ] {
        let text = format!("id = \"t4m9q2x7k1ab\"\nname = \"design\"\n{bad}");
        assert!(parse_team(&text).is_err(), "{bad}");
    }
}

/// A team's file: its members once each, a purpose a line, and its
/// shared folder an absolute path or under `~`.
#[test]
fn a_teams_file_says_its_members_and_folder() {
    let head = "id = \"t4m9q2x7k1ab\"\nname = \"research\"\n";
    let team = parse_team(&format!(
        "schema = 1\n{head}lead = \"scout\"\n[member.scout]\nid = \"a1b2c3d4e5f6\"\npurpose = \"finds sources\"\n\
         [member.writer]\nid = \"g7h8j9k0m1n2\"\n[files]\nshared = \"~/Link/Teams/research/shared\"\n"
    ))
    .unwrap();
    assert_eq!(
        team.shared,
        Some(link_files::home_dir().join("Link/Teams/research/shared"))
    );
    assert_eq!(team.member("g7h8j9k0m1n2").unwrap().0, "writer");
    assert_eq!(team.target(None).unwrap().1.id, "a1b2c3d4e5f6");
    assert_eq!(team.target(Some("writer")).unwrap().1.id, "g7h8j9k0m1n2");
    assert!(team.target(Some("absent")).is_err());
    assert!(team.member("p3q4r5s6t7u8").is_none());
    let plain = parse_team(head).unwrap();
    assert_eq!(plain.shared, None);
    assert!(plain.target(None).is_err());
    for bad in [
        "lead = \"absent\"\n",
        "lead = 7\n",
        "[member.a]\nid = \"a1b2c3d4e5f6\"\n[member.b]\nid = \"a1b2c3d4e5f6\"\n",
        "[member.a]\nid = \"a1b2c3d4e5f6\"\npurpose = \"two\\nlines\"\n",
        "[files]\nshared = \"Shared\"\n",
        "[files]\nshared = \"/srv/../etc\"\n",
        "[files]\nshared = \"/srv/x\"\nother = 1\n",
    ] {
        assert!(
            parse_team(&format!("schema = 1\n{head}{bad}")).is_err(),
            "{bad}"
        );
    }
    // What a member is for is its `purpose`; `role` is refused,
    // naming the key it takes instead.
    let role = parse_team(&format!(
        "schema = 1\n{head}[member.scout]\nid = \"a1b2c3d4e5f6\"\nrole = \"finds sources\"\n"
    ))
    .unwrap_err()
    .to_string();
    assert!(role.contains("role") && role.contains("purpose"), "{role}");
}
