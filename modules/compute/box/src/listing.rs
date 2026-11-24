//! `lnk box list`: the boxes this machine knows, settled against their
//! clouds and discovered in every account.

use super::*;
use link_format::{
    Done, Name, Note, Out, Report, Table, Text, Time, Value, WithinExt, cmd, prose, sentence,
    states,
};

states! {
    /// A box's state, as its cloud or Link last saw it.
    pub(super) enum BoxState {
        Running("running") => Good,
        Starting("starting") => Quiet,
        Pending("pending") => Quiet,
        Stopping("stopping") => Quiet,
        Stopped("stopped") => Quiet,
        Suspended("suspended") => Quiet,
        StoppedByCloud("stopped by its cloud") => Warn,
        StoppedElsewhere("stopped elsewhere") => Warn,
        Terminated("terminated") => Bad,
        Gone("gone") => Bad,
    }
}

impl BoxState {
    /// A box's state; a cloud's word Link doesn't know is shown as it is.
    pub(super) fn of(m: &Machine) -> Value {
        let state = match m.state.as_str() {
            _ if m.stopped_by_cloud && m.stop_guessed => BoxState::StoppedElsewhere,
            _ if m.stopped_by_cloud => BoxState::StoppedByCloud,
            "running" => BoxState::Running,
            "starting" => BoxState::Starting,
            "pending" => BoxState::Pending,
            "stopping" => BoxState::Stopping,
            "stopped" => BoxState::Stopped,
            "suspended" => BoxState::Suspended,
            "terminated" => BoxState::Terminated,
            "gone" => BoxState::Gone,
            other => return Value::from(clean(other)),
        };
        state.into()
    }
}

pub(super) fn list(
    out: &mut Out,
    refresh: bool,
    all_regions: bool,
    here: bool,
) -> Result<Done, Report> {
    let before = Boxes::load().within("reading your boxes")?;
    let mut boxes = before.clone();
    let (found, seen) = match here {
        true => Default::default(),
        false => discover(out, &mut boxes, all_regions),
    };
    if !here {
        settle(out, &mut boxes);
    }
    if refresh {
        // Only the boxes the listing didn't return (gone, or in an account
        // that couldn't be asked), all at once: each is a cloud call.
        let asking: Vec<&mut Machine> = boxes
            .boxes
            .iter_mut()
            .filter(|m| {
                m.here.is_none()
                    && !m.id.is_empty()
                    && !seen.contains(&(m.account.clone(), m.id.clone()))
            })
            .collect();
        let replies = std::thread::scope(|scope| {
            let threads: Vec<_> = asking
                .into_iter()
                .map(|machine| {
                    scope.spawn(move || {
                        let reply = call(machine, "state");
                        (machine, reply)
                    })
                })
                .collect();
            threads
                .into_iter()
                .filter_map(|t| t.join().ok())
                .collect::<Vec<_>>()
        });
        for (machine, reply) in replies {
            match reply {
                Ok(reply) => {
                    if let Some(state) = reply.state {
                        machine.seen(state, reply.stopped_by_cloud);
                    }
                }
                Err(err) => out.block(
                    Report::warning(sentence!(
                        "Couldn't ask {}'s cloud for its state.",
                        Name::new("box", &machine.name)
                    ))
                    .why(prose_of(&err)),
                ),
            }
        }
    }
    Boxes::update(|saved| saved.merge(&before, &boxes)).within("saving your boxes")?;
    for binding in link_box::machine::bindings().within("reading VM boxes")? {
        if boxes
            .boxes
            .iter()
            .any(|machine| machine.name == binding.name)
        {
            return Err(
                anyhow::anyhow!("a local and cloud box share the name {}", binding.name).into(),
            );
        }
        boxes.boxes.push(link_box::machine::listed(&binding));
    }
    for pending in link_box::machine::pending().within("reading pending VM boxes")? {
        if boxes
            .boxes
            .iter()
            .any(|machine| machine.name == pending.input.name)
        {
            return Err(anyhow::anyhow!(
                "a pending local and cloud box share the name {}",
                pending.input.name
            )
            .into());
        }
        boxes
            .boxes
            .push(link_box::machine::listed_pending(&pending));
    }
    boxes.boxes.sort_by(|a, b| a.name.cmp(&b.name));
    {
        let listed: Vec<link_box_contract::Listed> = boxes
            .boxes
            .iter()
            .map(|m| link_box_contract::Listed {
                tags: Default::default(),
                price: m.price.clone(),
                deadline: m.deadline,
                name: m.name.clone(),
                account: m.account.clone(),
                kind: m.kind.clone(),
                place: m.place.clone(),
                id: m.id.clone(),
                machine_id: m.machine_id.clone(),
                size: m.size.clone(),
                ip: m.ip.clone(),
                state: m.state.clone(),
                created: m.created,
                found: match found.contains(&m.name) {
                    true => "cloud",
                    false => "here",
                }
                .to_string(),
                stopped_by_cloud: m.stopped_by_cloud,
            })
            .collect();
        link_actions::answer::reply::<link_box::cli::Boxes>(out, &listed)?;
    }
    let mut table = Table::new(
        [
            "Name", "Account", "Place", "Size", "Address", "State", "Deadline",
        ],
        sentence!("No boxes yet."),
    );
    for m in &boxes.boxes {
        table.row([
            Name::new("box", &m.name).into(),
            Value::from(&m.account),
            Value::from(&m.place),
            Value::from(&m.size),
            Value::from(m.ip.clone().unwrap_or_default()),
            BoxState::of(m),
            m.deadline.map_or(Value::from(String::new()), |d| {
                Time(std::time::UNIX_EPOCH + Duration::from_secs(d)).into()
            }),
        ]);
    }
    let first = boxes.boxes.first().map(|m| Name::new("box", &m.name));
    let elsewhere = boxes
        .boxes
        .iter()
        .any(|m| m.stopped_by_cloud && m.stop_guessed);
    out.block(table);
    if elsewhere {
        out.block(Text(prose!(
            "A box stopped elsewhere was stopped by its cloud (spot) or by another computer."
        )));
    }
    if !found.is_empty() {
        let names: Vec<Value> = found.iter().map(|n| Name::new("box", n).into()).collect();
        out.block(Text(prose!(
            "Found in your clouds, made elsewhere: {}.",
            Value::List(names)
        )));
    }
    Ok(match first {
        Some(name) => Done::shown().next(cmd!("lnk box ssh {}", name), "open a shell on it"),
        None => Done::shown()
            .next(
                cmd!("lnk box create <cloud>"),
                "make one on AWS or Google Cloud",
            )
            .next(
                cmd!("lnk cloud connect <cloud>"),
                "connect a cloud account first",
            ),
    })
}

