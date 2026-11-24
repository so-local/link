//! Box-owned machine bridge. Callers supply inputs; the adapter owns composition.
use anyhow::{Context, bail, ensure};
use link_box_contract::*;
use link_service::protocol;
use std::{path::Path, process::Command};

pub const PREFIX: &str = "lnk-box-here:";
pub fn serve() -> anyhow::Result<()> {
    let request: HereCall = protocol::read(std::io::stdin().lock())?;
    MACHINE_PROTOCOL.accept("the caller", request.version)?;
    let result = {
        let _progress = ProtocolOutput::divert()?;
        call(request.operation).map_err(|error| format!("{error:#}"))
    };
    let reply = HereReply {
        version: MACHINE_PROTOCOL.current,
        result,
    };
    protocol::write(link_format::streams::protocol(), &reply)?;
    Ok(())
}

/// The hidden protocol owns stdout. Human progress from nested guest commands uses stderr.
struct ProtocolOutput(std::os::fd::OwnedFd);
impl ProtocolOutput {
    fn divert() -> anyhow::Result<Self> {
        use std::os::fd::FromRawFd;
        std::io::Write::flush(&mut link_format::streams::protocol())?;
        // SAFETY: duplicate the live standard output with close-on-exec for restoration.
        let saved = unsafe { libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3) };
        ensure!(saved >= 0, "cannot retain machine protocol output");
        let saved = unsafe { std::os::fd::OwnedFd::from_raw_fd(saved) };
        ensure!(
            unsafe { libc::dup2(2, 1) } >= 0,
            "cannot separate machine progress from protocol output"
        );
        Ok(Self(saved))
    }
}
impl Drop for ProtocolOutput {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let _ = std::io::Write::flush(&mut link_format::streams::protocol());
        // SAFETY: this owned duplicate is the original standard output.
        unsafe {
            libc::dup2(self.0.as_raw_fd(), 1);
        }
    }
}

pub fn call(operation: HereOperation) -> anyhow::Result<HereResponse> {
    let operation = operation.qualified().map_err(anyhow::Error::msg)?;
    // Without a VM provider, no name is one of its boxes, none is
    // listed and nothing requires one: anything else is refused.
    match operation {
        HereOperation::Lookup { .. } if provider()?.is_none() => {
            return Ok(HereResponse::NotManaged);
        }
        HereOperation::Policy => return policy().map(HereResponse::Policy),
        HereOperation::List => {
            let (bindings, pending) = inventory()?;
            return Ok(HereResponse::Bindings { bindings, pending });
        }
        _ => {}
    }
    if let HereOperation::Create(input) = &operation {
        ensure!(
            crate::state::Boxes::load()?.get(&input.name).is_none(),
            "a cloud box already uses the requested VM box name"
        );
    }
    match operation {
        HereOperation::Capture(refs) => {
            return inputs(
                refs.vm.as_ref().map(|r| (r.path.as_path(), r.source)),
                refs.environment
                    .as_ref()
                    .map(|r| (r.path.as_path(), r.source)),
            )
            .map(HereResponse::Inputs);
        }
        HereOperation::Prepare {
            name,
            environment,
            materialize,
            start,
            others,
        } => {
            prepare(&name, &environment, materialize, start, others)?;
            return inspect(&name).map(HereResponse::Binding);
        }
        _ => {}
    }
    provide(operation)
}

/// The question a VM provider answers: one machine operation, in Box's
/// types. Box finds its provider by the question, never by its name.
pub const QUESTION: link_core::answers::Question<HereCall, HereReply> =
    link_core::answers::Question::new(
        link_box_contract::QUESTION,
        link_box_contract::QUESTION_PROTOCOL,
    );

