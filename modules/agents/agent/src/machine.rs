//! Agent placement uses the box-owned resolver. No guest work runs here.
use anyhow::{bail, ensure};
use link_agent::{
    cli::{Command, Starting},
    moved,
    settings::Settings,
};
use link_box_contract::*;
use link_format::{Done, Exit, Name, Out, Report, WithinExt, cmd, prose, sentence};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Placement {
    created: bool,
    on: String,
    creation: CreateBox,
    binding: Option<MachineBinding>,
}

link_files::specification!(Placement, include_str!("machine.schema.toml"));
fn path(name: &str) -> anyhow::Result<PathBuf> {
    link_machine::machine::check_name(name)?;
    Ok(link_files::layout::Hidden::AgentMachines
        .try_path()?
        .join(format!("{name}.json")))
}
fn save(name: &str, placed: &Placement) -> anyhow::Result<()> {
    Ok(link_files::write_private(
        &path(name)?,
        &link_files::saved::print_json(placed)?,
    )?)
}
pub(crate) fn call(on: Option<&str>, operation: HereOperation) -> anyhow::Result<HereResponse> {
    let mut command = link_core::lnk();
    command.args(["box", "machine"]);
    if let Some(on) = on.filter(|on| *on != "here") {
        command.args(["--on", on]);
    }
    let reply: HereReply = link_service::protocol::exchange(
        command,
        &HereCall {
            version: MACHINE_PROTOCOL.current,
            operation,
        },
        Duration::from_secs(250),
    )?;
    MACHINE_PROTOCOL.accept("the VM provider", reply.version)?;
    reply.result.map_err(anyhow::Error::msg)
}
pub(crate) fn guest_upstream(name: &str) -> anyhow::Result<String> {
    // Dedicated cloud VMs keep their existing networking and do not nest.
    match call(None, HereOperation::Lookup { name: name.into() })? {
        HereResponse::NotManaged => Ok("direct".into()),
        HereResponse::Binding(binding) => Ok(if binding
            .bootstrap
            .as_ref()
            .and_then(|b| b.network.as_ref())
            .is_some()
        {
            GUEST_PROXY
        } else {
            "direct"
        }
        .into()),
        _ => bail!("managed guest has no captured runtime binding"),
    }
}
pub(crate) fn refs(starting: &Starting) -> anyhow::Result<InputReferences> {
    let read = starting
        .spec
        .as_deref()
        .map(link_spec::read_spec::<link_agent_spec::Spec>)
        .transpose()?;
    let cwd = std::env::current_dir()?;
    let home = link_files::try_home_dir()?;
    let reference =
        |flag: Option<&Path>, saved: Option<&Path>| -> anyhow::Result<Option<InputReference>> {
            if let Some(flag) = flag {
                return Ok(Some(InputReference {
                    path: link_spec::path(flag, &cwd, &home)?,
                    source: InputSource::Caller,
                }));
            }
            saved
                .map(|saved| {
                    Ok(InputReference {
                        path: link_spec::path(
                            saved,
                            &read.as_ref().within("no authoring origin")?.dir,
                            &home,
                        )?,
                        source: InputSource::Saved,
                    })
                })
                .transpose()
        };
    Ok(InputReferences {
        vm: reference(
            starting
                .given(&["isolation", "vm"])
                .as_deref()
                .map(Path::new),
            read.as_ref()
                .and_then(|r| r.spec.isolation.as_ref())
                .and_then(|isolation| isolation.vm.as_deref()),
        )?,
        environment: reference(
            starting.given(&["env"]).as_deref().map(Path::new),
            read.as_ref().and_then(|r| r.spec.env.as_deref()),
        )?,
    })
}

/// Whether a new agent here is made a VM box: on a cloud account that is
/// one machine (`accepted`), for a VM input, or where the host's owner requires one
/// (`vm_required`, asked only then). An environment alone is set up
/// natively ([`crate::environment`]), never a VM box of its own.
fn needs_vm(
    accepted: bool,
    requested: &InputReferences,
    vm_required: impl FnOnce() -> anyhow::Result<bool>,
) -> anyhow::Result<bool> {
    Ok(accepted || requested.vm.is_some() || vm_required()?)
}

