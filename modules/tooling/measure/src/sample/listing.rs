//! The agents running here, as the modules that answer
//! [`measure.agents`](link_measure_contract::client::asked::AGENTS) say
//! ([`Sampler::agents`]).

use super::*;

pub use link_measure_contract::client::asked::Agent;
use link_measure_contract::client::asked::{AGENTS, INVENTORY, TRAFFIC};

/// The agents here, each module that answers asked; none without one,
/// or from one that doesn't answer.
impl Sampler {
    /// Refresh agent and traffic providers together. The watcher already
    /// refreshes this discovery every minute and when an agent exits.
    pub fn agents(&mut self) -> Vec<Agent> {
        let listed = link_core::modules::list();
        self.traffic = TRAFFIC.answerers_in(&listed);
        self.sources.clear();
        let mut agents = Vec::new();
        let mut answered = BTreeSet::new();
        for provider in INVENTORY.answerers_in(&listed) {
            if let Ok(inventory) = provider.reply(&INVENTORY, &(), std::process::Stdio::null()) {
                answered.insert(provider.module);
                agents.extend(inventory.agents);
                self.sources.extend(inventory.traffic);
            }
        }
        agents.extend(
            AGENTS
                .answerers_in(&listed)
                .into_iter()
                .filter(|a| !answered.contains(&a.module))
                .filter_map(|answerer| {
                    answerer
                        .reply(&AGENTS, &(), std::process::Stdio::null())
                        .ok()
                })
                .flatten(),
        );
        agents
    }
}

/// Whether an agent of `agents` that ran has stopped or started again
/// since they were listed: its main process is gone.
pub fn changed(agents: &[Agent]) -> bool {
    agents
        .iter()
        .any(|a| a.running && a.pids.first().is_none_or(|p| !os::alive(*p)))
}