/// The installed module that answers [`QUESTION`], once per process:
/// none when no VM provider is installed, and refused when two are.
fn provider() -> anyhow::Result<Option<link_core::answers::Answerer>> {
    static PROVIDER: std::sync::OnceLock<Vec<link_core::answers::Answerer>> =
        std::sync::OnceLock::new();
    let answerers = PROVIDER.get_or_init(|| QUESTION.answerers());
    ensure!(
        answerers.len() <= 1,
        "more than one module answers {} ({}): uninstall all but one VM provider",
        QUESTION.name,
        answerers
            .iter()
            .map(|a| a.module.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(answerers.first().cloned())
}

/// Why a VM box is refused without a provider: nothing runs in its place.
const NO_PROVIDER: &str = "no VM provider is installed (no module answers box.machine), so no VM box is made, \
     reached or removed, and nothing is made in its place";

/// One operation for the VM provider, which Box runs and never links:
/// without one installed, a VM box is refused here.
fn provide(operation: HereOperation) -> anyhow::Result<HereResponse> {
    let provider = provider()?.context(NO_PROVIDER)?;
    let call = HereCall {
        version: MACHINE_PROTOCOL.current,
        operation,
    };
    let answered = protocol::exchange(
        provider.command(),
        &QUESTION.asked(&call),
        std::time::Duration::from_secs(240),
    )
    .with_context(|| {
        format!(
            "the VM provider ({} module) gave no answer",
            provider.module
        )
    })?;
    let reply = QUESTION.answer_of(&provider.module, answered)?;
    MACHINE_PROTOCOL.accept(
        &format!("the VM provider ({} module)", provider.module),
        reply.version,
    )?;
    reply.result.map_err(anyhow::Error::msg)
}

pub fn inputs(
    vm: Option<(&Path, InputSource)>,
    env: Option<(&Path, InputSource)>,
) -> anyhow::Result<MachineInputs> {
    // The VM provider reads its own spec; Box keeps what it captured.
    let vm = vm
        .map(|(path, source)| {
            match provide(HereOperation::Capture(InputReferences {
                vm: Some(InputReference {
                    path: path.to_path_buf(),
                    source,
                }),
                environment: None,
            }))? {
                HereResponse::Inputs(MachineInputs { vm: Some(vm), .. }) => Ok(vm),
                _ => bail!("the VM provider captured no VM input"),
            }
        })
        .transpose()?;
    let environment = env
        .map(|(path, source)| {
            let read = link_env_contract::file::read(path)?;
            let packed = link_env_contract::environment::pack(&read, "")?;
            Ok::<_, anyhow::Error>(EnvironmentInput {
                provenance: Provenance {
                    source,
                    reference: read.path.display().to_string(),
                    digest: link_digest::content_digest(&packed.files)?,
                },
                file: packed.file,
                files: packed.files,
            })
        })
        .transpose()?;
    Ok(MachineInputs { vm, environment })
}

/// A box's machine inputs, captured once for every placement: each flag
/// replaces its saved sibling, and each reference resolves at its
/// authoring origin, a flag's at the working folder and a saved one's
/// beside its spec.
pub fn captured(
    made: &link_spec::Made<crate::spec::BoxSpec>,
    flags: &link_spec::FieldFlags,
) -> anyhow::Result<MachineInputs> {
    // A path a field flag gave is the caller's; one the file gave, the
    // file's own. Both are absolute already.
    let source = |path: &[&str]| match flags.get(path) {
        Some(_) => InputSource::Caller,
        None => InputSource::Saved,
    };
    let vm = made
        .spec
        .isolation
        .as_ref()
        .and_then(|isolation| isolation.vm.as_deref())
        .map(|path| (path, source(&["isolation", "vm"])));
    let env = made
        .spec
        .env
        .as_deref()
        .map(|path| (path, source(&["env"])));
    inputs(vm, env)
}

/// A captured environment's files, staged and read back: the env
/// contract's, which Agent stages its environments with too.
pub use link_env_contract::environment::{
    Staged as StagedEnvironment, staged as staged_environment,
};

pub fn create(
    name: &str,
    inputs: MachineInputs,
    lifetime: Lifetime,
    lease: Lease,
) -> anyhow::Result<MachineBinding> {
    crate::state::check_name(name)?;
    ensure!(
        crate::state::Boxes::load()?.get(name).is_none(),
        "a cloud box named {name} exists already"
    );
    binding(call(HereOperation::Create(CreateBox {
        name: name.into(),
        machine_id: link_id::candidate()?,
        inputs,
        lifetime,
        lease,
    }))?)
}

pub fn binding(response: HereResponse) -> anyhow::Result<MachineBinding> {
    match response {
        HereResponse::Binding(binding) => Ok(binding),
        _ => bail!("adapter returned no box binding"),
    }
}

/// Whether `name` is a VM box: its provider's, by its own answer. With
/// no provider installed, no name is.
pub fn is_here(name: &str) -> bool {
    let (base, _) = BoxIdentity::split(name);
    crate::state::check_name(base).is_ok()
        && matches!(provider(), Ok(Some(_)))
        && matches!(
            provide(HereOperation::Lookup { name: name.into() }),
            Ok(HereResponse::Binding(_) | HereResponse::Pending(_))
        )
}

/// The VM provider's inventory: nothing when none is installed.
fn inventory() -> anyhow::Result<(Vec<MachineBinding>, Vec<PendingBox>)> {
    if provider()?.is_none() {
        return Ok(Default::default());
    }
    match provide(HereOperation::List)? {
        HereResponse::Bindings { bindings, pending } => Ok((bindings, pending)),
        _ => bail!("adapter returned no box inventory"),
    }
}

fn here_bindings() -> anyhow::Result<Vec<MachineBinding>> {
    Ok(inventory()?
        .0
        .into_iter()
        .filter(|binding| binding.state != BoxState::Removed)
        .collect())
}

fn here_pending() -> anyhow::Result<Vec<PendingBox>> {
    Ok(inventory()?
        .1
        .into_iter()
        .filter(|pending| !pending.cleanup_complete)
        .collect())
}

pub fn inspect(name: &str) -> anyhow::Result<MachineBinding> {
    binding(call(HereOperation::Inspect { name: name.into() })?)
}

pub fn bindings() -> anyhow::Result<Vec<MachineBinding>> {
    here_bindings()
}

pub fn pending() -> anyhow::Result<Vec<PendingBox>> {
    here_pending()
}

pub fn bootstrap(name: &str) -> anyhow::Result<MachineBinding> {
    let record = inspect(name)?;
    if record.readiness.link && record.readiness.sandbox {
        return Ok(record);
    }
    binding(call(HereOperation::Bootstrap { name: name.into() })?)
}

pub fn ssh(name: &str, options: &[&str]) -> anyhow::Result<Command> {
    ssh_forwarding(name, options, false)
}
fn ssh_forwarding(name: &str, options: &[&str], forwarding: bool) -> anyhow::Result<Command> {
    let HereResponse::Access(access) = call(HereOperation::Access { name: name.into() })? else {
        bail!("adapter returned no authenticated guest access");
    };
    // The provider says what carries the connection; Box runs it with
    // this `lnk`.
    let lnk = link_core::lnk();
    let proxy = std::iter::once(lnk.get_program().to_string_lossy().into_owned())
        .chain(access.proxy.iter().cloned())
        .map(|word| crate::setup::quote(&word))
        .collect::<Vec<_>>()
        .join(" ")
        .replace('%', "%%");
    let mut command = link_secretive::process::command(
        std::env::var_os("LNK_SSH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "ssh".into()),
    );
    command
        .args(["-F", "/dev/null", "-i"])
        .arg(access.private_identity)
        .args(["-o", "IdentityAgent=none", "-o", "IdentitiesOnly=yes"])
        .args(["-o", "ForwardAgent=no", "-o", "ForwardX11=no"])
        .args([
            "-o",
            "PermitLocalCommand=no",
            "-o",
            if forwarding {
                "ClearAllForwardings=no"
            } else {
                "ClearAllForwardings=yes"
            },
        ])
        .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes"])
        .args(["-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=15"])
        .arg("-o")
        .arg(format!("ProxyCommand={proxy}"))
        .arg("-o")
        .arg(format!("HostKeyAlias={}", access.host_alias))
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile=\"{}\"",
            access.known_hosts.display().to_string().replace('%', "%%")
        ))
        .args(options)
        .arg("--")
        .arg(format!("{}@{}", access.user, access.host_alias));
    Ok(command)
}

/// Only a captured, approved filtering socket can be forwarded. This does not
/// enable the generic box-forward operation, host routes or agent forwarding.
pub fn network_forward(name: &str) -> anyhow::Result<std::process::ExitStatus> {
    use std::os::unix::fs::FileTypeExt;
    let binding = inspect(name)?;
    ensure!(
        binding
            .bootstrap
            .as_ref()
            .and_then(|b| b.network.as_ref())
            .is_some(),
        "guest has no approved filtering bridge"
    );
    // The provider says where its approved socket is.
    let HereResponse::Access(access) = call(HereOperation::Access { name: name.into() })? else {
        bail!("adapter returned no authenticated guest access");
    };
    let socket = access
        .network
        .context("the VM provider named no approved proxy socket")?;
    ensure!(
        socket.is_absolute() && std::fs::symlink_metadata(&socket)?.file_type().is_socket(),
        "approved proxy socket is not a Unix socket"
    );
    let mapping = format!("127.0.0.1:{GUEST_PROXY_PORT}:{}", socket.display());
    ssh_forwarding(
        name,
        &[
            "-T",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveCountMax=3",
            "-R",
            &mapping,
        ],
        true,
    )?
    .arg("printf '{\"ready\":true}\\n'; exec sleep 2147483647")
    .stdin(std::process::Stdio::null())
    .status()
    .context("opening approved guest filtering bridge")
}

/// Environment preparation uses captured files and never selects an image recursively.
pub fn prepare(
    name: &str,
    input: &EnvironmentInput,
    materialize: bool,
    start: bool,
    others: bool,
) -> anyhow::Result<()> {
    let record = inspect(name)?;
    let machine = summary(&record)?;
    let (_staged, read) = staged_environment(input)?;
    let login = guest_login(&machine.target, &machine.user, read.file.os)?;
    link_env_contract::remote::prepare_private(name, &read, login, materialize, start, others)
        .map_err(crate::said::said)?;
    call(HereOperation::EnvironmentReady {
        name: name.into(),
        generation: record.handle.as_ref().context("guest handle")?.generation(),
        intent_revision: record.intent_revision,
        digest: input.provenance.digest.clone(),
    })?;
    Ok(())
}

/// How a guest of `target`, logging in as `user`, takes an environment
/// for `os`: a Linux guest of that release through its root login, a
/// macOS guest through its approved ordinary user, never root, whatever
/// its release.
fn guest_login<'a>(
    target: &GuestTarget,
    user: &'a str,
    os: Option<link_machine::machine::Os>,
) -> anyhow::Result<link_env_contract::remote::GuestLogin<'a>> {
    use link_env_contract::remote::GuestLogin;
    let macos = target.os == GuestOs::Macos;
    if let Some(os) = os {
        ensure!(
            if macos {
                os == link_machine::machine::Os::Macos
            } else {
                os.word() == target.release
            },
            "environment OS conflicts with selected guest"
        );
    }
    Ok(if macos {
        GuestLogin::Ordinary(user)
    } else {
        GuestLogin::Root
    })
}

