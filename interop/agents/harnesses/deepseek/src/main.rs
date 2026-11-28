//! `lnk-deepseek`: the DeepSeek Harness adapter. `lnk-agent` runs it;
//! see `link_harness_contract::harness` for the commands.
//!
//! DeepSeek Harness (`dsh`) is installed from npm at a pinned version
//! ([`VERSION`]), on a pinned Node ([`NODE`]) of its own, all in
//! `~/.deepseek`. It speaks no connection, so this program is its endpoint
//! too (`serve`): ACP on its port's `/acp`, each thread a session of one
//! `dsh --profile acp` behind it, through the hub (`link_harness_hub`).
//! `configure` writes `dsh`'s
//! settings as a patch layer of its own, passed with `--patch`, and the
//! model's key in `dsh`'s own store of keys.

mod features;
mod identity;
mod state;

use link_deepseek::cli::Command;
use std::{io::Read, path::PathBuf};

use anyhow::bail;
use link_format::{
    Done, Exit, Foreign, Note, Out, Path, Prose, Report, Sentence, WithinExt, cmd, phrase, prose,
    sentence,
};
use link_harness_contract::harness::traces::{self, Kind, Trace};
use link_harness_contract::harness::{self, Launch, Settings};
use serde_json::{Value, json};

// Installer pins are generated from this owner's authoritative records.
include!(concat!(env!("OUT_DIR"), "/installer-pins.rs"));

/// Its endpoint's port: this, unless Link gives the agent another base
/// port (`LNK_PORT`).
fn port() -> u16 {
    harness::base_port(18820)
}

/// The variable in `dsh`'s `.env` holding the model's key, which its
/// provider reads by name.
const KEY_ENV: &str = "LNK_MODEL_CREDENTIAL";

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[
        link_harness_contract::harness::continuation::saved,
        link_harness_hub::SAVED[0],
        link_harness_hub::SAVED[1],
    ]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// A failure with its own words as the why: `headline` says what didn't
/// happen, `err` what stopped it, and the retry is what was typed.
fn failed(out: &Out, headline: Sentence, err: anyhow::Error) -> Report {
    Report::error(headline)
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(out.typed(), phrase!("try again"))
        .cause(err)
}

