//! The box module: machines in your own cloud accounts, running `lnk`.
//! A box is just another machine running `lnk`: this module makes it,
//! reaches it over SSH, and removes it; what runs on it belongs to the
//! modules that run it anywhere.
//!
//! Which cloud a box is on is its account's adapter (`lnk-aws`,
//! `lnk-gcp`; the contract is `link_cloud_contract`), acting as an account
//! `lnk cloud connect` connected (`lnk cloud env <name> --json`). This
//! module never knows which cloud it is.

// The generated command line names the held spec by its crate path.
extern crate self as link_box;

pub mod bundled;
mod checks;
pub mod cli;
pub mod constants;
pub mod machine;
pub mod native;
pub mod said;
pub mod setup;
pub mod spec;
pub mod ssh;
pub mod state;

use link_cloud_contract::want::{Want, word};
use link_cloud_contract::{self as cloud, Account, MachineSize, Reply, Request, Spec};
use link_format::{
    Exit, Name, Out, Prose, Report, Sentence, WithinExt, cmd, phrase, prose, sentence,
};
use link_secretive::Duplicate;

use said::said;
use state::Machine;

/// Account `name`, from the cloud module.
pub fn account(name: &str) -> anyhow::Result<Account> {
    let output = link_core::lnk()
        .args(["cloud", "env", name, "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .within("running lnk cloud env")?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(said(
            Report::error(sentence!(
                "Couldn't find a cloud account named {}.",
                Name::new("account", name)
            ))
            .why(Prose::from(Sentence::adopt(
                why.trim().trim_start_matches("Error: "),
            )))
            .next(cmd!("lnk cloud list"), "list your cloud accounts")
            .next(cmd!("lnk cloud connect <cloud>"), "connect aws or gcp")
            .exit(Exit::Usage),
        ));
    }
    serde_json::from_slice(&output.stdout).within("reading lnk cloud env")
}

/// Adapter command `command` for `machine`, as its account.
pub fn call(machine: &Machine, command: &str) -> anyhow::Result<Reply> {
    call_with(machine, command, |_| {})
}

/// [`call`], with more in the request (`more` sets it).
pub fn call_with(
    machine: &Machine,
    command: &str,
    more: impl FnOnce(&mut Request),
) -> anyhow::Result<Reply> {
    let account = account(&machine.account)?;
    let kind = account.kind.clone();
    let mut request = Request {
        account,
        place: Some(machine.place.clone()),
        id: Some(machine.id.clone()),
        ..Request::default()
    };
    more(&mut request);
    cloud::call(&kind, command, &request)
}

/// Whose boxes this computer's are: the signed-in account, or a stable
/// local owner derived from this computer's user and machine name.
pub fn owner() -> Option<String> {
    need_owner().ok()
}

/// [`owner`], preserving an error reading this computer's identity.
pub fn need_owner() -> anyhow::Result<String> {
    let owner = link_core::owner::current_owner()?;
    Ok(if link_machine::owner::is_local(&owner) {
        // A user@installation identity can exceed a cloud label's length and
        // contains characters some clouds refuse. Keep the whole identity
        // in its digest, with one portable tag used for creation and lookup.
        format!("local-{}", &link_digest::sha256(owner.as_bytes())[..56])
    } else {
        owner
    })
}

/// What `lnk box create` makes, before it has an id: box `name` with
/// installation ID `id`, tagged as `owner`'s and with `want`'s tags (Link's
/// own win), its machine as `want` says. `gpus` are what `size` has.
pub fn cloud_spec(
    name: &str,
    size: &str,
    id: &str,
    owner: &str,
    want: &Want,
    gpus: (u32, &str),
) -> anyhow::Result<Spec> {
    let mut tags = want.tag_map();
    tags.extend(cloud::box_tags(name, id, owner));
    let made = Spec {
        attach: Default::default(),
        self_remove: None,
        deadline: None,
        name: name.into(),
        id: link_cloud_contract::said(id.into()),
        tags,
        size: size.into(),
        user_data: setup::cloud_init(
            link_machine::machine::Kind::Box {
                os: want.os,
                arch: want.arch,
            },
            trust(id)?.as_ref(),
        )?,
        // No key: the box trusts this computer's certificates.
        public_key: None,
        os: word(&want.os),
        arch: word(&want.arch),
        disk_gb: u16::try_from(want.disk.whole_gb())?,
        disk_type: word(&want.disk_type),
        spot: want.spot,
        gpus: u64::from(gpus.0),
        gpu_model: link_cloud_contract::said(gpus.1.to_string()),
        tunnel: (ssh::Reach::here()? == ssh::Reach::Tunnel).then_some(true),
    };
    made.check()?;
    Ok(made)
}

