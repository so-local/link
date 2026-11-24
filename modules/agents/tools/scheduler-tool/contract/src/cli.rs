//! `lnk-scheduler-tool`'s runtime arguments, the flags `serve`, `run` and
//! `cron install` share ([`Flags`]), and the command line that gives them
//! (`argv`), generated from the tool's declarations (`actions.toml` and
//! `types.toml`). Link builds each line, and the tool's own parser,
//! generated from the same files, reads it.

include!(concat!(env!("OUT_DIR"), "/args.rs"));

impl Flags {
    /// Each flag `other` sets laid over these: a value `other` gives wins,
    /// and its lists are added to these.
    pub fn with(mut self, other: Flags) -> Self {
        let Flags {
            state,
            definitions,
            floor,
            way_in,
            r#as,
            connection,
            keyring,
            per_day,
            subagents,
            hops,
            interval,
            late,
            time_zone,
            timeout,
            agent_id,
            team_id,
            teammate,
            way_in_of,
            team_relay,
            team_owner,
            driver,
            socket,
            store_bridge,
        } = other;
        self.state = state.or(self.state);
        self.definitions = definitions.or(self.definitions);
        self.floor = floor.or(self.floor);
        self.way_in = way_in.or(self.way_in);
        self.r#as = r#as.or(self.r#as);
        self.connection.extend(connection);
        self.keyring = keyring.or(self.keyring);
        self.per_day = per_day.or(self.per_day);
        self.subagents = subagents.or(self.subagents);
        self.hops = hops.or(self.hops);
        self.interval = interval.or(self.interval);
        self.late = late.or(self.late);
        self.time_zone = time_zone.or(self.time_zone);
        self.timeout = timeout.or(self.timeout);
        self.agent_id = agent_id.or(self.agent_id);
        self.team_id.extend(team_id);
        self.teammate.extend(teammate);
        self.way_in_of.extend(way_in_of);
        self.team_relay = team_relay.or(self.team_relay);
        self.team_owner.extend(team_owner);
        self.driver = driver.or(self.driver);
        self.socket = socket.or(self.socket);
        self.store_bridge = store_bridge.or(self.store_bridge);
        self
    }

    /// Its `serve` line, as `program` runs it.
    pub fn serve(&self, program: &std::path::Path) -> Vec<String> {
        let serve = SchedulerToolServeArgs {
            flags: self.clone(),
        };
        std::iter::once(program.display().to_string())
            .chain(serve.argv())
            .collect()
    }

    /// Its `run` line, one agent's runner, as `program` runs it.
    pub fn run(&self, program: &std::path::Path) -> Vec<String> {
        let run = SchedulerToolRunArgs {
            agents: None,
            flags: self.clone(),
        };
        std::iter::once(program.display().to_string())
            .chain(run.argv())
            .collect()
    }

    /// The flags it sets, as the command line spells them.
    pub fn argv(&self) -> Vec<String> {
        let serve = SchedulerToolServeArgs {
            flags: self.clone(),
        };
        // `serve` alone takes the group and nothing else: its words after
        // the command are these flags.
        serve.argv().into_iter().skip(1).collect()
    }
}

/// An agent's runner, as its bridge runs it: its program and its flags,
/// each line written from them by the generated argv, never read back.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Runner {
    pub program: std::path::PathBuf,
    pub flags: Flags,
}

impl Runner {
    /// Its line, `<program> run <flags>`.
    pub fn argv(&self) -> Vec<String> {
        self.flags.run(&self.program)
    }
}
