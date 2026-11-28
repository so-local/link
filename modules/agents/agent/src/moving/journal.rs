//! A move's journal, on the computer running it: `~/.lnk/safe/agents/transfers/moving/<id>.toml`,
//! beside its lock, one for each of the agent's moves not over yet. It
//! says where the agent goes from and to, and how far the move got. A move whose `lnk` ended without a word (killed, the
//! computer off or out of battery) leaves it behind, and the next `lnk
//! agent move` or `lnk agent start` of that agent here finishes the move
//! or takes it back, so the agent answers in one place, never two and
//! never none.

use link_format::WithinExt;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::bail;
use link_agent::moved;
use link_format::{Foreign, Name, Note, Out, Report, cmd, sentence};
use serde::{Deserialize, Serialize};

use super::away::{There, take_back, there, try_moving_lock};
use super::machines::boxes;
use super::sending::{Source, abort, said};
use super::{hosted_place, place};

/// One end of a move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "name")]
pub(crate) enum End {
    /// This computer.
    Here,
    /// A box, by its name.
    Box(String),
    /// A cloud account that hosts agents, by its name.
    Hosted(String),
}

impl End {
    /// As the move names a machine: a box's name, or none for this
    /// computer; none at all for an account.
    fn machine(&self) -> Option<Option<String>> {
        match self {
            End::Here => Some(None),
            End::Box(name) => Some(Some(name.clone())),
            End::Hosted(_) => None,
        }
    }

    fn place(&self) -> String {
        match self {
            End::Here => place(&None),
            End::Box(name) => place(&Some(name.clone())),
            End::Hosted(account) => hosted_place(account),
        }
    }
}

/// How far a move got.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Step {
    /// The copy, the agent answering where it was.
    Copy,
    /// Paused where it was for the switch: nothing of it started where
    /// it goes (`receive --no-start`), but an account that hosts agents
    /// starts it as it takes its switch.
    Paused,
    /// Put in place where it goes, or taken there, and maybe started.
    Switched,
    /// Answering where it went: where it was is to delete its copy.
    Answered,
    /// Answering where it was again: a copy stopped where it was going
    /// is to be removed.
    Back,
}

/// A move of an agent from this computer, as far as it got.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub(crate) name: String,
    pub(crate) id: String,
    /// The service it acts as, which its secrets are given to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) service: Option<String>,
    pub(crate) move_id: String,
    pub(crate) from: End,
    pub(crate) to: End,
    #[serde(default)]
    pub(crate) no_start: bool,
    /// Credentials kept at the source when a host supplies its models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) without: Vec<link_secret_contract::Use>,
    pub(crate) step: Step,
    /// The machine it answers on, once it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) at: Option<moved::Place>,
}

/// Agent `id`'s journals, its moves not over yet: a move ended `back`
/// (its stopped copy kept where it went, to remove) beside the move after
/// it, never saved over.
#[derive(Default, Serialize, Deserialize)]
struct Journals {
    #[serde(default, rename = "move")]
    moves: Vec<Journal>,
}

link_files::specification!(Journals, include_str!("journal/journals.schema.toml"));

/// Each agent's moves under way, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Journals>()
}

impl Journals {
    fn path(id: &str) -> PathBuf {
        link_files::layout::Hidden::Moving
            .path()
            .join(format!("{id}.toml"))
    }

    /// Agent `id`'s, those a move left. A journal this `lnk` can't read,
    /// as one a newer release wrote, is refused, naming the upgrade: never
    /// taken for none, which would forget a move under way.
    fn load(id: &str) -> anyhow::Result<Journals> {
        Self::load_from(&Self::path(id), id)
    }

    /// [`Journals::load`] from the file at `path`.
    fn load_from(path: &std::path::Path, id: &str) -> anyhow::Result<Journals> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Journals::default()),
            Err(e) => return Err(e).within(format!("reading {}", path.display())),
        };
        let mut journals = link_files::saved::parse::<Journals>(&text)
            .within(format!("reading {}", path.display()))?;
        journals.moves.retain(|j| j.id == id);
        Ok(journals)
    }

    /// Saved, agent `id`'s file gone once none is left.
    fn save(&self, id: &str) -> anyhow::Result<()> {
        if self.moves.is_empty() {
            let _ = std::fs::remove_file(Self::path(id));
            return Ok(());
        }
        link_files::write_private(&Self::path(id), link_files::saved::print(self)?.as_bytes())?;
        Ok(())
    }
}