/// An answer for the program that runs it (`lnk agent`): one line of JSON.
fn answer(out: &mut Out, json: serde_json::Result<String>) -> Result<(), Report> {
    let text = json.within("writing its answer")?;
    out.data(format!("{text}\n").as_bytes());
    Ok(())
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    link_files::layout::check_exec_environment().within("checking the program directory")?;
    match link_deepseek::cli::parse() {
        Command::Uninstall(given) => {
            let args = &given;
            uninstall(args)?;
            return Ok(Done::shown());
        }

        // It reaches a hosted model at the proxy's address
        // (`Model.base_url`), so the proxy can hold the key; its endpoint
        // proves it's the harness before Link's connections send it its key.
        Command::DeepseekContract(_) => {
            // The hub serves it over ACP, reads which agent a message is
            // for, and writes its sender into its text.
            let contract = harness::Contract {
                proof: true,
                agent: true,
                from: true,
                offline_state: true,
                state: state::kinds(),
                // Who the agent is, written into its `AGENTS.md` and read
                // back out, and the spec's skills a folder it scans.
                identity: true,
                // It takes the spec's [instructions] it carries; its
                // features say which.
                instructions: true,
                features: Some(features::honors()),
                ..harness::Contract::new(true)
            };
            answer(out, serde_json::to_string(&contract))?;
        }
        Command::DeepseekInstall(_) => return install(out),
        Command::DeepseekConfigure(_) => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .within("reading settings on stdin")?;
            let settings = serde_json::from_str(&input).within("reading settings on stdin")?;
            configure(out, &settings)?;
        }
        Command::DeepseekCommand(_) => answer(out, serde_json::to_string(&launch()?))?,
        Command::DeepseekHealth(_) => {
            if !harness::http_ok(port(), "/health") {
                return Ok(Done::passed(1));
            }
        }
        Command::DeepseekEndpoint(_) => {
            let url = format!(
                "http://127.0.0.1:{}{}",
                port(),
                link_connection_contract::acp::ACP_PATH
            );
            answer(out, serde_json::to_string(&url))?;
        }
        // It speaks no connection itself: Link carries them.
        Command::DeepseekConnections(_) => {
            answer(out, serde_json::to_string(&Vec::<Value>::new()))?
        }
        // Its state holds no version: nothing to refuse.
        Command::DeepseekWritten(_) => {
            answer(out, serde_json::to_string(&traces::Written::default()))?
        }
        Command::DeepseekTraces(_) => answer(out, serde_json::to_string(&TRACES))?,
        Command::StateExport(_) => {
            let state = link_harness_hub::Store::export(
                &state::DeepSeek {
                    home: link_files::home_dir(),
                },
                &link_harness_contract::asp::Kind::ALL,
            )?;
            let state = link_harness_hub::export_personal(
                &link_files::home_dir(),
                "deepseek",
                sessions_file(),
                state,
            )?;
            answer(out, serde_json::to_string(&state))?;
        }
        Command::StateShow(_) => answer(out, serde_json::to_string(&state()))?,
        Command::DeepseekInternetHosts(_) => answer(out, serde_json::to_string(&HOSTS))?,
        // Only any program: it runs the Node it installed in its home. It
        // runs in Link's sandbox with its own turned off, its model
        // through Link's proxy.
        Command::DeepseekNeeds(_) => {
            let needs = harness::Needs {
                needs: vec![harness::Need::runs(
                    &programs_root(),
                    "it runs the Node it installed in its home, and the commands its tools run",
                )],
                // Its install unpacks Node, then runs its npm.
                install_runs_downloads: true,
                ..Default::default()
            };
            answer(out, serde_json::to_string(&needs))?;
        }
        Command::DeepseekServe(_) => serve()?,
    }
    Ok(Done::shown())
}

/// Where DeepSeek writes when it isn't boxed: its folder, with Node, `dsh`
/// and `dsh`'s own home, its sessions and settings in it.
const TRACES: &[Trace] = &[Trace::new(Kind::Home, "~/.deepseek")];

/// `~/.deepseek`: Node, `dsh`, its home and what Link keeps for it.
fn root() -> PathBuf {
    root_in(&link_files::home_dir())
}

/// [`root`] in home `home`.
fn root_in(home: &std::path::Path) -> PathBuf {
    home.join(".deepseek")
}

/// `dsh`'s own home (`DSH_HOME`): its sessions, settings and `.env`.
fn dsh_home() -> PathBuf {
    root().join("home")
}

fn programs_root() -> PathBuf {
    link_files::layout::exec_in(
        link_files::layout::Hidden::DeepseekPrograms,
        &link_files::home_dir(),
    )
}

fn node_dir() -> PathBuf {
    programs_root().join(format!("node-v{NODE}"))
}

/// Where Node is unpacked before it goes to [`node_dir`]: beside it, so
/// the rename stays on one file system, which the root may not share.
fn node_aside() -> PathBuf {
    node_dir().with_file_name(format!(".node-v{NODE}"))
}

/// Where npm installs `dsh` (`--prefix`).
fn prefix() -> PathBuf {
    programs_root().join("dsh")
}

/// The installed `dsh` package's entry point.
fn dsh_entry() -> PathBuf {
    prefix().join("lib/node_modules/@deepseek-ai/dsh/lib/bin.js")
}

/// Where `install` puts this program, which the sandbox runs it from.
fn installed() -> PathBuf {
    programs_root().join("bin/lnk-deepseek")
}

/// Link's patch layer over `dsh`'s settings, readable only by this
/// user: the addresses in it (its tools', a model's on this machine) hold
/// the proxy's secret.
fn patch_file() -> PathBuf {
    root().join("link.patch.yml")
}

