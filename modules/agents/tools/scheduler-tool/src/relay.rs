//! The team relay's client ([`TeamRelay`], [`Relayed`]): how an agent on
//! a machine of yours talks with teammates that aren't, through the
//! account that hosts its team (`link_agent_place::relay`). It signs
//! in as itself with its own agent token for each team
//! ([`InstallationTokens`]), never as the machine, and the relay takes who
//! sent a message from that token.
//!
//! A message to a teammate this machine doesn't reach, on the team's
//! conversation (`team-<id>`), goes to the relay for that teammate
//! ([`TeamSend::to`]). What the relay kept for this agent is pulled at
//! each look of the runner ([`Driver::arrivals`]), from the last message
//! it had, and put as a job of its own: a message from `agent:<sender>`
//! on the team's conversation, answered nowhere, fired through the
//! agent's way in as any job is.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use link_agent_place::relay::{Conversation, Mail, Mailbox, Sent, TeamIndex, TeamSend};
use link_secretive::Secret;

use super::*;

/// The file in the scheduler's state that keeps, by team ID, the last
/// message pulled.
const CURSORS: &str = "team-relay.json";

/// Where an agent's team tokens come from: its own, for a team.
pub trait Tokens: Send + Sync {
    /// The agent's token for the relay, granted membership of `team`.
    fn token(&self, team: &TeamIndex) -> anyhow::Result<Secret>;
}

/// An agent's tokens as its machine signs them
/// ([`link_core::owner::TOKEN`], for the relay's host, the agent and
/// `team.member@team:<team ID>`): kept here, never in a file, until a
/// minute before each ends.
pub struct InstallationTokens {
    pub agent: String,
    /// The relay's host, the tokens' audience.
    pub audience: String,
    kept: Mutex<BTreeMap<String, (Secret, u64)>>,
}

impl InstallationTokens {
    pub fn new(agent: &str, audience: &str) -> Self {
        Self {
            agent: agent.into(),
            audience: audience.into(),
            kept: Mutex::default(),
        }
    }
}

impl Tokens for InstallationTokens {
    fn token(&self, team: &TeamIndex) -> anyhow::Result<Secret> {
        let at = now();
        if let Some((token, ends)) = self
            .kept
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&team.team)
            && *ends > at + limits().token_margin_secs
        {
            return Ok(token.duplicate());
        }
        let grants = format!("team.member@team:{}", team.team);
        let made = link_core::owner::signed_token(&link_core::owner::TokenAsk {
            audience: self.audience.clone(),
            workload: Some(self.agent.clone()),
            grants: Some(grants),
        })
        .with_context(|| {
            format!(
                "the machine made agent {} no token for the team relay",
                link_format::clean(&self.agent)
            )
        })?;
        let token = made.token.duplicate();
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(
            team.team.clone(),
            (made.token, made.expires_at.unwrap_or(at)),
        );
        Ok(token)
    }
}

/// The team relay at `url`, for the agent `agent` and its teams, by
/// their IDs, each with its owner.
pub struct TeamRelay {
    url: String,
    agent: String,
    teams: BTreeMap<String, TeamIndex>,
    tokens: Arc<dyn Tokens>,
    http: link_fetch::blocking::Client,
}