/// What a box of installation ID `id` trusts: this computer's certificate
/// authority, for certificates naming the box. None for an ID that names
/// no principal (a spec for cleaning up after a box made before IDs).
fn trust(id: &str) -> anyhow::Result<Option<setup::Trust>> {
    let Some(principal) = ssh::certs::principal(id) else {
        return Ok(None);
    };
    Ok(Some(setup::Trust {
        authority: ssh::certs::authority()?,
        principal,
    }))
}

/// The machine type a box gets, and what its cloud says it has.
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    pub size: String,
    /// The size as the adapter listed it; none when it lists none.
    pub info: Option<MachineSize>,
    /// What fits, best first, when the size was matched.
    pub fitting: Vec<MachineSize>,
    /// What to say before the question: a warning each.
    pub notes: Vec<String>,
    /// The place's rates for the rest of a box, where its cloud gives them.
    pub rates: Option<link_cloud_contract::prices::Rates>,
}

impl Chosen {
    /// What a box of it costs an hour, as `want` has it: its machine
    /// type's price and its rates; none with neither.
    pub fn price(&self, want: &Want) -> Option<link_cloud_contract::prices::Price> {
        let machine = self.info.as_ref().and_then(|i| i.price);
        let price = match &self.rates {
            Some(rates) => rates.price(machine, want.disk_type, want.disk.whole_gb()),
            None => link_cloud_contract::prices::Price {
                machine,
                ..Default::default()
            },
        };
        (price != link_cloud_contract::prices::Price::default()).then_some(price)
    }
}

/// What an adapter said when asked for its machine types at a place.
#[derive(Clone, Debug, PartialEq)]
pub enum Offered {
    /// The machine types it lists there, in its order of preference, and
    /// the place's rates for the rest of a box.
    Sizes(Vec<MachineSize>, Option<link_cloud_contract::prices::Rates>),
    /// It has no `sizes` command.
    NoCommand,
    /// It couldn't list them: why, as it said.
    Failed(String),
}

/// Adapter `kind`'s machine types in `account` at `place`, its stderr
/// kept: a failure is said only where it matters.
pub fn offered(account: &Account, place: &str) -> Offered {
    let request = Request {
        account: account.duplicate(),
        place: Some(place.to_string()),
        ..Request::default()
    };
    match cloud::call_quietly(&account.kind, "sizes", &request) {
        Ok(reply) => Offered::Sizes(reply.sizes.unwrap_or_default(), reply.rates),
        Err(err) if err.is::<cloud::NoCommand>() => Offered::NoCommand,
        Err(err) => Offered::Failed(format!("{err:#}")),
    }
}

/// Whether choosing the machine for `want` asks the adapter what it
/// has: to match one, or for what a pinned size has.
pub fn asks_sizes(want: &Want) -> bool {
    want.matching || want.size.is_some()
}

/// The machine type for a box of `want` in `account` at `place`: its
/// pinned `size`, as the adapter lists it there (a size it doesn't list,
/// one it doesn't fully know, is made as given); else the
/// best fit of its CPUs, memory and GPUs; else `default`, the adapter's,
/// as before specs. The adapter is asked only when [`asks_sizes`]; one
/// that doesn't answer `sizes` still makes a pinned or default size.
pub fn choose(
    account: &Account,
    place: &str,
    default: Option<String>,
    want: &Want,
) -> anyhow::Result<Chosen> {
    let listed = asks_sizes(want).then(|| offered(account, place));
    choose_from(&account.kind, listed, place, default, want)
}

