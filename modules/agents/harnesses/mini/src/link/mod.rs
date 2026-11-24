//! Link integration: adapter commands, ACP, sessions and MCP transport.

use anyhow::Context;
use link_harness_contract::harness::{self, Launch, Model, Settings};
use link_secretive::Duplicate;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};

pub(crate) mod acp;
mod agent;
mod model;
mod state;
mod threads;
mod tools;

#[derive(serde::Deserialize)]
pub(crate) struct Defaults {
    port: u16,
    pub answer: u32,
    pub window: u32,
    instructions: String,
}

pub(crate) fn defaults() -> &'static Defaults {
    static DEFAULTS: std::sync::OnceLock<Defaults> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| toml::from_str(include_str!("defaults.toml")).expect("Mini defaults"))
}

/// Its port: this, unless Link gives the agent another (`LNK_PORT`).
fn port() -> u16 {
    harness::base_port(defaults().port)
}

/// Where `install` puts this program, in its home: the sandbox runs
/// programs only from there.
fn installed(fixture: bool) -> PathBuf {
    link_files::home_dir().join("bin").join(if fixture {
        "lnk-harness-fixture"
    } else {
        "lnk-mini-harness"
    })
}

fn config_path() -> PathBuf {
    link_files::home_dir().join("config.json")
}

pub fn run(out: &mut link_format::Out) -> Result<link_format::Done, link_format::Report> {
    run_for(out, false)
}

pub(crate) fn run_for(
    out: &mut link_format::Out,
    fixture: bool,
) -> Result<link_format::Done, link_format::Report> {
    run_command(out, fixture, crate::cli::parse())
}

pub(crate) fn run_command(
    out: &mut link_format::Out,
    fixture: bool,
    command: crate::cli::Command,
) -> Result<link_format::Done, link_format::Report> {
    dispatch(out, fixture, command)
        .map(|()| link_format::Done::shown())
        .map_err(Into::into)
}

fn dispatch(
    out: &mut link_format::Out,
    fixture: bool,
    command: crate::cli::Command,
) -> anyhow::Result<()> {
    use crate::cli::Command;
    let mut print = |value: Value| out.data(format!("{value}\n").as_bytes());
    match command {
        Command::MiniHarnessContract(_) => print(serde_json::to_value(harness::Contract {
            proof: true,
            agent: true,
            from: true,
            tools: true,
            parts: fixture,
            ports: Some(1),
            state: state::kinds(),
            features: Some(
                harness::features::Feature::ALL
                    .into_iter()
                    .filter(|f| {
                        (fixture
                            && matches!(
                                f,
                                harness::features::Feature::Images
                                    | harness::features::Feature::Attachments
                            ))
                            || matches!(
                                f,
                                harness::features::Feature::Groups
                                    | harness::features::Feature::Streaming
                                    | harness::features::Feature::Subagents
                                    | harness::features::Feature::Schedules
                                    | harness::features::Feature::Proactive
                            )
                    })
                    .map(|f| (f, harness::features::Honor::Spec))
                    .collect(),
            ),
            ..harness::Contract::new(true)
        })?),
        Command::MiniHarnessInstall(_) => install(fixture)?,
        Command::MiniHarnessConfigure(_) => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let settings: Settings =
                serde_json::from_str(&input).context("reading settings on stdin")?;
            let config = Config {
                model: settings.model.duplicate(),
                endpoint_key: settings.endpoint_key.duplicate(),
                model_facts: settings.model_facts.clone(),
                window: window(&settings),
                instructions: (!settings.files.as_os_str().is_empty())
                    .then(|| settings.files.join(&defaults().instructions)),
            };
            link_files::write_private(&config_path(), &link_files::saved::print_json(&config)?)?;
            link_harness_hub::Tools::of(&settings).keep(&tools_path())?;
        }
        Command::MiniHarnessCommand(_) => print(serde_json::to_value(Launch {
            argv: vec![installed(fixture).display().to_string(), "serve".into()],
            ports: vec![port()],
            serve: Some(vec![port()]),
            ..Launch::default()
        })?),
        Command::MiniHarnessHealth(_) => {
            if !harness::http_ok(port(), "/health") {
                std::process::exit(1);
            }
        }
        Command::MiniHarnessEndpoint(_) => print(json!(format!(
            "http://127.0.0.1:{}{}",
            port(),
            link_connection_contract::acp::ACP_PATH
        ))),
        Command::MiniHarnessNeeds(_) => print(serde_json::to_value(harness::Needs::default())?),
        Command::MiniHarnessConnections(_) | Command::MiniHarnessTraces(_) => print(json!([])),
        Command::StateShow(_) => print(serde_json::to_value(harness::State {
            include: vec!["~/threads".into()],
            exclude: Vec::new(),
        })?),
        Command::MiniHarnessServe(_) => serve(fixture)?,
        Command::MiniHarnessAcp(_) => {
            let config = config()?;
            let mut options = acp::Options::new(config.model);
            options.model_facts = config.model_facts;
            options.window = config.window;
            options.instructions = config.instructions;
            options.media_descriptions = fixture;
            acp::serve(options)?;
        }
    }
    Ok(())
}

