//! How a firing reaches the agent ([`Driver`]): in this process, through
//! its bridge's way in, to a teammate's, on the calendar or an address.

use super::*;

/// How the runner wakes, and how a firing reaches the agent.
pub trait Driver: Send + Sync {
    /// Hands `firing` to the agent: what its answer was, once it went
    /// where it goes.
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired>;

    /// Hands `firing` to the agent's teammate whose agent ID is `agent`:
    /// its answer. None reached, for a driver without teammates.
    fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        let _ = firing;
        bail!("no way to reach agent {agent}: it isn't a teammate here")
    }

    /// Whether `agent`, a teammate, is reached on this machine
    /// ([`Driver::fire_to`]): none, for a driver without teammates.
    fn reaches(&self, agent: &str) -> bool {
        let _ = agent;
        false
    }

    /// Whether the runner stays, sleeping until the next job is due
    /// ([`run`]); else something else wakes it ([`Cron`], [`Addressed`]),
    /// and it fires what's due, waits for those firings to end, and stops
    /// ([`wake`]).
    fn resident(&self) -> bool {
        true
    }

    /// What a wake does once its firings ended, the jobs in `store` as
    /// they're left: nothing, or tell the machine when the next is due
    /// ([`Addressed`]).
    fn woken(&self, store: &Store) -> anyhow::Result<()> {
        let _ = store;
        Ok(())
    }

    /// What waits for the agent elsewhere, put in `store` as jobs at
    /// each look of the runner: nothing, or a team relay's messages
    /// ([`crate::Relayed`]). How many.
    fn arrivals(&self, store: &Store) -> anyhow::Result<usize> {
        let _ = store;
        Ok(0)
    }
}

/// The cron driver: the machine's calendar (a line of the user's
/// crontab, or launchd's on a Mac, [`calendar`]) starts the runner once
/// a minute, and nothing of it runs between; each firing goes through
/// the driver it holds, the bridge's way in. A job fires within a minute
/// of when it's due, its time in the scheduler's zone as on every
/// driver: the calendar only wakes it.
pub struct Cron(pub std::sync::Arc<dyn Driver>);

impl Driver for Cron {
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        self.0.fire(firing)
    }

    fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        self.0.fire_to(agent, firing)
    }

    fn reaches(&self, agent: &str) -> bool {
        self.0.reaches(agent)
    }

    fn resident(&self) -> bool {
        false
    }

    fn arrivals(&self, store: &Store) -> anyhow::Result<usize> {
        self.0.arrivals(store)
    }
}

/// The address driver, for a machine that runs the agent only while it
/// answers: it tells the machine at [`Address`] when the agent's jobs
/// are next due, and whatever wakes the agent then starts the runner
/// once ([`wake`]), which fires what's due through the driver it holds,
/// the bridge's way in, and tells the machine the next time. Nothing of
/// it runs between. A message to a teammate the driver it holds doesn't
/// reach goes to the machine, which carries it ([`Address::send`]).
pub struct Addressed(pub std::sync::Arc<dyn Driver>, pub Address);

impl Driver for Addressed {
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        self.0.fire(firing)
    }

    fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        match self.0.reaches(agent) {
            true => self.0.fire_to(agent, firing),
            false => self.1.send(agent, firing),
        }
    }

    fn reaches(&self, agent: &str) -> bool {
        self.0.reaches(agent)
    }

    fn resident(&self) -> bool {
        false
    }

    fn woken(&self, store: &Store) -> anyhow::Result<()> {
        self.1.post(store).map(|_| ())
    }

    fn arrivals(&self, store: &Store) -> anyhow::Result<usize> {
        self.0.arrivals(store)
    }
}

/// Where a machine that runs the agent only while it answers takes when
/// the agent's jobs are next due: a Unix socket of the machine's, each
/// post a bridge's call ([`link_sandbox_binding::call_at`]) proved as the
/// agent (`caller`) with its key from the keyring, as each firing is.
/// The action is `{"next": <seconds since 1970>}`, or `{"next": null}`
/// with none waiting, answered `{}`, or the error envelope
/// (`{"error": "<why>"}`); or, for a teammate not on the machine,
/// `{"firing": {"to": <its agent ID>, "firing": <Firing>}}` ([`Address::send`]),
/// answered the same once the machine has taken it. The tool
/// posts after each call that changes the jobs, and the runner after
/// each wake: one post at a time ([`Store::posting`]), each reading the
/// jobs once it's its turn, so the machine's last word is the jobs' last
/// state.
#[derive(Clone, Debug)]
pub struct Address {
    pub socket: PathBuf,
    pub caller: Caller,
}