/// `dsh`'s store of keys, where the model's goes.
fn tools_file() -> PathBuf {
    root().join("link-tools.json")
}

fn credentials_file() -> PathBuf {
    dsh_home().join(".credentials.yaml")
}

/// What Link keeps for the endpoint: its key.
fn link_env() -> PathBuf {
    link_env_in(&root())
}

/// [`link_env`] in root `root`.
fn link_env_in(root: &std::path::Path) -> PathBuf {
    root.join("link.env")
}

/// `uninstall` (`link_core::uninstall`): it runs nothing of its own
/// and keeps nothing outside the agents' folders, which are the harness
/// module's (`agents`); what it holds in each agent's home is said
/// there, for `lnk secret list`.
fn uninstall(args: &link_core::uninstall::Flags) -> anyhow::Result<()> {
    use link_core::uninstall as u;
    let kept = u::Kept {
        parts: vec![u::Part {
            name: "agents".into(),
            about: "your agents: their DeepSeek Harness homes".into(),
            credentials: held(&harness::homes("deepseek")),
            ..Default::default()
        }],
        ..Default::default()
    };
    u::run(args, &kept, || Ok(()), |_| Ok(()))
}

/// What it holds in each of `homes` (an agent's name and its home), as
/// `lnk secret list` shows it: the endpoint's key Link's connections call it
/// with. `dsh`'s store of keys also holds a copy of the agent's model
/// key, which the harness module lists.
fn held(homes: &[(String, PathBuf)]) -> Vec<link_core::uninstall::Held> {
    homes
        .iter()
        .map(|(agent, home)| (agent, link_env_in(&root_in(home))))
        // Only whether it's there: its value stays where it's kept.
        .filter(|(_, env)| {
            harness::read_env_file(env, "ENDPOINT_KEY").is_some_and(|k| !k.is_empty())
        })
        .map(|(agent, env)| link_core::uninstall::Held {
            what: link_core::uninstall::CredentialKind::AgentKey.into(),
            of: format!("{agent}'s endpoint, in DeepSeek Harness (ENDPOINT_KEY)"),
            kept: format!("{} (only yours)", env.display()),
            may: format!("Asks {agent} for a reply on its endpoint, as Link's connections do"),
        })
        .collect()
}

/// Each thread's `dsh` session.
fn sessions_file() -> PathBuf {
    root().join("threads.json")
}

