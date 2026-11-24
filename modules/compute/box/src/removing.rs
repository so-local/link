//! `lnk box delete`: a box deleted from its cloud and forgotten.

use super::*;
use link_box_contract::Released;
use link_format::{Question, Refuser, Sentence};

pub(super) fn delete(out: &mut Out, name: &str, yes: bool) -> anyhow::Result<Done> {
    if let Some(machine) = link_box::native::find(name)? {
        return delete_here(out, &machine, yes);
    }
    if link_box::machine::is_here(name) {
        let shown = Name::new("box", name);
        if !out.asks(
            Question::confirm(
                sentence!("Delete the VM box {} and its disk?", &shown),
                false,
            )
            .yes(yes),
        )? {
            return Err(said(
                Report::refusal(Refuser::Person, sentence!("Didn't delete {}.", &shown)).next(
                    cmd!("lnk box stop {}", &shown),
                    "stop it instead, keeping its disk",
                ),
            ));
        }
        let deleting = out.activity(phrase!("Deleting {}", &shown));
        link_box::machine::remove(name)?;
        drop(deleting);
        return Ok(
            Done::success(sentence!("{} is gone, with its disk.", &shown))
                .next(cmd!("lnk box list"), "list your boxes"),
        );
    }
    let machine = Boxes::load()?.pick(Some(name))?.clone();
    let shown = Name::new("box", name);
    if machine.id.is_empty() {
        // A name `lnk box create` kept, stopped before its cloud made it.
        forget(&machine)?;
        return Ok(
            Done::success(sentence!("{} was never made; forgot its name.", &shown))
                .next(cmd!("lnk box list"), "list your boxes"),
        );
    }
    if !out.asks(
        Question::confirm(
            sentence!("Delete {} and its disk? Everything on it is lost.", &shown),
            false,
        )
        .yes(yes),
    )? {
        return Err(said(
            Report::refusal(Refuser::Person, sentence!("Didn't delete {}.", &shown)).next(
                cmd!("lnk box stop {}", &shown),
                "stop it instead, keeping its disk",
            ),
        ));
    }
    let deleting = out.activity(phrase!("Deleting {}", &shown));
    call(&machine, "remove")?;
    // The adapter deletes what it made for boxes there once none is left.
    let account = account(&machine.account)?;
    let cleaned = cloud::call(
        &account.kind.clone(),
        "cleanup",
        &Request {
            account,
            place: Some(machine.place.clone()),
            spec: Some(cloud_spec(
                &machine.name,
                &machine.size,
                &machine.machine_id,
                &owner().unwrap_or_default(),
                &BoxSpec::default().want(&machine.kind),
                (0, ""),
            )?),
            ..Request::default()
        },
    );
    if let Some(ip) = &machine.ip {
        ssh::forget(ip);
        ssh::close(ip);
    }
    Boxes::update(|boxes| {
        if boxes.get(name).is_some_and(|saved| saved.id == machine.id) {
            boxes.remove(name);
        }
    })?;
    // Gone from its cloud: what other modules gave it (a key to their
    // storage) is taken back by them.
    let stays = released(&machine);
    drop(deleting);
    if let Err(err) = cleaned {
        out.block(
            Report::warning(sentence!("Not all Link made for boxes there is gone."))
                .why(words(&err)),
        );
    }
    for stay in stays {
        out.block(Report::warning(Sentence::adopt(&stay)));
    }
    Ok(
        Done::success(sentence!("{} is gone, with its disk.", &shown))
            .next(cmd!("lnk box list"), "list your boxes"),
    )
}

/// The question [`link_box_contract::REMOVED`], in Box's types.
const REMOVED: link_core::answers::Question<link_box_contract::BoxIdentity, Released> =
    link_core::answers::Question::new(
        link_box_contract::REMOVED,
        link_box_contract::REMOVED_PROTOCOL,
    );

/// Box `machine` gone from its cloud, asked of each module that gave it
/// something ([`REMOVED`]): what each couldn't take back, and each that
/// couldn't say.
fn released(machine: &Machine) -> Vec<String> {
    let gone = link_box_contract::BoxIdentity {
        name: machine.name.clone(),
        machine_id: machine.machine_id.clone(),
    };
    REMOVED
        .ask(&gone)
        .into_iter()
        .flat_map(|answer| match answer.reply {
            Ok(released) => released.stays,
            Err(err) => vec![format!(
                "The {} module couldn't take back what it gave {}: {}",
                answer.module,
                machine.name,
                link_format::clean(&format!("{err:#}"))
            )],
        })
        .collect()
}

/// `lnk box delete` of a box here: its services stopped and its record
/// gone, and its home, a folder of your data, only once you say so.
fn delete_here(out: &mut Out, machine: &Machine, yes: bool) -> anyhow::Result<Done> {
    let shown = Name::new("box", &machine.name);
    if !out.asks(
        Question::confirm(
            sentence!("Delete {}? Its environment's services stop.", &shown),
            false,
        )
        .yes(yes),
    )? {
        return Err(said(
            Report::refusal(Refuser::Person, sentence!("Didn't delete {}.", &shown))
                .next(cmd!("lnk box stop {}", &shown), "stop it instead"),
        ));
    }
    let home = machine
        .here
        .as_ref()
        .map(|here| here.home.clone())
        .unwrap_or_default();
    let remove_home = home.exists()
        && out.asks(
            Question::confirm(
                sentence!(
                    "Remove its home, {}, and everything in it?",
                    link_format::Path::new(&home)
                ),
                false,
            )
            .yes(yes),
        )?;
    let deleting = out.activity(phrase!("Deleting {}", &shown));
    link_box::native::remove(machine, remove_home)?;
    drop(deleting);
    if remove_home || !home.exists() {
        return Ok(
            Done::success(sentence!("{} is gone, with its home.", &shown))
                .next(cmd!("lnk box list"), "list your boxes"),
        );
    }
    Ok(Done::success(sentence!(
        "{} is gone; its home stays at {}.",
        &shown,
        link_format::Path::new(&home)
    ))
    .next(cmd!("lnk box list"), "list your boxes"))
}