/// The most of the machine's answer to a post that's read.
const POSTED_MAX: u64 = 4096;

impl Address {
    /// Tells the machine when the jobs in `store` are next due: that
    /// time, said.
    pub fn post(&self, store: &Store) -> anyhow::Result<Option<u64>> {
        let _turn = store.posting()?;
        let next = store.next()?;
        // Refused, the machine's answer is the error envelope: an error.
        let _: serde_json::Value = link_sandbox_binding::call_at(
            &self.socket,
            &self.caller.binding()?,
            &serde_json::json!({ "next": next }),
            POSTED_MAX,
        )
        .with_context(|| {
            format!(
                "telling {} when the jobs are next due",
                self.socket.display()
            )
        })?;
        Ok(next)
    }
}

impl Address {
    /// Hands `firing` for the teammate whose agent ID is `agent`, on no
    /// way in of this machine's, to the machine, which carries it to
    /// wherever that agent is, as this one's: answered once it's taken,
    /// with no answer, since the teammate's answer comes back as a
    /// message of its own, through the machine, never waited for here.
    pub fn send(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        let _: serde_json::Value = link_sandbox_binding::call_at(
            &self.socket,
            &self.caller.binding()?,
            &serde_json::json!({ "firing": { "to": agent, "firing": firing } }),
            POSTED_MAX,
        )
        .with_context(|| {
            format!(
                "handing {} a message for agent {}",
                self.socket.display(),
                link_format::clean(agent)
            )
        })?;
        Ok(Fired::default())
    }
}

/// The agent's bridge's way in: a Unix socket taking a [`Firing`] as a
/// bridge's call ([`link_sandbox_binding`]), proved as the agent the
/// runner fires for (`caller`), answered with a [`Fired`] once the answer
/// went where it goes. The bridge runs for good, restarting with the
/// machine.
pub struct WayIn {
    pub socket: PathBuf,
    pub caller: Caller,
}

/// Who a runner fires as: its agent, by its ID, proved with its key from
/// the machine's keyring, read at each firing, since the key is made at
/// each start of the agent. Never anyone else: a teammate's bridge takes
/// a message from it only as this agent's.
#[derive(Clone, Debug)]
pub struct Caller {
    pub agent: String,
    pub keyring: link_sandbox_binding::Keyring,
}

impl Caller {
    /// Its agent's binding now: an error while it isn't bound (stopped).
    pub fn binding(&self) -> anyhow::Result<link_sandbox_binding::Binding> {
        self.keyring.bound(&self.agent).with_context(|| {
            format!(
                "agent {} isn't bound on this machine (it isn't running): nothing to prove its \
                 messages with",
                link_format::clean(&self.agent)
            )
        })
    }
}

impl Driver for WayIn {
    /// Waits as long as the bridge takes: the runner gives up on a firing
    /// past its timeout, and its subagent counts until this ends.
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        link_connection_contract::fire(&self.socket, &self.caller.binding()?, firing)
            .with_context(|| format!("at {}", self.socket.display()))
    }
}

/// The agent's bridge's way in, and each teammate's, by its agent's ID:
/// a message to a teammate goes to its bridge, which asks its harness.
pub struct Ways {
    pub own: WayIn,
    pub teammates: std::collections::BTreeMap<String, WayIn>,
}

impl Driver for Ways {
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        self.own.fire(firing)
    }

    fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
        self.teammates
            .get(agent)
            .with_context(|| format!("agent {agent} isn't a teammate any more"))?
            .fire(firing)
    }

    fn reaches(&self, agent: &str) -> bool {
        self.teammates.contains_key(agent)
    }
}

/// Fires each firing by calling a function: tests, and a harness that
/// runs the scheduler in its own process.
pub struct InProcess<F>(pub F);

