//! The terms a way in takes a message for its agent on ([`Terms`]):
//! whoever serves an agent's way in, its bridge on someone's own machine
//! or a machine that packs agents, takes a [`Firing`] only on these, and
//! hands it on as they leave it.

use crate::bridge::Identity;
use link_agent_team::Team;

use super::{AnswerTo, Firing};

/// Who a way in takes messages for, and on what terms: its agent, by
/// its name and who it is; whether its harness keeps a conversation's
/// tools; and the tools its spec gives a stranger's message
/// (`[tools] strangers`). The teams it's on are handed to each check, as
/// they are then: a way in reads them as it takes each message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terms {
    /// The agent's name, as messages refused say it.
    pub name: String,
    /// The agent's ID and owner.
    pub me: Identity,
    /// Whether its harness keeps a conversation's tools.
    pub tools: bool,
    /// The tools a stranger's message may use.
    pub strangers: Vec<String>,
}

impl Terms {
    /// Whether `firing`, proved as `caller`'s, is one this agent takes:
    /// any of its own (its scheduler's, its tools'); another agent's only
    /// as that agent's own (`agent:<its ID>`), from a teammate on
    /// `teams`, to its conversation with this one or a team's they share,
    /// its answer going nowhere but back to the scheduler that sent it.
    /// Who it's from is never taken from the message alone.
    pub fn caller<S, T>(
        &self,
        caller: &Identity,
        firing: &Firing,
        teams: &[Team<S, T>],
    ) -> anyhow::Result<()> {
        self.someones()?;
        if caller.agent == self.me.agent && caller.owner == self.me.owner {
            return Ok(());
        }
        let sender = link_format::clean(&caller.agent);
        anyhow::ensure!(
            firing.from == format!("agent:{}", caller.agent),
            "agent {sender} can send only as itself: its message isn't taken"
        );
        anyhow::ensure!(
            self.teammate(&caller.agent, teams),
            "agent {sender} isn't on a team with {}: its message isn't taken",
            self.name
        );
        let shared = teams
            .iter()
            .filter(|team| team.has(&self.me.agent) && team.has(&caller.agent))
            .any(|team| firing.thread == super::teams_thread(&team.id));
        anyhow::ensure!(
            shared || firing.thread == super::teammates_thread(&caller.agent),
            "agent {sender}'s message goes only to its conversation with {} or a team's they \
             share, not {}",
            self.name,
            link_format::clean(&firing.thread)
        );
        anyhow::ensure!(
            firing.answer == AnswerTo::Nowhere,
            "agent {sender}'s message is answered only to it: its answer can't go elsewhere"
        );
        Ok(())
    }

    /// `firing`, whoever handed it, as this agent takes it, or why it
    /// doesn't: a stranger's only on its own conversation, answered
    /// nowhere, and with only the tools its spec gives strangers, always
    /// said, so its conversation never keeps more (refused for a harness
    /// that can't narrow them); a list of tools only for a harness that
    /// keeps them; another agent's only from a teammate on `teams`.
    pub fn firing<S, T>(&self, mut firing: Firing, teams: &[Team<S, T>]) -> anyhow::Result<Firing> {
        self.someones()?;
        if crate::access::Standing::of(Some(&firing.from)) == crate::access::Standing::Stranger {
            anyhow::ensure!(
                self.tools,
                "this agent's harness can't narrow a conversation's tools, so a message from \
                 {} isn't taken",
                link_format::clean(&firing.from)
            );
            // Its own conversation alone, named as it is, and no answer
            // anywhere: it never reads or narrows another, and nothing
            // it says reaches the owner's.
            anyhow::ensure!(
                firing.thread == firing.from,
                "a message from {} goes only to its own conversation ({}), not {}",
                link_format::clean(&firing.from),
                link_format::clean(&firing.from),
                link_format::clean(&firing.thread)
            );
            anyhow::ensure!(
                firing.answer == AnswerTo::Nowhere,
                "a message from {} is answered nowhere",
                link_format::clean(&firing.from)
            );
            firing.tools = Some(self.strangers_tools(firing.tools.take()));
        }
        // Fewer tools than its conversation has, for a harness that
        // can't keep them: refused, not sent with them all.
        anyhow::ensure!(
            firing.tools.is_none() || self.tools,
            "this agent's harness can't give a conversation fewer tools: send it without a \
             list of tools"
        );
        if let Some(("agent", sender)) = firing.from.split_once(':') {
            anyhow::ensure!(
                self.teammate(sender, teams),
                "agent {} isn't on a team with {}: its message isn't taken",
                link_format::clean(sender),
                self.name
            );
        }
        Ok(firing)
    }