/// The window a turn fills, as the agent's spec says, the window Link's
/// sandbox grants it: `[window] max`, held to the model's own window when
/// it's Link's (a key no team or you set), taken as set when it's yours;
/// none when the spec says nothing, so the model's own is used.
fn window(settings: &Settings) -> Option<u32> {
    let max = settings.spec.as_ref()?.window.as_ref()?.max?;
    let own = settings
        .model_facts
        .as_ref()
        .and_then(|known| known.facts.window);
    match own {
        Some(own) if settings.defaults.iter().any(|key| key == "window.max") => Some(max.min(own)),
        _ => Some(max),
    }
}

/// Copies this program into its home, unless it's there as it is.
fn install(fixture: bool) -> anyhow::Result<()> {
    let me = std::env::current_exe()?;
    let to = installed(fixture);
    if std::fs::read(&to).ok() != Some(std::fs::read(&me)?) {
        std::fs::create_dir_all(to.parent().context("no folder")?)?;
        let aside = to.with_extension("new");
        std::fs::copy(&me, &aside)?;
        std::fs::rename(&aside, &to)?;
    }
    Ok(())
}

/// What `configure` wrote, read at each request.
#[derive(serde::Serialize, serde::Deserialize)]
struct Config {
    model: Model,
    model_facts: Option<link_model_contract::Known>,
    /// The window a turn fills, as the agent's spec says ([`window`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<u32>,
    instructions: Option<PathBuf>,
    #[serde(default, with = "link_secretive::bare")]
    endpoint_key: Option<link_secretive::Secret>,
}

link_files::specification!(Config, include_str!("config.schema.toml"));

/// Link's tools, as `configure` kept them for `serve`.
fn tools_path() -> PathBuf {
    link_files::home_dir().join("tools.json")
}

fn config() -> anyhow::Result<Config> {
    let bytes = std::fs::read(config_path()).context("reading config.json")?;
    link_files::saved::parse_json(&bytes).context("reading Mini configuration")
}

/// Its endpoint: the hub, with `lnk-mini-harness acp` behind it.
fn serve(fixture: bool) -> anyhow::Result<()> {
    let key = config()?.endpoint_key;
    // Its copy in its home, where the sandbox runs programs from; else
    // this program, wherever it's run from.
    let me = match installed(fixture) {
        installed if installed.is_file() => installed,
        _ => std::env::current_exe()?,
    };
    link_harness_hub::Hub::new(link_harness_hub::Config {
        start: link_harness_hub::spawning(move || {
            let mut acp = link_secretive::process::command(&me);
            acp.arg("acp");
            acp
        }),
        cwd: link_files::home_dir().display().to_string(),
        key,
        agent: Some(own_agent()),
        sessions: link_files::home_dir().join("sessions.json"),
        state: None,
        tools: link_harness_hub::Tools::kept(&tools_path())?,
    })
    .serve(port())
}

/// Its agent (`LNK_AGENT`), the one it answers for.
fn own_agent() -> String {
    std::env::var("LNK_AGENT").unwrap_or_else(|_| "main".into())
}

#[cfg(test)]
mod window_tests {
    use super::*;

    fn settings(spec: &str, defaults: &[&str]) -> Settings {
        Settings {
            spec: Some(
                link_harness_contract::agent_spec::parse(&format!("schema = 1\n{spec}")).unwrap(),
            ),
            defaults: defaults.iter().map(|k| k.to_string()).collect(),
            model_facts: link_model_contract::known("anthropic/claude-haiku-5-5").unwrap(),
            ..Settings::default()
        }
    }

    #[test]
    fn a_turn_asks_for_the_window_the_spec_grants() {
        // Link's 128000 is held to the model's own; one you set is taken.
        let own = settings("", &[])
            .model_facts
            .and_then(|k| k.facts.window)
            .unwrap();
        assert!(own > 128_000, "{own}");
        assert_eq!(
            window(&settings("[window]\nmax = 128000\n", &["window.max"])),
            Some(128_000)
        );
        assert_eq!(
            window(&settings("[window]\nmax = 64000\n", &[])),
            Some(64_000)
        );
        let past = own + 1;
        assert_eq!(
            window(&settings(
                &format!("[window]\nmax = {past}\n"),
                &["window.max"]
            )),
            Some(own)
        );
        assert_eq!(window(&settings("", &[])), None);
    }
}