/// The provider's description of a box, which every reply carries.
fn summary(binding: &MachineBinding) -> anyhow::Result<&MachineSummary> {
    binding
        .machine
        .as_ref()
        .context("the VM provider described no machine")
}

pub fn listed(binding: &MachineBinding) -> crate::state::Machine {
    let machine = binding.machine.as_ref();
    crate::state::Machine {
        price: None,
        deadline: binding.lease.deadline().map(|ms| ms / 1000),
        name: binding.name.clone(),
        machine_id: binding.machine_id.clone(),
        account: "here".into(),
        kind: "here".into(),
        place: machine.map_or_else(|| "here".into(), |m| m.place.clone()),
        id: binding
            .handle
            .as_ref()
            .map(|handle| handle.id())
            .unwrap_or_else(|| binding.request_id.to_string()),
        size: machine.map_or_else(String::new, |m| format!("{} vCPU", m.vcpus)),
        ip: None,
        state: match binding.state {
            BoxState::Ready => "running",
            BoxState::Stopped => "stopped",
            BoxState::Stopping => "stopping",
            BoxState::Removed => "gone",
            BoxState::Creating => "starting",
            BoxState::CleanupPending => "cleanup-pending",
            BoxState::Failed => "failed",
        }
        .into(),
        created: binding.created_at / 1000,
        host_keys: Vec::new(),
        tagged: true,
        from_cloud: false,
        spec: None,
        spot: false,
        stopped_by_cloud: false,
        stop_guessed: false,
        certified: false,
        environment: None,
        here: None,
        staged: false,
    }
}

