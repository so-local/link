//! `lnk agent move`, on your computer: the copy and the switch, each
//! machine's `send` piped into the other's `receive`, and the agent taken
//! back where it was when the switch fails.

use link_format::WithinExt;
use std::process::Stdio;

use anyhow::{bail, ensure};
use link_agent::adapter::Adapter;
use link_agent::grants::Grants;
use link_agent::hosted;
use link_agent::moved;
use link_agent::sandbox_file::{self};
use link_agent::settings::{self};
use link_format::{
    Done, Exit, Foreign, Name, Note, Out, Question, Report, Sentence, Steps, cmd, phrase, prose,
    sentence,
};

use super::journal::{End, Journal, Step, Undone, hosted_unended, recover, undo_there, went};
use super::machines::{Dest, bucket_credentials, clouds, destination, reach, ready_box, start_box};
use super::receiving::{Target, settle_and_start};
use super::sending::{Source, human};
use super::{Header, find, hosted_place, lnk_at, place};

pub fn move_to(
    out: &mut Out,
    named: Option<&str>,
    to: &link_workplace::Target,
    image: Option<&str>,
    no_start: bool,
    yes: bool,
) -> Result<Done, Report> {
    // A box and a cloud account are the box and cloud modules'.
    if *to != link_workplace::Target::Here {
        link_core::ensure("box", &format!("lnk agent move {to}"))?;
        link_core::ensure("cloud", &format!("lnk agent move {to}"))?;
    }
    let mut found = find(named)?;
    let id = found.id.clone().ok_or_else(|| {
        Report::error(sentence!(
            "{} has no ID yet.",
            Name::new("agent", &found.name)
        ))
        .next(cmd!("lnk upgrade"), "upgrade lnk where it runs")
    })?;
    link_machine::machine::check_id(&id)?;
    // One move of an agent at a time, from this computer.
    let _moving = moving_lock(&id, &found.name)?;
    // One that ended before it finished, finished or taken back first:
    // the agent may be elsewhere by then.
    if recover(out, &id, false)? {
        found = find(named)?;
        if found.id.as_deref() != Some(id.as_str()) {
            return Err(Report::error(sentence!(
                "{} is another agent now.",
                Name::new("agent", &found.name)
            ))
            .exit(Exit::Usage)
            .next(
                cmd!("lnk agent move {}", Foreign::new(&to.to_string())),
                "move it again",
            ));
        }
    }
    let name = found.name.clone();
    let agent = Name::new("agent", &name);
    let from = found.at.clone();
    // A box says which agent it has, by ID: never one this computer
    // knows went elsewhere. So a box can't take another agent's place
    // here: its note, its buckets' key on the box it's on.
    let note = moved::of_id(&id)?;
    if let (Some(box_name), Some(note)) = (&from, &note)
        && note.to.name != *box_name
    {
        return Err(Report::error(sentence!(
            "{} says it has agent {} ({}), which runs on {} by this computer's note.",
            Name::new("box", box_name),
            &agent,
            Foreign::new(&id),
            Foreign::new(&note.place())
        ))
        .why(prose!(
            "Not moving it. If it went from there to {} from another computer, delete \
             ~/.lnk/safe/agents/transfers/moved/{}.toml and move it again.",
            Name::new("box", box_name),
            Foreign::new(&id)
        )));
    }
    let to = match image {
        // Always a new box: one from the image, set up already.
        Some(image) => {
            let link_workplace::Target::Cloud { cloud, .. } = to else {
                return Err(Report::error(sentence!(
                    "{} starts a new box from your image, in a cloud account: {}.",
                    Foreign::new("--image"),
                    cmd!("--cloud <cloud>")
                ))
                .exit(Exit::Usage));
            };
            if !clouds().iter().any(|c| c == cloud) {
                return Err(Report::error(sentence!(
                    "{} starts a new box from your image, in a cloud account: {} isn't one of \
                     yours.",
                    Foreign::new("--image"),
                    Name::new("cloud", cloud)
                ))
                .exit(Exit::Usage)
                .next(cmd!("lnk cloud list"), "list your cloud accounts"));
            }
            Dest::Image {
                account: cloud.clone(),
                image: image.to_string(),
            }
        }
        None => destination(out, to, &name)?,
    };
    // A member of a team goes alone only where the team's guarantees
    // hold, refused before anything is made for the move: of a team a
    // cloud hosting agents keeps, through the ways back that cloud says it
    // has; of a team kept here, which has none.
    let own = found
        .here
        .as_ref()
        .map(|settings| sandbox_file::of(&settings.name).spec.teams);
    let refused =
        |err: anyhow::Error| Report::error(Sentence::adopt(&format!("{err:#}"))).exit(Exit::Usage);
    // Each team a cloud keeps that keeps it where it goes goes with it,
    // as that cloud says it now: its file and where it's kept.
    let teams =
        hosted::check_member_move(&id, own.as_ref(), |account| to.toward_hosted(&account.name))
            .map_err(refused)?;
    settings::check_member_move(&id, own.as_ref(), to.toward(), &teams.kept_there)
        .map_err(refused)?;
    let hosted_target = match &to {
        Dest::Hosted(account) => {
            if found.runs_agents {
                return Err(Report::error(sentence!(
                    "Move {} here before moving it to another hosted account.",
                    &agent
                ))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk agent move here --agent {}", &agent),
                    "bring it here first",
                ));
            }
            if no_start {
                return Err(Report::error(sentence!(
                    "A hosted account starts the agent at the switch."
                ))
                .why(prose!(
                    "{} is only for a machine.",
                    Foreign::new("--no-start")
                ))
                .exit(Exit::Usage));
            }
            // Asked before its move there starts.
            super::hosted_billed(out, account, &name, yes)?;
            Some(Target::hosted(&found, &id, account)?)
        }
        _ => None,
    };
    if let Some(to) = to.known()
        && !found.runs_agents
        && from == to
    {
        return Err(Report::error(sentence!(
            "{} is on {} already.",
            &agent,
            Foreign::new(&place(&to))
        ))
        .exit(Exit::Usage));
    }
    // From a cloud account that hosts it: the account says whether it may
    // go now, before anything is made for it.
    let source = match found.runs_agents {
        true => {
            let account_name = from.clone().unwrap_or_default();
            let account = link_agent_place::account(&account_name).within(format!(
                "{name} runs on {}, which isn't a cloud account here: lnk cloud list",
                hosted_place(&account_name)
            ))?;
            let reply = hosted::call(&account, "agent-out", |r| {
                r.agent = Some(link_agent_place::HostedAgent {
                    id: id.clone(),
                    name: name.clone(),
                    ..Default::default()
                });
            })?;
            hosted::unless_refused(&account.name, &reply)?;
            let move_id = hosted::move_of(&reply, "agent-out")?;
            if let Some(bytes) = reply.size {
                out.block(Note(sentence!(
                    "{} holds {} of {}'s files and memory.",
                    Foreign::new(&hosted_place(&account.name)),
                    Foreign::new(&human(bytes)),
                    &agent
                )));
            }
            Source::Hosted {
                account: Box::new(account),
                move_id,
            }
        }
        false => Source::Machine(
            from.clone(),
            match &hosted_target {
                Some(Target::Hosted { move_id, .. }) => move_id.clone(),
                _ => link_agent_contract::moving::new_move_id()?,
            },
        ),
    };
    let from_place = source.place();
    if let Some(settings) = &found.here
        && settings.model.is_none()
    {
        return Err(Report::error(sentence!("{} isn't set up yet.", &agent))
            .next(cmd!("lnk agent start"), "set it up"));
    }
    // A model on this computer can't come to a box: a key for one there.
    let local_model = found
        .here
        .as_ref()
        .and_then(|s| s.model.as_ref())
        .is_some_and(|m| m.is_local());
    let key = match () {
        () if hosted_target.is_none() && from.is_none() && to != Dest::Here && local_model => {
            match settings::key_from_env(None)? {
                Some(key) => Some(key),
                None => {
                    if !out.can_ask() {
                        return Err(Report::error(sentence!(
                            "Your agent uses a model on this computer, which a box can't reach."
                        ))
                        .why(prose!(
                            "Set an API key for it there in {}.",
                            Foreign::new(settings::KEY_VARS_TEXT)
                        ))
                        .exit(Exit::Usage));
                    }
                    Some(
                        out.ask(
                            Question::hidden(sentence!(
                                "An API key for it there (Anthropic, OpenAI or OpenRouter)?"
                            ))
                            .why(prose!(
                                "Your agent uses a model on this computer, which a box can't \
                                 reach."
                            )),
                        )?,
                    )
                }
            }
        }
        () => None,
    };
    if let Some(key) = &key {
        crate::model::hosted(key, None).inspect_err(|_| {
            source.abort(out);
        })?;
    }
    // Leaving this computer: what its harness needs beyond Link's
    // defaults, asked here before anything moves (and allowed for its
    // harness here too); the answer goes with it.
    let grants = match &found.here {
        Some(settings) if hosted_target.is_none() => {
            let harness = settings.harness();
            if let Ok(adapter) = Adapter::find(harness) {
                crate::lossy(crate::permissions::answer_needs(
                    out, settings, harness, &adapter,
                ))?;
            }
            Some(Grants::load(harness)?)
        }
        _ => None,
    };
    // Until the copy's switch: a failure gives the agent back to where it
    // was, and the account that hosts it ends its move.
    let readied = (|| -> anyhow::Result<_> {
        // A cloud with no box of yours: one made there now, as `lnk box
        // create`.
        let to = match to {
            Dest::Here => None,
            Dest::Box(box_name) => Some(box_name),
            Dest::New(account) => Some(start_box(out, &account, None, yes)?),
            Dest::Image { account, image } => Some(start_box(out, &account, Some(&image), yes)?),
            Dest::Hosted(_) => None,
        };
        // Both ends answer.
        let from_box = from.as_ref().filter(|_| !found.runs_agents);
        for box_name in [from_box, to.as_ref()].into_iter().flatten() {
            reach(box_name)?;
        }
        Ok(to)
    })();
    let to = readied.inspect_err(|_| {
        source.abort(out);
        if let Some(target) = &hosted_target {
            target.abort(out);
        }
    })?;
    let target = hosted_target.unwrap_or_else(|| Target::Machine(to.clone()));
    let mut steps = Steps::new(1);

    // A box has no account of yours: credentials reaching only its buckets.
    let bucket_credentials = match &to {
        Some(_) => bucket_credentials(&id),
        None => None,
    };
    // A box starts with lnk alone: the harness, its sandbox, and for
    // backups the storage module, before the agent comes.
    if let Some(box_name) = &to {
        ready_box(
            box_name,
            bucket_credentials.is_some(),
            found.harness.as_deref(),
        )
        .inspect_err(|_| {
            source.abort(out);
        })?;
    }
    // On a box, always behind its proxy, from the box's own address (never
    // the cloud's metadata service or the provider's network).
    let upstream = to.as_ref().map(|_| "direct".to_string());
    out.step(
        &mut steps,
        phrase!(
            "Moving {} from {} to {}",
            &agent,
            Foreign::new(&from_place),
            Foreign::new(&target.place())
        ),
    );
    // Leaving this computer, it keeps what you granted it here; this
    // computer never takes that from a box's word (`receive`).
    let sandbox = match &found.here {
        Some(settings) if from.is_none() => Some(sandbox_file::load(&settings.name)?),
        _ => None,
    };
    let header = Header {
        key,
        sealed: None,
        upstream,
        bucket_credentials,
        sandbox,
        grants,
        yes,
        teams: teams.carried,
    };
    // The copy, while it answers where it is; then the switch, paused
    // there: put in place there, not started; what its harness needs
    // there asked here, in this terminal; then started. Refused, or
    // failing, the copy there goes again (`left`, as a move away would),
    // and it answers where it was: one copy, never two.
    let mut switched = None;
    let mut machine = None;
    let mut hosted_machine = None;
    // How far it got, noted as it goes: a move that ends without a word
    // (killed, the computer off) is finished or taken back by the next
    // `move` or `start` of it here.
    let mut journal = Journal {
        name: name.clone(),
        id: id.clone(),
        service: found.service.clone(),
        move_id: source.move_id().to_string(),
        from: match (&from, found.runs_agents) {
            (Some(account), true) => End::Hosted(account.clone()),
            (Some(box_name), false) => End::Box(box_name.clone()),
            (None, _) => End::Here,
        },
        to: match (&target, &to) {
            (Target::Hosted { account, .. }, _) => End::Hosted(account.name.clone()),
            (_, Some(box_name)) => End::Box(box_name.clone()),
            (_, None) => End::Here,
        },
        no_start,
        without: if matches!(
            &target,
            Target::Hosted {
                managed_models: true,
                ..
            }
        ) {
            vec![
                link_agent_identity::own::OwnSecret::Model.to_use()?,
                link_agent_identity::own::OwnSecret::Fallback.to_use()?,
            ]
        } else {
            Vec::new()
        },
        step: Step::Copy,
        at: None,
    };
    journal.save().inspect_err(|_| {
        source.abort(out);
    })?;
    // From the pause until it answers where it went, Ctrl-C or a closed
    // terminal ends the move as a failure does, never lnk midway.
    let guard = Unstoppable::new();
    let moved = (|| -> anyhow::Result<()> {
        let have = target.have(&id)?;
        let have = source.copy(out, &name, &id, &target, &have)?;
        let started = std::time::Instant::now();
        journal.reached(Step::Paused)?;
        source.pause(out, &name)?;
        guard.check()?;
        // Where its keys go, noted before they're sent: `lnk secret
        // remove` reaches them there.
        if let Some(destination) = target.keys_at()
            && let Some(service) = &journal.service
        {
            link_secret_contract::copied_without(service, destination, journal.without.clone())?;
        }
        hosted_machine = source.switch(out, &name, &id, &target, &header, &have)?;
        // An account answers as it takes the switch.
        journal.at = target.keys_at().filter(|_| target.is_hosted());
        journal.reached(Step::Switched)?;
        guard.check()?;
        if !no_start && !target.is_hosted() {
            settle_and_start(out, &name, &id, &from_place, &to)?;
        }
        guard.check()?;
        switched = Some(started.elapsed());
        // Done once it answers there; else its copy there goes again.
        machine = journal
            .at
            .clone()
            .or_else(|| arrived(&to, &name, &id, no_start));
        let Some(at) = &machine else {
            bail!(
                "{name} came to {} but didn't answer there as running: lnk agent show --agent \
                 {name} there says why",
                place(&to)
            );
        };
        journal.at = Some(at.clone());
        journal.reached(Step::Answered)
    })();
    if let Err(err) = moved {
        let ended = target.abort(out);
        // Its copy there removed, and it answering where it was, before
        // anything is said.
        // An account's copy is gone only once it says the move is over.
        let undone = match journal.step {
            _ if target.is_hosted() && !ended => hosted_unended(journal.step),
            Step::Copy => Undone::Gone,
            _ if target.is_hosted() => Undone::Gone,
            _ => undo_there(out, &journal, &source),
        };
        if let Undone::Held = undone {
            drop(guard);
            out.block(
                Report::warning(sentence!(
                    "The move didn't finish, and {} may run on {}, which doesn't answer or \
                     couldn't stop it.",
                    &agent,
                    Foreign::new(&target.place())
                ))
                .why(prose!(
                    "It stays stopped on {}, so it never answers in both. Moving it again once \
                     {} answers finishes the move or takes it back.",
                    Foreign::new(&from_place),
                    Foreign::new(&target.place())
                ))
                .next(
                    match (&to, &target) {
                        (Some(box_name), _) => cmd!(
                            "lnk agent move --on {} --agent {}",
                            Name::new("box", box_name),
                            &agent
                        ),
                        (None, Target::Hosted { account, .. }) => cmd!(
                            "lnk agent move --cloud {} --agent {}",
                            Name::new("cloud", &account.name),
                            &agent
                        ),
                        (None, _) => cmd!("lnk agent move --here --agent {}", &agent),
                    },
                    "finish or take it back",
                ),
            );
            return Err(err.into());
        }
        let resumed = source.resume(out, &name);
        drop(guard);
        let from_hosted = matches!(source, Source::Hosted { .. });
        if !resumed && from_hosted {
            // An account that didn't say it answers there again: kept as
            // far as it got, to tell it again.
            out.block(Report::warning(sentence!(
                "The move didn't finish, and {} didn't say {} answers there again: the next \
                 {} or {} of it here tells it again.",
                Foreign::new(&from_place),
                &agent,
                cmd!("lnk agent move"),
                cmd!("lnk agent start")
            )));
            return Err(err.into());
        }
        match undone {
            Undone::Kept => {
                let _ = journal.reached(Step::Back);
            }
            _ => journal.remove(),
        }
        out.block(Note(sentence!(
            "The move didn't finish; {} answers on {} again.",
            &agent,
            Foreign::new(&from_place)
        )));
        if !resumed {
            out.block(
                Report::warning(sentence!("It didn't start."))
                    .next(cmd!("lnk agent start"), "try again there"),
            );
        }
        match undone {
            Undone::Kept if target.is_hosted() => out.block(Report::warning(sentence!(
                "{} didn't say the move is over: the next {} or {} of {} here tells it again.",
                Foreign::new(&target.place()),
                cmd!("lnk agent move"),
                cmd!("lnk agent start"),
                &agent
            ))),
            Undone::Kept => out.block(Report::warning(sentence!(
                "{} doesn't answer: its copy there, if one came, is stopped, and the next {} or \
                 {} of {} here removes it.",
                Foreign::new(&place(&to)),
                cmd!("lnk agent move"),
                cmd!("lnk agent start"),
                &agent
            ))),
            _ => {}
        }
        return Err(err.into());
    }
    drop(guard);
    target.complete();
    if let Some(took) = switched {
        out.block(Note(sentence!(
            "Switched in {} s.",
            format!("{:.1}", took.as_secs_f64())
        )));
    }
    let machine = machine.within("where it went")?;
    // A copy where it was that couldn't be removed is said there, its
    // journal kept for the next move or start to remove it.
    let _went = went(out, &journal, &source, &machine)?;
    Ok(Done::success(sentence!(
        "{} runs on {} now{}.",
        &agent,
        Foreign::new(&target.place()),
        hosted_machine
            .map(|m| format!(" (on its machine {m})"))
            .unwrap_or_default()
    ))
    .next(
        match to.is_some() {
            true => cmd!("lnk agent move here --agent {}", &agent),
            false => cmd!("lnk agent move --on <box> --agent {}", &agent),
        },
        "move it again",
    ))
}

