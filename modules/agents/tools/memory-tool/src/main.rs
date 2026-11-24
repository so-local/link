//! `lnk-memory-tool`: Link Memory, Link's own memory engine. An MCP
//! server over stdio that searches, reads and lists a folder
//! (`link_memory_tool`), for any harness to reach as a tool:
//!
//! ```sh
//! lnk-memory-tool serve <folder> [--index <file>]
//! ```
//!
//! `--index` keeps the index in a file, so a new session reads again
//! only what changed; without it, the index is in memory. `--embed
//! <model>` searches by meaning too, with that model at `--embed-url`
//! (or `$LNK_MEMORY_MODEL`, served for it outside its sandbox), else
//! from `lnk model serve` (`semantic`); without it, or with the address
//! empty, by words.
//!
//! `--notes <file>` offers `remember`, which writes the agent's notes
//! there, at most `--notes-most` bytes.
//!
//! `serve-many <socket> [<agents>]` serves every agent `<agents>` names
//! from one process, on a Unix socket, each session its first line's
//! agent's, and each agent a session binds as its bridges reach it
//! (`serve-many`): `--cache <folder>` keeps the index of one `<agents>`
//! doesn't name, `--embed <model> --embed-url <url>` ranks its search by
//! meaning, `--peer <user>` takes that user's sessions too (the service
//! binding a machine's agents), its socket shared with `--socket-group <group>`,
//! and `--stay` runs it for good. A bound agent's bridges must be this
//! machine's: `--files-bridge <socket>`, `--storage-bridge <socket>`, or
//! its own in `--bridges <folder>` (`<folder>/<agent>/storage.sock`).
//!
//! `bridge` serves one agent whose Home is kept in a bucket, as its
//! machine's bridges reach it, from a process of its own run as that
//! agent's own user: `--socket` the files bridge, `--store` the storage
//! bridge for the collections `--kept` says are kept elsewhere, its key in
//! `--keyring`, its index fetched into `--cache` and written back when its
//! input closes, searching by words alone; `--team` serves its team's
//! memory instead, and `--once` only writes a current index back.
//!
//! A PDF's or a Word document's text is Link Document Converter's
//! (`lnk-document-converter`), a module of its own, run as a child holding no
//! key (`extract`).
//!
//! It is its own adapter too (`link_memory_tool_contract`): `install` has
//! nothing to do, and `command <folder> <state> [<agent>]` serves the
//! folder, with the agent's conversations and skills in its own folder
//! `<agent>` when Link names it, its index in `<state>` and meaning from the spec's model (else
//! [`DEFAULT_MODEL`]; `none`, by words only), served for it, and the
//! agent's notes where Link names them (`LNK_MEMORY_NOTES`), every call
//! allowed (they only read, or write the notes alone).

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use link_format::{Done, Out, Product, Prose, Report, Sentence, Usage, cmd, phrase, sentence};
use link_memory_tool::cli::{self, Command};
use link_memory_tool::notes::{self, Notes};
use link_memory_tool::semantic::DEFAULT_MODEL;
use link_memory_tool::{extract, mcp, service};
use link_memory_tool_contract::declared::{Agents, Launch};
use link_memory_tool_contract::{FROM_ENV, NOTES_ENV};

/// The index's file, in the state folder the harness module gives it.
const INDEX: &str = "index.sqlite";

/// The model a spec names to search by words only.
const NO_MODEL: &str = "none";

/// What the agent's spec says of its memory (`LNK_MEMORY_SPEC`, its
/// `[tool.memory]` as JSON): the embedding model, `none` to search by
/// words only, the most a search gives, and the most its notes hold
/// (`"4 KB"`). Without it, the engine's own.
#[derive(Default, serde::Deserialize)]
struct Opinions {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: Option<usize>,
    #[serde(default)]
    notes: Option<String>,
}

impl Opinions {
    /// The spec Link gives, refused when it isn't one: a typo never runs
    /// another model than asked.
    fn from_env() -> anyhow::Result<Self> {
        let Ok(json) = std::env::var(link_memory_tool_contract::SPEC_ENV) else {
            return Ok(Self::default());
        };
        let spec: Self = serde_json::from_str(&json)
            .with_context(|| format!("reading {}", link_memory_tool_contract::SPEC_ENV))?;
        // A model's name, as `lnk model serve` takes it.
        if let Some(model) = &spec.model {
            ensure!(
                !model.is_empty()
                    && model
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
                "{}: {:?} isn't a model's name",
                link_memory_tool_contract::SPEC_ENV,
                link_format::clean(model)
            );
        }
        ensure!(
            spec.tokens
                .is_none_or(|n| (1..=link_memory_tool::mcp::tokens_max()).contains(&n)),
            "{}: tokens is from 1 to {}, the most an answer holds",
            link_memory_tool_contract::SPEC_ENV,
            link_memory_tool::mcp::tokens_max()
        );
        spec.notes_most()?;
        Ok(spec)
    }

