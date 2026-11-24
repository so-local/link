//! `lnk agent stop`: an agent's harness and everything started for it.

use link_agent::{service, settings::Settings};
use link_format::{Done, Name, Out, Report, cmd, sentence};

/// Stop completely, then use the same default start path as `agent start`.
pub(crate) async fn restart(out: &mut Out, agent: &mut Settings) -> Result<Done, Report> {
    stop(agent)?;
    crate::creating::start(out, agent, &link_agent::cli::Starting::default()).await
}

/// `lnk agent stop`: its service stopped, one in a terminal told to stop
/// as Ctrl-C would, and what was given its proxy, its bridge key and its
/// background commands taken back.
pub(crate) fn stop(agent: &Settings) -> Result<Done, Report> {
    service::stop(&agent.name)?;
    // One in a terminal too: told to stop as Ctrl-C would.
    service::stop_foreground(&agent.name)?;
    // What its proxy was given goes with it.
    crate::confine::forget_routes(agent);
    // Its key proves nothing from now on, on any bridge.
    link_sandbox_binding::Keyring::here()
        .unbind(&agent.key(), &link_secret_contract::keeper::RouteRevoker);
    // Its background commands end with it, wherever its service
    // didn't take them.
    link_agent::background::stop_background(agent, false);
    // Its environment's services, which started with it.
    crate::environment::stop(&agent.name)?;
    Ok(
        Done::success(sentence!("{} is stopped.", Name::new("agent", &agent.name)))
            .next(cmd!("lnk agent start"), "start it again"),
    )
}