pub fn remove(name: &str) -> anyhow::Result<()> {
    match call(HereOperation::Remove { name: name.into() })? {
        HereResponse::Binding(binding) if binding.state == BoxState::Removed => Ok(()),
        HereResponse::Pending(pending) if pending.cleanup_complete => Ok(()),
        _ => bail!("VM box cleanup is incomplete"),
    }
}

pub fn listed_pending(pending: &PendingBox) -> crate::state::Machine {
    crate::state::Machine {
        price: None,
        deadline: pending.input.lease.deadline().map(|time| time / 1000),
        name: pending.input.name.clone(),
        machine_id: pending.input.machine_id.clone(),
        account: "here".into(),
        kind: "here".into(),
        place: "here".into(),
        id: format!("box-{}", pending.input.machine_id),
        size: String::new(),
        ip: None,
        state: if pending.cancelled {
            "cleanup-pending"
        } else {
            "starting"
        }
        .into(),
        created: 0,
        host_keys: Vec::new(),
        tagged: true,
        from_cloud: false,
        spec: None,
        spot: false,
        stopped_by_cloud: false,
        stop_guessed: false,
        certified: false,
        environment: None,
        here: None,
        staged: false,
    }
}

/// The host owner's policy, the VM provider's: with none installed,
/// nothing on this machine requires a VM.
pub fn policy() -> anyhow::Result<MachinePolicy> {
    if provider()?.is_none() {
        return Ok(MachinePolicy {
            revision: 0,
            vm_required: false,
        });
    }
    match provide(HereOperation::Policy)? {
        HereResponse::Policy(policy) => Ok(policy),
        _ => bail!("adapter returned no machine policy"),
    }
}