/// Agent `name`'s copy on `to`, stopped there and removed again (`lnk
/// agent left`, as a move away would) after a switch that didn't end with
/// it answering there: one copy, the one it answers from where it was,
/// `from`. Whether it was; said when it can't be, but `quiet`ly when it
/// may not be there at all.
pub(crate) fn take_back(
    out: &mut Out,
    name: &str,
    to: &Option<String>,
    source: &Source,
    from: &End,
    quiet: bool,
) -> bool {
    let back = match (source, from) {
        (Source::Hosted { account, .. }, _) => Some(account.name.clone()),
        (_, End::Box(box_name)) => Some(box_name.clone()),
        _ => link_machine::installation::installation()
            .ok()
            .map(|m| m.name),
    };
    // Started there already: stopped first, as only a stopped agent can
    // have left.
    let _ = lnk_at(to, name, &["stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let mut left = lnk_at(to, name, &["left"]);
    left.arg(if matches!(from, End::Hosted(_)) {
        "--cloud"
    } else {
        "--on"
    });
    left.arg(back.unwrap_or_else(|| "this-computer".into()));
    if *from == End::Here
        && let Ok(machine) = link_machine::installation::installation()
    {
        left.args(["--machine-id", &machine.id]);
    }
    let removed = left
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !removed && !quiet {
        out.block(Report::warning(sentence!(
            "Its copy on {} is stopped but couldn't be removed: don't start it there, where it \
             would answer beside the one on {}.",
            Foreign::new(&place(to)),
            Foreign::new(&source.place())
        )));
    }
    removed
}

/// Set while an [`Unstoppable`] is: Ctrl-C, a closed terminal or a
/// stop came.
pub(crate) static STOP_ASKED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `/dev/null`, open for [`stop_asked`] to put in place of stdin.
pub(crate) static NULL_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// SIGINT, SIGHUP or SIGTERM while an [`Unstoppable`] is: noted, and
/// stdin ended, so a question waiting on it is answered no.
extern "C" fn stop_asked(_: libc::c_int) {
    STOP_ASKED.store(true, std::sync::atomic::Ordering::SeqCst);
    let null = NULL_FD.load(std::sync::atomic::Ordering::SeqCst);
    if null >= 0 {
        // SAFETY: dup2 is async-signal-safe, on descriptors this
        // process holds.
        unsafe { libc::dup2(null, 0) };
    }
}

/// While it lives, Ctrl-C, a closed terminal or a stop doesn't end
/// `lnk` between a move's pause and the agent answering where it went:
/// what `lnk` runs then ends as it would, the question it asks is
/// answered no, and the move fails as any failure does ([`Self::check`]),
/// the agent answering where it was. As it drops, they end `lnk` again.
pub(crate) struct Unstoppable;

pub(crate) const STOPS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGHUP, libc::SIGTERM];

impl Unstoppable {
    fn new() -> Self {
        STOP_ASKED.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Ok(null) = std::fs::File::open("/dev/null") {
            NULL_FD.store(
                std::os::fd::IntoRawFd::into_raw_fd(null),
                std::sync::atomic::Ordering::SeqCst,
            );
        }
        for signal in STOPS {
            // SAFETY: the handler only stores and calls dup2.
            unsafe { libc::signal(signal, stop_asked as *const () as libc::sighandler_t) };
        }
        Unstoppable
    }

    /// An error once a stop came.
    fn check(&self) -> anyhow::Result<()> {
        ensure!(
            !STOP_ASKED.load(std::sync::atomic::Ordering::SeqCst),
            "stopped (Ctrl-C, or its terminal closed)"
        );
        Ok(())
    }
}

impl Drop for Unstoppable {
    fn drop(&mut self) {
        for signal in STOPS {
            // SAFETY: back to what ends the process.
            unsafe { libc::signal(signal, libc::SIG_DFL) };
        }
    }
}

/// The lock of agent `id`'s (`name`'s) move from this computer, held
/// while it lives: `~/.lnk/safe/agents/transfers/moving/<id>.lock`. Refused while
/// another move of it holds it.
pub(crate) fn moving_lock(id: &str, name: &str) -> anyhow::Result<MovingLock> {
    try_moving_lock(id)?.within(format!(
        "{name} is being moved already, by another lnk agent move: wait for it"
    ))
}

/// [`moving_lock`], none while another move of it holds it.
pub(crate) fn try_moving_lock(id: &str) -> anyhow::Result<Option<MovingLock>> {
    let dir = link_files::layout::Hidden::Moving.path();
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700)
        .create(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{id}.lock")))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(MovingLock(lock))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(err)) => Err(err.into()),
    }
}

