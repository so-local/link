//! A team's member moved without its team: refused before anything moves
//! where the team's guarantees can't hold, kept where they do.

use super::*;

/// Team research, kept on this computer, with main its lead.
const RESEARCH: &str = "schema = 1\nid = \"t4m9q2x7k1ab\"\nname = \"research\"\nlead = \"scout\"\n\
    [files]\nshared = \"~/Link/Teams/research/shared\"\n\
    [member.scout]\nid = \"a1b2c3d4e5f6\"\n\
    [member.writer]\nid = \"k3m4n5p6q7r8\"\n";

fn team_file(world: &World) -> PathBuf {
    link_files::layout::Home::TeamSpecs
        .at(&world.here.join("Link"))
        .join("research.toml")
}

/// Its lead, sharing the team's folder, goes to a box or a cloud alone:
/// refused before anything is made there or paused here, the team as it
/// was, the refusal naming the whole team's move.
#[test]
fn a_member_alone_is_refused_before_anything_moves() {
    let world = World::new("member-alone");
    an_agent_here(&world);
    write(&team_file(&world), RESEARCH);
    write(
        &world.here.join("Link/Safe/Agents/main/sandbox.toml"),
        "schema = 1\n[teams.research]\nwrite = [\"drafts\"]\n",
    );
    for to in [&["--on", "b1"][..], &["--cloud", "host"]] {
        let refused =
            world.lnk_agent(&[&["move", "--agent", "main", "--no-start"][..], to].concat());
        assert!(!refused.status.success(), "{}", text(&refused));
        let said = text(&refused);
        assert!(
            said.contains("scout is on team research")
                && said.contains("lnk agent team move research <cloud>"),
            "{said}"
        );
    }
    // Nothing went: not to the box, not to the cloud, and nothing paused.
    assert!(!world.there.join("Link/Agents/main").exists());
    assert!(host_log(&world).is_empty(), "{:?}", host_log(&world));
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(
        !link_files::layout::Hidden::Moving
            .at(&world.here.join(".lnk"))
            .join(format!("{ID}.toml"))
            .exists()
    );
    assert_eq!(
        std::fs::read_to_string(team_file(&world)).unwrap(),
        RESEARCH
    );
    assert!(
        std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/sandbox.toml"))
            .unwrap()
            .contains("[teams.research]")
    );
}

/// The lead on a box comes back to where its team is kept: kept, with its
/// ID and its place as lead, its folder's grant derived again here, never
/// taken from what the box says.
#[test]
fn a_member_coming_where_its_team_is_kept_stays_on_it() {
    let world = World::new("member-home");
    an_agent_here(&world);
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    // On the box it says it writes all of the team's folder; then the
    // team, kept here, lists it as its lead.
    write(
        &world.there.join("Link/Safe/Agents/main/sandbox.toml"),
        "schema = 1\n[teams.research]\nwrite = [\".\"]\n",
    );
    write(&team_file(&world), RESEARCH);
    let back = ok(&world.lnk_agent(&["move", "--here", "--agent", "main", "--no-start"]));
    let settings =
        std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    let identity =
        std::fs::read_to_string(world.here.join(".lnk/safe/agents/main/identity.toml")).unwrap();
    assert!(
        identity.contains(&format!("id = \"{ID}\"")),
        "{settings} {identity}"
    );
    assert_eq!(
        std::fs::read_to_string(team_file(&world)).unwrap(),
        RESEARCH
    );
    // A stream alone grants no team's folder: said, with how to give it.
    let sandbox = std::fs::read_to_string(world.here.join("Link/Safe/Agents/main/sandbox.toml"))
        .unwrap_or_default();
    assert!(!sandbox.contains("[teams.research]"), "{sandbox}");
    assert!(back.contains("team research"), "{back}");
    // Its generation here is the move it came back by.
    let generation =
        std::fs::read_to_string(world.here.join(".lnk/safe/agents/main/generation")).unwrap();
    link_agent_contract::moving::check_move_id(&generation).unwrap();
}

/// Team design's ID.
const DESIGN: &str = "d3e4s5g6n7a8";

/// Team design, which the cloud account `host` keeps, with main its own
/// member: its relay and its folder service as `host` lists them, its
/// file there, and a copy of it here, as a member's machine keeps one.
fn hosted_design(world: &World, relay: bool, files: bool) {
    let at = |on: bool, url: &str| on.then(|| url.to_string());
    let listed = serde_json::json!([{
        "id": DESIGN,
        "name": "design",
        "members": [ID],
        "owner": "acct-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "relay": at(relay, "https://dispatch.example"),
        "files": at(files, "https://services.example/move"),
    }]);
    write(&world.root.join("host/teams.json"), &listed.to_string());
    let file =
        format!("schema = 1\nid = \"{DESIGN}\"\nname = \"design\"\n[member.main]\nid = \"{ID}\"\n");
    write(&world.root.join(format!("host/teams/{DESIGN}.toml")), &file);
    write(
        &link_files::layout::Home::TeamSpecs
            .at(&world.here.join("Link"))
            .join("design.toml"),
        &file,
    );
}

