//! The end an agent leaves: `lnk agent state-send`, `pause`, `resume`
//! and `left`, and the stream piped to the other end.

use std::io::{Read, Write};
use std::process::Stdio;

use anyhow::ensure;
use link_agent::adapter::Adapter;
use link_agent::bridge;
use link_agent::hosted;
use link_agent::moved;
use link_agent::service;
use link_agent::settings::{self, Settings};
use link_agent::transfer::{self, Have, Phase};
use link_format::WithinExt;
use link_format::{Done, Foreign, Name, Note, Out, Report, Sentence, cmd, phrase, sentence};
use link_harness_contract::harness::State;
use link_secretive::Duplicate;

use super::backups::start_again_during;
use super::buckets::bucket_run;
use super::receiving::Target;
use super::{Header, hosted_place, lnk_at, place, state_of};

/// The most of what a box's `receive` says after its key that's shown.
const SAID: u64 = 1 << 20;

/// `child`'s exit and at most `most` bytes of what it writes (its stderr
/// is the terminal's): one that says more is stopped, and refused.
fn output_within(
    mut child: std::process::Child,
    most: u64,
) -> anyhow::Result<std::process::Output> {
    let mut stdout = Vec::new();
    if let Some(out) = child.stdout.take() {
        out.take(most + 1).read_to_end(&mut stdout)?;
    }
    if stdout.len() as u64 > most {
        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!("the other end said more than {} MB: refused", most >> 20);
    }
    let status = child.wait()?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

/// Where a move takes the agent from.
pub(crate) enum Source {
    /// A machine: a box, or this computer (none), and the move's ID.
    Machine(Option<String>, String),
    /// A cloud account that hosts it, with the move it started there
    /// (`agent-out`).
    Hosted {
        account: Box<link_agent_place::Account>,
        move_id: String,
    },
}

impl Source {
    /// The move's ID.
    pub(crate) fn move_id(&self) -> &str {
        match self {
            Source::Machine(_, move_id) | Source::Hosted { move_id, .. } => move_id,
        }
    }

    /// Where it is, for the user.
    pub(crate) fn place(&self) -> String {
        match self {
            Source::Machine(at, _) => place(at),
            Source::Hosted { account, .. } => hosted_place(&account.name),
        }
    }

    /// The copy, while agent `agent` (`id`) answers where it is: into
    /// staging on `to`, but what `have` says it holds already. What
    /// staging holds after.
    pub(crate) fn copy(
        &self,
        out: &mut Out,
        agent: &str,
        id: &str,
        to: &Target,
        have: &Have,
    ) -> anyhow::Result<Have> {
        let copying = out.activity(phrase!(
            "Copying {}'s files and memory to {} while it answers on {}",
            Name::new("agent", agent),
            Foreign::new(&to.place()),
            Foreign::new(&self.place())
        ));
        let mut receiver = to.receiver(id, Phase::Copy)?;
        // To a machine, the harness's state comes in the copy too; an
        // account that hosts agents takes it in the switch.
        let mut sender =
            self.sender((agent, id), Phase::Copy, have, None, !to.is_hosted(), false)?;
        let copied = pipe(&mut sender, &mut receiver, to.first_copy(), true);
        let sent = sender.wait()?;
        let output = output_within(receiver, link_agent_contract::moving::MAX_HAVE)?;
        drop(copying);
        ensure!(
            sent.success(),
            "copying your agent from {} failed (above)",
            self.place()
        );
        ensure!(
            output.status.success(),
            "copying your agent to {} failed (above)",
            to.place()
        );
        let count = copied.within("copying your agent")?;
        out.block(Note(sentence!(
            "{}, while {} answered.",
            count.text(),
            Name::new("agent", agent)
        )));
        to.copied(&output.stdout)
    }

    /// What sends phase `phase` of agent `agent`'s (`id`'s) move, `have` what the
    /// other end holds: its `send`, or the account's `agent-send`.
    /// `seal_to` is what the switch's secrets are sealed to, by the
    /// machine it's on or the account that hosts it.
    fn sender(
        &self,
        (agent, id): (&str, &str),
        phase: Phase,
        have: &Have,
        seal_to: Option<&link_secretive::SealTo>,
        with_state: bool,
        without_model_credentials: bool,
    ) -> anyhow::Result<std::process::Child> {
        match self {
            Source::Machine(from, move_id) => {
                let mut args = vec!["state-send", "--phase", phase.name(), "--held", "-"];
                if with_state {
                    args.push("--harness-state");
                }
                if without_model_credentials {
                    args.push("--without-model-credentials");
                }
                args.extend(["--move", move_id, "--agent-id", id]);
                let seal_to = seal_to.map(ToString::to_string);
                if let Some(seal_to) = &seal_to {
                    args.extend(["--seal-to", seal_to]);
                }
                let mut sender = lnk_at(from, agent, &args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .within("running lnk agent state-send")?;
                let mut input = sender.stdin.take().within("no input to send")?;
                input.write_all(&serde_json::to_vec(have)?)?;
                Ok(sender)
            }
            Source::Hosted { account, move_id } => {
                let request = link_agent_place::Request {
                    account: account.as_ref().duplicate(),
                    r#move: Some(move_id.clone()),
                    phase: Some(phase),
                    have: have.clone(),
                    seal_to: seal_to.cloned(),
                    ..Default::default()
                };
                let mut child = link_agent_place::spawn(&account.kind, "agent-send", &request)?;
                // The request was all it reads.
                drop(child.stdin.take());
                Ok(child)
            }
        }
    }

    /// Agent `agent` (`id`) paused where it is, for its switch, and held
    /// there for this move alone: on a machine, `lnk agent pause --move`;
    /// an account that hosts it pauses it as it gives its switch.
    pub(crate) fn pause(&self, out: &mut Out, agent: &str, id: &str) -> anyhow::Result<()> {
        let Source::Machine(from, move_id) = self else {
            return Ok(());
        };
        let _pausing = out.activity(phrase!(
            "Pausing {} on {} for the switch",
            Name::new("agent", agent),
            Foreign::new(&place(from))
        ));
        let paused = lnk_at(from, agent, &["pause", "--move", move_id, "--agent-id", id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .within("pausing your agent")?;
        ensure!(
            paused.success(),
            "couldn't pause {agent} on {}",
            place(from)
        );
        Ok(())
    }

    /// The switch: what differs from what `to` holds (`have`), and
    /// everything else of agent `agent` (`id`), put in place there, not
    /// started.
    pub(crate) fn switch(
        &self,
        out: &mut Out,
        agent: &str,
        id: &str,
        to: &Target,
        header: &Header,
        have: &Have,
    ) -> anyhow::Result<Option<String>> {
        let mut receiver = to.receiver(id, Phase::Switch)?;
        if to.is_hosted() {
            let Target::Hosted {
                seal_to,
                managed_models,
                ..
            } = to
            else {
                unreachable!()
            };
            let mut sender = self.sender(
                (agent, id),
                Phase::Switch,
                have,
                Some(seal_to),
                false,
                *managed_models,
            )?;
            let carried = pipe(&mut sender, &mut receiver, None, false);
            let sent = sender.wait()?;
            let output = output_within(receiver, link_agent_contract::moving::MAX_MANIFEST)?;
            ensure!(
                sent.success() && output.status.success(),
                "the switch to {} failed",
                to.place()
            );
            carried?;
            let reply: link_agent_place::Reply = serde_json::from_slice(&output.stdout)?;
            ensure!(
                reply.answers == Some(true),
                "{} took {agent} but does not say it answers",
                to.place()
            );
            return Ok(reply
                .machine
                .filter(|m| link_machine::machine::check_name(m).is_ok()));
        }
        // What its secrets are sealed to: the key that `receive` made for
        // this switch alone, and said first. This computer, in the middle,
        // passes on what it can't read.
        let output = receiver
            .stdout
            .take()
            .within("no output from the other end")?;
        let (seal_to, mut rest) = match link_core::announced(output) {
            Ok(announced) => announced,
            Err(err) => {
                let _ = receiver.kill();
                let _ = receiver.wait();
                return Err(err).within(format!("receiving your agent on {}", to.place()));
            }
        };
        // What it says after, shown as it says it.
        let shown = std::thread::spawn(move || {
            let mut said = Vec::new();
            let _ = std::io::copy(&mut (&mut rest).take(SAID), &mut said);
            // Past what's shown, read and dropped, so the other end
            // never stops on a full pipe.
            let _ = std::io::copy(&mut rest, &mut std::io::sink());
            said
        });
        let mut sender = self.sender(
            (agent, id),
            Phase::Switch,
            have,
            Some(&seal_to),
            false,
            false,
        )?;
        let first = header.sealed_to(&seal_to)?;
        let carried = pipe(&mut sender, &mut receiver, Some(&first), false);
        let sent = sender.wait()?;
        let received = receiver.wait()?;
        if let Ok(said) = shown.join() {
            out.data(&said);
        }
        ensure!(
            sent.success(),
            "sending your agent from {} failed (above)",
            self.place()
        );
        ensure!(
            received.success(),
            "receiving your agent on {} failed (above)",
            to.place()
        );
        carried.within("carrying your agent")?;
        Ok(None)
    }

    /// Agent `agent` (`id`) answering where it was again, after a move
    /// that failed: `lnk agent resume --move` on a machine, `agent-abort`
    /// for an account that hosts it. Whether it does.
    pub(crate) fn resume(&self, out: &mut Out, agent: &str, id: &str) -> bool {
        match self {
            Source::Machine(from, move_id) => lnk_at(
                from,
                agent,
                &["resume", "--move", move_id, "--agent-id", id],
            )
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success()),
            Source::Hosted { .. } => self.abort(out),
        }
    }

    /// For an account that hosts it, its move ended (`agent-abort`): the
    /// agent answers there again. Whether it does, said when it can't be.
    pub(crate) fn abort(&self, out: &mut Out) -> bool {
        match self {
            Source::Hosted { account, move_id } => said(out, abort(account, move_id)),
            Source::Machine(..) => true,
        }
    }

    /// Where it is, as a copy of its keys is noted there: a box, or the
    /// account that hosts it; none for this computer.
    pub(crate) fn keys_at(&self) -> Option<moved::Place> {
        match self {
            Source::Machine(from, _) => {
                from.as_deref().map(|name| moved::Place::of_box(name, None))
            }
            Source::Hosted { account, .. } => Some(moved::Place::of_account(&account.name, None)),
        }
    }

    /// The agent answers on `machine` (`to`) now: where it was deletes its
    /// copy. An account that hosts it hands over what came for it while it
    /// was paused there (`agent-left`), and it's given them where it went
    /// (`lnk agent deliver`). Whether it says it did.
    pub(crate) fn left(
        &self,
        out: &mut Out,
        agent: &str,
        id: &str,
        to: &Option<String>,
        machine: &moved::Place,
    ) -> bool {
        match self {
            Source::Machine(from, move_id) => {
                let slot = if machine.cloud { "--cloud" } else { "--on" };
                let mut left = lnk_at(
                    from,
                    agent,
                    &[
                        "left",
                        slot,
                        &machine.name,
                        "--move",
                        move_id,
                        "--agent-id",
                        id,
                    ],
                );
                if let Some(machine_id) = &machine.id {
                    left.args(["--machine-id", machine_id]);
                }
                let left = left.stdin(Stdio::null()).status();
                let done = left.is_ok_and(|s| s.success());
                if !done {
                    out.block(Report::warning(sentence!(
                        "Its copy on {} is stopped but couldn't be removed: don't start it there, \
                         where it would answer beside this one. The next {} or {} of it here \
                         removes it.",
                        Foreign::new(&place(from)),
                        cmd!("lnk agent move"),
                        cmd!("lnk agent start")
                    )));
                }
                done
            }
            Source::Hosted { account, move_id } => {
                let left = hosted::call(account, "agent-left", |r| {
                    r.r#move = Some(move_id.clone());
                });
                match left {
                    Ok(reply) => {
                        super::delivering::deliver_at(out, to, agent, &reply.held);
                        true
                    }
                    Err(err) => {
                        out.block(
                            Report::warning(sentence!(
                                "{} didn't say it deleted its copy.",
                                Foreign::new(&hosted_place(&account.name))
                            ))
                            .why(Sentence::adopt(&format!("{err:#}")))
                            .next(
                                cmd!("lnk agent move"),
                                "move there again, or delete it there",
                            ),
                        );
                        false
                    }
                }
            }
        }
    }
}

/// `agent-abort` of move `move_id` in `account`: its copy removed, or its
/// agent answering there again. What to say when it fails.
pub(crate) fn abort(account: &link_agent_place::Account, move_id: &str) -> Option<Report> {
    let aborted = hosted::call(account, "agent-abort", |r| {
        r.r#move = Some(move_id.to_string());
    });
    let Err(err) = aborted else {
        return None;
    };
    Some(
        Report::warning(sentence!(
            "{} didn't end the move.",
            Foreign::new(&hosted_place(&account.name))
        ))
        .why(Sentence::adopt(&format!("{err:#}")))
        .next(cmd!("lnk agent list"), "see where the agent is"),
    )
}

/// Whether an [`abort`] went: said, when it didn't.
pub(crate) fn said(out: &mut Out, aborted: Option<Report>) -> bool {
    match aborted {
        None => true,
        Some(report) => {
            out.block(report);
            false
        }
    }
}

/// `lnk agent receive --phase <phase>` of agent `id` on `to`, its output
/// to `out`. It's named by the stream: never `-a`, which would name an
/// agent there. The stream must be of `id`, the agent this computer chose.
pub(crate) fn receiver(
    to: &Option<String>,
    id: &str,
    phase: Phase,
    out: Stdio,
) -> anyhow::Result<std::process::Child> {
    let mut receiver = link_core::lnk();
    if let Some(name) = to {
        receiver.args(["box", "run", name, "--"]);
    }
    // Started after, by `settle_and_start`, once what it needs is asked.
    receiver.args(["agent", "receive", "--no-start", "--phase", phase.name()]);
    receiver.args(["--agent-id", id]);
    receiver
        .stdin(Stdio::piped())
        .stdout(out)
        .spawn()
        .within("running lnk agent receive")
}

/// What agent `id`'s staging on `to` holds (`lnk agent receive --have`).
pub(crate) fn have_at(to: &Option<String>, id: &str) -> anyhow::Result<Have> {
    let mut command = link_core::lnk();
    if let Some(name) = to {
        command.args(["box", "run", name, "--"]);
    }
    let output = command
        .args(["agent", "receive", "--have", "--agent-id", id])
        .stdin(Stdio::null())
        .output()
        .within("running lnk agent receive --have")?;
    ensure!(
        output.status.success(),
        "asking {} what it holds of your agent failed: {}",
        place(to),
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    link_agent_contract::moving::read_have(output.stdout.as_slice())
}

/// `source`'s stdout into `sink`'s stdin, `first` and a newline before
/// it. What passed, its files counted when it is a tar (`tarred`: the
/// copy; a switch is opaque ciphertext, only its bytes).
pub(crate) fn pipe(
    source: &mut std::process::Child,
    sink: &mut std::process::Child,
    first: Option<&str>,
    tarred: bool,
) -> anyhow::Result<Count> {
    let mut input = sink.stdin.take().within("no input to the other end")?;
    let mut output = source.stdout.take().within("no output from the sender")?;
    if let Some(first) = first {
        writeln!(input, "{first}")?;
    }
    let mut count = Count::default();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = match std::io::Read::read(&mut output, &mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        };
        input.write_all(&buffer[..n])?;
        match tarred {
            true => count.add(&buffer[..n]),
            false => count.bytes += n as u64,
        }
    }
    drop(input);
    Ok(count)
}

/// What of a stream passed so far: its bytes, and its files, counted from
/// the tar headers as they pass.
#[derive(Default)]
pub(crate) struct Count {
    bytes: u64,
    files: u64,
    /// Where the next header starts, in the stream; past its end, none.
    next: u64,
    header: Vec<u8>,
}

impl Count {
    fn add(&mut self, mut data: &[u8]) {
        let mut at = self.bytes;
        self.bytes += data.len() as u64;
        while !data.is_empty() {
            if at < self.next {
                let skip = (self.next - at).min(data.len() as u64) as usize;
                data = &data[skip..];
                at += skip as u64;
                continue;
            }
            let take = (512 - self.header.len()).min(data.len());
            self.header.extend_from_slice(&data[..take]);
            data = &data[take..];
            at += take as u64;
            if self.header.len() < 512 {
                continue;
            }
            let header = std::mem::take(&mut self.header);
            if header.iter().all(|b| *b == 0) {
                self.next = at;
                continue;
            }
            let size = tar_size(&header[124..136]);
            // Its files: not the manifest, nor the stream's end.
            let name = &header[..100];
            let named = |entry: &str| {
                name.starts_with(entry.as_bytes()) && name.get(entry.len()) == Some(&0)
            };
            let ours = named(link_agent_contract::moving::MANIFEST)
                || named(link_agent_contract::moving::END);
            if matches!(header[156], b'0' | 0) && !ours {
                self.files += 1;
            }
            self.next = at + size.div_ceil(512) * 512;
        }
    }

    pub(crate) fn text(&self) -> String {
        let files = match self.files {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        format!("Copied {files}, {}", human(self.bytes))
    }
}

/// A tar header's size field: octal, or base-256 when its first bit is
/// set.
pub(crate) fn tar_size(field: &[u8]) -> u64 {
    if field[0] & 0x80 != 0 {
        return field[1..].iter().fold(0u64, |n, b| {
            n.saturating_mul(256).saturating_add(u64::from(*b))
        });
    }
    let text = String::from_utf8_lossy(field);
    u64::from_str_radix(text.trim_matches(|c: char| c == '\0' || c == ' '), 8).unwrap_or(0)
}

/// `bytes` for people: `4.2 MB`.
pub(crate) fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} bytes"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

/// `lnk agent state-send`: phase `phase` of the agent here's move `move_id`
/// (one made, if none) on stdout, but what `have` says the other end
/// holds the same. A copy goes while the agent runs; a switch only once
/// it's stopped (`lnk agent pause`), so its files don't change under it.
pub fn send(
    settings: &Settings,
    phase: Phase,
    move_id: Option<String>,
    have: Have,
    seal_to: Option<&link_secretive::SealTo>,
    with_state: bool,
) -> anyhow::Result<()> {
    send_to(
        std::io::BufWriter::with_capacity(64 * 1024, link_format::streams::protocol()),
        settings,
        phase,
        move_id,
        have,
        seal_to,
        with_state,
    )
}

pub(crate) fn send_to(
    out: impl Write,
    settings: &Settings,
    phase: Phase,
    move_id: Option<String>,
    have: Have,
    seal_to: Option<&link_secretive::SealTo>,
    with_state: bool,
) -> anyhow::Result<()> {
    let move_id = match move_id {
        Some(move_id) => move_id,
        None => link_agent_contract::moving::new_move_id()?,
    };
    check_send(settings, phase, seal_to)?;
    let switch = phase == Phase::Switch;
    let adapter = Adapter::find(settings.harness())?;
    let (state, connections, bucket) = match switch {
        true => {
            let state = state_of(&adapter, settings)?;
            // What links the connections Link carries (a harness with an
            // endpoint); a harness speaking one itself keeps its link in
            // its own state.
            let carried = match adapter.endpoint(settings.port) {
                Some(_) => bridge::carried(settings, &adapter.connections()),
                None => Vec::new(),
            };
            let connections = bridge::links(settings, &carried);
            let bucket = settings
                .agent_id()
                .and_then(|id| link_agent::owners::agent_flag(id).ok())
                .and_then(|[flag, owner]| bucket_run(&["export", &flag, &owner]));
            (state, connections, bucket)
        }
        // A copy to a machine: its harness's state while it runs, so the
        // switch sends only what changed since.
        false if with_state => (state_of(&adapter, settings)?, Default::default(), None),
        false => (State::default(), Default::default(), None),
    };
    transfer::send(
        out,
        settings,
        &transfer::Sending {
            phase,
            move_id: &move_id,
            have: &have,
        },
        &state,
        &connections,
        bucket,
        seal_to,
    )
}

/// What refuses phase `phase` of agent `settings`' send, before anything
/// of it is read, sent or released: a switch with no key to seal its
/// secrets to, or while the agent runs; files alone.
pub fn check_send(
    settings: &Settings,
    phase: Phase,
    seal_to: Option<&link_secretive::SealTo>,
) -> anyhow::Result<()> {
    let switch = phase == Phase::Switch;
    ensure!(
        !switch || seal_to.is_some(),
        "a switch needs its receiver's public key"
    );
    ensure!(
        phase != Phase::Files,
        "a move sends a copy, then a switch: not files alone"
    );
    ensure!(
        !switch || !service::running_anywhere(&settings.name),
        "{} is running: lnk agent pause first, so its files don't change under the switch",
        settings.name
    );
    Ok(())
}

/// An error unless agent `settings` has ID `id`, when a move names it so
/// (`--agent-id`): an agent made since under its name is another, never
/// stopped, paused, sent, resumed or removed for the move.
pub fn check_agent_id(settings: &Settings, id: Option<&str>) -> anyhow::Result<()> {
    let Some(id) = id else {
        return Ok(());
    };
    ensure!(
        settings.agent_id() == Some(id),
        "{} here isn't agent {}: another agent has its name",
        settings.name,
        link_format::clean(id)
    );
    Ok(())
}

/// What `--have` names for `send`: a file of JSON, or `-` for stdin;
/// none, nothing.
pub fn read_have(from: Option<&std::path::Path>) -> anyhow::Result<Have> {
    match from {
        None => Ok(Have::new()),
        Some(path) if path == std::path::Path::new("-") => {
            link_agent_contract::moving::read_have(std::io::stdin().lock())
        }
        Some(path) => link_agent_contract::moving::read_have(
            std::fs::File::open(path).within(format!("opening {}", path.display()))?,
        ),
    }
}

/// `lnk agent pause`: the agent here stops taking messages for a move's
/// switch. Whatever carries its connections stops reading them, so their
/// platforms keep what comes, and its way in takes nothing; the turn in
/// flight finishes, up to [`TURN_WAIT`]; then it stops. Nothing for an
/// agent that isn't running.
#[expect(
    clippy::disallowed_methods,
    reason = "polls for quiet and the turn's end, which nothing announces"
)]
pub fn pause(settings: &Settings) -> anyhow::Result<()> {
    let name = &settings.name;
    if !service::running_anywhere(name) {
        return Ok(());
    }
    link_files::create_private_dir(&settings::config_of(name))?;
    let _ = std::fs::remove_file(bridge::quiet_mark(name));
    link_files::write_private(&bridge::pause_mark(name), b"")?;
    // What carries its connections says it took the pause, when something
    // does: its connections' bridges, or its way in.
    let carried =
        bridge::Plan::load(name).is_ok_and(|plan| plan.from || !plan.connections.is_empty());
    let asked = std::time::Instant::now();
    while carried
        && !bridge::quiet_mark(name).exists()
        && asked.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // The turn in flight, finished.
    while transfer::turn_running(settings)? && asked.elapsed() < TURN_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if service::running(name) {
        service::stop(name)?;
    }
    service::stop_foreground(name)?;
    // As a stop: its key proves nothing from now on, and its background
    // commands end with it.
    link_sandbox_binding::Keyring::here()
        .unbind(&settings.key(), &link_secret_contract::keeper::RouteRevoker);
    link_agent::background::stop_background(settings, false);
    Ok(())
}

/// How long `pause` waits for the turn in flight.
pub const TURN_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// `lnk agent resume`: the agent here takes messages again, after a move
/// whose switch failed: no longer paused, and started.
pub fn resume(out: &mut Out, settings: &Settings) {
    resume_during(out, settings, None);
}

/// [`resume`] for move `move_id`, the start it makes that move's own
/// (`lnk agent start --during-move`).
pub fn resume_during(out: &mut Out, settings: &Settings, move_id: Option<&str>) {
    bridge::unpause(&settings.name);
    start_again_during(out, settings, move_id);
}

/// `lnk agent receive --have --agent-id <id>`: what agent `id`'s staging here
/// holds, as JSON, each file's path and hash.
pub fn print_have(out: &mut Out, id: &str) -> anyhow::Result<()> {
    out.data(format!("{}\n", serde_json::to_string(&transfer::have(id)?)?).as_bytes());
    Ok(())
}

/// `lnk agent left`: agent `settings` answers on machine `to` now, so
/// this machine deletes its copy and keeps a note of where it went.
pub fn left(out: &mut Out, settings: &Settings, to: moved::Place) -> anyhow::Result<Done> {
    ensure!(
        !service::running_anywhere(&settings.name),
        "{} runs here: it can't have left",
        settings.name
    );
    let place = match link_machine::installation::installation() {
        Ok(machine) => machine.name,
        Err(_) => "here".into(),
    };
    let mut cleared = moved::leave(settings, to)?;
    crate::services::on_stop(&settings.name, false);
    // A copy of it that came here for a move that never switched.
    if let Some(id) = settings.agent_id() {
        transfer::drop_staging(id);
    }
    // Its buckets' password is its own: it went with it, or it's said
    // with what left it.
    if let Some(id) = settings.agent_id()
        && link_core::find_module("storage").is_some()
        && let Ok(owner) = link_agent::owners::agent_flag(id)
    {
        let gone = link_core::lnk()
            .args(["storage", "leave"])
            .args(&owner)
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !gone {
            cleared.left.push(format!(
                "its buckets' password, still kept here: lnk storage leave {} removes it",
                owner.join(" ")
            ));
        }
    }
    for note in cleared.notes {
        out.block(Note(Sentence::adopt(&note)));
    }
    crate::removing::left_behind(out, &Name::new("agent", &settings.name), &cleared.left);
    moved::prune();
    Ok(Done::success(sentence!(
        "{} deleted {}'s settings, state and keys.",
        Foreign::new(&place),
        Name::new("agent", &settings.name)
    )))
}

#[cfg(test)]
mod tests;
