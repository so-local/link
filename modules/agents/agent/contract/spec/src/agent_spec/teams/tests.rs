//! The desired roster and resource policy never supply identity authority.
use super::*;

const TEAM: &str = "t4m9q2x7k1ab";
const LEAD: &str = "a1b2c3d4e5f6";
const SCOUT: &str = "g7h8j9k0m1n2";
const DESIRED: &str = r#"# The research team
schema = 1
name = "research"
lead = "lead"
[member.lead]
purpose = "Coordinate"
[member.lead.agent]
harness = { name = "link" }
[member.scout.agent]
model = { default = "local/qwen3:8b" }
[settings.agent.turns]
rounds = 12
[settings.memory]
semantic = true
daily_credits = 100
indexer = { kind = "member", member = "scout" }
payer = { kind = "owner" }
"#;

fn ids() -> BTreeMap<String, String> {
    [("lead".into(), LEAD.into()), ("scout".into(), SCOUT.into())].into()
}

#[test]
fn files_flags_and_inspection_share_the_owning_teams_semantic_checks() {
    let dir = link_test_kit::TempDir::new("team-input-checks");
    let path = dir.join("team.toml");
    std::fs::write(&path, DESIRED).unwrap();
    for (flag, value, invalid, expected) in [
        (
            "--lead",
            "absent",
            DESIRED.replace("lead = \"lead\"", "lead = \"absent\""),
            "the lead is not a team member",
        ),
        (
            "--spec.name",
            "Bad Name",
            DESIRED.replace("name = \"research\"", "name = \"Bad Name\""),
            "name",
        ),
        (
            "--settings.agent.name",
            "other",
            format!("{DESIRED}\n[settings.agent]\nname = 'other'\n"),
            "opinions only",
        ),
        (
            "--settings.agent.access.message",
            "['chat:slack:C0TEAM']",
            format!("{DESIRED}\n[settings.agent.access]\nmessage = ['chat:slack:C0TEAM']\n"),
            "opinions only",
        ),
        (
            "--settings.memory.daily_credits",
            "0",
            DESIRED.replace("daily_credits = 100", "daily_credits = 0"),
            "daily_credits is positive",
        ),
    ] {
        let own = parse_team_definition(&invalid).expect_err("owner refuses it");
        assert!(format!("{own:#}").contains(expected), "{own:#}");
        let binding = link_spec::Binding::of::<TeamFile>();
        let (flags, rest) =
            link_spec::split([flag, value].map(Into::into), Some(&binding), None).unwrap();
        assert!(rest.is_empty());
        let refused = link_spec::read_with::<TeamFile>(Some(&path), &flags)
            .expect_err("field flags use the same checks");
        assert!(format!("{refused:#}").contains(expected), "{refused:#}");
        let shown = link_spec::show::<TeamFile>(
            &link_spec::Show {
                spec: Some(path.clone()),
                ..link_spec::Show::default()
            },
            &flags,
            None,
        )
        .expect_err("inspection refuses an unusable spec");
        assert!(format!("{shown:#}").contains(expected), "{shown:#}");
    }
}