/// The `dsh` installed, by its package's version.
fn installed_version() -> Option<String> {
    let manifest = prefix().join("lib/node_modules/@deepseek-ai/dsh/package.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).ok()?).ok()?;
    manifest["version"].as_str().map(str::to_string)
}

/// This machine's Node archive: its name and SHA-256.
fn node_archive() -> anyhow::Result<(String, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => bail!("DeepSeek Harness runs on macOS and Linux here, not {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => bail!("no Node for {other} here"),
    };
    let platform = format!("{os}-{arch}");
    let sha256 = NODE_ARCHIVES
        .iter()
        .find(|(p, _)| *p == platform)
        .map(|(_, sha)| *sha)
        .within(format!("no Node for {platform} here"))?;
    Ok((format!("node-v{NODE}-{platform}.tar.gz"), sha256))
}

/// Node's npm, trusting the machine's CAs as well as Node's own
/// ([`harness::trusting_machine_cas`]).
fn npm(cas: &[&str]) -> std::process::Command {
    let mut npm = link_secretive::process::command(node_dir().join("bin/npm"));
    harness::trusting_machine_cas(&mut npm, cas);
    npm
}

/// `install`: this program copied into the home, and Node and `dsh` put
/// there, the first time only.
fn install(out: &mut Out) -> Result<Done, Report> {
    let headline = || sentence!("Couldn't install DeepSeek Harness.");
    copy_self().map_err(|err| failed(out, headline(), err))?;
    if node_dir().join("bin/node").is_file() && installed_version().as_deref() == Some(VERSION) {
        return Ok(Done::shown());
    }
    out.block(Note(sentence!(
        "Installing DeepSeek Harness {} into {} (first time only, about 800 MB).",
        Foreign::new(VERSION),
        Path::new(root())
    )));
    // Node and npm write to the terminal themselves, npm only its errors:
    // while it works, Link says it's still going.
    out.hand_over();
    let _still = harness::progress::Heartbeat::start("installing DeepSeek Harness");
    install_dsh().map_err(|err| failed(out, headline(), err))?;
    Ok(Done::shown())
}

fn install_dsh() -> anyhow::Result<()> {
    install_node()?;
    let path = format!(
        "{}:/usr/bin:/bin:/usr/sbin:/sbin",
        node_dir().join("bin").display()
    );
    let cache = root().join("npm-cache");
    let result = harness::run(
        npm(harness::SYSTEM_CAS)
            .args([
                "install",
                "--global",
                "--no-fund",
                "--no-audit",
                "--loglevel=error",
                "--prefix",
            ])
            .arg(prefix())
            .arg(format!("@deepseek-ai/dsh@{VERSION}"))
            .env("PATH", path)
            .env("npm_config_cache", &cache)
            .env("npm_config_update_notifier", "false"),
    );
    let _ = std::fs::remove_dir_all(&cache);
    result.within("installing DeepSeek Harness with npm")?;
    if installed_version().as_deref() != Some(VERSION) {
        bail!(
            "npm finished, but DeepSeek Harness {VERSION} isn't in {}",
            prefix().display()
        );
    }
    Ok(())
}

/// Node, from nodejs.org, checked against its pinned SHA-256.
fn install_node() -> anyhow::Result<()> {
    if node_dir().join("bin/node").is_file() {
        return Ok(());
    }
    let (archive, sha256) = node_archive()?;
    std::fs::create_dir_all(root())?;
    let download = root().join(format!(".{archive}"));
    let result = (|| {
        let pin: serde_json::Value = serde_json::from_str(include_str!("../externals.json"))?;
        let url = pin["node"]["platforms"]
            .as_object()
            .and_then(|m| m.values().find(|p| p["sha256"].as_str() == Some(sha256)))
            .and_then(|p| p["url"].as_str())
            .within("missing Node artifact")?;
        let bytes = link_runtime::block_on(link_artifact::Cache::shared()?.bytes(
            url,
            sha256,
            100_000_000,
        ))?;
        std::fs::write(&download, bytes)?;
        let aside = node_aside();
        let _ = std::fs::remove_dir_all(&aside);
        std::fs::create_dir_all(&aside)?;
        harness::run(
            link_secretive::process::command("tar")
                .arg("-xzf")
                .arg(&download)
                .arg("--strip-components=1")
                .arg("-C")
                .arg(&aside),
        )?;
        std::fs::rename(&aside, node_dir())?;
        anyhow::Ok(())
    })();
    let _ = std::fs::remove_file(&download);
    result
}

/// Copies this program into the home, unless it's there as it is.
fn copy_self() -> anyhow::Result<()> {
    let me = std::env::current_exe().within("finding this program")?;
    let to = installed();
    if std::fs::read(&to).ok() == Some(std::fs::read(&me)?) {
        return Ok(());
    }
    std::fs::create_dir_all(to.parent().within("no folder")?)?;
    let aside = to.with_extension("new");
    std::fs::copy(&me, &aside).within(format!("copying to {}", aside.display()))?;
    std::fs::rename(&aside, &to).within(format!("writing {}", to.display()))?;
    Ok(())
}

fn configure(out: &Out, settings: &Settings) -> Result<(), Report> {
    harness::continuation::Context::configure(&link_files::home_dir(), "deepseek", settings)?;
    identity::configure(&link_files::home_dir(), settings).map_err(|err| {
        failed(
            out,
            sentence!("Couldn't write who the agent is into DeepSeek Harness's instructions."),
            err,
        )
    })?;
    let (patch, key) = patch(settings)?;
    write_settings(settings, &patch, &key).map_err(|err| {
        failed(
            out,
            sentence!("Couldn't write DeepSeek Harness's settings."),
            err,
        )
    })
}

fn write_settings(
    settings: &Settings,
    patch: &Value,
    key: &link_secretive::Secret,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dsh_home())?;
    let text = serde_json::to_string_pretty(patch)?;
    write_private(&patch_file(), &format!("{text}\n"))?;
    link_harness_hub::Tools::of(settings).keep(&tools_file())?;
    write_private(&credentials_file(), &credentials(key))?;
    harness::merge_env_file(
        &link_env(),
        &[(
            "ENDPOINT_KEY",
            settings
                .endpoint_key
                .as_ref()
                .map(|key| key.expose().as_str()),
        )],
    )?;
    Ok(())
}