/// Captures creation before setup. A retry uses the same sealed inputs and identity.
pub(crate) fn create(
    name: &str,
    on: Option<&str>,
    starting: &Starting,
) -> anyhow::Result<Option<String>> {
    let persisted = path(name)?;
    let previous: Option<Placement> = if persisted.exists() {
        Some(link_files::saved::parse_json(&std::fs::read(&persisted)?)?)
    } else {
        None
    };
    let destination = on
        .or_else(|| previous.as_ref().map(|p| p.on.as_str()))
        .unwrap_or("here")
        .to_owned();
    let accepted = previous.as_ref().map_or_else(
        || link_cloud_contract::account(&destination).is_some_and(|a| a.machine == Some(true)),
        |p| p.on != "here",
    );
    let requested = if previous.is_none() {
        refs(starting)?
    } else {
        InputReferences::default()
    };
    if destination != "here" && !accepted {
        ensure!(
            requested.vm.is_none() && requested.environment.is_none(),
            "an existing box keeps its VM and environment creation binding; select here for new inputs"
        );
        return Ok(None);
    }
    let mut placed = if let Some(placed) = previous {
        ensure!(
            placed.on == destination,
            "this agent already has a creation on {}; recover or remove it before changing placement",
            placed.on
        );
        placed
    } else {
        // The host's policy is the VM provider's, asked through Box: with
        // no provider installed, nothing requires a VM.
        let needed = needs_vm(accepted, &requested, || {
            let provider = link_core::answers::Question::<HereCall, HereReply>::new(
                QUESTION,
                QUESTION_PROTOCOL,
            );
            Ok(!provider.answerers().is_empty()
                && matches!(
                    call(None, HereOperation::Policy)?,
                    HereResponse::Policy(MachinePolicy {
                        vm_required: true,
                        ..
                    })
                ))
        })?;
        if !needed {
            return Ok(None);
        }
        ensure!(
            !Settings::path_of(name).exists(),
            "an existing native agent keeps its authority; use an explicit move into a VM box"
        );
        let HereResponse::Inputs(inputs) = call(None, HereOperation::Capture(requested))? else {
            bail!("box bridge returned no captured inputs")
        };
        let _ = call(
            accepted.then_some(destination.as_str()),
            HereOperation::Check {
                inputs: inputs.clone(),
                workload: true,
            },
        )?;
        let placed = Placement {
            created: true,
            on: destination.clone(),
            creation: CreateBox {
                name: format!("agent-{}", link_id::candidate()?),
                machine_id: link_id::candidate()?,
                inputs,
                lifetime: Lifetime::Retained,
                lease: Lease::Retained {
                    principal_expires_at: None,
                },
            },
            binding: None,
        };
        save(name, &placed)?;
        placed
    };
    let binding = if let Some(binding) = placed.binding.clone() {
        wake(
            &BoxIdentity {
                name: binding.name.clone(),
                machine_id: binding.machine_id.clone(),
            }
            .selector(),
        )?;
        binding
    } else {
        let HereResponse::Binding(binding) = call(
            accepted.then_some(destination.as_str()),
            HereOperation::Create(placed.creation.clone()),
        )?
        else {
            bail!("box bridge returned no guest binding")
        };
        binding
    };
    let selector = BoxIdentity {
        name: binding.name.clone(),
        machine_id: binding.machine_id.clone(),
    }
    .selector();
    placed.binding = Some(binding.clone());
    save(name, &placed)?;
    // Save the destination before activation; interruption never creates a native copy.
    moved::Note::new(
        None,
        name,
        moved::Place::of_box(&binding.name, Some(binding.machine_id)),
    )
    .save()?;
    let _ = call(
        None,
        HereOperation::Bootstrap {
            name: selector.clone(),
        },
    )?;
    if let Some(environment) = &binding.inputs.environment {
        let _ = call(
            None,
            HereOperation::Prepare {
                name: selector.clone(),
                environment: environment.clone(),
                materialize: true,
                start: true,
                others: false,
            },
        )?;
    }
    Ok(Some(selector))
}