/// A move's lock, let go of by name as it ends, never only closed: a
/// lock is its open file's, which a child this process forks shares
/// until it runs its program (the file is close-on-exec, so no longer),
/// and a closed one could be held that while yet.
pub(crate) struct MovingLock(std::fs::File);

impl Drop for MovingLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Whether agent `name` answers on `to` as agent `id`, running (or only
/// there, with `no_start`), a few times over a few seconds; the machine
/// it's on, if it does.
#[expect(
    clippy::disallowed_methods,
    reason = "backs off before asking where it arrived again"
)]
pub(crate) fn arrived(
    to: &Option<String>,
    name: &str,
    id: &str,
    no_start: bool,
) -> Option<crate::moved::Place> {
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        match there(to, name, id) {
            There::Running(at) => return Some(at),
            There::Stopped(at) if no_start => return Some(at),
            _ => {}
        }
    }
    None
}

/// What of agent `id` is on `to`, as it answers there once.
pub(crate) enum There {
    /// Running there, on that machine.
    Running(crate::moved::Place),
    /// There, stopped.
    Stopped(crate::moved::Place),
    /// Not there: that machine answers, listing no agent of that ID.
    Absent,
    /// That machine answers, but not what of it is there: its status
    /// failed, and its list did or names it.
    Unknown,
    /// That machine doesn't answer.
    Unreachable,
}