    /// Refused for terms naming no agent, or no owner: a way in for no
    /// agent takes nothing, not even "its own".
    fn someones(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.me.agent.is_empty() && !self.me.owner.is_empty(),
            "a way in for no agent takes nothing"
        );
        Ok(())
    }

    /// Whether agent `sender` (its ID) is on a team of `teams` with this
    /// one.
    pub fn teammate<S, T>(&self, sender: &str, teams: &[Team<S, T>]) -> bool {
        let me = &self.me.agent;
        if me.is_empty() || sender == me {
            return false;
        }
        // The sender, holding the role of each team it's on, may message
        // this agent: `allows` says, as every check does.
        let on: Vec<_> = teams.iter().filter(|team| team.has(sender)).collect();
        // Held by the service the sender acts as, as its teams name it:
        // one that names none is no one's teammate.
        let Some(service) = on.iter().find_map(|team| team.service_of(sender)) else {
            return false;
        };
        let roles: Vec<_> = on.iter().filter_map(|team| team.role().ok()).collect();
        let sender = crate::bridge::Identity {
            agent: sender.to_string(),
            service: service.to_string(),
            owner: self.me.owner.clone(),
            teams: Vec::new(),
        };
        let message = link_policy::Action::of(link_policy::Kind::Agent, "message").expect("a verb");
        link_sandbox_bridge::grants::principal(&sender, roles).is_ok_and(|sender| {
            link_sandbox_bridge::grants::may(
                &sender,
                message,
                &format!("agent:{me}"),
                Default::default(),
            )
        })
    }

    /// The tools a stranger's message is given: those it asked for that
    /// the spec gives strangers, or all of those when it asked for none;
    /// never one more.
    pub fn strangers_tools(&self, asked: Option<Vec<String>>) -> Vec<String> {
        match asked {
            Some(asked) => asked
                .into_iter()
                .filter(|tool| self.strangers.contains(tool))
                .collect(),
            None => self.strangers.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "a1b2c3d4e5f6";
    const MATE: &str = "b2c3d4e5f6a1";
    const OTHER: &str = "c3d4e5f6a1b2";
    const TEAM: &str = "t4m9q2x7k1ab";

    fn terms() -> Terms {
        Terms {
            name: "ada".into(),
            me: Identity {
                agent: ME.into(),
                service: String::new(),
                owner: "local-link-aaaaaaaaaaaa".into(),
                teams: Vec::new(),
            },
            tools: true,
            strangers: vec!["memory__search".into()],
        }
    }

    /// The team, each member naming its service as the agent module
    /// fills it in from its settings.
    fn teams() -> Vec<link_agent_spec::Team> {
        let mut team = crate::agent_spec::parse_team(&format!(
            "schema = 1\nid = \"{TEAM}\"\nname = \"research\"\n\n[member.ada]\nid = \"{ME}\"\n\n\
             [member.bo]\nid = \"{MATE}\"\n"
        ))
        .unwrap();
        for member in team.members.values_mut() {
            member.service = Some(link_id::candidate().unwrap());
        }
        vec![team]
    }

    fn caller(agent: &str) -> Identity {
        Identity {
            agent: agent.into(),
            ..terms().me
        }
    }

    /// Its own calls pass whatever they say; another agent's only as
    /// itself, a teammate's, to their conversation or a team's they
    /// share, answered nowhere.
    #[test]
    fn another_agent_is_taken_only_as_a_teammate_to_it() {
        let (terms, teams) = (terms(), teams());
        let own = Firing {
            thread: "telegram:42".into(),
            from: "schedule:a1b2c3d4e5f6".into(),
            ..Firing::default()
        };
        terms.caller(&terms.me, &own, &teams).unwrap();
        let mate = Firing {
            thread: super::super::teammates_thread(MATE),
            from: format!("agent:{MATE}"),
            answer: AnswerTo::Nowhere,
            ..Firing::default()
        };
        terms.caller(&caller(MATE), &mate, &teams).unwrap();
        let team = Firing {
            thread: super::super::teams_thread(TEAM),
            ..mate.clone()
        };
        terms.caller(&caller(MATE), &team, &teams).unwrap();
        for (who, firing, why) in [
            (
                MATE,
                Firing {
                    from: "schedule:a1b2c3d4e5f6".into(),
                    ..mate.clone()
                },
                "only as itself",
            ),
            (
                OTHER,
                Firing {
                    from: format!("agent:{OTHER}"),
                    ..mate.clone()
                },
                "isn't on a team",
            ),
            (
                MATE,
                Firing {
                    thread: "telegram:42".into(),
                    ..mate.clone()
                },
                "goes only to",
            ),
            (
                MATE,
                Firing {
                    answer: AnswerTo::Chat,
                    ..mate.clone()
                },
                "answered only to it",
            ),
        ] {
            let err = terms.caller(&caller(who), &firing, &teams).unwrap_err();
            assert!(format!("{err}").contains(why), "{err}");
        }
        // With no teams, no other agent is.
        assert!(
            terms
                .caller(&caller(MATE), &mate, &[] as &[Team<()>])
                .is_err()
        );
    }

    /// A stranger's message: its own conversation, answered nowhere, its
    /// tools narrowed to the strangers'; refused by a harness that can't
    /// narrow them.
    #[test]
    fn a_stranger_is_taken_on_its_own_thread_with_strangers_tools() {
        let terms = terms();
        let stranger = Firing {
            thread: "webhook:github".into(),
            from: "webhook:github".into(),
            answer: AnswerTo::Nowhere,
            tools: Some(vec!["memory__search".into(), "web__fetch".into()]),
            ..Firing::default()
        };
        let taken = terms.firing(stranger.clone(), &[] as &[Team<()>]).unwrap();
        assert_eq!(taken.tools, Some(vec!["memory__search".to_string()]));
        let asked_none = terms
            .firing(
                Firing {
                    tools: None,
                    ..stranger.clone()
                },
                &[] as &[Team<()>],
            )
            .unwrap();
        assert_eq!(asked_none.tools, Some(terms.strangers.clone()));
        for (firing, why) in [
            (
                Firing {
                    thread: "telegram:42".into(),
                    ..stranger.clone()
                },
                "own conversation",
            ),
            (
                Firing {
                    answer: AnswerTo::Chat,
                    ..stranger.clone()
                },
                "answered nowhere",
            ),
        ] {
            let err = terms.firing(firing, &[] as &[Team<()>]).unwrap_err();
            assert!(format!("{err}").contains(why), "{err}");
        }
        let blunt = Terms {
            tools: false,
            ..terms.clone()
        };
        assert!(blunt.firing(stranger, &[] as &[Team<()>]).is_err());
    }

    /// Terms for no agent, or no owner, take nothing: two empty
    /// identities aren't one agent's own.
    #[test]
    fn terms_for_no_agent_take_nothing() {
        let own = Firing {
            thread: "telegram:42".into(),
            from: "schedule:a1b2c3d4e5f6".into(),
            ..Firing::default()
        };
        let none = Terms::default();
        let err = none
            .caller(&Identity::default(), &own, &[] as &[Team<()>])
            .unwrap_err();
        assert!(format!("{err}").contains("for no agent"), "{err}");
        assert!(none.firing(own.clone(), &[] as &[Team<()>]).is_err());
        let ownerless = Terms {
            me: Identity {
                owner: String::new(),
                ..terms().me
            },
            ..terms()
        };
        assert!(
            ownerless
                .caller(&ownerless.me, &own, &[] as &[Team<()>])
                .is_err()
        );
        assert!(ownerless.firing(own, &[] as &[Team<()>]).is_err());
    }

    /// A list of tools only for a harness that keeps them; another
    /// agent's message only from a teammate.
    #[test]
    fn tools_need_a_harness_that_keeps_them_and_agents_a_team() {
        let (terms, teams) = (terms(), teams());
        let narrowed = Firing {
            thread: "telegram:42".into(),
            from: "schedule:a1b2c3d4e5f6".into(),
            tools: Some(Vec::new()),
            ..Firing::default()
        };
        terms.firing(narrowed.clone(), &teams).unwrap();
        let blunt = Terms {
            tools: false,
            ..terms.clone()
        };
        assert!(blunt.firing(narrowed, &teams).is_err());
        let from_mate = Firing {
            from: format!("agent:{MATE}"),
            ..Firing::default()
        };
        terms.firing(from_mate.clone(), &teams).unwrap();
        assert!(terms.firing(from_mate, &[] as &[Team<()>]).is_err());
        let from_other = Firing {
            from: format!("agent:{OTHER}"),
            ..Firing::default()
        };
        assert!(terms.firing(from_other, &teams).is_err());
    }
}
