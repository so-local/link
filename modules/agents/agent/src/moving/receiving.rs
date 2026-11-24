//! The end an agent arrives at: `lnk agent receive`, the stream
//! unpacked into staging, then put in place and started.

use link_format::WithinExt;
use std::io::BufRead;
use std::process::Stdio;

use anyhow::{bail, ensure};
use link_agent::adapter::Adapter;
use link_agent::bridge;
use link_agent::grants::Grants;
use link_agent::hosted;
use link_agent::moved;
use link_agent::sandbox_file::{self};
use link_agent::service;
use link_agent::settings::{self, Settings};
use link_agent::transfer::{self, Have, Phase};
use link_format::{Foreign, Name, Note, Out, Report, Sentence, cmd, prose, sentence};
use link_harness_contract::harness::{Model, State};
use link_secretive::Duplicate;
use serde::Deserialize;

use super::buckets::adopt_buckets;
use super::machines::box_machine;
use super::sending::{abort, have_at, receiver, said};
use super::{Found, Header, hosted_place, lnk_at, place, state_of};

/// The receiver of a move: a machine's receive or an account's agent-receive.
/// Sources use the same copy, pause, switch and acknowledgement sequence.
pub(crate) enum Target {
    Machine(Option<String>),
    Hosted {
        managed_models: bool,
        account: Box<link_agent_place::Account>,
        seal_to: link_secretive::SealTo,
        move_id: String,
        have: Have,
        owner: Option<String>,
        finished: std::cell::Cell<bool>,
    },
}
impl Target {
    pub(crate) fn hosted(found: &Found, id: &str, account: &str) -> anyhow::Result<Self> {
        let account =
            link_agent_place::account(account).within("the hosted account isn't connected")?;
        let owner = found
            .here
            .as_ref()
            .and_then(|s| s.owner.clone())
            .or_else(link_core::owner::account_owner);
        let size = found
            .here
            .as_ref()
            .map(|settings| -> anyhow::Result<u64> {
                let index = settings
                    .memory
                    .as_ref()
                    .map(|engine| settings.memory_state(engine).join(transfer::INDEX));
                Ok(folder_size(&settings.folder().home)
                    + transfer::collection_bytes(settings)?
                    + index.map_or(0, |p| folder_size(&p)))
            })
            .transpose()?;
        let reply = hosted::call(&account, "agent-in", |r| {
            r.agent = Some(link_agent_place::HostedAgent {
                id: id.into(),
                name: found.name.clone(),
                owner: owner.clone(),
                ..Default::default()
            });
            r.size = size;
        })?;
        hosted::unless_refused(&account.name, &reply)?;
        let move_id = hosted::move_of(&reply, "agent-in")?;
        let managed_models = reply.manages_models();
        let have = reply.have;
        let seal_to = reply
            .seal_to
            .within("the hosting account supplied no per-move receiver: upgrade its adapter")?;
        if let Err(error) = link_agent_contract::moving::check_have(&have) {
            abort(&account, &move_id);
            return Err(error);
        }
        Ok(Self::Hosted {
            managed_models,
            account: Box::new(account),
            seal_to,
            move_id,
            have,
            owner,
            finished: std::cell::Cell::new(false),
        })
    }
    pub(crate) fn is_hosted(&self) -> bool {
        matches!(self, Self::Hosted { .. })
    }
    pub(crate) fn place(&self) -> String {
        match self {
            Self::Machine(at) => place(at),
            Self::Hosted { account, .. } => hosted_place(&account.name),
        }
    }
    pub(crate) fn have(&self, id: &str) -> anyhow::Result<Have> {
        match self {
            Self::Machine(at) => have_at(at, id),
            Self::Hosted { have, .. } => Ok(have.clone()),
        }
    }
    pub(crate) fn first_copy(&self) -> Option<&str> {
        (!self.is_hosted()).then_some("{}")
    }
    /// What takes agent `id`'s `phase` there.
    pub(crate) fn receiver(&self, id: &str, phase: Phase) -> anyhow::Result<std::process::Child> {
        match self {
            Self::Machine(at) => receiver(at, id, phase, Stdio::piped()),
            Self::Hosted {
                account, move_id, ..
            } => link_agent_place::spawn(
                &account.kind,
                "agent-receive",
                &link_agent_place::Request {
                    account: account.as_ref().duplicate(),
                    r#move: Some(move_id.clone()),
                    phase: Some(phase.name().into()),
                    ..Default::default()
                },
            ),
        }
    }
    pub(crate) fn copied(&self, output: &[u8]) -> anyhow::Result<Have> {
        let have = match self {
            Self::Machine(_) => link_agent_contract::moving::read_have(output)?,
            Self::Hosted { .. } => serde_json::from_slice::<link_agent_place::Reply>(output)?.have,
        };
        link_agent_contract::moving::check_have(&have)?;
        Ok(have)
    }
    pub(crate) fn keys_at(&self) -> Option<moved::Place> {
        match self {
            Self::Machine(Some(name)) => Some(moved::Place::of_box(name, box_machine(name))),
            Self::Machine(None) => None,
            Self::Hosted { account, owner, .. } => {
                Some(moved::Place::of_account(&account.name, owner.clone()))
            }
        }
    }
    /// For an account, its move ended (`agent-abort`), told once: what to
    /// say when it doesn't say so, as a machine's always does.
    fn try_abort(&self) -> Option<Report> {
        match self {
            Self::Hosted {
                account,
                move_id,
                finished,
                ..
            } if !finished.replace(true) => abort(account, move_id),
            _ => None,
        }
    }
    /// [`Self::try_abort`], said: whether it did.
    pub(crate) fn abort(&self, out: &mut Out) -> bool {
        said(out, self.try_abort())
    }
    pub(crate) fn complete(&self) {
        if let Self::Hosted { finished, .. } = self {
            finished.set(true);
        }
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        // Cleaning up: nobody to tell, as the move ended another way.
        let _ = self.try_abort();
    }
}

