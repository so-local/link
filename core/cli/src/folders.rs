//! `lnk folder list`, `set` and `create`, and the question asked once when
//! the Link folder isn't where it was: where it went, or whether to make a
//! new one. Link never searches the disk for it. Where each location is on
//! this machine is the map's, `~/.lnk/safe/map.toml`.

use std::path::{Path, PathBuf};

use crate::failed;
use link_files::layout::map::{self, Entry};
use link_format::{
    Count, Done, Exit, Foreign, Note, Out, Path as Shown, Question, Report, Table, Value,
    WithinExt, cmd, phrase, prose, sentence,
};
use link_settings::folders::{self, Found};

/// Before a command: the Link folder where this machine's settings say,
/// or the question of where it went, on a terminal, else the commands
/// that answer it.
pub fn check(out: &mut Out) -> Result<(), Report> {
    let Found::Lost(dir) = folders::inspect().map_err(untrusted)? else {
        return Ok(());
    };
    let lost = sentence!(
        "Link can't find your Link folder: it isn't at {} anymore.",
        Shown::new(&dir)
    );
    if !out.can_ask() {
        return Err(Report::error(lost)
            .why(prose!("Nothing was run."))
            .next(
                cmd!("lnk folder set link <path>"),
                phrase!("point Link at it, if you moved it"),
            )
            .next(cmd!("lnk folder create"), phrase!("start a new, empty one"))
            .exit(Exit::Usage));
    }
    out.block(Report::warning(lost));
    let answer = out.ask(Question::text(sentence!(
        "Where is it now? Its path, or new for a new, empty one at ~/Link?"
    )))?;
    let made = match answer.as_str() {
        "new" => make_new()?,
        path => {
            let made = folders::point_at(&expand(path)?).map_err(|err| {
                failed(
                    sentence!("Couldn't point Link at {}.", Foreign::new(path)),
                    err,
                )
                .exit(Exit::Usage)
            })?;
            moved(out, &dir, &made)?;
            made
        }
    };
    out.block(Note(sentence!("Link's folder is {}.", Shown::new(&made))));
    Ok(())
}

/// A refusal naming the folder that made Link's settings untrusted, its
/// owner and why, where reading them was refused for one: else the error
/// as it is.
fn untrusted(err: anyhow::Error) -> Report {
    use link_files::fs::{Distrust, Untrusted};
    let Some(refused) = err.chain().find_map(|e| e.downcast_ref::<Untrusted>()) else {
        return err.into();
    };
    let why = match refused.why {
        Distrust::Owner => prose!(
            "{} is owned by user {}, neither root nor you, so Link reads none of its settings through it.",
            Shown::new(&refused.folder),
            refused.owner.to_string()
        ),
        Distrust::Written => prose!(
            "{} is written by others than its owner, so Link reads none of its settings through it.",
            Shown::new(&refused.folder)
        ),
    };
    Report::error(sentence!("Link can't trust {}.", Shown::new(&refused.file)))
        .why(why)
        .exit(Exit::Refused)
}

/// A new, empty Link folder at `~/Link`, refused where something is
/// there already but the empty Safe startup prepares.
fn make_new() -> Result<PathBuf, Report> {
    let there = link_files::layout::Root::Home.default_in(&link_files::try_home_dir()?);
    folders::make_new().map_err(|err| {
        failed(sentence!("Couldn't make {}.", Shown::new(&there)), err)
            .next(
                cmd!("lnk folder set link {}", Shown::new(&there)),
                phrase!("point Link at it"),
            )
            .exit(Exit::Usage)
    })
}