/// An error's words, as prose: what a warning says of why.
fn prose_of(err: &anyhow::Error) -> link_format::Prose {
    link_format::Prose::from(link_format::Sentence::adopt(&format!("{err:#}")))
}

/// Asks each connected cloud account for the boxes tagged as yours, and
/// keeps the ones this computer didn't know; returns their names, and
/// each box the clouds listed (its account and id), whose state is now
/// theirs. On AWS it asks the regions your boxes there are in, else (or
/// with `all_regions`) every enabled one. An account that can't be asked
/// is said on stderr and skipped.
fn discover(out: &mut Out, boxes: &mut Boxes, all_regions: bool) -> (Vec<String>, Seen) {
    let Some(owner) = owner() else {
        out.block(
            Report::warning(sentence!(
                "Didn't ask your clouds for boxes made elsewhere."
            ))
            .why(prose!("This computer's owner identity couldn't be read.")),
        );
        return Default::default();
    };
    // Every account is asked at once (a cloud call can take seconds),
    // and the answers are merged in the accounts' order, so the output
    // and the names given to new boxes don't depend on which is first.
    let asks: Vec<(String, Vec<String>)> = accounts()
        .into_iter()
        .map(|name| {
            let mut places: Vec<String> = boxes
                .boxes
                .iter()
                .filter(|m| m.account == name)
                .map(|m| m.place.clone())
                .collect();
            places.sort();
            places.dedup();
            if all_regions {
                places.clear();
            }
            (name, places)
        })
        .collect();
    let answers: Vec<Result<(Account, Reply), String>> = std::thread::scope(|scope| {
        let asking: Vec<_> = asks
            .iter()
            .map(|(name, places)| {
                let owner = &owner;
                scope.spawn(move || ask(name, places.clone(), owner))
            })
            .collect();
        asking
            .into_iter()
            .map(|t| {
                t.join()
                    .unwrap_or_else(|_| Err("Couldn't ask a cloud for your boxes".to_string()))
            })
            .collect()
    });
    let mut found = Vec::new();
    let mut listed = Seen::new();
    for ((name, _), answer) in asks.into_iter().zip(answers) {
        let (account, reply) = match answer {
            Ok(answer) => answer,
            Err(why) => {
                out.block(Report::warning(link_format::Sentence::adopt(&why)));
                continue;
            }
        };
        if !reply.denied().is_empty() {
            out.block(Report::warning(sentence!(
                "Not looking for boxes in {}: {}'s login isn't allowed to look there.",
                reply.denied().join(", "),
                Value::from(&name)
            )));
        }
        for f in reply.boxes.unwrap_or_default() {
            listed.insert((name.clone(), f.id.clone()));
            if let Some(known) = boxes
                .boxes
                .iter_mut()
                .find(|m| m.account == name && m.id == f.id)
            {
                known.seen(f.state, f.stopped_by_cloud);
                if f.ip.is_some() {
                    known.ip = f.ip;
                    let known = known.name.clone();
                    boxes.claim_address(&known);
                }
                continue;
            }
            // What the cloud says goes into file names, prompts and the
            // cloud's CLI: only names and IDs as Link makes them, and an
            // instance and place that can't be an option.
            let plain = |text: &str| {
                !text.is_empty()
                    && !text.starts_with('-')
                    && text
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            };
            if check_name(&f.name).is_err() || !plain(&f.id) || !plain(&f.place) {
                out.block(Report::warning(sentence!(
                    "Skipped a box in {} with no name Link could use.",
                    Value::from(&name)
                )));
                continue;
            }
            let machine_id = match link_machine::machine::check_id(&f.machine_id) {
                Ok(()) => f.machine_id.clone(),
                Err(_) => String::new(),
            };
            let taken = boxes.get(&f.name).is_some();
            let box_name = match (taken, machine_id.get(..4)) {
                (false, _) => f.name.clone(),
                (true, Some(short)) => format!("{}-{short}", f.name),
                (true, None) => boxes.next_name(&f.name, &[]),
            };
            boxes.put(Machine {
                price: None,
                deadline: f.deadline,
                name: box_name.clone(),
                machine_id,
                account: name.clone(),
                kind: account.kind.clone(),
                place: f.place,
                id: f.id,
                size: f.size,
                ip: f.ip,
                state: f.state,
                created: 0,
                host_keys: Vec::new(),
                tagged: true,
                from_cloud: true,
                spec: None,
                spot: false,
                stopped_by_cloud: f.stopped_by_cloud.unwrap_or(false),
                stop_guessed: false,
                certified: false,
                environment: None,
                here: None,
                staged: false,
            });
            found.push(box_name);
        }
    }
    (found, listed)
}