/// Whether a move of agent `id` on this computer has it paused or
/// switching: not answering where it was, maybe not yet where it goes.
/// A team's move under way here counts for each of its members.
pub(crate) fn switching(id: &str) -> anyhow::Result<bool> {
    let team_moving = std::fs::read_dir(link_files::layout::Hidden::TeamMoving.path())
        .is_ok_and(|mut folders| folders.next().is_some());
    Ok(team_moving
        || Journals::load(id)?
            .moves
            .iter()
            .any(|j| matches!(j.step, Step::Paused | Step::Switched)))
}

impl Journal {
    /// A move to a box taken back: the key made for that box revoked, and
    /// one made again where the agent stays, on a box
    /// ([`super::buckets::key_back`]).
    pub(crate) fn key_back(&self, out: &mut Out) {
        if let End::Box(_) = &self.to {
            let stays_on = match &self.from {
                End::Box(name) => Some(name.as_str()),
                _ => None,
            };
            if !matches!(self.from, End::Hosted(_)) {
                super::buckets::key_back(out, &self.id, stays_on);
            }
        }
    }

    pub(crate) fn save(&self) -> anyhow::Result<()> {
        let mut journals = Journals::load(&self.id)?;
        journals.moves.retain(|j| j.move_id != self.move_id);
        journals.moves.push(self.clone());
        journals
            .save(&self.id)
            .within("noting how far the move got")
    }

    /// Step `step` reached, noted before what comes after it.
    pub(crate) fn reached(&mut self, step: Step) -> anyhow::Result<()> {
        self.step = step;
        self.save()
    }

    /// The move is over: nothing of it is left to finish or take back.
    pub(crate) fn remove(&self) {
        // Unreadable, the file is left as it is, for the next move or
        // start to refuse by name.
        let Ok(mut journals) = Journals::load(&self.id) else {
            return;
        };
        journals.moves.retain(|j| j.move_id != self.move_id);
        let _ = journals.save(&self.id);
    }

    /// Where the move takes it from, as the move does.
    fn source(&self) -> anyhow::Result<Source> {
        Ok(match &self.from {
            End::Here => Source::Machine(None, self.move_id.clone()),
            End::Box(name) => Source::Machine(Some(name.clone()), self.move_id.clone()),
            End::Hosted(account) => Source::Hosted {
                account: Box::new(link_agent_place::account(account).within(format!(
                    "{} isn't a cloud account here",
                    hosted_place(account)
                ))?),
                move_id: self.move_id.clone(),
            },
        })
    }

    /// The account hosting the end `end`, told the move is over: whether
    /// it says so, as an end no account hosts does.
    fn abort_at(&self, out: &mut Out, end: &End) -> bool {
        let End::Hosted(name) = end else {
            return true;
        };
        link_agent_place::account(name)
            .is_some_and(|account| said(out, abort(&account, &self.move_id)))
    }
}

/// What became of a move that didn't finish, for where it was going.
pub(crate) enum Undone {
    /// Nothing of it is left there.
    Gone,
    /// A copy stopped there that couldn't be removed yet, or an account
    /// that didn't say the move is over: the journal keeps it, for the
    /// next move or start to finish.
    Kept,
    /// It may run there: started, and that machine doesn't answer or
    /// couldn't stop it. It stays stopped where it was, so it never
    /// answers in two places.
    Held,
}