#[cfg(test)]
mod tests {

    /// The question this owns declares its protocol beside it.

    #[test]

    fn its_question_declares_its_protocol() {
        let protocol = super::QUESTION.protocol();

        assert!(protocol.oldest <= protocol.current, "{protocol:?}");
    }
    use link_box_contract::{GuestArch, GuestOs, GuestTarget};
    use link_env_contract::remote::GuestLogin;
    use link_machine::machine::Os;

    #[test]
    fn a_macos_guest_takes_its_environment_as_its_ordinary_user_and_linux_as_root() {
        let mac = GuestTarget {
            os: GuestOs::Macos,
            arch: GuestArch::Aarch64,
            release: "15.0".into(),
        };
        let login = |target: &GuestTarget, os| super::guest_login(target, "builder", os);
        assert_eq!(
            login(&mac, Some(Os::Macos)).unwrap(),
            GuestLogin::Ordinary("builder")
        );
        assert_eq!(login(&mac, None).unwrap(), GuestLogin::Ordinary("builder"));
        assert!(login(&mac, Some(Os::Ubuntu2404)).is_err());
        let linux = GuestTarget {
            os: GuestOs::Linux,
            arch: GuestArch::X86_64,
            release: "ubuntu-24.04".into(),
        };
        assert_eq!(
            login(&linux, Some(Os::Ubuntu2404)).unwrap(),
            GuestLogin::Root
        );
        assert!(login(&linux, Some(Os::Macos)).is_err());
    }
}
