//! What a start undoes unless it gets as far as starting: its agent's
//! service left stopped, and its bridge secret retired.

use link_agent::service;

/// An agent's service stopped ([`service::stop_before_start`]) that no
/// `up` starts again while it's armed: dropped, the stop is finished as
/// [`service::stop`] does, with systemd reloaded and the calendar's
/// wake taken away. [`up_after`](super::up_after) disarms it as it
/// starts the service, not before: one failing earlier leaves the agent
/// stopped.
pub(crate) struct StoppedUnlessStarted {
    pub(crate) agent: String,
    pub(crate) armed: bool,
}

/// Retire this start's bridge secret unless a running background service
/// took ownership. Foreground and failed starts keep no usable binding.
pub(crate) struct BindingUntilStarted {
    pub(super) agent: String,
    pub(super) armed: bool,
}

impl Drop for BindingUntilStarted {
    fn drop(&mut self) {
        if self.armed {
            link_sandbox_binding::Keyring::here()
                .unbind(&self.agent, &link_secret_contract::keeper::RouteRevoker);
        }
    }
}

impl Drop for StoppedUnlessStarted {
    fn drop(&mut self) {
        if self.armed {
            link_service::reload();
            service::remove_calendar(&self.agent);
        }
    }
}