/// Where agent `journal` was going, after a move that didn't finish
/// there: its copy there stopped and removed, or the account there told
/// the move is over (`agent-abort`). On a box that doesn't answer, or an
/// account that doesn't say so, kept for later, unless it may have
/// started there.
pub(crate) fn undo_there(out: &mut Out, journal: &Journal, source: &Source) -> Undone {
    let Some(to) = journal.to.machine() else {
        return match journal.abort_at(out, &journal.to) {
            true => Undone::Gone,
            false => hosted_unended(journal.step),
        };
    };
    let may_run = journal.step == Step::Switched && !journal.no_start;
    match there(&to, &journal.name, &journal.id) {
        // It may be there, its status failing: removed if it is.
        There::Absent => {
            take_back(
                out,
                &journal.name,
                &journal.id,
                &to,
                source,
                &journal.from,
                true,
            );
            Undone::Gone
        }
        // Running there and not removed, so maybe not stopped: it stays
        // stopped where it was.
        There::Running(_) => {
            match take_back(
                out,
                &journal.name,
                &journal.id,
                &to,
                source,
                &journal.from,
                false,
            ) {
                true => Undone::Gone,
                false => Undone::Held,
            }
        }
        There::Stopped(_) => {
            match take_back(
                out,
                &journal.name,
                &journal.id,
                &to,
                source,
                &journal.from,
                false,
            ) {
                true => Undone::Gone,
                false => Undone::Kept,
            }
        }
        // What of it is there can't be read: removed if it can be, else
        // held stopped where it was while it may run there.
        There::Unknown => match take_back(
            out,
            &journal.name,
            &journal.id,
            &to,
            source,
            &journal.from,
            false,
        ) {
            true => Undone::Gone,
            false if may_run => Undone::Held,
            false => Undone::Kept,
        },
        There::Unreachable if !listed(&to) => Undone::Gone,
        There::Unreachable if may_run => Undone::Held,
        There::Unreachable => Undone::Kept,
    }
}

/// A move to an account that hosts agents, which didn't say it's over:
/// one the switch may have reached, which starts the agent as it takes
/// it, holds it stopped where it was; one still copying is kept.
pub(crate) fn hosted_unended(step: Step) -> Undone {
    match step {
        Step::Paused | Step::Switched => Undone::Held,
        _ => Undone::Kept,
    }
}

/// Whether box `to` is one of yours still: lost (removed, or its cloud
/// gone), nothing of the agent can run there.
fn listed(to: &Option<String>) -> bool {
    let Some(name) = to else { return true };
    boxes(false).map_or(true, |boxes| boxes.iter().any(|b| b.name == *name))
}

/// The agent answers on `machine` now, as agent `journal`'s move took it:
/// where it was deletes its copy, and what this computer notes follows
/// it. The journal goes once that copy has gone; while it stays (`lnk
/// agent left` refused there, its keys kept), the journal is kept at
/// [`Step::Answered`], so the next move or start of it here removes it
/// before anything starts. Whether it went, or an error, before any of
/// it, when this computer's note of the agent can't be read.
/// `new_box_keyless`: moving from a box to another, no key reaching its
/// buckets could be made for the new one.
pub(crate) fn went(
    out: &mut Out,
    journal: &Journal,
    source: &Source,
    machine: &moved::Place,
    new_box_keyless: bool,
) -> anyhow::Result<bool> {
    let to = journal.to.machine().flatten();
    let hosted_target = journal.to.machine().is_none();
    let id = &journal.id;
    let note = moved::of_id(id)?;
    if !source.left(out, &journal.name, id, &to, machine) {
        return Ok(false);
    }
    if let Some(from) = source.keys_at()
        && let Some(service) = &journal.service
    {
        // Its keys went with its copy there.
        let _ = link_secret_contract::gone_from_without(service, &from, journal.without.clone());
    }
    let from_box = match &journal.from {
        End::Box(name) => Some(name),
        _ => None,
    };
    // From one box to another: this computer's note follows it.
    if let (Some(mut note), Some(_)) = (note, from_box)
        && (to.is_some() || hosted_target)
    {
        note.to = machine.clone();
        let _ = note.save();
    }
    // Here, it backs up with your accounts; on an account that hosts it,
    // with that account's: the box's keys go. To another box, the key
    // made for that box replaced them, unless none could be made
    // (`new_box_keyless`): then the old box's go too, and the new box has
    // no backups.
    if let Some(from_box) = from_box
        && (to.is_none() || new_box_keyless)
        && link_core::find_module("storage").is_some()
        && let Ok(owner) = link_agent::owners::agent_flag(id)
    {
        let revoked = link_core::lnk()
            .args(["storage", "revoke"])
            .args(&owner)
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !revoked {
            out.block(Report::warning(sentence!(
                "The key {} had to reach {}'s buckets may still work: {} revokes it.",
                Foreign::new(from_box),
                Name::new("agent", &journal.name),
                Foreign::new(&format!("lnk storage revoke {}", owner.join(" ")))
            )));
        }
        if let (Some(to), true) = (&to, new_box_keyless) {
            out.block(Report::warning(sentence!(
                "{} has no backups on {}: no key reaching its buckets could be made for it.",
                Name::new("agent", &journal.name),
                Foreign::new(to)
            )));
        }
    }
    journal.remove();
    Ok(true)
}