/// The bytes of the files under `path`, never following a link: each
/// name read from its folder's handle, and no deeper than
/// [`link_files::nofollow::DEEPEST`].
pub(crate) fn folder_size(path: &std::path::Path) -> u64 {
    use link_files::nofollow::{DEEPEST, Dir, Kind};
    fn under(dir: &Dir, depth: usize) -> u64 {
        dir.names()
            .unwrap_or_default()
            .iter()
            .map(|name| match dir.stat(name) {
                Ok(stat) if stat.kind == Kind::File => stat.size,
                Ok(stat) if stat.kind == Kind::Folder && depth < DEEPEST => dir
                    .folder(name)
                    .map_or(0, |inside| under(&inside, depth + 1)),
                _ => 0,
            })
            .sum()
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(meta) if meta.is_dir() => Dir::open_unlinked(path).map_or(0, |dir| under(&dir, 1)),
        _ => 0,
    }
}

/// What a box's `lnk agent needs` answers: its harness, what it needs
/// there and hasn't been answered, and the agents there that run it.
#[derive(Deserialize)]
pub(crate) struct Pending {
    harness: String,
    needs: Vec<(String, String)>,
    #[serde(default)]
    running: Vec<String>,
}

impl Pending {
    /// An error unless every name in it is one Link knows: the box's
    /// answer is shown on this terminal.
    fn check(&self) -> anyhow::Result<()> {
        link_agent::grants::check_name(&self.harness)?;
        for (need, why) in &self.needs {
            // A risk's name, or a spec's flags: one line Link shows as it
            // is, which the box checks again when it's allowed.
            ensure!(
                need != "unsandboxed"
                    && !need.is_empty()
                    && need.len() <= 1024
                    && *need == link_format::clean(need)
                    && !need.chars().any(char::is_control)
                    && !why.chars().any(char::is_control),
                "the box answered a need Link doesn't show: {}",
                link_format::clean(need)
            );
        }
        for agent in &self.running {
            link_machine::machine::check_name(agent)?;
        }
        Ok(())
    }
}