impl TeamRelay {
    /// The relay at `url` (`https://`, or `http://` to this machine), for
    /// `agent` on `teams`, signing in with `tokens`.
    pub fn new(
        url: &str,
        agent: &str,
        teams: impl IntoIterator<Item = TeamIndex>,
        tokens: Arc<dyn Tokens>,
    ) -> anyhow::Result<Self> {
        link_machine::machine::check_id(agent)?;
        let teams = teams
            .into_iter()
            .map(|team| team.check().map(|()| (team.team.clone(), team)))
            .collect::<anyhow::Result<_>>()?;
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            agent: agent.into(),
            teams,
            tokens,
            http: link_fetch::blocking::Client::new()?,
        })
    }

    /// The relay's host: the audience of the tokens it takes.
    pub fn host(url: &str) -> anyhow::Result<String> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .context("the team relay's address is https://<host>")?;
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
        anyhow::ensure!(!host.is_empty(), "the team relay's address names no host");
        Ok(host.to_ascii_lowercase())
    }

    /// The team whose conversation `thread` is, if it's one of the
    /// relay's.
    fn team_of(&self, thread: &str) -> Option<&TeamIndex> {
        let conversation = Conversation::try_from(thread.to_string()).ok()?;
        self.teams.get(conversation.team())
    }

    /// Whether a message on `thread` goes through the relay.
    pub fn carries(&self, thread: &str) -> bool {
        self.team_of(thread).is_some()
    }

    /// `firing`, on a team's conversation, sent to teammate `agent`
    /// through the relay: answered once the relay took it, with no
    /// answer, since the teammate's comes back as a message of its own.
    pub fn send(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        let team = self
            .team_of(&firing.thread)
            .with_context(|| format!("{} isn't a team the relay carries", firing.thread))?;
        let sent = TeamSend {
            team: team.clone(),
            conversation: Conversation::of(team),
            to: Some(agent.to_string()),
            hops: firing.hops,
            text: firing.text.clone(),
        };
        sent.check()?;
        let token = self.tokens.token(team)?;
        let url = format!("{}{}", self.url, team.messages_path());
        let reached: Sent = self
            .http
            .post(&url)
            .bearer(&token)
            .json(&sent)
            .answer()
            .with_context(|| {
                format!(
                    "handing the team relay a message for agent {}",
                    link_format::clean(agent)
                )
            })?;
        anyhow::ensure!(reached.reached > 0, "the team relay reached no one");
        Ok(Fired::default())
    }

    /// What the relay kept for this agent on `team` after `after`.
    pub fn pull(&self, team: &TeamIndex, after: u64) -> anyhow::Result<Vec<Mail>> {
        let token = self.tokens.token(team)?;
        let url = format!("{}{}", self.url, team.messages_path());
        let mailbox: Mailbox = self
            .http
            .get(&url)
            .bearer(&token)
            .query(&link_agent_place::relay::After { after })
            .answer()
            .context("pulling the team relay's messages")?;
        Ok(mailbox.mail)
    }

    /// Each team's messages pulled after the last had, and put in `store`
    /// as jobs: how many. A team's cursor moves past a message once its
    /// job is kept, so one that failed is pulled again.
    pub fn deliver(&self, store: &Store) -> anyhow::Result<usize> {
        let _turn = link_files::lock_file(&store.dir().join("team-relay.lock"))?;
        let path = store.dir().join(CURSORS);
        let mut cursors = read_cursors(&path);
        let mut put = 0;
        for team in self.teams.values() {
            for _ in 0..limits().pulls_at_a_look {
                let after = cursors.get(&team.team).copied().unwrap_or(0);
                let mail = self.pull(team, after)?;
                let Some(last) = mail.last().map(|m| m.seq) else {
                    break;
                };
                for each in &mail {
                    anyhow::ensure!(
                        store.waiting() < limits().jobs_max,
                        "{} jobs waiting: the team relay's messages wait there",
                        limits().jobs_max
                    );
                    // Only on the conversation of the team it was pulled
                    // from, which its sender shares: one naming another
                    // conversation is passed over, never put there.
                    if each.message.team == *team && each.message.conversation.is_of(team) {
                        store.put(&self.job(each)?)?;
                        put += 1;
                    } else {
                        warn(sentence!(
                            "A message the team relay kept for team {} names another team's \
                             conversation, so it was passed over.",
                            Name::new("team", &team.team)
                        ));
                    }
                    cursors.insert(team.team.clone(), each.seq);
                    write_cursors(&path, &cursors)?;
                }
                if mail.len() < link_agent_place::relay::MAIL_PAGE || last <= after {
                    break;
                }
            }
        }
        Ok(put)
    }

    /// A pulled message as a job: from its sender, on its team's
    /// conversation, now, answered nowhere.
    fn job(&self, mail: &Mail) -> anyhow::Result<Job> {
        let message = &mail.message;
        let at = now();
        Ok(Job {
            occurrence: None,
            portable: None,
            id: link_id::candidate()?,
            kind: Kind::Reply,
            thread: message.conversation.as_str().to_string(),
            text: message.text.clone(),
            answer: AnswerTo::Nowhere,
            due: at,
            cron: None,
            hops: message.hops,
            created: at,
            tries: 0,
            tools: None,
            standing: link_connection_contract::access::Standing::Member,
            model: None,
            to: Some(Peers {
                from: self.agent.clone(),
                to: message.from.agent().id().to_string(),
            }),
            parent: None,
            after: Vec::new(),
            description: None,
            outcome: None,
        })
    }
}