/// `lnk agent start` of agent `name` (`id`), refused while another `lnk
/// agent move` of it on this computer holds its lock: it would take the
/// agent out of its pause midway through that move's switch. A start
/// that's a part of a move, `during_move` (`lnk agent start
/// --during-move`), is let through for a move this computer's journal
/// holds: its own start where it lands, or its take-back's.
pub(crate) fn check_start(id: &str, name: &str, during_move: Option<&str>) -> anyhow::Result<()> {
    if let Some(move_id) = during_move {
        link_agent_contract::moving::check_move_id(move_id)?;
    }
    let lock = link_files::layout::Hidden::Moving
        .path()
        .join(format!("{id}.lock"));
    if !lock.exists() || try_moving_lock(id)?.is_some() {
        return Ok(());
    }
    if let Some(move_id) = during_move
        && Journals::load(id)?
            .moves
            .iter()
            .any(|j| j.move_id == move_id)
    {
        return Ok(());
    }
    bail!("{name} is being moved by another lnk agent move: start it once that move is over")
}

/// An earlier move of agent `id` from this computer that ended without a
/// word, finished or taken back before `lnk agent start` starts it here:
/// nothing while another `lnk agent move` holds it.
pub(crate) fn recover_before_start(out: &mut Out, id: &str) -> anyhow::Result<()> {
    if Journals::load(id)?.moves.is_empty() {
        return Ok(());
    }
    let Some(_lock) = try_moving_lock(id)? else {
        return Ok(());
    };
    recover(out, id, true).map(|_| ())
}

/// An earlier move of agent `id` from this computer that ended without a
/// word (its journal left, its lock held by this `lnk` now), finished
/// where it may have answered, else taken back: whether there was one.
/// `starting`: `lnk agent start` starts it here next, so here it's only
/// let go of its pause. An error, changing nothing, when it can't be told
/// where it answers.
pub(crate) fn recover(out: &mut Out, id: &str, starting: bool) -> anyhow::Result<bool> {
    let mut journals = Journals::load(id)?.moves;
    if journals.is_empty() {
        return Ok(false);
    }
    // Those ended `back` first: the last move's may then go on.
    journals.sort_by_key(|j| j.step != Step::Back);
    for journal in journals {
        recover_one(out, journal, starting)?;
    }
    Ok(true)
}

