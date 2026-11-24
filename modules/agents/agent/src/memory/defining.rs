//! An agent's memory tool defined ([`define`]): its notes, what it reads,
//! where its conversations are kept, and the tool's definition.

use super::engine::{Network, Port};
use super::*;
use link_format::WithinExt;

/// The agent's notes file, which its memory may write (`remember`),
/// though by default its harness writes nothing else of its
/// instructions: none when its sandbox file locks them (`instructions`
/// under `[files] read`).
pub fn notes_of(agent: &Settings) -> Option<PathBuf> {
    let folders = link_agent::sandbox_file::of(&agent.name).folders();
    notes_written(&folders).then(|| agent.folder().agent.join(layout::NOTES))
}

/// What memory reads of the agent's own folder `own`: its conversations
/// and its skills, never the rest.
pub(super) fn own_reads(own: &Path) -> Vec<PathBuf> {
    [layout::CONVERSATIONS, layout::SKILLS]
        .iter()
        .map(|name| own.join(name))
        .collect()
}

/// Whether memory writes the notes under `folders`: unless the owner
/// locked `instructions` (under `read`, and not under `write`).
fn notes_written(folders: &link_harness_contract::harness::Folders) -> bool {
    folders.writes_notes()
}

/// Defines `agent`'s memory as its tool, as it should be now: its engine
/// installed, serving its Home and its own folder. The tool's name, or
/// none when it has no memory.
pub fn define(out: &mut Out, agent: &Settings) -> Result<Option<String>, Report> {
    let (Some(name), Some(tool)) = (&agent.memory, agent.memory_tool()) else {
        return Ok(None);
    };
    let engine = Engine::find_or_add(out, name, "Your agent's memory", true)?;
    let defining = Defining {
        folder: agent.files(),
        own: Some(agent.folder().agent),
        state: agent.memory_state(&engine.name),
        notes: notes_of(agent),
        serving: Serving::of(agent)?,
        record: settings::config_of(&agent.name).join("memory.json"),
        shared: false,
        owner: Some((agent.owner()?, agent.key())),
        agent: Some(agent.name.clone()),
        store: Store::of(agent)?,
        engine,
        tool,
    };
    Ok(define_tool(&defining).map(Some)?)
}