pub(crate) fn captured(name: &str) -> anyhow::Result<Option<String>> {
    // A placement is saved only under a machine name, so an agent named
    // otherwise has none.
    if link_machine::machine::check_name(name).is_err() {
        return Ok(None);
    }
    let path = path(name)?;
    if Settings::path_of(name).exists() {
        return Ok(None);
    }
    if !path.exists() {
        let Some(note) = moved::of_name(name)?.filter(|n| !n.to.cloud) else {
            return Ok(None);
        };
        return match call(
            None,
            HereOperation::Lookup {
                name: note.to.name.clone(),
            },
        )? {
            HereResponse::NotManaged => Ok(None),
            HereResponse::Binding(binding) => {
                ensure!(
                    note.to.id.as_deref() == Some(binding.machine_id.as_str()),
                    "agent destination identity changed; recovery requires its original guest"
                );
                Ok(Some(
                    BoxIdentity {
                        name: binding.name,
                        machine_id: binding.machine_id,
                    }
                    .selector(),
                ))
            }
            _ => bail!("agent destination has no captured binding"),
        };
    }
    let placed: Placement = link_files::saved::parse_json(&std::fs::read(path)?)?;
    let binding = placed
        .binding
        .within("guest creation was interrupted; retry the same agent create to recover")?;
    if Settings::path_of(name).exists() {
        return Ok(None);
    }
    if let Some(note) = moved::of_name(name)?
        && (note.to.name != binding.name
            || note.to.id.as_deref() != Some(binding.machine_id.as_str()))
    {
        return Ok(None);
    }
    Ok(Some(
        BoxIdentity {
            name: binding.name,
            machine_id: binding.machine_id,
        }
        .selector(),
    ))
}

/// Whether `command` went to the agent's captured guest (or removed an
/// interrupted creation). A retained creation never follows a mutable host
/// default or reused box name.
pub(crate) fn routed(
    out: &mut Out,
    command: &Command,
    fields: &link_spec::FieldFlags,
    named: Option<&str>,
    arguments: &[std::ffi::OsString],
) -> Result<Option<Done>, Report> {
    if let Command::AgentDelete(remove) = command
        && remove_unactivated(&remove.name)?
    {
        return Ok(Some(Done::shown()));
    }
    let placed_name = match command {
        Command::AgentDelete(remove) => Some(remove.name.clone()),
        Command::AgentCreate(_)
        | Command::AgentList(_)
        | Command::AgentMove(_)
        | Command::AgentMigrate(_)
        | Command::AgentRestore(_)
        | Command::AgentReceive(_) => None,
        // A named agent may live only in its guest, with no settings here.
        // Unnamed, a command naming no single agent has no captured
        // placement; its own lookup says why.
        _ => match named {
            Some(name) => Some(name.to_owned()),
            None => Settings::resolve_without_keys(None).ok().map(|s| s.name),
        },
    };
    let Some(name) = placed_name else {
        return Ok(None);
    };
    let Some(selector) = captured(&name)? else {
        return Ok(None);
    };
    if let Command::AgentStart(_) = command
        && (fields.get(&["isolation", "vm"]).is_some() || fields.get(&["env"]).is_some())
    {
        return Err(Report::error(sentence!(
            "Couldn't give {} a VM or an environment.",
            Name::new("agent", &name)
        ))
        .why(prose!(
            "An existing agent keeps the VM and environment it was created with."
        ))
        .exit(Exit::Usage));
    }
    wake(&selector)?;
    if let Command::AgentStart(start) = command {
        return Ok(Some(crate::creating::start_in_guest(
            out,
            &selector,
            &name,
            &Starting::start(start, fields),
            start.yes,
            arguments,
        )?));
    }
    // This program's arguments never hold its namespace: `lnk agent` runs
    // it again in the guest.
    let again = std::iter::once("agent".into()).chain(arguments.iter().cloned());
    let code = forward(out, &selector, again)?;
    if code == 0 && matches!(command, Command::AgentDelete(_)) {
        remove(&name, &selector)?;
    }
    Ok(Some(Done::passed(code)))
}