/// [`recover`] of one move, `journal`.
fn recover_one(out: &mut Out, mut journal: Journal, starting: bool) -> anyhow::Result<()> {
    let id = journal.id.clone();
    let id = id.as_str();
    let (name, from, to) = (
        journal.name.clone(),
        journal.from.place(),
        journal.to.place(),
    );
    let agent = Name::new("agent", &name);
    out.block(Note(sentence!(
        "An earlier move of {} from {} to {} ended before it finished: finishing it, or taking \
         it back.",
        &agent,
        Foreign::new(&from),
        Foreign::new(&to)
    )));
    let source = journal.source()?;
    let answering = match (journal.step, journal.to.machine()) {
        (Step::Copy, _) => {
            // Each account told, whether or not the other says so.
            let ended = [
                journal.abort_at(out, &journal.from),
                journal.abort_at(out, &journal.to),
            ];
            if ended.contains(&false) {
                out.block(Report::warning(sentence!(
                    "It was still copying, but {}'s cloud account didn't say the move is over: \
                     the next {} or {} of it here tells it again.",
                    &agent,
                    cmd!("lnk agent move"),
                    cmd!("lnk agent start")
                )));
                return Ok(());
            }
            journal.remove();
            journal.key_back(out);
            out.block(Note(sentence!(
                "It was still copying: {} answers on {}, as it did.",
                &agent,
                Foreign::new(&from)
            )));
            return Ok(());
        }
        (Step::Answered, _) => journal.at.clone(),
        // An account answers as it takes the switch.
        (Step::Switched, None) => journal.at.clone().or_else(|| match &journal.to {
            End::Hosted(account) => Some(moved::Place::of_account(account, None)),
            _ => None,
        }),
        (step, Some(machine)) if step != Step::Back => match there(&machine, &name, id) {
            There::Running(at) => Some(at),
            There::Stopped(at) if journal.no_start && step == Step::Switched => Some(at),
            _ => None,
        },
        _ => None,
    };
    if let Some(machine) = answering {
        if !went(out, &journal, &source, &machine, false)? {
            bail!(
                "{name} answers on {to}, but its copy on {from} couldn't be removed: it stays \
                 stopped there, so it never answers in both, and the next lnk agent move or lnk \
                 agent start of it here tries again."
            );
        }
        out.block(Note(sentence!(
            "{} answers on {}; {} deleted its copy.",
            &agent,
            Foreign::new(&to),
            Foreign::new(&from)
        )));
        return Ok(());
    }
    let undone = undo_there(out, &journal, &source);
    if let Undone::Held = undone {
        bail!(
            "{name} may run on {}, which doesn't answer or couldn't stop it: it stays stopped on \
             {}, so it never answers in both. lnk agent move again once {} answers finishes the move or \
             takes it back.",
            to,
            from,
            to
        );
    }
    if journal.step != Step::Back {
        let resumed = match (&journal.from, starting) {
            // `start` starts it next.
            (End::Here, true) => true,
            _ => source.resume(out, &name, id),
        };
        journal.key_back(out);
        // An account that doesn't say it answers there again: kept, as
        // far as it got, to tell it again.
        if !resumed && matches!(journal.from, End::Hosted(_)) {
            bail!(
                "{} didn't say {name} answers there again: the next lnk agent move or lnk \
                 agent start of it here tells it again.",
                from
            );
        }
        if !resumed {
            out.block(
                Report::warning(sentence!(
                    "{} didn't start on {}.",
                    &agent,
                    Foreign::new(&from)
                ))
                .next(cmd!("lnk agent start"), "try again there"),
            );
        }
    }
    match undone {
        Undone::Kept => {
            journal.reached(Step::Back)?;
            out.block(Note(sentence!(
                "{} answers on {}; its copy on {}, stopped, is removed once {} answers.",
                &agent,
                Foreign::new(&from),
                Foreign::new(&to),
                Foreign::new(&to)
            )));
        }
        _ => {
            journal.remove();
            out.block(Note(sentence!(
                "{} answers on {}; nothing of it is left on {}.",
                &agent,
                Foreign::new(&from),
                Foreign::new(&to)
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A journal a newer release wrote is refused, naming the upgrade,
    /// never read as no move: a pending move isn't forgotten after a
    /// rollback. One this release wrote reads back, and none is no move.
    #[test]
    fn journals_refuse_a_newer_schema() {
        let fixture = link_test_kit::TempDir::new("journal-newer");
        let path = fixture.canonicalize().unwrap().join("a1.toml");
        assert!(Journals::load_from(&path, "a1").unwrap().moves.is_empty());
        let journals = Journals {
            moves: vec![Journal {
                name: "main".into(),
                id: "a1".into(),
                service: None,
                move_id: "m1".into(),
                from: End::Here,
                to: End::Box("gcp-1".into()),
                no_start: false,
                without: Vec::new(),
                step: Step::Paused,
                at: None,
            }],
        };
        let text = link_files::saved::print(&journals).unwrap();
        std::fs::write(&path, &text).unwrap();
        let read = Journals::load_from(&path, "a1").unwrap();
        assert_eq!(read.moves.len(), 1);
        assert_eq!(read.moves[0].step, Step::Paused);
        let newer = text.replace("schema = 1", "schema = 2");
        assert_ne!(newer, text);
        std::fs::write(&path, newer).unwrap();
        let refused = format!("{:#}", Journals::load_from(&path, "a1").err().unwrap());
        assert!(refused.contains("lnk upgrade"), "{refused}");
        assert!(refused.contains("a1.toml"), "{refused}");
    }

    /// A journal that's there but won't parse stops the move, recovery
    /// and a start's switching check, never read as no move.
    #[test]
    fn an_unreadable_journal_stops_the_move() {
        let fixture = link_test_kit::TempDir::new("journal-unreadable");
        let path = fixture.canonicalize().unwrap().join("a1.toml");
        std::fs::write(&path, "schema = 1\n[[move]\nname = ").unwrap();
        let refused = format!("{:#}", Journals::load_from(&path, "a1").err().unwrap());
        assert!(refused.contains("a1.toml"), "{refused}");
    }
}