/// [`choose`], from what adapter `kind` said to `sizes` at `place` (none:
/// not asked): what the box spec cases check.
pub fn choose_from(
    kind: &str,
    listed: Option<Offered>,
    place: &str,
    default: Option<String>,
    want: &Want,
) -> anyhow::Result<Chosen> {
    let (kind, place) = (link_format::clean(kind), link_format::clean(place));
    let mut notes = Vec::new();
    let (sizes, why, rates) = match listed {
        Some(Offered::Sizes(sizes, rates)) if !sizes.is_empty() => (Some(sizes), None, rates),
        Some(Offered::Failed(why)) => (None, Some(why), None),
        _ => (None, None, None),
    };
    if let Some(size) = &want.size {
        let info = match &sizes {
            Some(sizes) => {
                let found = sizes.iter().find(|s| s.size == size.as_str()).cloned();
                // Its image is the box's arch's: another arch's machine
                // couldn't boot it.
                let arch = word(&want.arch);
                if let Some(found) = &found
                    && found.arch != arch
                {
                    let other = link_format::clean(&found.arch);
                    return Err(said(
                        Report::error(sentence!(
                            "Couldn't make the box on the machine type {}.",
                            size.to_string()
                        ))
                        .why(prose!(
                            "It is an {} machine, and the box is {}: give a machine type of the box's architecture, or set arch to match.",
                            other,
                            arch
                        ))
                        .exit(Exit::Usage),
                    ));
                }
                if found.is_none() {
                    notes.push(format!(
                        "lnk-{kind} doesn't list {size} in {place} (it lists the machine types it \
                         knows fully): making it as given, if its cloud has it there."
                    ));
                }
                found
            }
            None => {
                if let Some(why) = &why {
                    notes.push(format!(
                        "lnk-{kind} couldn't list its machine types in {place}: making {size} as \
                         given. ({why})"
                    ));
                }
                None
            }
        };
        return Ok(Chosen {
            size: size.to_string(),
            fitting: info.iter().cloned().collect(),
            info,
            notes,
            rates,
        });
    }
    if want.matching {
        let Some(sizes) = sizes else {
            let what = match &why {
                Some(_) => "couldn't list its machine types",
                None => "doesn't list its machine types",
            };
            let matched = format!(
                "The adapter lnk-{kind} {what} in {place}, so a box's cpus, memory and gpus can't be matched there: give its machine type with --size, or size in its cloud's table."
            );
            return Err(said(
                Report::error(sentence!(
                    "Couldn't match a machine type for the box in {}.",
                    place.to_string()
                ))
                .why(Prose::from(Sentence::adopt(&match why {
                    Some(why) => format!("{why}: {matched}"),
                    None => matched,
                })))
                .exit(Exit::Usage),
            ));
        };
        let best = link_cloud_contract::want::pick(want, &sizes)
            .within(format!("in {place}"))?
            .clone();
        return Ok(Chosen {
            size: best.size.clone(),
            fitting: link_cloud_contract::want::ranked(want, &sizes)
                .into_iter()
                .cloned()
                .collect(),
            info: Some(best),
            notes,
            rates,
        });
    }
    let size = default.within("the adapter didn't say how big")?;
    let info = sizes.and_then(|sizes| sizes.into_iter().find(|s| s.size == size));
    Ok(Chosen {
        size,
        fitting: info.iter().cloned().collect(),
        info,
        notes,
        rates,
    })
}

