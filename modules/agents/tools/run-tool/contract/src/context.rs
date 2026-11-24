//! A tool definition names routes and a live binding lookup, never a key.
use anyhow::{Context as _, ensure};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ENV: &str = "LNK_RUN_CONTEXT";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub agent: String,
    pub owner: String,
    pub keys: PathBuf,
    pub routes: Vec<link_sandbox_route::Route>,
}
impl Context {
    pub fn check(&self) -> anyhow::Result<()> {
        ensure!(
            !self.agent.is_empty() && !self.owner.is_empty() && self.keys.is_absolute(),
            "run context needs an agent and absolute binding lookup"
        );
        link_sandbox_binding::check_agent(&self.agent)?;
        link_sandbox_binding::check_owner(&self.owner)?;
        ensure!(!self.routes.is_empty(), "run context has no named routes");
        for route in &self.routes {
            route.check()?;
            route.names_only()?;
            for (name, value) in &route.env {
                link_sandbox_client::check_given_variable(name)?;
                ensure!(
                    !value.contains('\0'),
                    "named route variable {name} has a NUL"
                );
            }
            ensure!(
                !route.env.contains_key(ENV),
                "a route cannot expose the run context variable"
            );
        }
        Ok(())
    }
    /// A restart replaces the binding. Resolve it for each actual call,
    /// outside the sandbox; the bridge checks it and the current grant.
    pub fn resolve(&self) -> anyhow::Result<link_sandbox_client::RunWith> {
        self.check()?;
        let caller = link_sandbox_binding::Keyring::at(&self.keys)
            .bound(&self.agent)
            .context("the run context's agent is not bound")?;
        ensure!(
            caller.identity.agent == self.agent && caller.identity.owner == self.owner,
            "run context resolves a foreign agent binding"
        );
        let named = link_sandbox_client::RunRoutes {
            routes: self.routes.iter().map(Duplicate::duplicate).collect(),
            caller,
        };
        named.check()?;
        Ok(link_sandbox_client::RunWith {
            named: Some(named),
            ..Default::default()
        })
    }
    pub fn check_variables(&self, variables: &[(String, String)]) -> anyhow::Result<()> {
        ensure!(
            !variables.iter().any(|(name, _)| name == ENV
                || self.routes.iter().any(|route| route.env.contains_key(name))),
            "a task variable replaces its sealed context or a sealed variable"
        );
        Ok(())
    }
}