/// The last message pulled, by team ID, as [`CURSORS`] keeps it, naming
/// its schema.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Cursors(BTreeMap<String, u64>);

link_files::specification!(Cursors, include_str!("relay.schema.toml"));

fn read_cursors(path: &Path) -> BTreeMap<String, u64> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Cursors>(&bytes).ok())
        .unwrap_or_default()
        .0
}

fn write_cursors(path: &Path, cursors: &BTreeMap<String, u64>) -> anyhow::Result<()> {
    link_files::write_private(
        path,
        &link_files::saved::print_json(&Cursors(cursors.clone()))?,
    )
    .with_context(|| format!("writing {}", path.display()))
}

/// The relay driver: the driver it holds, and the team relay for a
/// teammate that driver doesn't reach, on a team's conversation the relay
/// carries; what the relay kept for the agent comes in at each look.
pub struct Relayed(pub Arc<dyn Driver>, pub TeamRelay);

impl Driver for Relayed {
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        self.0.fire(firing)
    }

    fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        match !self.0.reaches(agent) && self.1.carries(&firing.thread) {
            true => self.1.send(agent, firing),
            false => self.0.fire_to(agent, firing),
        }
    }

    fn reaches(&self, agent: &str) -> bool {
        self.0.reaches(agent)
    }

    fn resident(&self) -> bool {
        self.0.resident()
    }

    fn woken(&self, store: &Store) -> anyhow::Result<()> {
        self.0.woken(store)
    }

    fn arrivals(&self, store: &Store) -> anyhow::Result<usize> {
        Ok(self.0.arrivals(store)? + self.1.deliver(store)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;
    use link_agent_place::relay::{Sender, TeamMessage};
    use std::io::{BufRead, BufReader, Read, Write};

    const ME: &str = "a1b2c3d4e5f6";
    const NEAR: &str = "g7h8j9k0m1n2";
    const FAR: &str = "p3q4r5s6t7u8";
    const ACCOUNT: &str = "acct-0123456789abcdef0123456789abcdef";
    const TOKEN: &str = "eyJ.member.token";

    fn team() -> TeamIndex {
        TeamIndex {
            owner: ACCOUNT.into(),
            team: "t4m9q2x7k1ab".into(),
        }
    }

    /// The agent's token, the same for every team.
    struct Fixed;

    impl Tokens for Fixed {
        fn token(&self, asked: &TeamIndex) -> anyhow::Result<Secret> {
            assert_eq!(*asked, team());
            Ok(TOKEN.to_string().into())
        }
    }

    /// One request the stand-in relay took: its method, path and query,
    /// bearer and body.
    #[derive(Debug)]
    struct Took {
        method: String,
        target: String,
        bearer: String,
        body: serde_json::Value,
    }

    /// A team relay standing in on a held loopback port: it reads each
    /// whole request and keeps it, then answers a send `{"reached": 1}`,
    /// and a pull the mail in `kept` after its `after`, as the relay does.
    fn stand_in(kept: Vec<Mail>) -> (String, Arc<Mutex<Vec<Took>>>) {
        let held = link_test_kit::net::Held::loopback();
        let url = format!("http://{}", held.address());
        let listener = held.listener();
        let took = Arc::new(Mutex::new(Vec::new()));
        let seen = took.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut words = line.split_whitespace();
                let (method, target) = (words.next().unwrap(), words.next().unwrap());
                let (mut length, mut bearer) = (0, String::new());
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(':').unwrap();
                    match name.to_ascii_lowercase().as_str() {
                        "content-length" => length = value.trim().parse().unwrap(),
                        "authorization" => bearer = value.trim().to_string(),
                        _ => {}
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let answer = match method {
                    "POST" => serde_json::json!({"reached": 1}),
                    _ => {
                        let after: u64 = target
                            .split_once("?after=")
                            .map_or(0, |(_, after)| after.parse().unwrap());
                        let mail: Vec<&Mail> = kept.iter().filter(|m| m.seq > after).collect();
                        serde_json::json!({ "mail": mail })
                    }
                };
                seen.lock().unwrap().push(Took {
                    method: method.into(),
                    target: target.into(),
                    bearer,
                    body: serde_json::from_slice(&body).unwrap_or_default(),
                });
                let answer = answer.to_string();
                let _ = write!(
                    &stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (url, took)
    }

    fn relay(url: &str) -> TeamRelay {
        TeamRelay::new(url, ME, [team()], Arc::new(Fixed)).unwrap()
    }

    fn mail(seq: u64, from: &str, text: &str) -> Mail {
        Mail {
            seq,
            message: TeamMessage {
                from: Sender::new(ACCOUNT, format!("service:{from}").parse().unwrap()).unwrap(),
                team: team(),
                conversation: Conversation::of(&team()),
                hops: 2,
                text: text.into(),
            },
        }
    }

    /// The agent's own driver, reaching one teammate on this machine.
    struct Near(Arc<Mutex<Vec<String>>>);

    impl Driver for Near {
        fn fire(&self, _: &Firing) -> anyhow::Result<Fired> {
            Ok(Fired::default())
        }
        fn fire_to(&self, agent: &str, _: &Firing) -> anyhow::Result<Fired> {
            self.0.lock().unwrap().push(agent.to_string());
            match agent {
                NEAR => Ok(Fired::default()),
                _ => anyhow::bail!("no way to reach agent {agent}"),
            }
        }
        fn reaches(&self, agent: &str) -> bool {
            agent == NEAR
        }
    }

    /// A team's message to a teammate this machine doesn't reach goes to
    /// the relay, for that teammate, signed in with the agent's own token
    /// and never saying who sent it; one it reaches goes to its way in,
    /// and a message off the team's conversation isn't the relay's.
    #[test]
    fn a_message_off_this_machine_goes_through_the_relay_as_the_agent() {
        let (url, took) = stand_in(Vec::new());
        let near = Arc::new(Mutex::new(Vec::new()));
        let driver = Relayed(Arc::new(Near(near.clone())), relay(&url));
        let on_team = Firing {
            thread: "team-t4m9q2x7k1ab".into(),
            from: format!("agent:{ME}"),
            text: "Draft the summary?".into(),
            answer: AnswerTo::Nowhere,
            hops: 3,
            ..Default::default()
        };
        driver.fire_to(FAR, &on_team).unwrap();
        driver.fire_to(NEAR, &on_team).unwrap();
        let ours = Firing {
            thread: format!("agent-{ME}"),
            ..on_team.clone()
        };
        assert!(driver.fire_to(FAR, &ours).is_err());
        assert_eq!(*near.lock().unwrap(), [NEAR, FAR]);
        let took = took.lock().unwrap();
        assert_eq!(took.len(), 1, "{took:?}");
        assert_eq!(took[0].method, "POST");
        assert_eq!(
            took[0].target,
            format!("/v1/teams/{ACCOUNT}/t4m9q2x7k1ab/messages")
        );
        assert_eq!(took[0].bearer, format!("Bearer {TOKEN}"));
        assert_eq!(
            took[0].body,
            serde_json::json!({"team": {"owner": ACCOUNT, "team": "t4m9q2x7k1ab"},
                "conversation": "team-t4m9q2x7k1ab", "to": FAR, "hops": 3,
                "text": "Draft the summary?"})
        );
    }

    /// What the relay kept for the agent comes in at a wake as jobs, each
    /// fired through its way in from its sender on the team's
    /// conversation, answered nowhere; the next pull goes on from the
    /// last had.
    #[test]
    fn the_relays_messages_come_in_from_their_senders() {
        let (url, took) = stand_in(vec![mail(7, NEAR, "One."), mail(9, FAR, "Two.")]);
        let (dir, store) = store("relay-in");
        let (inner, seen) = recording();
        let driver: Arc<dyn Driver> = Arc::new(Cron(Arc::new(Relayed(inner, relay(&url)))));
        wake(&store, &driver, &CAPS, now() + 1).unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        let mut froms: Vec<(String, String)> = seen
            .iter()
            .map(|f| (f.from.clone(), f.text.clone()))
            .collect();
        froms.sort();
        assert_eq!(
            froms,
            [
                (format!("agent:{NEAR}"), "One.".to_string()),
                (format!("agent:{FAR}"), "Two.".to_string())
            ]
        );
        for firing in &seen {
            assert_eq!(firing.thread, "team-t4m9q2x7k1ab");
            assert_eq!(firing.answer, AnswerTo::Nowhere);
            assert_eq!(firing.hops, 2);
        }
        assert!(store.jobs().is_empty());
        // The next wake pulls after the last had, and finds nothing new.
        wake(&store, &driver, &CAPS, now() + 1).unwrap();
        let took = took.lock().unwrap();
        let pulls: Vec<&str> = took.iter().map(|t| t.target.as_str()).collect();
        assert_eq!(
            pulls,
            [
                format!("/v1/teams/{ACCOUNT}/t4m9q2x7k1ab/messages?after=0"),
                format!("/v1/teams/{ACCOUNT}/t4m9q2x7k1ab/messages?after=9"),
            ]
        );
        assert!(took.iter().all(|t| t.bearer == format!("Bearer {TOKEN}")));
        let kept = std::fs::read_to_string(dir.join(CURSORS)).unwrap();
        assert_eq!(kept, r#"{"t4m9q2x7k1ab":9,"schema":1}"#);
    }

    /// A message pulled from a team lands only on that team's
    /// conversation: one naming another team's, which its sender may not
    /// share, is passed over.
    #[test]
    fn a_relayed_message_lands_only_on_its_teams_conversation() {
        let other = TeamIndex {
            owner: ACCOUNT.into(),
            team: "z9y8x7w6v5ab".into(),
        };
        let mut astray = mail(8, FAR, "Elsewhere.");
        astray.message.conversation = Conversation::of(&other);
        let (url, _took) = stand_in(vec![mail(7, NEAR, "One."), astray]);
        let (_dir, store) = store("relay-astray");
        let relay = relay(&url);
        assert_eq!(relay.deliver(&store).unwrap(), 1);
        let jobs = store.jobs();
        assert_eq!(jobs.len(), 1, "{jobs:?}");
        assert_eq!(jobs[0].text, "One.");
        assert_eq!(jobs[0].thread, "team-t4m9q2x7k1ab");
        // Passed over for good: the next pull goes on after it.
        assert_eq!(relay.deliver(&store).unwrap(), 0);
    }

    /// The relay's host is its tokens' audience.
    #[test]
    fn the_relays_host_is_its_tokens_audience() {
        for (url, host) in [
            ("https://dispatch.example", "dispatch.example"),
            ("https://Dispatch.Example:8443/x", "dispatch.example"),
            ("http://127.0.0.1:7600", "127.0.0.1"),
        ] {
            assert_eq!(TeamRelay::host(url).unwrap(), host);
        }
        for bad in ["dispatch.example", "ftp://x", "https://"] {
            assert!(TeamRelay::host(bad).is_err(), "{bad}");
        }
    }
}
