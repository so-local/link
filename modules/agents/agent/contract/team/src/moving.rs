//! A member moved without its team: whether where it goes keeps the
//! team's guarantees, decided before anything of the move is cut over.
//! The machine running a move and a cloud that hosts teams both ask this
//! one question, each with the paths it has to where the team is kept.
use crate::Team;
use crate::harness::TeamFolders;

/// Where a team is kept: its canonical file, its shared folder and the
/// relay of its conversation. None of them moves with a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// On a machine, its folder under `~/Link/Teams/<team>/`.
    Machine,
    /// On a cloud that hosts teams, its folder in the owner's bucket.
    Hosted,
}

/// Where a member goes, as seen from where its team is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    /// Where the team is kept.
    Home,
    /// A machine away from it: another computer or a box.
    Machine,
    /// A cloud that hosts agents, away from it.
    Hosted,
}

/// What a destination must keep for a member that moves alone, besides
/// its permanent ID and its place on the roster, which a move never
/// changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guarantee {
    /// The team's one shared folder, reached where it stays.
    Folder,
    /// The team's conversation, its teammates' messages delivered.
    Conversation,
}

/// The ways from a destination away from where a team is kept back to
/// it: the seam an authenticated folder path (the files bridge's route)
/// and the team relay widen. Where a team is kept keeps every guarantee
/// without them.
pub trait Paths {
    /// Whether a member at `to` reaches the folder of team `team` (its
    /// ID), kept at `kept`, through an authenticated folder path.
    fn folder(&self, team: &str, kept: Kept, to: Toward) -> bool;
    /// Whether the conversation of team `team`, kept at `kept`, reaches a
    /// member at `to` through its relay.
    fn conversation(&self, team: &str, kept: Kept, to: Toward) -> bool;
}

/// Whether a member going `to` keeps every guarantee of any team, whatever
/// its roster and the paths: only where the team is kept, so no member's
/// move there needs its team's roster read.
pub fn keeps_every(to: Toward) -> bool {
    to == Toward::Home
}

/// No way back from anywhere: a team is reached only where it's kept, as
/// for a team kept on a machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct Direct;

impl Paths for Direct {
    fn folder(&self, _: &str, _: Kept, _: Toward) -> bool {
        false
    }
    fn conversation(&self, _: &str, _: Kept, _: Toward) -> bool {
        false
    }
}

/// The ways a host says it reaches its teams from a machine of the
/// person's, team by team (each by its ID): the teams whose folder its
/// folder service answers a member there, and those whose conversation
/// its team relay carries there. A machine runs the files bridge and the
/// runner that use them; another cloud that hosts agents runs neither,
/// so neither reaches it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ways {
    pub folder: std::collections::BTreeSet<String>,
    pub conversation: std::collections::BTreeSet<String>,
}

impl Paths for Ways {
    fn folder(&self, team: &str, _: Kept, to: Toward) -> bool {
        to == Toward::Machine && self.folder.contains(team)
    }
    fn conversation(&self, team: &str, _: Kept, to: Toward) -> bool {
        to == Toward::Machine && self.conversation.contains(team)
    }
}

/// A member a destination keeps: who it is on the team, unchanged, and
/// its grants as the destination derives them again.
#[derive(Clone, Debug, PartialEq)]
pub struct Stays {
    /// The team's ID.
    pub team: String,
    /// Its name on the roster.
    pub member: String,
    /// Its permanent ID.
    pub id: String,
    /// Whether it's the team's lead, which a move never changes.
    pub lead: bool,
    /// The team's role, derived from its current roster
    /// ([`Team::role`]): being on it and messaging each member.
    pub role: link_policy::Policy,
    /// What it reaches of the team's folder there: an outside member's,
    /// its roster entry's; an own member's, the grant the destination
    /// vouches for from its sandbox file. None of it is ever taken from
    /// the move's stream.
    pub folders: TeamFolders,
}

/// A destination that can't keep a guarantee: the move is refused
/// before any cutover, offering the whole team's move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub team: String,
    pub member: String,
    pub kept: Kept,
    pub to: Toward,
    pub lost: Guarantee,
}

impl Refusal {
    /// The command that moves the whole team instead, as the person runs
    /// it on the machine holding the team, or naming its cloud.
    pub fn whole_team(&self) -> String {
        match self.kept {
            Kept::Machine => format!("lnk agent team move {} <cloud>", self.team),
            Kept::Hosted => format!("lnk agent team move {}@<cloud> --here", self.team),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lost = match self.lost {
            Guarantee::Folder => "shared folder",
            Guarantee::Conversation => "conversation",
        };
        let place = match self.to {
            Toward::Home => "where the team is kept",
            Toward::Machine => "a machine",
            Toward::Hosted => "a cloud that hosts agents",
        };
        write!(
            f,
            "{} is on team {}, whose {lost} stays where the team is kept, which {place} \
             can't reach: move the whole team with {}",
            self.member,
            self.team,
            self.whole_team()
        )
    }
}

impl std::error::Error for Refusal {}

/// Whether a destination keeps the team's guarantees for its member.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Keeps(Stays),
    Refuses(Refusal),
}