#[test]
fn desired_members_become_authority_only_after_stable_ids_are_kept() {
    let desired = parse_team_definition(DESIRED).unwrap();
    assert_eq!(desired.id, None);
    assert_eq!(desired.members["lead"].id, None);
    assert!(desired.clone().into_team().is_err());
    assert!(parse_team(DESIRED).is_err());
    let (written, team) = assign_team_ids(DESIRED, TEAM, &ids()).unwrap();
    assert!(written.contains("# The research team"));
    assert_eq!(team.target(None).unwrap().1.id, LEAD);
    assert_eq!(team.target(Some("scout")).unwrap().1.id, SCOUT);
    assert_eq!(team.settings.memory.daily_credits, Some(100));
    assert_eq!(assign_team_ids(&written, TEAM, &ids()).unwrap().0, written);
    assert!(assign_team_ids(&written, SCOUT, &ids()).is_err());
    let mut swapped = ids();
    swapped.insert("lead".into(), "p3q4r5s6t7u8".into());
    assert!(assign_team_ids(&written, TEAM, &swapped).is_err());
    assert!(assign_team_ids(DESIRED, TEAM, &BTreeMap::new()).is_err());
    assert!(
        assign_team_ids(
            DESIRED,
            TEAM,
            &[("lead".into(), LEAD.into()), ("scout".into(), LEAD.into())].into()
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<link_agent_team::Member<Spec>>(serde_json::json!({})).is_err()
    );
}

#[test]
fn shared_opinions_are_read_only_from_settings() {
    let root = format!(
        "# keep my choice\nschema = 1\nid = '{TEAM}'\nname = 'research'\n[turns]\nrounds = 12 # deliberate\n"
    );
    let refused = parse_team(&root).unwrap_err().to_string();
    assert!(refused.contains("turns"), "{refused}");
    let current = format!(
        "# keep my choice\nschema = 1\nid = '{TEAM}'\nname = 'research'\n[settings.agent.turns]\nrounds = 12 # deliberate\n"
    );
    let team = parse_team(&current).unwrap();
    let document = team_document(&current).unwrap();
    assert_eq!(team_document(&document).unwrap(), document);
    let resolved = resolve(
        None,
        &[team],
        &Known {
            name: "lead".into(),
        },
    )
    .unwrap();
    assert_eq!(resolved.spec.turns.unwrap().rounds, Some(12));
}

#[test]
fn memory_settings_without_agent_opinions_survive_printing_and_field_flags() {
    for opinions in ["", "\n[settings.agent.turns]\nrounds = 12\n"] {
        let text = format!(
            "kind = 'team'\nschema = 1\nname = 'research'\n[settings.memory]\nsemantic = false\n{opinions}"
        );
        let spec = link_spec::read_text_with::<TeamFile>(&text, &Default::default()).unwrap();
        let binding = link_spec::Binding::of::<TeamFile>();
        let (flags, rest) = link_spec::split(
            link_spec::flags_of(&spec)
                .unwrap()
                .into_iter()
                .map(Into::into),
            Some(&binding),
            None,
        )
        .unwrap();
        assert!(rest.is_empty());
        let replay = link_spec::read_text_with::<TeamFile>("", &flags).unwrap();
        let printed = TeamFile::format().print(&spec).unwrap();
        let reread = link_spec::read_text_with::<TeamFile>(&printed, &Default::default()).unwrap();
        let expected = serde_json::to_value(spec).unwrap();
        assert_eq!(serde_json::to_value(replay).unwrap(), expected);
        assert_eq!(serde_json::to_value(reread).unwrap(), expected);
        assert_eq!(
            expected["settings"].get("agent").is_some(),
            !opinions.is_empty()
        );
    }
}

#[test]
fn shared_settings_cannot_grant_an_agents_chat_access() {
    let (_, mut team) = assign_team_ids(DESIRED, TEAM, &ids()).unwrap();
    let own: Spec = toml::from_str("[access]\nmessage = ['chat:slack:C0OWN']\n").unwrap();
    let known = Known {
        name: "lead".into(),
    };
    let resolved = resolve(Some(&own), std::slice::from_ref(&team), &known).unwrap();
    assert_eq!(resolved.spec.access, own.access);

    // A managed member's full definition remains that agent's own policy.
    team.members
        .get_mut("lead")
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .access = own.access.clone();
    let resolved = resolve_member(None, std::slice::from_ref(&team), &known, LEAD).unwrap();
    assert_eq!(resolved.spec.access, own.access);

    // Typed or saved team input must not bypass the file reader's boundary,
    // even when the agent has its own overriding access declaration.
    team.settings.agent.access = own.access.clone();
    for own in [None, Some(&own)] {
        let error = resolve(own, std::slice::from_ref(&team), &known).unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("team research"), "{error}");
        assert!(
            error.contains("opinions only") && error.contains("[access]"),
            "{error}"
        );
    }
}