pub(crate) fn wake(selector: &str) -> anyhow::Result<()> {
    let HereResponse::Binding(binding) = call(
        None,
        HereOperation::Inspect {
            name: selector.into(),
        },
    )?
    else {
        bail!("captured guest is not available")
    };
    ensure!(
        binding.state != BoxState::Removed,
        "the agent's captured guest was removed; no native fallback"
    );
    if binding.state == BoxState::Stopped {
        let _ = call(
            None,
            HereOperation::Start {
                name: selector.into(),
            },
        )?;
    }
    let _ = call(
        None,
        HereOperation::Bootstrap {
            name: selector.into(),
        },
    )?;
    if binding.state == BoxState::Stopped
        && let Some(environment) = binding.inputs.environment
    {
        let _ = call(
            None,
            HereOperation::Prepare {
                name: selector.into(),
                environment,
                materialize: false,
                start: true,
                others: false,
            },
        )?;
    }
    Ok(())
}
pub(crate) fn remove(name: &str, selector: &str) -> anyhow::Result<()> {
    let path = path(name)?;
    if path.exists() {
        let placed: Placement = link_files::saved::parse_json(&std::fs::read(&path)?)?;
        if placed.created {
            ensure!(
                matches!(call(None, HereOperation::Remove { name:selector.into() })?, HereResponse::Binding(b) if b.state == BoxState::Removed),
                "agent removed but guest cleanup is incomplete; retry removal"
            );
        }
        std::fs::remove_file(path)?;
    }
    if let Some(note) = moved::of_name(name)? {
        note.remove();
    }
    Ok(())
}

/// After a move, each placement whose agent left its guest follows it:
/// a guest its creation made is removed (never certified while cleanup is
/// incomplete), a borrowed one is kept, and the record rebinds to a
/// managed destination or goes.
pub(crate) fn left_guests() -> Result<(), Report> {
    match left_guests_inner()? {
        Some(report) => Err(report),
        None => Ok(()),
    }
}

/// [`left_guests`], a guest not yet removed given as the report to end on.
fn left_guests_inner() -> anyhow::Result<Option<Report>> {
    let dir = link_files::layout::Hidden::AgentMachines.try_path()?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(None);
    };
    for entry in entries {
        let file = entry?.path();
        let Some(name) = file
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|_| file.extension().is_some_and(|e| e == "json"))
        else {
            continue;
        };
        let placed: Placement = link_files::saved::parse_json(&std::fs::read(&file)?)?;
        let Some(binding) = &placed.binding else {
            continue;
        };
        let note = moved::of_name(name)?;
        let destination = note
            .as_ref()
            .filter(|n| !n.to.cloud)
            .map(|n| (n.to.name.clone(), n.to.id.clone()));
        if destination.as_ref().is_some_and(|(to, id)| {
            *to == binding.name && id.as_deref() == Some(&binding.machine_id)
        }) {
            continue;
        }
        if placed.created {
            let selector = BoxIdentity {
                name: binding.name.clone(),
                machine_id: binding.machine_id.clone(),
            }
            .selector();
            if !matches!(call(None, HereOperation::Remove { name: selector.clone() })?, HereResponse::Binding(b) if b.state == BoxState::Removed)
            {
                return Ok(Some(
                    Report::error(sentence!(
                        "Couldn't delete {}'s former guest {}.",
                        Name::new("agent", name),
                        Name::new("box", &selector)
                    ))
                    .why(prose!("The agent moved, but its guest isn't removed yet."))
                    .next(
                        cmd!("lnk box delete {} --yes", Name::new("box", &selector)),
                        "retry deleting it",
                    ),
                ));
            }
        }
        let rebound = match &destination {
            Some((to, id)) => match call(None, HereOperation::Lookup { name: to.clone() })? {
                HereResponse::Binding(b) if id.as_deref() == Some(b.machine_id.as_str()) => Some(b),
                _ => None,
            },
            None => None,
        };
        match rebound {
            Some(b) => save(
                name,
                &Placement {
                    created: false,
                    on: placed.on.clone(),
                    creation: CreateBox {
                        name: b.name.clone(),
                        machine_id: b.machine_id.clone(),
                        inputs: b.inputs.clone(),
                        lease: b.lease.clone(),
                        lifetime: Lifetime::Borrowed,
                    },
                    binding: Some(b),
                },
            )?,
            None => std::fs::remove_file(&file)?,
        }
    }
    Ok(None)
}