/// Whether `team` (its current roster, from its canonical file, kept at
/// `kept`) keeps its member `agent` going to `to`, through `paths`.
/// `own` is an own member's grant of the team's folder as the asker
/// vouches for it; none when it can't tell, which counts as reaching the
/// folder when the team has one, as a team a cloud keeps always does. An
/// error when `agent` isn't a member.
pub fn check<S, T>(
    team: &Team<S, T>,
    kept: Kept,
    agent: &str,
    own: Option<&TeamFolders>,
    to: Toward,
    paths: &dyn Paths,
) -> anyhow::Result<Verdict> {
    let (name, member) = team
        .member(agent)
        .ok_or_else(|| anyhow::anyhow!("agent {agent} isn't on team {}", team.name))?;
    let folders = match (&member.owner, own) {
        (Some(_), _) => member.folders(),
        (None, Some(own)) => own.clone(),
        (None, None) => TeamFolders::default(),
    };
    folders.check(&format!("team {}'s grant for {name}", team.name))?;
    let uses_folder = match (&member.owner, own) {
        (None, None) => team.shared.is_some() || kept == Kept::Hosted,
        _ => !folders.is_empty(),
    };
    let refuse = |lost| {
        Verdict::Refuses(Refusal {
            team: team.name.clone(),
            member: name.clone(),
            kept,
            to,
            lost,
        })
    };
    let home = keeps_every(to);
    if uses_folder && !home && !paths.folder(&team.id, kept, to) {
        return Ok(refuse(Guarantee::Folder));
    }
    if !home && !paths.conversation(&team.id, kept, to) {
        return Ok(refuse(Guarantee::Conversation));
    }
    Ok(Verdict::Keeps(Stays {
        team: team.id.clone(),
        member: name.clone(),
        id: member.id.clone(),
        lead: team.lead.as_deref() == Some(name.as_str()),
        role: team.role()?,
        folders,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Member;
    use std::collections::BTreeMap;

    const SCOUT: &str = "a1b2c3d4e5f6";
    const WRITER: &str = "k3m4n5p6q7r8";
    const CHECKER: &str = "c9d8e7f6g5h4";

    fn member(id: &str, owner: Option<&str>, write: &[&str]) -> Member {
        Member {
            id: id.into(),
            service: None,
            agent: None,
            purpose: None,
            owner: owner.map(str::to_string),
            write: write.iter().map(|w| w.to_string()).collect(),
            read: Vec::new(),
        }
    }

    fn research() -> Team<()> {
        Team {
            id: "r7".into(),
            name: "research".into(),
            lead: Some("scout".into()),
            members: BTreeMap::from([
                ("scout".to_string(), member(SCOUT, None, &[])),
                ("writer".to_string(), member(WRITER, None, &[])),
                (
                    "checker".to_string(),
                    member(CHECKER, Some("other-account"), &["notes"]),
                ),
            ]),
            shared: Some("~/Link/Teams/research/shared".into()),
            access: Default::default(),
            settings: (),
        }
    }

    fn drafts() -> TeamFolders {
        TeamFolders {
            write: vec!["drafts".into()],
            read: Vec::new(),
        }
    }

    /// A host's ways to team `r7` from a machine: its folder service, its
    /// relay, or both.
    fn ways(folder: bool, conversation: bool) -> Ways {
        let r7 = || std::collections::BTreeSet::from(["r7".to_string()]);
        Ways {
            folder: if folder { r7() } else { Default::default() },
            conversation: if conversation {
                r7()
            } else {
                Default::default()
            },
        }
    }

    fn verdict(
        team: &Team<()>,
        agent: &str,
        own: Option<&TeamFolders>,
        to: Toward,
        paths: &dyn Paths,
    ) -> Verdict {
        check(team, Kept::Hosted, agent, own, to, paths).unwrap()
    }

    fn refused(verdict: Verdict) -> Refusal {
        match verdict {
            Verdict::Refuses(refusal) => refusal,
            Verdict::Keeps(stays) => panic!("kept: {stays:?}"),
        }
    }

    fn kept(verdict: Verdict) -> Stays {
        match verdict {
            Verdict::Keeps(stays) => stays,
            Verdict::Refuses(refusal) => panic!("refused: {refusal}"),
        }
    }

    #[test]
    fn a_destination_that_cant_reach_the_team_refuses_offering_the_whole_team() {
        let team = research();
        // Only where a team is kept does every team keep its members.
        assert!(keeps_every(Toward::Home));
        assert!(!keeps_every(Toward::Machine));
        let on_machine = |own: Option<&TeamFolders>, to| {
            refused(check(&team, Kept::Machine, WRITER, own, to, &Direct).unwrap())
        };
        // Its folder grant is what's lost first.
        let folder = on_machine(Some(&drafts()), Toward::Machine);
        assert_eq!(folder.lost, Guarantee::Folder);
        let said = folder.to_string();
        assert!(
            said.contains("writer is on team research, whose shared folder")
                && said.ends_with("lnk agent team move research <cloud>"),
            "{said}"
        );
        assert_eq!(
            on_machine(Some(&drafts()), Toward::Hosted).to,
            Toward::Hosted
        );
        // An unknown grant counts as reaching a team's folder.
        let unknown = refused(verdict(&team, WRITER, None, Toward::Machine, &Direct));
        assert_eq!(unknown.lost, Guarantee::Folder);
        // A team a cloud keeps has its folder there, named in its file or
        // not.
        let unnamed = Team {
            shared: None,
            ..team.clone()
        };
        let hosted = refused(verdict(
            &unnamed,
            WRITER,
            None,
            Toward::Machine,
            &ways(false, true),
        ));
        assert_eq!(hosted.lost, Guarantee::Folder);
        // No folder grant: its conversation is still the team's.
        let none = TeamFolders::default();
        let talk = refused(verdict(
            &team,
            WRITER,
            Some(&none),
            Toward::Machine,
            &Direct,
        ));
        assert_eq!(talk.lost, Guarantee::Conversation);
        assert!(
            talk.to_string()
                .ends_with("lnk agent team move research@<cloud> --here"),
            "{talk}"
        );
        // A host without a relay keeps the folder alone; one with a relay
        // and no folder service, the conversation alone.
        let no_relay = refused(verdict(
            &team,
            WRITER,
            Some(&drafts()),
            Toward::Machine,
            &ways(true, false),
        ));
        assert_eq!(no_relay.lost, Guarantee::Conversation);
        let no_folder = refused(verdict(
            &team,
            WRITER,
            Some(&drafts()),
            Toward::Machine,
            &ways(false, true),
        ));
        assert_eq!(no_folder.lost, Guarantee::Folder);
        // Its ways reach a machine, never another cloud's runner, and
        // only the teams they name.
        let elsewhere = refused(verdict(
            &team,
            WRITER,
            Some(&none),
            Toward::Hosted,
            &ways(true, true),
        ));
        assert_eq!(elsewhere.lost, Guarantee::Conversation);
        let other = Team {
            id: "q8".into(),
            ..team.clone()
        };
        assert!(matches!(
            verdict(
                &other,
                WRITER,
                Some(&none),
                Toward::Machine,
                &ways(true, true)
            ),
            Verdict::Refuses(_)
        ));
        // Not a member: no verdict at all.
        assert!(
            check(
                &team,
                Kept::Machine,
                "z9z9z9z9z9z9",
                None,
                Toward::Home,
                &Direct
            )
            .is_err()
        );
        // A grant that isn't one is refused as such.
        let odd = TeamFolders {
            write: vec!["../up".into()],
            read: Vec::new(),
        };
        assert!(
            check(
                &team,
                Kept::Machine,
                WRITER,
                Some(&odd),
                Toward::Home,
                &Direct
            )
            .is_err()
        );
    }

    #[test]
    fn a_kept_member_keeps_its_id_and_lead_and_its_grants_are_derived_again() {
        let team = research();
        let scout = kept(
            check(
                &team,
                Kept::Machine,
                SCOUT,
                Some(&drafts()),
                Toward::Home,
                &Direct,
            )
            .unwrap(),
        );
        assert_eq!(
            (
                scout.team.as_str(),
                scout.member.as_str(),
                scout.id.as_str()
            ),
            ("r7", "scout", SCOUT)
        );
        assert!(scout.lead, "the lead stays the lead");
        assert_eq!(scout.folders, drafts());
        assert_eq!(scout.role, team.role().unwrap());
        // Through a host's folder service and relay, a machine keeps it:
        // the lead too, with the role of the roster as it is now, and the
        // grant its sandbox vouches for.
        let smaller = Team {
            members: team
                .members
                .clone()
                .into_iter()
                .filter(|(name, _)| name != "writer")
                .collect(),
            ..team.clone()
        };
        let away = kept(verdict(
            &smaller,
            SCOUT,
            Some(&drafts()),
            Toward::Machine,
            &ways(true, true),
        ));
        assert!(away.lead);
        assert_eq!(away.folders, drafts());
        let role = toml::to_string(&away.role).unwrap();
        assert!(!role.contains(WRITER), "{role}");
        assert!(role.contains(SCOUT) && role.contains(CHECKER), "{role}");
        // No folder grant: the relay alone keeps it.
        let none = TeamFolders::default();
        let talking = kept(verdict(
            &team,
            WRITER,
            Some(&none),
            Toward::Machine,
            &ways(false, true),
        ));
        assert_eq!(talking.folders, none);
        // An outside member's grant is its roster entry's, never what the
        // asker says of it.
        let whole = TeamFolders {
            write: vec![".".into()],
            read: Vec::new(),
        };
        let checker = kept(verdict(&team, CHECKER, Some(&whole), Toward::Home, &Direct));
        assert_eq!(checker.folders.write, ["notes"]);
        assert!(!checker.lead);
    }
}