/// The names of the store drivers `agent`'s conversations are kept by (a
/// conversation's folder holds `store.<name>`) or its spec names
/// (`[storage] conversations`).
pub fn store_drivers(agent: &Settings) -> anyhow::Result<Vec<String>> {
    use link_agent_spec::{COMPACT, LOG};
    let mut names = Vec::new();
    let named = agent.spec()?.spec.storage.unwrap_or_default();
    let named = named.conversations();
    if ![LOG, COMPACT].contains(&named) {
        names.push(named.to_string());
    }
    let conversations = agent
        .folder()
        .of(link_store_contract::storage::Collection::Conversations);
    for folder in std::fs::read_dir(&conversations)
        .into_iter()
        .flatten()
        .flatten()
    {
        for file in std::fs::read_dir(folder.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(name) = file
                .file_name()
                .to_str()
                .and_then(|n| n.strip_prefix("store."))
                && link_store_contract::store::check_name(name).is_ok()
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// What an agent's memory needs of where its conversations are kept: the
/// programs of the store drivers that keep them, installed, which it runs
/// to read them; and the agent as they name it, in its environment.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Store {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) drivers: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(super) env: std::collections::BTreeMap<String, String>,
}

impl Store {
    /// `agent`'s, as Link Harness names it to a driver.
    fn of(agent: &Settings) -> anyhow::Result<Self> {
        use link_agent_spec::{COMPACT, LOG};
        // Only the driver its spec names: a `store.<name>` in a folder the
        // agent writes never picks what memory runs.
        let storage = agent.spec()?.spec.storage.unwrap_or_default();
        let named = storage.conversations();
        if [LOG, COMPACT].contains(&named) {
            return Ok(Self::default());
        }
        let drivers: Vec<PathBuf> =
            link_core::find_module(link_store_contract::store::module_for(named))
                .map(|program| program.canonicalize().unwrap_or(program))
                .into_iter()
                .collect();
        if drivers.is_empty() {
            return Ok(Self::default());
        }
        let mut env = std::collections::BTreeMap::from([
            (
                link_agent::adapter::AGENT_ENV.to_string(),
                agent.name.clone(),
            ),
            (
                link_memory_tool_contract::DRIVER_ENV.to_string(),
                named.to_string(),
            ),
        ]);
        if let (Some(id), Some(owner)) = (agent.agent_id(), &agent.owner) {
            env.insert(
                link_agent_contract::identity::AGENT_ID_ENV.into(),
                id.to_string(),
            );
            env.insert(
                link_agent_contract::identity::OWNER_ENV.into(),
                owner.clone(),
            );
        }
        if let Some(service) = &agent.service {
            env.insert(
                link_agent_contract::identity::SERVICE_ENV.into(),
                service.clone(),
            );
        }
        Ok(Self { drivers, env })
    }
}

/// What a memory tool is defined from.
pub(super) struct Defining {
    pub(super) tool: String,
    pub(super) engine: Engine,
    /// The folder it serves, the agent's own folder beside it (none for a
    /// team's), where it keeps what it knows, and the notes it may write.
    pub(super) folder: PathBuf,
    pub(super) own: Option<PathBuf>,
    pub(super) state: PathBuf,
    pub(super) notes: Option<PathBuf>,
    pub(super) serving: Serving,
    /// Its record: what it was last defined from.
    pub(super) record: PathBuf,
    /// One server for every session ([`define_team`]'s).
    pub(super) shared: bool,
    /// An agent's owner and key, whose memory its owner's agents' one
    /// service serves when its engine can ([`Launch::shared`](link_memory_tool_contract::declared::Launch::shared)); none for
    /// a team's.
    pub(super) owner: Option<(String, String)>,
    /// The agent's name, whose settings name its storage bridge; none for
    /// a team's.
    pub(super) agent: Option<String>,
    /// Where an agent's conversations are kept, for its memory to read.
    pub(super) store: Store,
}

/// Defines memory tool `defining.tool`, unless it's defined so already.
/// Its name.
pub(super) fn define_tool(defining: &Defining) -> anyhow::Result<String> {
    let Defining {
        tool,
        engine,
        folder,
        own,
        state,
        notes,
        serving,
        record,
        shared,
        owner,
        agent,
        store,
    } = defining;
    let tool = tool.clone();
    // Defined as it would be again, the programs it ran last time where
    // they are now: nothing to do.
    let mut defined = defined_as(defining, &programs_of(&recorded_programs(record)));
    let kept = crate::defined::kept::<Definition>(record);
    defined.tool_argv_sha256 = kept.as_ref().and_then(|kept| kept.tool_argv_sha256.clone());
    if state.is_dir()
        && kept.as_ref() == Some(&defined)
        && crate::confine::defined_tools().is_ok_and(|tools| tools.contains(&tool))
    {
        return Ok(tool);
    }
    let _ = std::fs::remove_file(record);
    std::fs::create_dir_all(folder).within(format!("creating {}", folder.display()))?;
    link_files::create_private_dir(state).within(format!("creating {}", state.display()))?;
    engine.install(serving, state)?;
    // The folder its notes are in, written: a file alone couldn't be
    // replaced whole, in one step.
    let instructions = notes
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    if let Some(instructions) = &instructions {
        std::fs::create_dir_all(instructions)
            .within(format!("creating {}", instructions.display()))?;
    }
    let reads = own.as_deref().map(own_reads).unwrap_or_default();
    for read in &reads {
        std::fs::create_dir_all(read).within(format!("creating {}", read.display()))?;
    }
    let serve = engine.serve(serving, folder, own.as_deref(), state, notes.as_deref())?;
    let program = PathBuf::from(&serve.argv[0]);
    // The other modules' programs it runs, read where they're installed.
    let programs = programs_of(&serve.programs);
    let mut defined = defined_as(defining, &programs);
    let programs: Vec<PathBuf> = programs.into_values().flatten().collect();
    let lnk = link_core::lnk()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let mut add: Vec<String> = vec!["sandbox".into(), "tool".into(), "add".into(), tool.clone()];
    for call in &serve.allow {
        add.extend(["--add".into(), call.clone()]);
    }
    if *shared {
        add.push("--shared".into());
    }
    add.push("--".into());
    let write: Vec<&Path> = instructions.iter().map(PathBuf::as_path).collect();
    // Whether it reaches this machine's models, and how its sandbox
    // does: the model served for it, at its port alone, or, finding
    // runtimes itself, the machine's whole network.
    let models = serve.models == Some(true);
    let network = models || serve.model.is_some();
    let reach = match (&serve.model, models) {
        (Some(_), _) => Network::Models(vec![Port::Served]),
        (None, true) => Network::Host,
        (None, false) => Network::Closed,
    };
    // An agent's, one process for every agent of its owner here, which
    // each session reaches through `connect`: what it serves this agent
    // with is kept where no sandbox reads.
    let kept_at = owner
        .as_ref()
        .map(|(owner, key)| settings::memory_record_of(owner, key));
    match (owner, &kept_at) {
        (Some((owner, key)), Some(kept_at))
            if serve.shared == Some(true) && serving.sandbox && !*shared =>
        {
            let kept = Record {
                program: program.clone(),
                folder: folder.clone(),
                reads: reads.clone(),
                programs: programs.clone(),
                state: state.clone(),
                notes: write.first().map(|dir| dir.to_path_buf()),
                network,
                model: serve.model.clone(),
                argv: serve.argv.clone(),
                store: store.clone(),
                whose: agent.as_ref().map(|name| Whose {
                    owner: owner.clone(),
                    name: name.clone(),
                }),
            };
            let dir = kept_at.parent().within("no folder")?;
            link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
            link_files::write_private(kept_at, &link_files::saved::print_json(&kept)?)
                .within(format!("writing {}", kept_at.display()))?;
            let service = settings::memory_service_of(owner);
            add.extend([
                lnk,
                "agent".into(),
                "memory".into(),
                "connect".into(),
                service.display().to_string(),
                "--agent-id".into(),
                key.clone(),
            ]);
        }
        _ => {
            if let Some(kept_at) = &kept_at {
                let _ = std::fs::remove_file(kept_at);
            }
            // Its model, served outside its sandbox while it runs.
            let mut env: Vec<String> = match &serve.model {
                Some(model) => {
                    add.extend([lnk.clone(), "agent".into(), "memory".into(), "serve".into()]);
                    add.extend(["--model".into(), model.clone(), "--".into()]);
                    vec![model_placeholder()]
                }
                None => Vec::new(),
            };
            env.extend(
                store
                    .env
                    .iter()
                    .map(|(key, value)| format!("{key}={value}")),
            );
            let mut read: Vec<&Path> = vec![folder.as_path()];
            read.extend(reads.iter().map(PathBuf::as_path));
            read.extend(programs.iter().map(PathBuf::as_path));
            read.extend(store.drivers.iter().map(PathBuf::as_path));
            match serving.sandbox {
                true => {
                    add.push(lnk);
                    add.extend(confined_writing(
                        &program,
                        state,
                        &write,
                        &read,
                        &reach,
                        &env,
                        &serve.argv,
                    ));
                }
                false => {
                    add.extend(["/usr/bin/env".into(), format!("HOME={}", state.display())]);
                    add.extend(env);
                    add.extend(serve.argv.iter().cloned());
                }
            }
        }
    }
    let output = link_core::lnk()
        .args(&add)
        .current_dir(state)
        .output()
        .within("running lnk sandbox tool add")?;
    crate::confine::forget_defined_tools();
    if !output.status.success() {
        bail!(
            "lnk sandbox tool add {tool} failed: {}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    let command = add
        .iter()
        .position(|argument| argument == "--")
        .within("a managed memory definition names its command")?;
    defined.tool_argv_sha256 = Some(link_digest::sha256(&serde_json::to_vec(
        &add[command + 1..],
    )?));
    crate::defined::keep(record, &defined)?;
    Ok(tool)
}

/// Only Link Memory with an intact managed definition can attest a read.
pub(crate) fn read_fingerprint(agent: &Settings, actual: &[String]) -> Option<String> {
    if agent.memory.as_deref() != Some(settings::DEFAULT_MEMORY) {
        return None;
    }
    let kept =
        crate::defined::kept::<Definition>(&settings::config_of(&agent.name).join("memory.json"))?;
    let program = link_core::find_module(settings::memory_module_of(settings::DEFAULT_MEMORY))?;
    let metadata = std::fs::metadata(&program).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .to_string();
    let digest = link_digest::sha256(&serde_json::to_vec(actual).ok()?);
    (kept.engine == settings::DEFAULT_MEMORY
        && kept.program.canonicalize().ok()? == program.canonicalize().ok()?
        && kept.modified.as_deref() == Some(&modified)
        && kept.size == Some(metadata.len())
        && kept.version == env!("CARGO_PKG_VERSION")
        && kept.tool_argv_sha256.as_deref() == Some(&digest))
    .then_some(digest)
}

/// The programs of the Link modules `names`, by name: each where it's
/// installed, or none. A name that isn't a module's is left out.
fn programs_of(names: &[String]) -> std::collections::BTreeMap<String, Option<PathBuf>> {
    names
        .iter()
        .filter(|name| link_machine::machine::check_name(name).is_ok())
        .map(|name| {
            let program = link_core::find_module(name)
                .map(|program| program.canonicalize().unwrap_or(program));
            (name.clone(), program)
        })
        .collect()
}

/// The modules whose programs the engine ran when `record` was kept: none
/// before the first `define`.
fn recorded_programs(record: &Path) -> Vec<String> {
    crate::defined::kept::<Definition>(record)
        .map(|kept| kept.programs.into_keys().collect())
        .unwrap_or_default()
}

/// What a memory tool was last defined from, `memory.json`, naming its
/// schema ([`defined_as`]).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_argv_sha256: Option<String>,
    engine: String,
    program: PathBuf,
    modified: Option<String>,
    size: Option<u64>,
    folder: PathBuf,
    own: Option<PathBuf>,
    state: PathBuf,
    notes: Option<PathBuf>,
    /// The spec's memory table, the sandbox and where its conversations
    /// are kept, as their own types write them.
    spec: serde_json::Value,
    sandbox: serde_json::Value,
    shared: bool,
    owner: Option<String>,
    agent: Option<String>,
    store: serde_json::Value,
    programs: std::collections::BTreeMap<String, Option<PathBuf>>,
    lnk: String,
    version: String,
}

link_files::specification!(Definition, include_str!("defining/record.schema.toml"));

/// What `define` defines the memory tool from: the engine's
/// program (which a new release changes), the other modules' programs it
/// runs (one installed since changes them), its folders and notes, the
/// spec's memory table, the sandbox, where its conversations are kept,
/// and this Link. The same record, kept
/// after the last `define`, means it's defined as it would be again.
fn defined_as(
    defining: &Defining,
    programs: &std::collections::BTreeMap<String, Option<PathBuf>>,
) -> Definition {
    let Defining {
        engine,
        folder,
        own,
        state,
        notes,
        serving,
        shared,
        owner,
        agent,
        store,
        ..
    } = defining;
    let owner = owner.as_ref().map(|(owner, _)| owner.as_str());
    let program = std::fs::metadata(&engine.program).ok();
    let modified = program
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    Definition {
        tool_argv_sha256: None,
        engine: engine.name.clone(),
        program: engine.program.clone(),
        modified: modified.map(|n| n.to_string()),
        size: program.map(|m| m.len()),
        folder: folder.clone(),
        own: own.clone(),
        state: state.clone(),
        notes: notes.clone(),
        spec: serde_json::to_value(&serving.spec).unwrap_or_default(),
        sandbox: serde_json::to_value(serving.sandbox).unwrap_or_default(),
        shared: *shared,
        owner: owner.map(String::from),
        agent: agent.clone(),
        store: serde_json::to_value(store).unwrap_or_default(),
        programs: programs.clone(),
        lnk: link_core::lnk()
            .get_program()
            .to_string_lossy()
            .into_owned(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memory keeps the notes by default, though the agent's harness no
    /// longer writes its instructions; an owner who locks them locks the
    /// notes too.
    #[test]
    fn notes_are_written_unless_locked() {
        let folders = |write: &[&str], read: &[&str]| link_harness_contract::harness::Folders {
            write: write.iter().map(|n| n.to_string()).collect(),
            read: read.iter().map(|n| n.to_string()).collect(),
            teams: Default::default(),
        };
        assert!(notes_written(
            &link_harness_contract::harness::Folders::default()
        ));
        assert!(notes_written(&folders(&["instructions", "work"], &[])));
        assert!(!notes_written(&folders(&["work"], &["instructions"])));
    }

    /// The modules an engine ran are read back from its record, so one
    /// installed since changes what it's defined from; a name that isn't
    /// a module's is never looked for.
    #[test]
    fn the_programs_an_engine_runs_are_kept_in_its_record() {
        let dir = link_test_kit::TempDir::new("memory-programs");
        let record = dir.join("memory.json");
        assert!(recorded_programs(&record).is_empty());
        let kept = Definition {
            tool_argv_sha256: None,
            engine: "e".into(),
            program: "/e".into(),
            modified: None,
            size: None,
            folder: "/f".into(),
            own: None,
            state: "/s".into(),
            notes: None,
            spec: serde_json::Value::Null,
            sandbox: serde_json::Value::Null,
            shared: false,
            owner: None,
            agent: None,
            store: serde_json::Value::Null,
            programs: [("document-converter".to_string(), None)].into(),
            lnk: "lnk".into(),
            version: "0".into(),
        };
        crate::defined::keep(&record, &kept).unwrap();
        let mut previous = serde_json::to_value(&kept).unwrap();
        previous["schema"] = 1.into();
        let previous: Definition =
            link_files::saved::parse_json(&serde_json::to_vec(&previous).unwrap()).unwrap();
        assert!(previous.tool_argv_sha256.is_none());
        assert_eq!(recorded_programs(&record), ["document-converter"]);
        let programs = programs_of(&["../lnk".into(), "document-converter".into()]);
        assert_eq!(programs.keys().collect::<Vec<_>>(), ["document-converter"]);
    }
}