    /// The most the notes hold, in bytes, as the spec says.
    fn notes_most(&self) -> anyhow::Result<Option<u64>> {
        let Some(notes) = &self.notes else {
            return Ok(None);
        };
        let most = link_agent_spec::size(notes)
            .with_context(|| format!("{}: notes", link_memory_tool_contract::SPEC_ENV))?;
        ensure!(
            most > 0,
            "{}: notes is at least a byte",
            link_memory_tool_contract::SPEC_ENV
        );
        Ok(Some(most))
    }
}

/// What the agent's spec says its memory may spend (`LNK_MEMORY_SPEC`,
/// its `[tool.memory]`), each key it leaves out Link's specification's.
fn spending() -> anyhow::Result<link_memory_tool::opinions::Opinions> {
    use link_memory_tool::opinions::Opinions;
    let Ok(json) = std::env::var(link_memory_tool_contract::SPEC_ENV) else {
        return Ok(Opinions::link().clone());
    };
    let tool: link_agent_spec::Tool = serde_json::from_str(&json)
        .with_context(|| format!("reading {}", link_memory_tool_contract::SPEC_ENV))?;
    Opinions::of(&tool, Some(Opinions::link()))
        .with_context(|| format!("reading {}", link_memory_tool_contract::SPEC_ENV))
}

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| try_main(out).map_err(failed))
}

/// A failure as a report: a usage error says where to read the usage,
/// anything else keeps its words.
fn failed(err: anyhow::Error) -> Report {
    if err.is::<Usage>() {
        return Report::usage("lnk-memory-tool");
    }
    let program = Product::new("lnk-memory-tool");
    Report::error(sentence!("{} stopped.", program))
        .why(Prose::from(Sentence::adopt(&format!("{err:#}"))))
        .next(cmd!("lnk agent list"), phrase!("see which agents run"))
        .cause(err)
}

fn try_main(out: &mut Out) -> anyhow::Result<Done> {
    // Its warnings are log lines: it runs with no one to read them.
    link_format::daemon::logs();
    let ended: anyhow::Result<()> = match cli::parse().command.command {
        Command::MemoryToolServe(serve) => {
            let env = |key: &str| std::env::var(key).ok();
            let (index, options) = service::served(&serve, &env)?;
            link_memory_tool::serve::Served::start(index, options)
                .session(std::io::stdin().lock(), link_format::streams::protocol())
        }
        // Every agent of an owner, from one process, on its socket: each
        // as `<agents>` says, or as its bridges reach it once bound (a
        // machine that binds its agents: `--cache`, `--stay`, `--peer`).
        Command::MemoryToolServeMany(many) => {
            let agents: Agents = match &many.agents {
                Some(file) => link_files::saved::parse_json(
                    &std::fs::read(file).with_context(|| format!("reading {}", file.display()))?,
                )
                .with_context(|| format!("reading {}", file.display()))?,
                None => Agents(Default::default()),
            };
            let config = service::Config {
                idle: (!many.stay).then_some(service::IDLE),
                write_every: service::WRITE_EVERY,
                cache: many.cache,
                peer: many.peer.as_deref().map(service::user_id).transpose()?,
                group: many
                    .socket_group
                    .as_deref()
                    .map(service::group_id)
                    .transpose()?,
                files_bridge: many.files_bridge,
                storage_bridge: many.storage_bridge,
                bridges: many.bridges,
                // Its check hook holds the two together.
                embed: many.embed_url.zip(many.embed),
            };
            // Its caches are its own: no one else on the machine reads them.
            // SAFETY: umask only sets this process's file mode mask.
            unsafe { libc::umask(0o077) };
            service::serve_with(&many.socket, agents, config)
        }
        // One agent, or its team's memory, through its machine's bridges:
        // a process of its own, as its own user.
        Command::MemoryToolBridge(bridge) => {
            let one = bridged(bridge)?;
            // Its cache is its own: no one else on the machine reads it.
            // SAFETY: umask only sets this process's file mode mask.
            unsafe { libc::umask(0o077) };
            service::serve_one(
                one,
                std::io::stdin().lock(),
                link_format::streams::protocol(),
            )
        }
        // Nothing to install: this program is the engine.
        Command::MemoryToolInstall(_) => Ok(()),
        // A model's calls, made over MCP through serve: no command line
        // reads them.
        Command::MemoryToolSearch(_)
        | Command::MemoryToolRead(_)
        | Command::MemoryToolList(_)
        | Command::MemoryToolLinks(_)
        | Command::MemoryToolRemember(_) => Err(anyhow::anyhow!(
            "memory's calls are a model's, made through serve"
        )),
        Command::LaunchShow(cli::LaunchShowArgs {
            folder,
            state,
            agent: own,
        }) => {
            let me = std::env::current_exe().context("finding this program")?;
            // The agent's opinions, from its spec, when Link gives them.
            let spec = Opinions::from_env()?;
            let model = spec.model.clone().unwrap_or_else(|| DEFAULT_MODEL.into());
            // `none`: by words only, no model served.
            let model = (model != NO_MODEL).then_some(model);
            let mut serve = cli::MemoryToolServeArgs {
                folder: folder.clone(),
                index: Some(state.join(INDEX)),
                agent_folder: own.clone(),
                embed: model.clone(),
                tokens: spec.tokens.and_then(|n| u16::try_from(n).ok()),
                embed_url: None,
                notes: None,
                notes_most: None,
                from_others: false,
                index_memory: None,
                reserve: None,
                file_most: None,
                threads: None,
                deadline_ms: None,
                dimensions: None,
                folders: Vec::new(),
                kept_off: None,
            };
            // What its memory may spend: each key of its spec's, else Link's.
            spending()?.serve_with(&mut serve);
            // Its calls that only read, as its contract declares them.
            let mut allow: Vec<String> = link_memory_tool_contract::reads()?;
            // The agent's notes, where Link says it may write them.
            if let Some(file) = std::env::var_os(NOTES_ENV).filter(|f| !f.is_empty()) {
                let most = spec.notes_most()?.unwrap_or(notes::MOST);
                // Checked here, so a start says what's wrong.
                Notes::new(own.as_deref().unwrap_or(&folder), Path::new(&file), most)?;
                serve.notes = Some(PathBuf::from(file));
                serve.notes_most = Some(most);
                // Sent others' messages, its harness names each call's
                // conversation, and only the owner's are noted in.
                serve.from_others = std::env::var_os(FROM_ENV).is_some_and(|v| v == "1");
                allow.push(mcp::REMEMBER.into());
            }
            let launch = Launch {
                argv: std::iter::once(me.display().to_string())
                    .chain(cli::serve_argv(&serve))
                    .collect(),
                allow,
                // The embedding model, served for it outside its sandbox.
                models: model.is_some().then_some(true),
                model,
                // One process serves every agent of an owner.
                shared: Some(true),
                // Link Document Converter, which reads PDFs and Word documents.
                programs: vec![extract::MODULE.to_string()],
            };
            out.data(format!("{}\n", serde_json::to_string(&launch)?).as_bytes());
            Ok(())
        }
    };
    ended?;
    Ok(Done::shown())
}