/// Makes sure this computer gets in to `machine`, running at `ip`, with
/// its certificates. A box found in the cloud (made on another computer)
/// gets its host keys pinned from the cloud (what the computer that made
/// it kept there) first. A box that doesn't trust this computer's
/// certificates yet ([`certify`]) is made to.
pub fn reach(out: &mut Out, machine: &mut Machine, ip: &str) -> anyhow::Result<()> {
    if machine.from_cloud {
        if machine.host_keys.is_empty() {
            match call(machine, "hostkeys") {
                Ok(reply) => machine.host_keys = reply.host_keys.unwrap_or_default(),
                Err(err) => out.block(
                    Report::warning(sentence!(
                        "Its host key is trusted on first contact instead, and pinned after."
                    ))
                    .why(said::words(&err)),
                ),
            }
        }
        if !machine.host_keys.is_empty() {
            ssh::pin(ip, &machine.host_keys)?;
        }
    }
    if !machine.certified {
        certify(out, machine, ip)?;
    }
    machine.from_cloud = false;
    Ok(())
}

/// Has `machine` trust this computer's certificates, and drops Link's
/// plain keys from it: a box made before certificates, or by another
/// computer. In with this computer's key (let in through the cloud if it
/// isn't), it's given this computer's authority and its principal as its
/// first boot would have; once a certificate alone gets in, Link's keys
/// go from what the cloud gives it and from `authorized_keys`, anyone
/// else's kept. A box with no installation ID yet keeps the key until Link
/// reads its ID.
fn certify(out: &mut Out, machine: &mut Machine, ip: &str) -> anyhow::Result<()> {
    let Some(principal) = ssh::certs::principal(&machine.machine_id) else {
        if !ssh::can_login(ip)? {
            let_in(out, machine, ip)?;
        }
        return Ok(());
    };
    if !ssh::certified_login(ip)? {
        if !ssh::can_login(ip)? {
            let_in(out, machine, ip)?;
        }
        let _trusting = out.activity(phrase!(
            "Having {} trust this computer's SSH certificates",
            Name::new("box", &machine.name)
        ));
        let authority = ssh::certs::authority()?;
        ssh::run_script(ip, &ssh::certs::trust_script(&authority, &principal))?;
        if !ssh::certified_login(ip)? {
            return Err(said(
                Report::error(sentence!(
                    "{} doesn't take this computer's SSH certificates.",
                    Name::new("box", &machine.name)
                ))
                .why(prose!("Its key still gets in."))
                .next(
                    cmd!("lnk box ssh {}", Name::new("box", &machine.name)),
                    "look at its sshd settings",
                ),
            ));
        }
    }
    // Link's plain keys go: from what the cloud gives the box, then from
    // the box itself, over the certificate.
    if let Err(err) = call(machine, "remove-keys") {
        out.block(
            Report::warning(sentence!(
                "Link's old SSH keys for {} may still be in your {} account's keys for it.",
                Name::new("box", &machine.name),
                Name::new("account", &machine.account)
            ))
            .why(Prose::from(Sentence::adopt(&format!("{err:#}")))),
        );
    }
    ssh::run_script(ip, ssh::certs::DROP_KEYS)?;
    machine.certified = true;
    Ok(())
}

/// Lets this computer's key in to `machine` through its cloud: on Google
/// Cloud in the instance's keys, on AWS pushed for a minute and appended
/// to `authorized_keys` meanwhile.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a box until it lets this computer's key in"
)]
fn let_in(out: &mut Out, machine: &Machine, ip: &str) -> anyhow::Result<()> {
    let _letting = out.activity(phrase!(
        "Letting this computer in to {} through your {} account",
        Name::new("box", &machine.name),
        Name::new("account", &machine.account)
    ));
    let key = ssh::public_key()?;
    let reply = call_with(machine, "add-key", |r| r.public_key = Some(key.clone()))?;
    if reply.temporary() {
        return ssh::append_key(ip, &key);
    }
    let start = std::time::Instant::now();
    while !ssh::can_login(ip)? {
        if start.elapsed() > std::time::Duration::from_secs(90) {
            return Err(said(
                Report::error(sentence!(
                    "{} didn't let this computer's key in.",
                    Name::new("box", &machine.name)
                ))
                .next(cmd!("lnk box list --refresh"), "ask its cloud again"),
            ));
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    Ok(())
}

pub mod ending;

#[cfg(test)]
link_test_kit::program!(system);
