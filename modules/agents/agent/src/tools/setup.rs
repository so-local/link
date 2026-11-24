//! A tool's module is installed and its missing grants are proposed and
//! confirmed before it is enabled, so the agent restarts with it whole.
use super::*;
use link_actions::value::Words;
use link_format::{Phrase, phrase};
use link_sandbox_client::{
    Spec,
    spec::{Commands, Files},
};
use link_sandbox_contract::toolchains::Toolchain;
mod approval;

pub(super) enum Prepared {
    Unchanged,
    Changed,
    Cancelled,
}

/// What `run` may start, as `lnk agent tool add run` offers it when its
/// sandbox file allows no program yet (`run_choices.toml`): toolchains,
/// the first the default.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    name: String,
    grants: Grants,
}

/// What a choice grants, by its kind.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Grants {
    /// These toolchains.
    Toolchains { toolchains: Vec<Toolchain> },
    /// The toolchains Link's own sandbox template grants.
    Template,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Choices {
    choice: Vec<Choice>,
}

fn choices() -> Vec<Choice> {
    toml::from_str::<Choices>(include_str!("run_choices.toml"))
        .expect("the built-in run choices are valid")
        .choice
}

impl Choice {
    /// The toolchains it grants.
    fn toolchains(&self) -> Vec<Toolchain> {
        match &self.grants {
            Grants::Toolchains { toolchains } => toolchains.clone(),
            Grants::Template => Sandbox::link()
                .commands()
                .map(|commands| commands.toolchains)
                .unwrap_or_default(),
        }
    }

    /// Its toolchains, each with what it is, for a person choosing.
    fn about(&self) -> Phrase {
        let about: Vec<String> = self
            .toolchains()
            .iter()
            .map(|toolchain| match toolchain.meaning() {
                Some(meaning) => format!("{} ({meaning})", toolchain.word()),
                None => toolchain.word().to_string(),
            })
            .collect();
        phrase!("{}", Value::from(about.join("; ")))
    }
}

/// What `toolchains` let the agent run, each toolchain with the programs
/// of its this machine has (`present`), for the proposal to name.
fn runs(toolchains: &[Toolchain], present: impl Fn(&str) -> bool) -> String {
    let named: Vec<String> = toolchains
        .iter()
        .map(|toolchain| {
            let programs: Vec<&str> = link_sandbox_contract::toolchains::named(*toolchain)
                .programs
                .iter()
                .map(String::as_str)
                .filter(|program| present(program))
                .collect();
            format!("{} ({})", toolchain.word(), programs.join(", "))
        })
        .collect();
    format!("It lets the agent run the toolchains {}.", named.join("; "))
}

/// The module that serves `tool`, for those that have one of their own.
fn module(tool: &str) -> Option<&'static str> {
    match tool {
        "run" => Some(run_file::RUN),
        "file" => Some(run_file::FILE),
        "code" => Some(run_file::CODE),
        "scheduler" => Some(crate::scheduler::MODULE),
        _ => None,
    }
}

/// `tool`'s module, installed now if it's missing, before the agent
/// restarts with the tool: a restart before it is there starts the agent
/// without it.
pub(super) fn install(tool: &str) -> Result<(), Report> {
    let Some(module) = module(tool) else {
        return Ok(());
    };
    if link_core::find_module(module).is_some() {
        return Ok(());
    }
    let installed = match link_core::RELEASED {
        true => link_core::modules::add(module, "Adding the tool").map(|_| ()),
        false => Err(anyhow::anyhow!(
            "lnk-{module} isn't beside lnk: this lnk is built from source, so build Link's tools too"
        )),
    };
    installed.map_err(|err| {
        Report::error(sentence!(
            "{} wasn't added: its module isn't installed.",
            Name::new("tool", tool)
        ))
        .why(Sentence::adopt(&format!("{err:#}")))
        .next(
            cmd!("lnk module add {}", Name::new("module", module)),
            "install it",
        )
    })
}

pub(super) fn prepare(
    out: &mut Out,
    agent: &Settings,
    tool: &str,
    yes: bool,
) -> Result<Prepared, Report> {
    let sandbox = sandbox_file::load(&agent.name)?;
    let spec = agent.spec()?;
    let path = link_agent::programs::path(agent);
    let offered = choices();
    let chosen = match tool == "run" && allows_nothing(&sandbox) && !yes && out.can_ask() {
        true => {
            let options = offered
                .iter()
                .map(|choice| (Value::from(choice.name.as_str()), choice.about()))
                .collect();
            out.ask(
                Question::choose(
                    sentence!("Which toolchains may {} start?", Name::new("tool", tool)),
                    options,
                )
                .default(0),
            )?
        }
        false => 0,
    };
    let present =
        |program: &str| link_harness_contract::harness::find_program(program, &path).is_some();
    let needed = minimum(tool, &sandbox, &spec.spec, &offered[chosen], present)?;
    if needed.items().is_empty() {
        return Ok(Prepared::Unchanged);
    }
    let flags = link_sandbox_client::flags::shown(&needed);
    let runs = match needed.commands.as_ref() {
        Some(commands) => match (&commands.toolchains, &commands.allow) {
            (Some(toolchains), _) => runs(toolchains, present),
            (None, Some(allow)) => format!("It lets the agent run {}.", allow.join(", ")),
            (None, None) => String::new(),
        },
        None => String::new(),
    };
    let mut replacement = sandbox.clone();
    replacement.spec = replacement.spec.join(&needed);
    let approval = approval::GrantsApproval::new(agent, tool, &sandbox, &replacement)?;
    let question = Question::confirm(
        sentence!(
            "Allow this sandbox access and add {}?",
            Name::new("tool", tool)
        ),
        false,
    )
    .flag("--yes")
    .why(prose!(
        "Adding this tool needs {} in {}.{} Its work folder is {}. {}.",
        flags.as_str(),
        Path::new(sandbox_file::path(&agent.name)),
        match runs.is_empty() {
            true => String::new(),
            false => format!(" {runs}"),
        },
        Path::new(agent.files().join("work")),
        approval.review().trim_end_matches('.').to_string()
    ));
    let approved = if yes {
        out.block(Note(sentence!(
            "Approving the proposed sandbox access for {}: {}.",
            Name::new("tool", tool),
            flags.as_str()
        )));
        if !runs.is_empty() {
            out.block(Note(Sentence::adopt(&runs)));
        }
        true
    } else {
        out.ask(question)?
    };
    approval.decide(approved)?;
    if !approved {
        out.block(Note(sentence!(
            "The tool and its sandbox permissions are unchanged."
        )));
        return Ok(Prepared::Cancelled);
    }
    approval.apply(agent, &sandbox, &replacement)?;
    Ok(Prepared::Changed)
}