/// `dsh`'s stored keys, as it reads them: the model's key, by the name
/// its provider asks for. Its store wins over a `.env` in the workspace,
/// which `dsh` also reads keys from. YAML takes JSON as it is.
fn credentials(key: &link_secretive::Secret) -> String {
    // Written to its credentials file, mode 600: what `dsh` reads.
    let document = json!({"version": 1, "refs": {KEY_ENV: key.expose()}});
    format!(
        "{}\n",
        serde_json::to_string_pretty(&document).unwrap_or_default()
    )
}

/// Writes `text` to `path`, readable only by this user (`dsh` refuses
/// its credentials otherwise), in one step. What's beside it from a write
/// that stopped halfway (`<path>.new`) goes first.
fn write_private(path: &std::path::Path, text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let aside = path.with_extension("new");
    let _ = std::fs::remove_file(&aside);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&aside)
        .within(format!("writing {}", aside.display()))?;
    file.write_all(text.as_bytes())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&aside, path)?;
    Ok(())
}

/// How `dsh` reaches the model: its API (as `dsh`'s `llm-pi-ai` names
/// it) and its address, Link's proxy when it's given one.
fn route(model: &harness::Model) -> Result<(&'static str, String), Report> {
    let local = |base: &str| format!("{}/v1", base.trim_end_matches('/'));
    Ok(match (model.provider.as_str(), &model.base_url) {
        ("ollama" | "local", Some(base)) => ("openai-completions", local(base)),
        ("anthropic", base) => (
            "anthropic-messages",
            base.clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into()),
        ),
        ("openai", base) => (
            "openai-completions",
            base.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
        ),
        ("openrouter", base) => (
            "openai-completions",
            base.clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".into()),
        ),
        (provider, _) => {
            return Err(Report::error(sentence!(
                "DeepSeek Harness can't reach a model from {} here.",
                Foreign::new(provider)
            ))
            .next(
                cmd!("lnk agent start --model.default <name>"),
                phrase!("name a model from another provider"),
            )
            .exit(Exit::Usage));
        }
    })
}