/// Once the Link folder moved from `was` to `now`: the paths its
/// harnesses keep in their settings and state, rewritten to where they
/// are now. What every start writes again (Link's own files in `~/.lnk`)
/// is written right at the next start.
fn moved(out: &mut Out, was: &Path, now: &Path) -> Result<(), Report> {
    if was == now {
        return Ok(());
    }
    let changed = folders::rewrite_tree(&link_files::layout::Home::Agents.at(now), was, now)?
        + folders::rewrite_tree(&link_files::layout::Home::AgentSettings.at(now), was, now)?
        + folders::rewrite_tree(&link_files::layout::Hidden::Agents.try_path()?, was, now)?
        + folders::rewrite_tree(&link_files::layout::Hidden::AgentData.try_path()?, was, now)?;
    if changed > 0 {
        out.block(Note(sentence!(
            "Pointed {} of your agents' harnesses at {}.",
            Count::new(changed as u64, "file", "files"),
            Shown::new(now)
        )));
    }
    Ok(())
}

/// `lnk folder create`.
pub fn create() -> Result<Done, Report> {
    let made = make_new()?;
    Ok(Done::success(sentence!(
        "Made a new Link folder, {}.",
        Shown::new(&made)
    )))
}

/// `lnk folder set <location> <path>`: the location's files and its entry
/// in the map, with what's inside it; for the Link folder that's gone from
/// where it was, Link pointed at where it is now.
pub fn set(out: &mut Out, location: &str, path: &Path) -> Result<Done, Report> {
    let key = Entry::parse(location).map_err(|err| {
        failed(sentence!("Couldn't set {}.", Foreign::new(location)), err)
            .next(cmd!("lnk folder list"), phrase!("see every location"))
            .exit(Exit::Usage)
    })?;
    let to = expand(&path.display().to_string())?;
    let home = link_files::try_home_dir()?;
    let machine = link_files::try_installation_dir().within("finding this machine's folder")?;
    let was = map::load(&home, &machine)?.path(
        key,
        &map::Roots {
            user_home: home.clone(),
            hidden: machine.clone(),
        },
    );
    let unset = |err: anyhow::Error| {
        failed(
            sentence!(
                "Couldn't set {} to {}.",
                Foreign::new(location),
                Shown::new(&to)
            ),
            err,
        )
        .next(
            cmd!("lnk folder list"),
            phrase!("see where each location is"),
        )
        .exit(Exit::Usage)
    };
    let now = if key == Entry::Link && !was.exists() {
        folders::point_at(&to).map_err(unset)?
    } else {
        map::set(&home, &machine, key, &to).map_err(unset)?;
        to.clone()
    };
    moved(out, &was, &now)?;
    Ok(Done::success(sentence!(
        "{} is {}.",
        Foreign::new(location),
        Shown::new(&now)
    )))
}

/// `lnk folder list`, and `lnk folder`: the map, the Link folder first.
pub fn list(out: &mut Out) -> Result<Done, Report> {
    let mut done = Done::shown();
    if let Found::Lost(_) = folders::inspect()? {
        out.block(Note(sentence!("The Link folder isn't there anymore.")));
        done = done.next(
            cmd!("lnk folder set link <path>"),
            phrase!("point Link at where it is now"),
        );
    }
    let home = link_files::try_home_dir()?;
    let machine = link_files::try_installation_dir().within("finding this machine's folder")?;
    let pinned = map::load(&home, &machine)?;
    let roots = map::Roots {
        user_home: home,
        hidden: machine,
    };
    let mut table = Table::new(["Location", "Path"], sentence!("No locations are mapped."));
    let mut json = serde_json::Map::new();
    for key in Entry::all() {
        let path = pinned.path(key, &roots);
        json.insert(key.name(), path.display().to_string().into());
        table.row([Value::from(key.name()), Value::from(Shown::new(path))]);
    }
    out.block(table);
    if out.is_json() {
        out.reply(&serde_json::Value::Object(json));
    }
    Ok(done
        .next(
            cmd!("lnk folder set <location> <path>"),
            phrase!("move one"),
        )
        .next(
            cmd!("lnk folder check"),
            phrase!("find staging files a write left behind"),
        ))
}

/// `path` with a leading `~` as the home folder, relative to here
/// otherwise.
fn expand(path: &str) -> anyhow::Result<PathBuf> {
    let home = link_files::try_home_dir()?;
    Ok(match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home,
        None => std::env::current_dir()?.join(path),
    })
}