#[test]
fn member_definitions_cannot_claim_another_identity_or_account() {
    for bad in [
        "[member.lead]\n",
        "[member.lead.agent]\nname = 'another'\n",
        "[member.lead.agent]\nowner = 'acct-1'\n",
        "[member.lead.agent]\nteams = ['research']\n",
        "[member.lead]\nowner = 'acct-1'\n[member.lead.agent]\n",
        "[member.lead]\nid = 'nope'\n",
    ] {
        assert!(
            parse_team_definition(&format!("schema = 1\nname = 'research'\n{bad}")).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn memory_policy_requires_explicit_limits_and_existing_members() {
    let prefix =
        format!("schema = 1\nid = '{TEAM}'\nname = 'research'\n[member.lead]\nid = '{LEAD}'\n");
    let plain = parse_team(&prefix).unwrap();
    assert!(!plain.settings.memory.semantic);
    // The declared defaults: an elected keeper, paid for by the owner.
    assert!(matches!(
        plain.settings.memory.indexer,
        TeamIndexer::Auto(_)
    ));
    assert!(matches!(plain.settings.memory.payer, TeamPayer::Owner(_)));
    assert!(plain.allows_indexer(LEAD));
    for bad in [
        "semantic = true",
        "semantic = true\ndaily_credits = 0",
        "daily_credits = -1",
        "indexer = { kind = 'member', member = 'absent' }",
        "payer = { kind = 'member', member = 'absent' }",
        "payer = { kind = 'account', account = 'acct-stranger' }",
        "semantic = true\ndaily_credits = 10\nunbounded = true",
    ] {
        assert!(
            parse_team(&format!("schema = 1\n{prefix}[settings.memory]\n{bad}\n")).is_err(),
            "{bad}"
        );
    }
    assert!(
        parse_team(&format!(
            "schema = 1\n{prefix}[settings.agent]\nvm = '/some/box.toml'\n"
        ))
        .is_err()
    );
}

#[test]
fn managed_behavior_is_read_from_the_current_roster_by_permanent_id() {
    let (_, mut team) = assign_team_ids(DESIRED, TEAM, &ids()).unwrap();
    let own = parse("schema = 1\n[turns]\nrounds = 99\n").unwrap();
    let known = Known {
        name: "a-renamed-agent".into(),
    };
    let read =
        |team: &Team| resolve_member(Some(&own), std::slice::from_ref(team), &known, LEAD).unwrap();
    assert_eq!(read(&team).spec.turns.unwrap().rounds, Some(12));
    team.members
        .get_mut("lead")
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .turns = Some(Turns {
        rounds: Some(7),
        ..Default::default()
    });
    let resolved = read(&team);
    assert_eq!(resolved.spec.turns.unwrap().rounds, Some(7));
    assert_eq!(resolved.from["turns.rounds"], From::Team("research".into()));
    let mut conflicting = team.clone();
    conflicting.id = "p3q4r5s6t7u8".into();
    conflicting
        .members
        .get_mut("lead")
        .unwrap()
        .agent
        .as_mut()
        .unwrap()
        .turns = None;
    assert!(resolve_member(Some(&own), &[team.clone(), conflicting], &known, LEAD).is_err());
    team.members.remove("lead");
    assert_eq!(read(&team).spec.turns.unwrap().rounds, Some(99));
}

#[test]
fn maintained_link_opinions_qualify_in_both_team_reader_contexts() {
    // Link's spec as a managed member's own agent, and its opinions,
    // identity and authority left out, as the opinions every member
    // shares: the team reader takes both, each held to its checks.
    let link: Spec = crate::agent_spec::parse(LINK_SPEC).unwrap();
    let mut shared = link.clone();
    shared.name = None;
    shared.version = None;
    shared.extends = None;
    shared.harness = None;
    shared.isolation = None;
    shared.env = None;
    shared.access = None;
    if let Some(tools) = &mut shared.tools {
        tools.strangers = None;
    }
    check_opinions(&shared).unwrap();
    let mut file = toml::Table::new();
    file.insert("name".into(), "research".into());
    file.insert("lead".into(), "scout".into());
    let mut scout = toml::Table::new();
    scout.insert("agent".into(), toml::Value::try_from(&link).unwrap());
    let mut member = toml::Table::new();
    member.insert("scout".into(), scout.into());
    file.insert("member".into(), member.into());
    let mut settings = toml::Table::new();
    settings.insert("agent".into(), toml::Value::try_from(&shared).unwrap());
    file.insert("settings".into(), settings.into());
    let text = format!("schema = 1\n{}", toml::to_string(&file).unwrap());
    let definition = parse_team_definition(&text).unwrap();
    assert_eq!(definition.members["scout"].agent.as_ref(), Some(&link));
    assert_eq!(definition.settings.agent, shared);
}

// Exercise the published source specimen through the actual composed owner.
// Observation capture is test-only; it makes no new production schema or API.
fn every_option_team_observations() -> Vec<serde_json::Value> {
    use link_spec::{Binding, Exported, FieldFlags, read_text_at};
    let dir = std::env::current_dir().unwrap();
    let source_text = link_spec::reference::every_option_text(&Exported::of::<Spec>());
    let source = read_text_at::<Spec>(&source_text, &dir, &FieldFlags::default()).unwrap();
    let mut source_input: toml::Value = toml::from_str(&source_text).unwrap();
    for header in ["kind", "schema"] {
        source_input.as_table_mut().unwrap().remove(header);
    }
    let team_text = link_spec::reference::every_option_text(&Exported::of::<TeamFile>());
    let team_input: toml::Value = toml::from_str(&team_text).unwrap();
    let mut observations = Vec::new();
    for context in ["member", "settings"] {
        let mut input = team_input.clone();
        let mut payload = source_input.clone();
        let mut expected = source.clone();
        let child_pointer = if context == "member" {
            // The complete maintained specimen keeps its own name and authority.
            let name = source.name.as_deref().unwrap();
            let members = input["member"].as_table_mut().unwrap();
            let mut member = members.remove("scout").unwrap();
            member["agent"] = payload;
            assert!(members.insert(name.into(), member).is_none());
            if input.get("lead").and_then(toml::Value::as_str) == Some("scout") {
                input["lead"] = toml::Value::String(name.into());
            }
            format!("/member/{name}/agent")
        } else {
            // Only the owning shared-opinion exclusions are removed.
            for key in [
                "name",
                "version",
                "extends",
                "harness",
                "isolation",
                "env",
                "access",
            ] {
                payload.as_table_mut().unwrap().remove(key);
            }
            if let Some(tools) = payload.get_mut("tools").and_then(toml::Value::as_table_mut) {
                tools.remove("strangers");
            }
            expected.name = None;
            expected.version = None;
            expected.extends = None;
            expected.harness = None;
            expected.isolation = Default::default();
            expected.env = None;
            expected.access = None;
            if let Some(tools) = &mut expected.tools {
                tools.strangers = None;
            }
            input["settings"]["agent"] = payload;
            "/settings/agent".into()
        };
        let text = toml::to_string(&input).unwrap();
        let accepted = read_text_at::<TeamFile>(&text, &dir, &FieldFlags::default())
            .unwrap_or_else(|error| panic!("{context} composed Team reader: {error:#}"));
        let typed = serde_json::to_value(&accepted).unwrap();
        assert_eq!(
            typed.pointer(&child_pointer),
            Some(&serde_json::to_value(expected).unwrap()),
            "{context}: the actual Team reader must retain the accepted source Spec"
        );
        let binding = Binding::of::<TeamFile>();
        let (flags, rest) = link_spec::split(
            link_spec::flags_of(&accepted)
                .unwrap()
                .into_iter()
                .map(Into::into),
            Some(&binding),
            None,
        )
        .unwrap();
        assert!(rest.is_empty(), "{context}: no runtime arguments emitted");
        let replay = read_text_at::<TeamFile>("", &dir, &flags).unwrap();
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            typed,
            "{context}: the complete actual file and flag readers must agree"
        );
        observations.push(serde_json::json!({
            "context": context,
            "child_pointer": child_pointer,
            "input": input,
            "typed": typed,
            "flags": flags.table(),
        }));
    }
    observations
}

#[test]
fn published_every_option_is_retained_by_both_team_readers_and_actual_flags() {
    assert_eq!(every_option_team_observations().len(), 2);
}