/// Link's patch layer over `dsh`'s settings, and the model's key. A
/// patch replaces each named row's whole `config`, so this one says all
/// of it: the model as a provider of its own (`link`), reached at its
/// address; `dsh`'s own sandbox and approvals off (Link's sandbox
/// confines it, and nobody is there to approve); the workspace the
/// agent's files; nothing sent to DeepSeek but the model's requests; and
/// each tool an MCP server at its address on Link's proxy. YAML takes
/// JSON as it is.
fn patch(settings: &Settings) -> Result<(Value, link_secretive::Secret), Report> {
    let model = &settings.model;
    let name = model.named().ok_or_else(|| {
        Report::error(sentence!("The agent's model has no name."))
            .next(
                cmd!("lnk agent start --model.default <name>"),
                phrase!("set one"),
            )
            .exit(Exit::Usage)
    })?;
    let (api, base_url) = route(model)?;
    // A model here takes any key.
    let key = if model.api_key.is_empty() {
        link_secretive::Secret::from("none".to_string())
    } else {
        model.api_key.duplicate()
    };
    let files = settings.files.display().to_string();
    let patch = vec![
        json!({"id": "llm-pi-ai", "config": {"providers": {"link": {
            "apiKeyEnv": KEY_ENV,
            "api": api,
            "baseURL": base_url,
            "models": [{"id": name}],
        }}}}),
        json!({"id": "acp", "config": {"provider": "link", "model": name}}),
        json!({"id": "sandbox-policy", "config": {"mode": "danger-full-access", "workspaceRoot": files}}),
        json!({"id": "approval", "config": {"policy": "never"}}),
        // Sent with each request to DeepSeek otherwise: the session's log,
        // the plugins installed; the log to DeepSeek's collector on
        // `/feedback`; and web search, through DeepSeek's API.
        json!({"id": "session-log-deepseek", "disabled": true}),
        json!({"id": "plugin-package-inventory-deepseek", "disabled": true}),
        json!({"id": "session-telemetry-otel", "disabled": true}),
        json!({"id": "web-search-deepseek", "disabled": true}),
        // Its tool too, which would answer that it has no search.
        json!({"id": "tool-web", "config": {"search": false, "fetch": true}}),
        identity::skills_row(&link_files::home_dir()),
    ];
    Ok((Value::Array(patch), key))
}

/// What of `~/.deepseek` is the agent: `dsh`'s home (its sessions and
/// settings) and which session is which thread's, but not Node, `dsh`
/// itself or this program, nor what `configure` writes again.
fn state() -> harness::State {
    let path = |p: &str| format!("~/.deepseek{p}");
    harness::State {
        include: vec![path(""), "~/.lnk/data/agents/context".into()],
        exclude: [
            "/bin",
            "/dsh",
            "/npm-cache",
            "/link.env",
            "/link.patch.yml",
            "/home/.credentials.yaml",
            "/home/cache",
            "/home/logs",
            "/home/storages",
        ]
        .into_iter()
        .map(path)
        .chain([path(&format!("/node-v{NODE}"))])
        .collect(),
    }
}

/// The hosts `install` and `configure` reach, behind their proxy: Node's
/// releases and npm's registry.
const HOSTS: &[&str] = &["nodejs.org", "registry.npmjs.org"];

fn launch() -> Result<Launch, Report> {
    if !dsh_entry().is_file() {
        return Err(
            Report::error(sentence!("DeepSeek Harness isn't installed."))
                .next(
                    cmd!("lnk agent harness set deepseek"),
                    phrase!("install it"),
                )
                .exit(Exit::Failure),
        );
    }
    Ok(Launch {
        argv: vec![installed().display().to_string(), "serve".into()],
        env: [
            ("DSH_HOME".to_string(), dsh_home().display().to_string()),
            // Its telemetry, off whatever its settings say.
            ("DSH_TELEMETRY_DISABLED".into(), "1".into()),
            (
                "PATH".into(),
                format!(
                    "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                    node_dir().join("bin").display()
                ),
            ),
        ]
        .into(),
        // HOME is its home folder: everything is in ~/.deepseek.
        write: vec![],
        read: vec![],
        ports: vec![port()],
        serve: Some(vec![port()]),
        services: vec![],
        // The 180 s every harness had before an adapter could say.
        wait_secs: Some(180),
    })
}