/// Whether `sandbox` lets `run` start no program yet.
fn allows_nothing(sandbox: &Sandbox) -> bool {
    sandbox
        .commands()
        .map(|commands| commands.present(sandbox_file::PATH))
        .unwrap_or_default()
        .is_empty()
}

fn minimum(
    tool: &str,
    sandbox: &Sandbox,
    behavior: &link_agent_spec::Spec,
    choice: &Choice,
    present: impl Fn(&str) -> bool,
) -> anyhow::Result<Spec> {
    let mut needed = Spec::default();
    if matches!(tool, "file" | "run") && sandbox.folders().write.is_empty() {
        needed.files = Some(Files {
            write: Some(settings::default_files().write),
            ..Default::default()
        });
    }
    let allowed = sandbox
        .commands()
        .map(|commands| commands.present(sandbox_file::PATH))
        .unwrap_or_default();
    if tool == "run" && allowed.is_empty() {
        needed.commands = Some(Commands {
            toolchains: Some(choice.toolchains()),
            ..Default::default()
        });
        return Ok(needed);
    }
    let programs = match tool {
        "code" => {
            let servers = behavior.tool("code").servers;
            let available: Vec<_> = servers
                .values()
                .filter_map(|server| server.command().first())
                .filter(|program| present(program))
                .cloned()
                .collect();
            anyhow::ensure!(
                !available.is_empty(),
                "No configured language server is installed. Install a server listed by lnk agent spec show, then add code again."
            );
            available
                .into_iter()
                .filter(|program| !allowed.contains(program))
                .collect()
        }
        _ => Vec::new(),
    };
    if !programs.is_empty() {
        needed.commands = Some(Commands {
            allow: Some(programs),
            ..Default::default()
        });
    }
    Ok(needed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Choice {
        choices().remove(0)
    }

    #[test]
    fn a_file_tool_proposes_links_folders_and_reuses_existing_grants() {
        let needed = minimum(
            "file",
            &Sandbox::none(),
            &link_agent_spec::link(),
            &shell(),
            |_| false,
        )
        .unwrap();
        assert_eq!(
            needed.files.unwrap().write.unwrap(),
            ["work", "instructions", "skills"]
        );
        assert!(needed.internet.is_none() && needed.commands.is_none());
        assert!(
            minimum(
                "file",
                &Sandbox::link(),
                &link_agent_spec::link(),
                &shell(),
                |_| false
            )
            .unwrap()
            .items()
            .is_empty()
        );
    }

    /// Every choice names declared toolchains, never `*`: the first the
    /// shell's, and the template's the toolchains Link's own grants.
    #[test]
    fn run_offers_declared_toolchains_the_shell_first() {
        let offered = choices();
        assert_eq!(offered[0].toolchains(), [Toolchain::Shell]);
        for choice in &offered {
            assert!(
                !choice.toolchains().is_empty(),
                "{} grants nothing",
                choice.name
            );
        }
        let template = offered
            .iter()
            .find(|choice| choice.grants == Grants::Template)
            .unwrap();
        assert_eq!(
            template.toolchains(),
            Sandbox::link().commands().unwrap().toolchains
        );
    }

    #[test]
    fn run_and_code_propose_programs_without_opening_the_network() {
        let run = minimum(
            "run",
            &Sandbox::none(),
            &link_agent_spec::link(),
            &shell(),
            |_| false,
        )
        .unwrap();
        let commands = run.commands.unwrap();
        assert_eq!(commands.toolchains.unwrap(), [Toolchain::Shell]);
        assert!(commands.allow.is_none() && run.internet.is_none());
        // The proposal names each toolchain's programs this machine has,
        // `date` among the shell's.
        let said = runs(&[Toolchain::Shell], |program| program == "date");
        assert_eq!(said, "It lets the agent run the toolchains shell (date).");
        let spec = link_agent_spec::parse("schema = 1\n[tool.code.servers.rust]\ncommand = [\"rust-analyzer\"]\nfiles = [\"rs\"]\nroots = [\"Cargo.toml\"]\n").unwrap();
        let code = minimum("code", &Sandbox::none(), &spec, &shell(), |program| {
            program == "rust-analyzer"
        })
        .unwrap();
        assert_eq!(code.commands.unwrap().allow.unwrap(), ["rust-analyzer"]);
        assert!(code.internet.is_none() && code.files.is_none());
        assert!(minimum("code", &Sandbox::none(), &spec, &shell(), |_| false).is_err());
    }
}