/// What agent `name` (`id`) needs there (`lnk agent needs` where it came,
/// `to`), asked in this process, which has the terminal, and granted
/// there; then it starts there. An error, changing nothing more, when
/// what it needs isn't allowed or it doesn't start.
pub(crate) fn settle_and_start(
    out: &mut Out,
    name: &str,
    id: &str,
    from: &str,
    to: &Option<String>,
) -> anyhow::Result<()> {
    let output = lnk_at(to, name, &["needs"])
        .stdin(Stdio::null())
        .output()
        .within("asking what its harness needs there")?;
    ensure!(
        output.status.success(),
        "asking what its harness needs on {} failed: {}",
        place(to),
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    let pending: Pending =
        serde_json::from_slice(&output.stdout).within("reading what its harness needs there")?;
    pending.check()?;
    if !pending.needs.is_empty() {
        crate::lossy(crate::permissions::ask_needs(
            out,
            &pending.harness,
            &pending.needs,
            Some(&pending.running),
        ))?;
        // `--allow=<need>`: a need may be a spec's flags, `--network.mode
        // host`, which is the flag's value, not a flag of its own.
        let allows: Vec<String> = pending
            .needs
            .iter()
            .map(|(need, _)| format!("--add={need}"))
            .collect();
        let mut args = vec!["needs"];
        args.extend(allows.iter().map(String::as_str));
        let allowed = lnk_at(to, name, &args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .within("allowing what its harness needs there")?;
        ensure!(
            allowed.success(),
            "allowing what {} needs on {} failed",
            pending.harness,
            place(to)
        );
    }
    out.hand_over();
    let started = lnk_at(to, name, &["start"])
        .stdin(Stdio::null())
        .status()
        .within("starting your agent there")?;
    ensure!(
        started.success(),
        "{name} ({id}) didn't start on {}, coming from {from}",
        place(to),
    );
    Ok(())
}

/// `lnk agent permission add` of `flags`, as a sandbox file's grant is said: a flag
/// and perhaps its value, quoted (`--files.read '~'`).
fn allow_command(flags: &str) -> link_format::Command {
    let (flag, value) = match flags.split_once(' ') {
        Some((flag, value)) => (flag, Some(value.trim_matches('\''))),
        None => (flags, None),
    };
    link_format::Command::typed(
        ["lnk", "agent", "permission", "add", flag]
            .into_iter()
            .chain(value)
            .map(String::from),
    )
}

/// Whether this machine takes agent `agent` (named `named` by `-a`, if
/// it is): an agent the account it's signed in as serves, not another
/// of that name or with its ID here, and not running here.
pub(crate) fn admit(named: Option<&str>, agent: &transfer::Agent) -> anyhow::Result<()> {
    if let Some(named) = named {
        ensure!(
            named == agent.name,
            "the agent that came is {}, not {named}",
            agent.name
        );
    }
    if let Some(owner) = &agent.owner {
        let account = link_core::owner::account_owner();
        ensure!(
            link_agent_contract::identity::serves(owner, account.as_deref()),
            "{}",
            match account {
                Some(account) => format!(
                    "{} is another account's: this machine is signed in as {account}, and \
                     takes its agents only",
                    agent.name
                ),
                None => format!(
                    "{} is an account's ({owner}): it moves only to a machine signed in as \
                     that account through its account provider",
                    agent.name
                ),
            }
        );
    }
    let settings = Settings::load(&agent.name)?;
    if settings.path().exists() && settings.agent_id() != Some(agent.id()) {
        bail!(
            "another agent named {} is on this machine already: move it elsewhere, \
             or this one to another machine",
            agent.name
        );
    }
    // Its services, sockets and tools are keyed by its ID: no other
    // agent here may have it.
    for other in settings::names()?.iter().filter(|n| **n != agent.name) {
        ensure!(
            Settings::load(other)?.agent_id() != Some(agent.id()),
            "{} has the ID of {other}, an agent here already: not taken",
            agent.name
        );
    }
    ensure!(
        !service::running_anywhere(&agent.name),
        "{} runs here already: lnk agent stop it first",
        agent.name
    );
    Ok(())
}

/// `lnk agent receive`: phase `phase` of a move, on stdin after the line
/// `move` sends first. A copy goes into the agent's staging here, and
/// what staging holds is printed, as JSON. A switch is put in place here,
/// as the agent the stream names (`named`, `-a`, must be the same):
/// returns its settings and harness. An agent of that name here must be
/// the same agent (its ID), coming back.
///
/// A switch first says on stdout what its secrets are sealed to: a key
/// made for this one, whose private half never leaves this process and is
/// dropped once they're opened.
///
/// `expected`, the agent ID the move chose (`--agent-id`): a stream of any
/// other agent is refused before anything of it is staged, so the machine
/// it comes from can't bring another agent's ID here, taking its grants
/// and ending the note of where it is.
pub fn receive(
    out: &mut Out,
    named: Option<&str>,
    phase: Phase,
    expected: Option<&str>,
) -> anyhow::Result<Option<(Settings, String)>> {
    let recipient = (phase == Phase::Switch).then(link_secretive::Recipient::new);
    if let Some(recipient) = &recipient {
        let mut said = Vec::new();
        recipient
            .announce(&mut said)
            .within("saying what to seal to")?;
        out.data(&said);
    }
    let stdin = std::io::stdin();
    let mut input = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();
    std::io::Read::take(&mut input, 1 << 20).read_line(&mut line)?;
    let mut header: Header = serde_json::from_str(&line).within("reading what move sent first")?;
    if let Some(recipient) = &recipient {
        header.open(recipient)?;
    }
    receive_stream(
        out,
        named,
        phase,
        input,
        recipient.as_ref(),
        header,
        expected.map(Expected::Agent),
    )
}

/// What a stream must be of, as the machine running the move chose it.
pub(crate) enum Expected<'a> {
    /// That agent, by its ID.
    Agent(&'a str),
    /// That member of a team's move.
    Member(&'a link_agent_place::teams::MovingMember),
}

pub(crate) fn receive_team(
    out: &mut Out,
    input: impl std::io::Read,
    recipient: &link_secretive::Recipient,
    yes: bool,
    expected: &link_agent_place::teams::MovingMember,
) -> anyhow::Result<Option<(Settings, String)>> {
    receive_stream(
        out,
        None,
        Phase::Switch,
        input,
        Some(recipient),
        Header {
            yes,
            ..Default::default()
        },
        Some(Expected::Member(expected)),
    )
}

pub(crate) fn receive_stream(
    out: &mut Out,
    named: Option<&str>,
    phase: Phase,
    mut input: impl std::io::Read,
    recipient: Option<&link_secretive::Recipient>,
    header: Header,
    expected: Option<Expected<'_>>,
) -> anyhow::Result<Option<(Settings, String)>> {
    let received = transfer::receive(&mut input, recipient, |manifest| {
        match &expected {
            Some(Expected::Member(expected)) => ensure!(
                manifest.agent.id() == expected.id
                    && manifest.agent.name == expected.name
                    && manifest.move_id == expected.movement,
                "the stream names another team member or move"
            ),
            Some(Expected::Agent(id)) => ensure!(
                manifest.agent.id() == *id,
                "the stream is of another agent ({}), not the one moving ({id})",
                link_format::clean(manifest.agent.id())
            ),
            None => {}
        }
        ensure!(
            manifest.phase == phase,
            "the stream is a move's {}, not its {}",
            manifest.phase.name(),
            phase.name()
        );
        admit(named, &manifest.agent)
    })?;
    let incoming = match received {
        transfer::Received::Copied { have, .. } => {
            out.data(format!("{}\n", serde_json::to_string(&have)?).as_bytes());
            return Ok(None);
        }
        transfer::Received::Switched(incoming) => incoming,
    };
    let agent = incoming.manifest.agent.clone();
    // The move it arrives by: its generation here, the only one what was
    // held for it while it was paused is given to.
    let generation = incoming.manifest.move_id.clone();
    let mut source_models = moved::models::load(agent.id())?;
    let checked = (|| -> anyhow::Result<(Settings, Model, State)> {
        let settings = Settings::load(&agent.name)?;
        let given = header
            .key
            .duplicate()
            .map(|key| crate::model::hosted(&key, None))
            .transpose()?;
        anyhow::ensure!(
            given.is_some() || source_models.as_ref().is_none_or(|models| !models.revoked),
            "the source's model credential was removed: provide a new model key before moving the agent back"
        );
        let given = given.or_else(|| {
            source_models
                .as_ref()
                .and_then(|models| models.model.duplicate())
        });
        let model = transfer::settle_model(
            settings.model.as_ref(),
            incoming.manifest.model.as_ref(),
            given,
            header.sandbox.is_some(),
            crate::model::model_answers,
        )?;
        // What the machine running the move gave, and what came, checked
        // as a file of grants is before it's kept.
        if let Some(grants) = &header.grants {
            grants
                .check()
                .within("the grants the move gave its harness")?;
        }
        if let Some(sandbox) = &header.sandbox {
            sandbox
                .check()
                .within("the grants the move gave your agent")?;
        }
        if let Some(sandbox) = &incoming.manifest.sandbox {
            sandbox.check().within("the grants your agent came with")?;
        }
        // What of its home comes is what this machine's adapter says,
        // never what the stream does.
        let state = state_of(
            &Adapter::find_or_add(&incoming.manifest.harness)?,
            &settings,
        )?;
        Ok((settings, model, state))
    })();
    let (mut settings, model, state) = match checked {
        Ok(checked) => checked,
        Err(err) => {
            incoming.discard();
            return Err(err);
        }
    };
    let harness = incoming.manifest.harness.clone();
    let incoming_bucket = incoming.manifest.bucket.clone();
    let incoming_fallback = source_models
        .as_mut()
        .and_then(|models| models.fallback.take())
        .or_else(|| incoming.manifest.fallback.duplicate());
    if header.yes {
        crate::permissions::YES.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // What you granted the agent: what the machine running the move gave,
    // else what it has here, else what came, less what opens this machine.
    let here_sandbox = sandbox_file::path(&agent.name)
        .exists()
        .then(|| sandbox_file::load(&agent.name))
        .transpose()?;
    // The teams a cloud keeps that it's on, as the machine running the move
    // read them from that cloud, kept here first: their files are what
    // its grants of them are derived from, as every team's.
    let namesakes = link_agent::hosted_teams::keep(agent.id(), &header.teams)?;
    for team in namesakes {
        out.block(Report::warning(sentence!(
            "Team {} here is another team: the one a cloud keeps that {} is on isn't kept here.",
            Name::new("team", &team),
            Name::new("agent", &agent.name)
        )));
    }
    // Its teams' folders derived again here from their files, the teams'
    // homes: never taken from the stream. Coming back, what this machine
    // granted it of a team's folder when it left is vouched for again.
    let granted_here = moved::of_id(agent.id())?
        .map(|note| note.teams)
        .unwrap_or_default();
    let (sandbox, left_out) = transfer::settle_sandbox(
        header.sandbox.as_ref(),
        here_sandbox.as_ref(),
        incoming.manifest.sandbox.as_ref(),
        &|team, came, vouched| match granted_here.get(team) {
            Some(granted) => settings::arriving_grant(agent.id(), team, granted, true),
            None => settings::arriving_grant(agent.id(), team, came, vouched),
        },
    );
    if let Some(sandbox) = &sandbox {
        sandbox
            .check()
            .within(format!("in what {} may do", agent.name))?;
    }
    // Each connection is what its module here says of it, and its link what
    // that module names: added first if it's missing, as `lnk agent
    // connect` does.
    let left_out_connections = incoming.apply(
        &mut settings,
        &state,
        |connection| {
            crate::connecting::connection_module(connection).ok()?;
            bridge::module_state(connection)
        },
        |connection| {
            crate::connecting::connection_module(connection).ok()?;
            crate::connections::describe(connection).ok()
        },
    )?;
    for connection in left_out_connections {
        out.block(
            Report::warning(sentence!(
                "{} came with connection {}, which no module here describes: left out.",
                Name::new("agent", &agent.name),
                Foreign::new(&connection)
            ))
            .next(cmd!("lnk agent connect <connection>"), "connect it again"),
        );
    }
    if let Some(sandbox) = sandbox {
        sandbox_file::save(&agent.name, &sandbox)?;
    }
    for left_out in left_out {
        // A program `run` may start: its sandbox file's, not a grant of
        // `lnk agent permission add`'s.
        let agent_name = Name::new("agent", &agent.name);
        let report = Report::warning(sentence!(
            "{} came with {}, which reaches past its sandbox here: not taken from the machine \
             it came from.",
            &agent_name,
            Foreign::new(&left_out)
        ));
        out.block(
            match (
                left_out.strip_prefix("--commands.allow "),
                left_out.strip_prefix("team "),
            ) {
                (Some(program), _) => report.why(prose!(
                    "Its sandbox file's [commands] ({}) gives {} here.",
                    cmd!("lnk agent start --sandbox"),
                    Foreign::new(program)
                )),
                (_, Some(team)) => report.next(
                    cmd!(
                        "lnk agent team add {} --member {} --write <folder>",
                        Name::new("team", team),
                        &agent_name
                    ),
                    "give it here (or --read)",
                ),
                _ => report.next(allow_command(&left_out), "give it here"),
            },
        );
    }
    settings.model = transfer::arrived(model, transfer::Role::Model);
    settings.fallback = incoming_fallback.and_then(|fallback| {
        let arrived = transfer::arrived(fallback, transfer::Role::Fallback);
        if arrived.is_none() {
            out.block(
                Report::warning(sentence!(
                    "{} came with a fallback on the machine it came from, which isn't here: \
                     left out.",
                    Name::new("agent", &agent.name)
                ))
                .next(
                    cmd!("lnk agent start --model.fallback_model <model>"),
                    "give it one",
                ),
            );
        }
        arrived
    });
    // The route its move gave it; else this machine's default, as an
    // agent made here starts with (`lnk agent route --default`).
    settings.upstream = match header.upstream {
        Some(upstream) => Some(upstream),
        None => settings::Defaults::load()?.upstream,
    };
    // What its harness needs here: of what the machine running the move
    // gave, only what the harness asks for here (`needs`, given the agent
    // as it lands), added to its own grants here; else the harness's own
    // here, which a stream never changes, and asks at its start.
    let here = Grants::load(&harness)?;
    let mut grants = here.clone();
    if let Some(given) = &header.grants {
        let needs = Adapter::find(&harness)
            .and_then(|adapter| adapter.needs(&settings))
            .unwrap_or_default();
        grants.union(&given.only_asked(&needs));
    }
    if grants != here {
        grants.save(&harness)?;
    }
    if let Some(note) = settings.check_files()? {
        out.block(Note(Sentence::adopt(&note)));
    }
    settings.take_ports_here()?;
    if let Some(id) = settings.agent_id() {
        link_agent::settings::ids::arrive(&settings.name, id)?;
    }
    if let Some(service) = &settings.service {
        link_agent::settings::ids::SERVICES.arrive(&settings.name, service)?;
    }
    settings.save()?;
    bridge::set_generation(&agent.name, &generation)?;
    if matches!(expected, Some(Expected::Member(_))) && source_models.is_some() {
        // A team can still roll back while another member is prepared.
        // Its durable Finish releases this retention only after the
        // cloud confirms the whole team left.
        moved::models::keep(&settings)?;
    } else {
        moved::models::remove(agent.id())?;
    }
    adopt_buckets(
        out,
        agent.id(),
        incoming_bucket.as_ref(),
        header.bucket_credentials.as_ref(),
    );
    // It's back: no note says it's elsewhere.
    if let Some(note) = moved::of_id(agent.id())? {
        note.remove();
    }
    moved::prune();
    Ok(Some((settings, harness)))
}

#[cfg(test)]
mod tests {

    /// A box's needs answer with an escape sequence, or a need Link
    /// doesn't know, is refused before anything is shown.
    #[test]
    fn a_boxs_needs_answer_is_only_names_link_knows() {
        let pending = |needs: &[&str], running: &[&str]| super::Pending {
            harness: "someharness".into(),
            needs: needs
                .iter()
                .map(|n| (n.to_string(), "why".into()))
                .collect(),
            running: running.iter().map(|r| r.to_string()).collect(),
        };
        // No sandbox at all is no harness's need, on a box least of all.
        assert!(pending(&["unsandboxed"], &["main"]).check().is_err());
        let known = "--network.mode host";
        pending(&[known], &[]).check().unwrap();
        // A risk the harness names itself.
        pending(&["signal-relay"], &[]).check().unwrap();
        assert!(pending(&["\x1b]0;owned\x07"], &[]).check().is_err());
        assert!(pending(&[known], &["main\x1b[2J"]).check().is_err());
    }
}