/// Boxes a cloud listed, each by its account and id.
type Seen = std::collections::HashSet<(String, String)>;

/// Account `name`'s boxes in `places` (every place when empty) tagged
/// as `owner`'s, or the line saying why not.
fn ask(name: &str, places: Vec<String>, owner: &str) -> Result<(Account, Reply), String> {
    let account = account(name).map_err(|err| format!("{}: {err:#}", clean(name)))?;
    let request = Request {
        account: account.duplicate(),
        owner: Some(owner.to_string()),
        places: link_cloud_contract::carried(places),
        ..Request::default()
    };
    let reply = cloud::call(&account.kind, "list", &request)
        .map_err(|err| format!("Couldn't ask {} for your boxes: {err:#}", clean(name)))?;
    Ok((account, reply))
}

/// The connected cloud accounts' names (`lnk cloud list --json`).
pub(super) fn accounts() -> Vec<String> {
    link_core::lnk()
        .args(["cloud", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<serde_json::Value>>(&o.stdout).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a["name"].as_str().map(str::to_string))
        .collect()
}

/// Boxes made before IDs and tags get them, when they run: their ID
/// from the box (its `installation.toml`, written with its name, or made),
/// then their tags. Best effort: one that can't be reached now is
/// settled another time.
fn settle(out: &mut Out, boxes: &mut Boxes) {
    let pending: Vec<String> = boxes
        .boxes
        .iter()
        .filter(|m| !m.tagged && m.state == "running" && m.ip.is_some())
        .map(|m| m.name.clone())
        .collect();
    if pending.is_empty() {
        return;
    }
    let Some(owner) = owner() else {
        return;
    };
    for name in pending {
        let Some(machine) = boxes.get_mut(&name) else {
            continue;
        };
        let settled = (|| -> anyhow::Result<()> {
            let ip = machine.ip.clone().within("no address")?;
            let candidate = match machine.machine_id.is_empty() {
                true => link_id::candidate()?,
                false => machine.machine_id.clone(),
            };
            let me = link_machine::installation::Installation {
                id: candidate,
                name: machine.name.clone(),
            };
            let said = ssh::script_output(&ip, &setup::installation_script(&me)?)?;
            let id = said
                .lines()
                .find_map(|l| l.strip_prefix("lnk-installation "))
                .map(str::trim)
                .within("the box didn't say its ID")?;
            link_machine::machine::check_id(id)?;
            let tags = cloud::box_tags(&machine.name, id, &owner);
            call_with(machine, "tag", |r| r.tags = tags)?;
            machine.machine_id = id.to_string();
            machine.tagged = true;
            Ok(())
        })();
        let name = Name::new("box", &name);
        match settled {
            Ok(()) => out.block(Note(sentence!(
                "Tagged your box {} as yours in its cloud.",
                name
            ))),
            Err(err) => out.block(
                Report::warning(sentence!(
                    "Your box {} isn't tagged yet; Link tries again next time.",
                    name
                ))
                .why(prose_of(&err)),
            ),
        }
    }
}
