//! OpenClaw's warning-only ACP respawn retains an idle Node parent and
//! starts its real client detached. Supply the same flag at first launch;
//! keep its other startup checks, including automatic TLS configuration.

use serde::Deserialize;
use std::{ffi::OsString, process::Command, sync::OnceLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Startup {
    node_option: String,
}

fn startup() -> &'static Startup {
    static STARTUP: OnceLock<Startup> = OnceLock::new();
    STARTUP
        .get_or_init(|| toml::from_str(include_str!("acp_startup.toml")).expect("ACP Node option"))
}

pub(crate) fn prepare(command: &mut Command) {
    let options = match command.get_envs().find(|(name, _)| *name == "NODE_OPTIONS") {
        Some((_, value)) => value.map(OsString::from),
        None => std::env::var_os("NODE_OPTIONS"),
    };
    command.env("NODE_OPTIONS", options_with(options));
}

fn options_with(options: Option<OsString>) -> OsString {
    let mut options = options.unwrap_or_default();
    if !options.is_empty() {
        options.push(" ");
    }
    options.push(&startup().node_option);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_node_launch_has_the_native_warning_option_and_preserves_owner_options() {
        let home = link_test_kit::TempDir::new("openclaw-acp-startup");
        let mut command = link_test_kit::env::command("node", &home);
        command
            .env("NODE_OPTIONS", "--max-old-space-size=1024 --use-system-ca")
            .env("NODE_EXTRA_CA_CERTS", "/owner/ca.pem");
        prepare(&mut command);
        let env: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env[std::ffi::OsStr::new("NODE_OPTIONS")].unwrap(),
            "--max-old-space-size=1024 --use-system-ca --disable-warning=ExperimentalWarning"
        );
        assert_eq!(
            env[std::ffi::OsStr::new("NODE_EXTRA_CA_CERTS")].unwrap(),
            "/owner/ca.pem"
        );
        assert!(!env.contains_key(std::ffi::OsStr::new("OPENCLAW_NO_RESPAWN")));
        assert_eq!(options_with(None), startup().node_option.as_str());
    }
}