/// Whether `to`'s own agents (`lnk agent list --here --json`) include
/// agent `id`; none when it doesn't answer as a list.
fn lists(to: &Option<String>, id: &str) -> Option<bool> {
    let mut list = link_core::lnk();
    if let Some(name) = to {
        list.args(["box", "run", name, "--"]);
    }
    let listed = list
        .args(["agent", "list", "--here", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())?;
    let agents = listed["agents"].as_array()?;
    Some(
        agents
            .iter()
            .any(|agent| crate::listing::workload_of(agent).as_deref() == Some(id)),
    )
}

/// [`There`] agent `name` (`id`) is on `to`.
pub(crate) fn there(to: &Option<String>, name: &str, id: &str) -> There {
    let status = lnk_at(to, name, &["show", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    let Some(status) = status else {
        // No word of it: whether the box answers at all, then whether it
        // lists the agent. Only a list without it says it's absent.
        if let Some(name) = to
            && reach(name).is_err()
        {
            return There::Unreachable;
        }
        return match lists(to, id) {
            Some(false) => There::Absent,
            _ => There::Unknown,
        };
    };
    if crate::listing::workload_of(&status).as_deref() != Some(id) {
        return There::Absent;
    }
    let at = crate::moved::Place {
        cloud: false,
        id: crate::listing::machine_of(&status),
        // A box by the name you type for it; this computer by its own.
        name: to.clone().unwrap_or_else(|| {
            status["machine"]["name"]
                .as_str()
                .unwrap_or("here")
                .to_string()
        }),
        owner: None,
    };
    match status["running"] == true {
        true => There::Running(at),
        false => There::Stopped(at),
    }
}