/// What a `bridge` line serves: the agent as its keyring binds it, its
/// collections where `--kept` says, and its team's memory with `--team`.
fn bridged(bridge: cli::MemoryToolBridgeArgs) -> anyhow::Result<service::One> {
    use link_store_contract::storage::{Collection, Place, Places, StorageAt};
    let keyring = match &bridge.keyring {
        Some(dir) => link_sandbox_binding::Keyring::at(dir),
        None => link_sandbox_binding::Keyring::here(),
    };
    // Its key while it's bound, and only as the account named.
    let binding = keyring
        .bound(&bridge.r#as)
        .filter(|b| b.identity.owner == bridge.account)
        .with_context(|| {
            format!(
                "agent {} isn't bound here",
                link_format::clean(&bridge.r#as)
            )
        })?;
    let mut places = std::collections::BTreeMap::new();
    for kept in &bridge.kept {
        let (collection, place) = kept
            .split_once('=')
            .with_context(|| format!("--kept {kept}: <collection>=<place>"))?;
        let collection: Collection = serde_json::from_value(collection.into())
            .with_context(|| format!("--kept {kept}: no collection {collection}"))?;
        places.insert(
            collection,
            Place {
                place: place.to_string(),
                overridden: false,
            },
        );
    }
    let team = match (&bridge.team, &bridge.team_name) {
        (Some(id), Some(name)) => Some(link_memory_tool_contract::TeamSession {
            id: id.clone(),
            named: link_memory_tool_contract::TeamRef::parse(name)
                .with_context(|| format!("--team-name {name}"))?,
            writes: bridge.team_writes,
            semantic: false,
        }),
        // Its check hook holds the team's flags together.
        _ => None,
    };
    Ok(service::One {
        agent: bridge.r#as,
        bound: link_memory_tool_contract::Bound {
            index: link_memory_tool_contract::IndexAt::Home,
            owner: binding.identity.owner,
            service: binding.identity.service,
            key: binding.key,
            files: Some(bridge.socket),
            storage: bridge.store.map(|socket| StorageAt {
                places: Places(places),
                socket: Some(socket),
            }),
            notes: bridge.notes_most,
        },
        team,
        cache: bridge.cache,
        once: bridge.once,
    })
}

#[cfg(test)]
link_test_kit::program!(unit);