/// A member of a team a cloud keeps goes alone from here to a box only
/// where that cloud says its ways reach: its relay for the conversation,
/// its folder service for the grant its sandbox file here says. Refused,
/// nothing moved and the refusal names the whole team's move; the copy
/// of the team's file here is never taken for a team kept here.
#[test]
fn a_member_of_a_hosted_team_goes_alone_only_where_its_ways_reach() {
    let world = World::new("member-hosted");
    an_agent_here(&world);
    let sandbox = world.here.join("Link/Safe/Agents/main/sandbox.toml");
    for (relay, files, grant, lost) in [
        (false, true, "schema = 1\n", "conversation"),
        (
            true,
            false,
            "schema = 1\n[teams.design]\nwrite = [\"drafts\"]\n",
            "shared folder",
        ),
    ] {
        hosted_design(&world, relay, files);
        write(&sandbox, grant);
        let refused = world.lnk_agent(&["move", "--agent", "main", "--no-start", "--on", "b1"]);
        assert!(!refused.status.success(), "{}", text(&refused));
        let said = text(&refused);
        assert!(
            said.contains(&format!("main is on team design, whose {lost}"))
                && said.contains("lnk agent team move design@<cloud> --here"),
            "{said}"
        );
        assert!(!world.there.join("Link/Agents/main").exists());
    }
    // Its relay and folder service reach a box: it goes, with the team's
    // file as the cloud has it and where it's kept, which the box has no
    // account to ask.
    hosted_design(&world, true, true);
    ok(&world.lnk_agent(&["move", "--agent", "main", "--no-start", "--on", "b1"]));
    assert!(
        world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    let file = std::fs::read_to_string(
        link_files::layout::Home::TeamSpecs
            .at(&world.there.join("Link"))
            .join("design.toml"),
    )
    .unwrap();
    assert_eq!(
        file,
        std::fs::read_to_string(world.root.join(format!("host/teams/{DESIGN}.toml"))).unwrap()
    );
    let known: link_agent::hosted_teams::Known = link_files::saved::parse(
        &std::fs::read_to_string(
            link_files::layout::Hidden::AgentPlaces
                .at(&world.there.join(".lnk"))
                .join("cloud-teams.toml"),
        )
        .unwrap(),
    )
    .unwrap();
    let kept = &known.teams[DESIGN];
    assert_eq!(kept.files.as_deref(), Some("https://services.example/move"));
    // Its runner there reaches its teammates through the team's relay.
    let team = link_agent_spec::parse_team(&file).unwrap();
    let relayed = link_scheduler_tool_contract::RelayedTeam {
        id: kept.id.clone(),
        owner: kept.owner.clone().unwrap(),
        relay: kept.relay.clone().unwrap(),
    };
    assert_eq!(
        link_scheduler_tool_contract::relay(ID, &[team], &[relayed]).argv(),
        [
            "--team-owner",
            "design=acct-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--team-relay",
            "https://dispatch.example"
        ]
    );
    // Its folder's grant there is derived again from that file, as the
    // machine running the move vouched for it.
    let sandbox =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/sandbox.toml")).unwrap();
    assert!(sandbox.contains("[teams.design]"), "{sandbox}");
}

/// A team a move brought word of here, the agent on it: when the cloud
/// that may keep it doesn't answer, the agent's move alone is refused,
/// never let through unasked.
#[test]
fn a_member_of_a_team_whose_cloud_doesnt_answer_stays() {
    let world = World::new("member-unanswered");
    an_agent_here(&world);
    hosted_design(&world, true, true);
    write(&world.root.join("host/fail-agent-team"), "down\n");
    write(
        &link_files::layout::Hidden::AgentPlaces
            .at(&world.here.join(".lnk"))
            .join("cloud-teams.toml"),
        &format!("[teams.{DESIGN}]\nid = \"{DESIGN}\"\nname = \"design\"\nmembers = [\"{ID}\"]\n"),
    );
    let refused = world.lnk_agent(&["move", "--agent", "main", "--no-start", "--on", "b1"]);
    assert!(!refused.status.success(), "{}", text(&refused));
    let said = text(&refused);
    assert!(
        said.contains("team design") && said.contains("host didn't answer"),
        "{said}"
    );
    assert!(!world.there.join("Link/Agents/main").exists());
}