pub(crate) fn remove_unactivated(name: &str) -> anyhow::Result<bool> {
    let path = path(name)?;
    if !path.exists() {
        return Ok(false);
    }
    let placed: Placement = link_files::saved::parse_json(&std::fs::read(&path)?)?;
    if placed.binding.is_some() {
        return Ok(false);
    }
    let selector = BoxIdentity {
        name: placed.creation.name,
        machine_id: placed.creation.machine_id,
    }
    .selector();
    let response = call(None, HereOperation::Remove { name: selector })?;
    let removed = match response {
        HereResponse::Binding(b) => b.state == BoxState::Removed,
        HereResponse::Pending(p) => p.cleanup_complete,
        _ => false,
    };
    ensure!(
        removed,
        "interrupted agent guest cleanup remains incomplete; retry"
    );
    std::fs::remove_file(path)?;
    Ok(true)
}

pub(crate) fn borrow(name: &str, on: &str) -> anyhow::Result<Option<String>> {
    let binding = match call(None, HereOperation::Lookup { name: on.into() })? {
        HereResponse::NotManaged => return Ok(None),
        HereResponse::Binding(binding) => binding,
        _ => bail!("existing guest has no usable captured binding"),
    };
    let selector = BoxIdentity {
        name: binding.name.clone(),
        machine_id: binding.machine_id.clone(),
    }
    .selector();
    wake(&selector)?;
    let _ = call(
        None,
        HereOperation::Bootstrap {
            name: selector.clone(),
        },
    )?;
    save(
        name,
        &Placement {
            created: false,
            on: on.into(),
            creation: CreateBox {
                name: binding.name.clone(),
                machine_id: binding.machine_id.clone(),
                inputs: binding.inputs.clone(),
                lease: binding.lease.clone(),
                lifetime: Lifetime::Borrowed,
            },
            binding: Some(binding.clone()),
        },
    )?;
    moved::Note::new(
        None,
        name,
        moved::Place::of_box(&binding.name, Some(binding.machine_id)),
    )
    .save()?;
    Ok(Some(selector))
}

/// Controller forwarding carries no model credential or native runtime onto this host.
/// The guest's command is the person's own for its run, and its exit code
/// is this command's.
pub(crate) fn forward(
    out: &mut Out,
    selector: &str,
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> anyhow::Result<i32> {
    let mut command = link_core::lnk();
    command.args(["box", "run", selector, "--"]).args(args);
    for key in link_agent::settings::key_vars() {
        command.env_remove(key);
    }
    out.hand_over();
    let status = command
        .status()
        .within("running the complete agent inside its captured guest")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(path: &str) -> Option<InputReference> {
        Some(InputReference {
            path: path.into(),
            source: InputSource::Caller,
        })
    }

    /// An environment alone sets an agent up natively: only a VM input, a
    /// cloud account that is one machine or the host owner's policy makes a
    /// VM box.
    #[test]
    fn an_environment_alone_makes_no_vm_box() {
        let env = InputReferences {
            vm: None,
            environment: reference("/env.toml"),
        };
        let vm = InputReferences {
            vm: reference("/vm.toml"),
            environment: reference("/env.toml"),
        };
        assert!(!needs_vm(false, &env, || Ok(false)).unwrap());
        assert!(needs_vm(false, &env, || Ok(true)).unwrap());
        assert!(needs_vm(false, &vm, || Ok(false)).unwrap());
        assert!(needs_vm(true, &env, || Ok(false)).unwrap());
        // The policy is asked only when nothing else decides.
        assert!(needs_vm(false, &vm, || anyhow::bail!("not asked")).unwrap());
    }
}