/// The endpoint: ACP on its port until stopped, with `dsh` behind it.
fn serve() -> Result<(), Report> {
    let files = std::env::var("LNK_FILES").map_err(|_| {
        Report::error(sentence!("The variable LNK_FILES isn't set."))
            .why(prose!(
                "{} runs it with the agent's files.",
                cmd!("lnk agent start")
            ))
            .next(cmd!("lnk agent start"), phrase!("start the agent"))
            .exit(Exit::Usage)
    })?;
    let key = harness::read_env_file(&link_env(), "ENDPOINT_KEY").filter(|k| !k.is_empty());
    let start = link_harness_hub::spawning({
        let files = files.clone();
        move || {
            let mut dsh = link_secretive::process::command(node_dir().join("bin/node"));
            dsh.arg(dsh_entry())
                .args(["--profile", "acp", "--patch"])
                .arg(patch_file())
                .current_dir(&files);
            dsh
        }
    });
    link_harness_hub::Hub::new(link_harness_hub::Config {
        start,
        cwd: files,
        key: key.map(link_secretive::Secret::from),
        agent: std::env::var("LNK_AGENT").ok().filter(|a| !a.is_empty()),
        sessions: sessions_file(),
        state: Some(std::sync::Arc::new(state::DeepSeek {
            home: link_files::home_dir(),
        })),
        // The ACP adapter advertises HTTP MCP and mounts servers per session.
        tools: link_harness_hub::Tools::kept(&tools_file())?,
    })
    .with_continuity(&link_files::home_dir())?
    .serve(port())
    .within("serving its endpoint")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_is_unpacked_beside_where_it_goes() {
        // A rename across file systems fails (EXDEV), and the programs'
        // folder may be on another than the root.
        assert_eq!(node_aside().parent(), node_dir().parent());
    }

    /// npm trusts the machine's CAs too, the first bundle there is: behind
    /// a proxy that re-signs TLS, Node's own refuse the registry.
    #[test]
    fn npm_trusts_the_machines_cas() {
        let dir = link_test_kit::TempDir::new("cas");
        let bundle = dir.join("ca-bundle.crt");
        std::fs::write(&bundle, "").unwrap();
        let given = |npm: &std::process::Command| {
            npm.get_envs()
                .find(|(k, _)| *k == "NODE_EXTRA_CA_CERTS")
                .and_then(|(_, v)| v.map(std::path::PathBuf::from))
        };
        let missing = dir.join("none.crt");
        let cas = [missing.to_str().unwrap(), bundle.to_str().unwrap()];
        // Those this process was given stay: they're the machine's own.
        let mine = std::env::var_os("NODE_EXTRA_CA_CERTS").map(std::path::PathBuf::from);
        assert_eq!(given(&npm(&cas)), mine.clone().or(Some(bundle.clone())));
        assert_eq!(given(&npm(&cas[..1])), mine);
    }

    fn rows(patch: &Value) -> Vec<Value> {
        patch.as_array().unwrap().clone()
    }

    fn row(patch: &Value, id: &str) -> Value {
        rows(patch)
            .into_iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("no row {id}"))
    }

    #[test]
    fn the_model_is_a_provider_of_its_own_with_its_key_out_of_the_patch() {
        let settings = Settings {
            files: "/Users/me/Link".into(),
            model: harness::Model {
                provider: "anthropic".into(),
                api_key: "sk-ant-api03-secret".to_string().into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let (patch, key) = patch(&settings).unwrap();
        assert!(!patch.to_string().contains("secret"));
        assert_eq!(
            key,
            link_secretive::Secret::from("sk-ant-api03-secret".to_string())
        );
        let link = &row(&patch, "llm-pi-ai")["config"]["providers"]["link"];
        assert_eq!(link["api"], "anthropic-messages");
        assert_eq!(link["baseURL"], "https://api.anthropic.com");
        assert_eq!(link["apiKeyEnv"], KEY_ENV);
        // Link's default for the provider, the same in every harness.
        let name = harness::default_model("anthropic").unwrap();
        assert_eq!(link["models"][0]["id"], name);
        assert_eq!(
            row(&patch, "acp")["config"],
            json!({"provider": "link", "model": name})
        );
        assert_eq!(
            row(&patch, "sandbox-policy")["config"]["workspaceRoot"],
            "/Users/me/Link"
        );
    }

    #[test]
    fn nothing_but_the_model_s_requests_goes_to_deepseek() {
        let (patch, _) = patch(&Settings {
            model: harness::Model {
                provider: "openrouter".into(),
                name: Some("deepseek/deepseek-v4-pro".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        for id in [
            "session-log-deepseek",
            "plugin-package-inventory-deepseek",
            "session-telemetry-otel",
            "web-search-deepseek",
        ] {
            assert_eq!(row(&patch, id)["disabled"], true, "{id}");
        }
        assert_eq!(row(&patch, "tool-web")["config"]["search"], false);
    }

    #[test]
    fn a_model_on_the_wire_or_here_is_reached_at_its_address() {
        let at = |provider: &str, base: Option<&str>| {
            route(&harness::Model {
                provider: provider.into(),
                base_url: base.map(str::to_string),
                ..Default::default()
            })
            .unwrap()
        };
        let proxy = "http://127.0.0.1:40781/_link/model";
        assert_eq!(
            at("anthropic", Some(proxy)),
            ("anthropic-messages", proxy.into())
        );
        assert_eq!(
            at("openai", Some(&format!("{proxy}/v1"))),
            ("openai-completions", format!("{proxy}/v1"))
        );
        assert_eq!(
            at("openrouter", None),
            ("openai-completions", "https://openrouter.ai/api/v1".into())
        );
        assert_eq!(
            at("ollama", Some("http://localhost:11434/")),
            ("openai-completions", "http://localhost:11434/v1".into())
        );
        let (_, key) = patch(&Settings {
            model: harness::Model {
                provider: "ollama".into(),
                name: Some("qwen3:8b".into()),
                base_url: Some("http://localhost:11434".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        assert_eq!(key, link_secretive::Secret::from("none".to_string()));
    }

    #[test]
    fn each_tool_is_an_mcp_server_at_its_proxy() {
        let mut settings = Settings {
            model: harness::Model {
                provider: "openai".into(),
                ..Default::default()
            },
            tools: [(
                "gh".to_string(),
                "http://127.0.0.1:40781/_link/mcp/gh".to_string(),
            )]
            .into(),
            ..Default::default()
        };
        settings.memory = Some("http://127.0.0.1:40781/_link/mcp/memory-main".into());
        let (patch, _) = patch(&settings).unwrap();
        assert!(rows(&patch).iter().all(|row| row.get("insert").is_none()));
        let own = link_test_kit::TempDir::new("deepseek-session-tools");
        let file = own.join("link-tools.json");
        let tools = link_harness_hub::Tools::of(&settings);
        tools.keep(&file).unwrap();
        assert_eq!(link_harness_hub::Tools::kept(&file).unwrap(), tools);
        let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            kept,
            json!({
                "schema": 1,
                "servers": {
                "gh": "http://127.0.0.1:40781/_link/mcp/gh",
                "memory": "http://127.0.0.1:40781/_link/mcp/memory-main"
                },
                "actions": {"urls":{},"actions":[]}
            })
        );
        // None: no rows inserted, so those of before are gone.
        settings.tools.clear();
        settings.memory = None;
        let (patch, _) = super::patch(&settings).unwrap();
        assert!(rows(&patch).iter().all(|r| r.get("insert").is_none()));
    }

    #[test]
    fn the_key_is_in_dsh_s_store_by_its_provider_s_name() {
        let stored: Value = serde_json::from_str(&credentials(&link_secretive::Secret::from(
            "sk-\"odd\nkey".to_string(),
        )))
        .unwrap();
        assert_eq!(
            stored,
            json!({"version": 1, "refs": {"LNK_MODEL_CREDENTIAL": "sk-\"odd\nkey"}})
        );
    }

    #[test]
    fn node_is_pinned_for_every_platform_link_ships() {
        for (platform, sha256) in NODE_ARCHIVES {
            assert!(platform.starts_with("darwin-") || platform.starts_with("linux-"));
            assert_eq!(sha256.len(), 64);
        }
        node_archive().unwrap();
    }

    #[test]
    fn its_installs_hosts_are_listed() {
        for host in HOSTS {
            link_sandbox_contract::net::check_host(host).unwrap();
        }
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
