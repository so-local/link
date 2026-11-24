//! Workarounds for bugs in the OpenClaw release this adapter pins
//! ([`crate::VERSION`]), kept apart from the code that's meant to stay.
//! Each names the bug and the upstream report, and goes once the pin
//! moves to a release that fixes it.

pub(crate) mod acp_session_ids;
pub(crate) mod acp_startup;