impl<F> Driver for InProcess<F>
where
    F: Fn(&Firing) -> anyhow::Result<Fired> + Send + Sync,
{
    fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
        (self.0)(firing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;
    use std::sync::{Arc, Mutex};

    /// On the address driver, a wake fires what's due and tells the
    /// machine when the rest are next due ([`Driver::woken`]).
    #[test]
    fn on_the_address_driver_a_wake_posts_the_next_time() {
        let (dir, store) = store("address");
        let keys = dir.join("keys");
        let binding = link_store_contract::testing::bound(&keys, "aaaaaaaaaaaa");
        let socket = dir.join("machine.sock");
        let posts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let kept = posts.clone();
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Ok((_, action)) =
                    link_sandbox_binding::take::<serde_json::Value>(&stream, &binding)
                {
                    kept.lock().unwrap().push(action);
                }
                let _ = std::io::Write::write_all(&mut &stream, b"{}\n");
            }
        });
        let (inner, seen) = recording();
        let address = Address {
            socket,
            caller: Caller {
                agent: "aaaaaaaaaaaa".into(),
                keyring: link_sandbox_binding::Keyring::at(&keys),
            },
        };
        let driver: Arc<dyn Driver> = Arc::new(Addressed(inner, address));
        assert!(!driver.resident());
        store.put(&job("aaaaaaaaaas1", 600)).unwrap();
        store.put(&job("aaaaaaaaaas2", 900)).unwrap();
        assert_eq!(wake(&store, &driver, &CAPS, 600).unwrap().fired.len(), 1);
        assert_eq!(seen.lock().unwrap().len(), 1);
        wake(&store, &driver, &CAPS, 900).unwrap();
        assert_eq!(
            *posts.lock().unwrap(),
            [
                serde_json::json!({"next": 900}),
                serde_json::json!({"next": null})
            ]
        );
    }

    /// On the cron driver, `run` fires what's due and returns, rather
    /// than staying.
    #[test]
    fn on_cron_the_runner_fires_whats_due_and_stops() {
        let (_dir, store) = store("cron-run");
        let (driver, seen) = recording();
        store.put(&job("aaaaaaaaaan1", now() - 5)).unwrap();
        store.put(&job("aaaaaaaaaan2", now() + 3600)).unwrap();
        run(&store, Arc::new(Cron(driver)), &CAPS).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(store.jobs().len(), 1);
    }

    /// A driver for two teammates: the agent's own way in (`own`) and its
    /// teammate's (`mate`), each a function of the firing.
    struct Pair<A, B>(A, B);

    impl<A, B> Driver for Pair<A, B>
    where
        A: Fn(&Firing) -> anyhow::Result<Fired> + Send + Sync,
        B: Fn(&str, &Firing) -> anyhow::Result<Fired> + Send + Sync,
    {
        fn fire(&self, firing: &Firing) -> anyhow::Result<Fired> {
            (self.0)(firing)
        }
        fn fire_to(&self, agent: &str, firing: &Firing) -> anyhow::Result<Fired> {
            (self.1)(agent, firing)
        }
    }

    const SCOUT: &str = "a1b2c3d4e5f6";
    const WRITER: &str = "g7h8j9k0m1n2";

    fn sent(id: &str) -> Job {
        Job {
            kind: Kind::Send,
            thread: teammates_thread(SCOUT),
            answer: AnswerTo::Thread("telegram:42".into()),
            to: Some(Peers {
                from: SCOUT.into(),
                to: WRITER.into(),
            }),
            ..job(id, 5)
        }
    }

    /// A teammate's answer one hop past the most a chain may have isn't
    /// sent back: the message at the cap is delivered, its answer not.
    #[test]
    fn an_answer_past_the_hops_isnt_sent_back() {
        let (_dir, store) = store("send-past-hops");
        let driver: Arc<dyn Driver> = Arc::new(Pair(
            |_: &Firing| Ok(Fired::default()),
            |_: &str, _: &Firing| {
                Ok(Fired {
                    answer: "Drafted.".into(),
                })
            },
        ));
        store
            .put(&Job {
                hops: CAPS.hops,
                ..sent("aaaaaaaaaak2")
            })
            .unwrap();
        assert_eq!(
            fire_at(EVERY_DRIVER[0], &store, &driver, &CAPS, 5)
                .fired
                .len(),
            1
        );
        assert!(store.jobs().is_empty(), "{:?}", store.jobs());
    }

    /// A message the teammate's way in refuses (a cap, its terms) is
    /// final: never tried again, ended failed with the refusal, and its
    /// sender told why in the conversation it sent from, as the
    /// teammate's answer would be. A refusal of that, in turn, is final
    /// too, and tells no one.
    #[test]
    fn a_refused_message_ends_and_its_sender_is_told() {
        use link_sandbox_binding::{Failure, Why};
        let cap = "8 agents' messages in this conversation since a person last spoke in it";
        let refusal = move || -> anyhow::Error {
            anyhow::Error::new(Failure::new(Why::Refused, cap)).context("handing it over")
        };
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("send-refused-{wakes:?}"));
            let (theirs, ours) = (
                Arc::new(Mutex::new(0_usize)),
                Arc::new(Mutex::new(Vec::<Firing>::new())),
            );
            let (t, o) = (theirs.clone(), ours.clone());
            let driver: Arc<dyn Driver> = Arc::new(Pair(
                move |firing: &Firing| {
                    o.lock().unwrap().push(firing.clone());
                    Err(refusal())
                },
                move |_: &str, _: &Firing| {
                    *t.lock().unwrap() += 1;
                    Err(refusal())
                },
            ));
            store.put(&sent("aaaaaaaaaak3")).unwrap();
            assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 5).fired.len(), 1);
            assert_eq!(*theirs.lock().unwrap(), 1);
            let ended = store.ended("aaaaaaaaaak3").expect("kept ended");
            match ended.outcome.map(|o| o.how) {
                Some(How::Failed { why }) => assert!(why.contains(cap), "{why}"),
                other => panic!("{other:?}"),
            }
            // Not tried again: what waits is the sender's notice alone.
            let told = store.jobs();
            assert_eq!(told.len(), 1, "{told:?}");
            assert_eq!(told[0].kind, Kind::Reply);
            assert_eq!(told[0].thread, "telegram:42");
            assert!(told[0].text.contains(cap), "{}", told[0].text);
            // The notice refused in turn ends there.
            let due = told[0].due;
            assert_eq!(fire_at(wakes, &store, &driver, &CAPS, due).fired.len(), 1);
            assert_eq!(ours.lock().unwrap().len(), 1);
            assert!(store.jobs().is_empty(), "{:?}", store.jobs());
            assert_eq!(*theirs.lock().unwrap(), 1);
        }
    }

    /// A message to a teammate goes to its bridge, from this agent, its
    /// answer not sent anywhere there; the answer comes back as a job of
    /// its own, to the conversation it was sent from, from the teammate,
    /// one hop further; a failure to deliver it never sends the message
    /// again.
    #[test]
    fn a_teammates_answer_comes_back_from_it() {
        for wakes in EVERY_DRIVER {
            let (_dir, store) = store(&format!("send-{wakes:?}"));
            let (theirs, ours) = (
                Arc::new(Mutex::new(Vec::<(String, Firing)>::new())),
                Arc::new(Mutex::new(Vec::<Firing>::new())),
            );
            let (t, o) = (theirs.clone(), ours.clone());
            let fail_ours = Arc::new(Mutex::new(true));
            let failing = fail_ours.clone();
            let driver: Arc<dyn Driver> = Arc::new(Pair(
                move |firing: &Firing| {
                    o.lock().unwrap().push(firing.clone());
                    match *failing.lock().unwrap() {
                        true => bail!("the bridge isn't up"),
                        false => Ok(Fired::default()),
                    }
                },
                move |agent: &str, firing: &Firing| {
                    t.lock().unwrap().push((agent.to_string(), firing.clone()));
                    Ok(Fired {
                        answer: "Drafted.".into(),
                    })
                },
            ));
            store.put(&sent("aaaaaaaaaak1")).unwrap();
            assert_eq!(fire_at(wakes, &store, &driver, &CAPS, 5).fired.len(), 1);
            let theirs = theirs.lock().unwrap().clone();
            assert_eq!(theirs.len(), 1);
            let (agent, firing) = &theirs[0];
            assert_eq!(agent, WRITER);
            assert_eq!(firing.from, format!("agent:{SCOUT}"));
            assert_eq!(firing.thread, format!("agent-{SCOUT}"));
            assert_eq!(firing.answer, AnswerTo::Nowhere);
            // The message is gone; its answer waits as a job of its own.
            let replies = store.jobs();
            assert_eq!(replies.len(), 1);
            let reply = &replies[0];
            assert_eq!(reply.kind, Kind::Reply);
            assert_eq!(reply.thread, "telegram:42");
            assert_eq!(reply.hops, 2);
            let firing = reply.firing();
            assert_eq!(firing.from, format!("agent:{WRITER}"));
            assert_eq!(firing.text, "Drafted.");
            assert_eq!(firing.answer, AnswerTo::Chat);
            // Delivering it fails: it's tried again, the teammate not asked
            // again.
            fire_at(wakes, &store, &driver, &CAPS, reply.due);
            let kept = store.get(&reply.id).expect("kept after a failure");
            *fail_ours.lock().unwrap() = false;
            fire_at(wakes, &store, &driver, &CAPS, kept.due);
            assert!(store.jobs().is_empty());
            assert_eq!(ours.lock().unwrap().len(), 2);
            assert_eq!(theirs.len(), 1);
            // A driver without teammates reaches none.
            let (alone, _) = recording();
            store.put(&sent("aaaaaaaaaak2")).unwrap();
            fire_at(wakes, &store, &alone, &CAPS, 5);
            assert_eq!(store.get("aaaaaaaaaak2").unwrap().tries, 1);
        }
    }

    /// Two teammates that answer each other by sending again stop at the
    /// cap on hops: each `send` past it says so, and nothing more is sent.
    #[test]
    fn two_teammates_looping_stop_at_the_cap() {
        let caps = Caps { hops: 6, ..CAPS };
        let teams = |me: &str, mate: (&str, &str)| Teams {
            agent: Some(me.into()),
            teams: [("research".to_string(), "t4m9q2x7k1ab".to_string())].into(),
            teammates: vec![Teammate {
                team: "research".into(),
                name: mate.0.into(),
                agent: mate.1.into(),
            }],
        };
        let (_a_dir, a_store) = store("loop-a");
        let (_b_dir, b_store) = store("loop-b");
        let a = Arc::new(mcp::Tool {
            store: a_store.clone(),
            caps,
            teams: teams(SCOUT, ("writer", WRITER)),
            address: None,
            floor: link_connection_contract::access::Standing::Owner,
            connections: Vec::new(),
        });
        let b = Arc::new(mcp::Tool {
            store: b_store.clone(),
            caps,
            teams: teams(WRITER, ("scout", SCOUT)),
            address: None,
            floor: link_connection_contract::access::Standing::Owner,
            connections: Vec::new(),
        });
        let said = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        // Each agent's turn: send to the other, from the conversation the
        // message came to, as many hops on as it was.
        let turn = |tool: Arc<mcp::Tool>,
                    to: &'static str,
                    said: Arc<Mutex<Vec<serde_json::Value>>>| {
            move |firing: &Firing| -> anyhow::Result<Fired> {
                let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "send", "arguments": {"to": to, "message": "Again?"},
                    "_meta": {"link.local/standing": firing.standing, "link.local/thread": firing.thread, "link.local/hops": firing.hops}}});
                let answer = mcp::answer(&tool, &call.to_string()).unwrap()["result"].clone();
                said.lock().unwrap().push(answer);
                Ok(Fired {
                    answer: "Sent.".into(),
                })
            }
        };
        let a_turn = turn(a.clone(), "writer", said.clone());
        let b_turn = turn(b.clone(), "scout", said.clone());
        let (a_own, b_own) = (Arc::new(a_turn), Arc::new(b_turn));
        let (a_mate, b_mate) = (b_own.clone(), a_own.clone());
        let a_driver: Arc<dyn Driver> = Arc::new(Pair(
            {
                let own = a_own.clone();
                move |f: &Firing| own(f)
            },
            move |_: &str, f: &Firing| a_mate(f),
        ));
        let b_driver: Arc<dyn Driver> = Arc::new(Pair(
            {
                let own = b_own.clone();
                move |f: &Firing| own(f)
            },
            move |_: &str, f: &Firing| b_mate(f),
        ));
        // The owner asks the scout to start it.
        a_own(&Firing {
            thread: "telegram:42".into(),
            hops: 0,
            ..Default::default()
        })
        .unwrap();
        let mut rounds = 0;
        while !(a_store.jobs().is_empty() && b_store.jobs().is_empty()) {
            rounds += 1;
            assert!(rounds < 50, "still going after 50 rounds");
            for job in a_store.jobs() {
                look(&a_store, &a_driver, &caps, job.due, true);
            }
            for job in b_store.jobs() {
                look(&b_store, &b_driver, &caps, job.due, true);
            }
        }
        // Each turn sends again and answers too, so the chains branch:
        // every one ends at the cap, saying so, and nothing is left.
        let said = said.lock().unwrap();
        let refused: Vec<&serde_json::Value> =
            said.iter().filter(|s| s["isError"] == true).collect();
        assert!(!refused.is_empty());
        for each in &refused {
            assert!(each.to_string().contains("past the cap of 6"), "{each}");
        }
        assert!(said.len() < 64, "{}", said.len());
    }

    /// The same job fires through the bridge's way in (a Unix socket, a
    /// bridge's call proved as the runner's agent) as in process.
    #[test]
    fn the_way_in_driver_fires_the_same() {
        for wakes in EVERY_DRIVER {
            let (dir, store) = store(&format!("way-in-{wakes:?}"));
            let keys = dir.join("keys");
            link_store_contract::testing::bound(&keys, "aaaaaaaaaaaa");
            let caller = Caller {
                agent: "aaaaaaaaaaaa".into(),
                keyring: link_sandbox_binding::Keyring::at(&keys),
            };
            let socket = dir.join("way-in.sock");
            let seen = Arc::new(Mutex::new(Vec::<Firing>::new()));
            let kept = seen.clone();
            link_agent_contract::testing::stand_in_bridge(
                &socket,
                &keys,
                move |who, firing: Firing| {
                    assert_eq!(who.agent, "aaaaaaaaaaaa");
                    kept.lock().unwrap().push(firing);
                    serde_json::json!({"answer": "Done."})
                },
            );
            let way_in = |socket: PathBuf| WayIn {
                socket,
                caller: caller.clone(),
            };
            let driver: Arc<dyn Driver> = Arc::new(way_in(socket.clone()));
            store.put(&job("aaaaaaaaaad1", 5)).unwrap();
            assert_eq!(
                fire_at(wakes, &store, &driver, &CAPS, 5).fired,
                ["aaaaaaaaaad1"]
            );
            let seen = seen.lock().unwrap().clone();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].from, "schedule:aaaaaaaaaad1");
            assert_eq!(seen[0].thread, "telegram:42");
            // A bridge that says it failed is an error.
            link_agent_contract::testing::stand_in_bridge(
                &dir.join("failing.sock"),
                &keys,
                |_, _: Firing| serde_json::json!({"error": "no harness"}),
            );
            let failed = way_in(dir.join("failing.sock")).fire(&seen[0]);
            assert!(format!("{:#}", failed.unwrap_err()).contains("no harness"));
            // Its agent not bound (stopped): nothing to prove with.
            let unbound = WayIn {
                socket: socket.clone(),
                caller: Caller {
                    agent: "bbbbbbbbbbbb".into(),
                    ..caller.clone()
                },
            };
            assert!(format!("{:#}", unbound.fire(&seen[0]).unwrap_err()).contains("isn't bound"));
            // A bridge not listening yet: an error, and the job is kept.
            let absent: Arc<dyn Driver> = Arc::new(way_in(dir.join("absent.sock")));
            store.put(&job("aaaaaaaaaad2", 5)).unwrap();
            fire_at(wakes, &store, &absent, &CAPS, 5);
            assert_eq!(store.get("aaaaaaaaaad2").unwrap().tries, 1);
        }
    }
}
