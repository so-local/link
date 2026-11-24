//! Explicit team-folder grants compose with an agent's own grants.

use super::*;
use link_harness_contract::harness::TeamFolders;

fn local_team(shared: &Path, name: &str, member: &str) -> link_agent_spec::Team {
    link_agent_spec::parse_team(&format!(
        "schema = 1\nid = 't4m9q2x7k1ab'\nname = '{name}'\n[member.main]\nid = '{member}'\n[files]\nshared = '{}'\n",
        shared.display()
    ))
    .unwrap()
}

#[test]
fn explicit_team_folders_combine_with_the_agents_own_grants() {
    let dir = link_test_kit::TempDir::new("explicit-team-folders");
    let shared = dir.join("research");
    std::fs::create_dir(&shared).unwrap();
    let text = "schema = 1\n[files]\nwrite = ['work']\n[teams.research]\nwrite = ['drafts']\nread = ['references']\n";
    let sandbox = crate::sandbox_file::Sandbox::parse(text).unwrap();
    let mut boxed = crate::adapter::Boxed::for_tests();
    boxed.dir = dir.join("harness");
    boxed.folder = link_agent_contract::folder::Folder {
        home: dir.join("own/home"),
        agent: dir.join("own/agent"),
    };
    boxed = boxed.granted(text, "");
    boxed.folders = Some(sandbox.folders());
    boxed.teams = reach_of(
        "k3m4n5p6q7r8",
        vec![local_team(&shared, "research", "k3m4n5p6q7r8")],
        &[],
        &sandbox.spec.teams,
    );
    let (spec, way) = boxed.unproxied("explicit-folders");
    let confinement = boxed
        .confinement(
            spec,
            Path::new("/bin/sh"),
            "explicit-folders",
            &[],
            &[],
            true,
            way,
        )
        .unwrap();
    assert!(confinement.write.contains(&boxed.folder.home.join("work")));
    assert!(confinement.write.contains(&shared.join("drafts")));
    assert!(confinement.read.contains(&shared.join("references")));
    assert!(!confinement.write.contains(&shared));
    assert!(!confinement.read.contains(&shared));
    assert!(!confinement.write.contains(&shared.join("references")));
}

#[test]
fn membership_and_other_teams_grants_open_no_ungranted_folder() {
    let dir = link_test_kit::TempDir::new("explicit-team-refusal");
    let shared = dir.join("research");
    std::fs::create_dir(&shared).unwrap();
    let member = "k3m4n5p6q7r8";
    let team = local_team(&shared, "research", member);
    let none = reach_of(member, vec![team.clone()], &[], &Default::default());
    assert!(none.write.is_empty() && none.read.is_empty(), "{none:?}");
    let grants = std::collections::BTreeMap::from([
        ("research".to_string(), folders(&[], &["references"])),
        ("foreign".to_string(), folders(&["."], &[])),
    ]);
    let foreign = local_team(&dir.join("foreign"), "foreign", "a1b2c3d4e5f6");
    let reach = reach_of(member, vec![team, foreign], &[], &grants);
    assert_eq!(reach.read, [shared.join("references")]);
    assert!(reach.write.is_empty(), "{reach:?}");
    assert_eq!(reach.teams.len(), 1);
    assert_eq!(reach.teams[0].0, "research");
    // Membership and a grant naming one child never grant its parent or siblings.
    assert!(!reach.read.contains(&shared));
    assert!(!reach.read.contains(&shared.join("private")));
}

fn folders(write: &[&str], read: &[&str]) -> TeamFolders {
    TeamFolders {
        write: write.iter().map(|s| s.to_string()).collect(),
        read: read.iter().map(|s| s.to_string()).collect(),
    }
}

/// A team whose folder an account hosting agents keeps is reached
/// through the agent's files bridge, with what its sandbox file
/// grants, and never as a folder here: its shared folder is neither
/// read nor written. A team kept here is reached as before.
#[test]
fn a_hosted_teams_folder_is_reached_through_the_bridge_never_here() {
    let dir = link_test_kit::TempDir::new("team-reach-hosted");
    let team = |id: &str, name: &str| {
        let shared = dir.join(name);
        std::fs::create_dir_all(shared.join("notes")).unwrap();
        link_agent_spec::parse_team(&format!(
            "schema = 1\nid = \"{id}\"\nname = \"{name}\"\n[member.main]\nid = \"k3m4n5p6q7r8\"\n\
             [files]\nshared = \"{}\"\n",
            shared.display()
        ))
        .unwrap()
    };
    let teams = vec![
        team("t4m9q2x7k1ab", "research"),
        team("z9y8x7w6v5u4", "local"),
    ];
    let at = crate::hosted::TeamFolderAt {
        id: "t4m9q2x7k1ab".into(),
        owner: "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e".into(),
        files: "https://services.example/move".into(),
    };
    let granted = std::collections::BTreeMap::from([
        ("research".to_string(), folders(&["notes"], &[])),
        ("local".to_string(), folders(&["notes"], &[])),
    ]);
    let hosted = std::slice::from_ref(&at);
    let reach = reach_of("k3m4n5p6q7r8", teams, hosted, &granted);
    assert_eq!(
        reach.hosted,
        [HostedReach {
            name: "research".into(),
            id: at.id.clone(),
            owner: at.owner.clone(),
            files: at.files.clone(),
            folders: folders(&["notes"], &[]),
        }]
    );
    let names: Vec<&str> = reach.teams.iter().map(|(n, ..)| n.as_str()).collect();
    assert_eq!(names, ["local"]);
    let research = dir.join("research");
    let reached = reach.write.iter().chain(&reach.read);
    assert!(
        !reached.clone().any(|p| p.starts_with(&research)),
        "{reach:?}"
    );
    assert!(reach.write.contains(&dir.join("local/notes")), "{reach:?}");
}
