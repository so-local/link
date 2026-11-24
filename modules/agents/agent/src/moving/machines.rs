//! The machines a move goes between: a box made ready and upgraded,
//! the destination chosen, and a box started for it.

use link_format::WithinExt;
use std::process::Stdio;

use anyhow::{bail, ensure};
use link_agent::hosted;
use link_box_contract::Listed;
use link_format::{Exit, Foreign, Name, Note, Out, Question, Report, cmd, prose, sentence};
use link_workplace::Target;
use serde::Deserialize;

/// The modules a box is given before an agent comes: agent itself, the
/// secret module for `lnk secret delete` to reach its keys there, model,
/// which serves the sandbox's bridge every model call goes through, and
/// storage for its backups when it has a bucket.
fn box_modules(bucket: bool) -> Vec<&'static str> {
    let mut modules = vec!["agent", "secret", "model"];
    if bucket {
        modules.push("storage");
    }
    modules
}

/// Box `name` made ready for an agent: the harness module there and what
/// it needs ([`box_modules`]: agent, which brings the sandbox, secret and
/// model), with `bucket` the storage module for its backups; the sandbox
/// able to run (`lnk sandbox check --ready --yes`: a box's sudo asks nothing); and agents
/// made there behind their proxy from the start (`lnk agent route --direct
/// --default`). A box starts with `lnk` and your login alone; each step
/// leaves what's there as it is. With `harness` one added from outside
/// Link here, the box adds it too, from the same repository, trusting
/// only the key this computer pinned for it.
pub fn ready_box(name: &str, bucket: bool, harness: Option<&str>) -> anyhow::Result<()> {
    let mut add = vec!["module", "add"];
    add.extend(box_modules(bucket));
    let outside = harness.and_then(|h| {
        link_core::modules::outside()
            .into_iter()
            .find(|p| p.name == h)
    });
    let mut steps = vec![add];
    if let Some(module) = &outside
        && let (Some(source), Some(key)) = (&module.source, &module.signer)
    {
        steps.push(vec![
            "module",
            "add",
            "--url",
            source.as_str(),
            "--yes",
            "--signer",
            key.as_str(),
        ]);
    }
    steps.push(vec!["sandbox", "check", "--ready", "--yes"]);
    let upstream = crate::machine::guest_upstream(name)?;
    steps.push(
        match link_sandbox_contract::net::Upstream::parse(&upstream)? {
            link_sandbox_contract::net::Upstream::Direct => {
                vec!["agent", "route", "--direct", "--default"]
            }
            link_sandbox_contract::net::Upstream::Http(_) => {
                let address = upstream.trim_start_matches("http://");
                vec!["agent", "route", "--http", address, "--default"]
            }
            _ => vec!["agent", "route", "--socks", upstream.as_str(), "--default"],
        },
    );
    for args in steps {
        let status = link_core::lnk()
            .args(["box", "run", name, "--"])
            .args(&args)
            .stdin(Stdio::null())
            .status()
            .within("running lnk box run")?;
        ensure!(
            status.success(),
            "couldn't get your box {name} ready for an agent: lnk {} failed there (above)",
            args.join(" ")
        );
    }
    Ok(())
}

/// Box `name` answers: an error saying how to start it when it doesn't.
pub fn reach(name: &str) -> anyhow::Result<()> {
    let output = link_core::lnk()
        .args(["box", "run", name, "--", "--version"])
        .stdin(Stdio::null())
        .output()
        .within("running lnk box run")?;
    ensure!(
        output.status.success(),
        "couldn't reach your box {name} (above): lnk box start {name} if it's stopped"
    );
    Ok(())
}

/// Credentials reaching only agent `id`'s buckets, for a box (`lnk storage
/// keys` here); none without the storage module, or buckets with them.
pub(crate) fn bucket_credentials(id: &str) -> Option<link_secretive::Secret<link_secretive::Json>> {
    link_core::find_module("storage")?;
    let output = link_core::lnk()
        .args(["storage", "keys"])
        .args(link_agent::owners::agent_flag(id).ok()?)
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let printed = link_secretive::Secret::new(output.stdout);
    let keys: link_secretive::Secret<link_secretive::Json> =
        serde_json::from_slice(printed.expose()).ok()?;
    keys.expose()
        .as_object()
        .is_some_and(|k| !k.is_empty())
        .then_some(keys)
}

/// Where `lnk agent move` takes an agent.
#[derive(Debug, PartialEq)]
pub(crate) enum Dest {
    /// This computer.
    Here,
    /// Your box of that name.
    Box(String),
    /// A new box in the cloud account of that name, which has none of yours.
    New(String),
    /// A new box in the cloud account of that name, from your image there.
    Image { account: String, image: String },
    /// The cloud account of that name, which hosts agents itself.
    Hosted(String),
}

impl Dest {
    /// A place [`place`](super::place) names: `None` for a box not made yet.
    pub(crate) fn known(&self) -> Option<Option<String>> {
        match self {
            Dest::Here => Some(None),
            Dest::Box(name) => Some(Some(name.clone())),
            Dest::New(_) | Dest::Image { .. } | Dest::Hosted(_) => None,
        }
    }

    /// Where it is from a team kept on this computer: this computer is
    /// the team's home, a box another machine.
    pub(crate) fn toward(&self) -> link_agent_team::moving::Toward {
        use link_agent_team::moving::Toward;
        match self {
            Dest::Here => Toward::Home,
            Dest::Box(_) | Dest::New(_) | Dest::Image { .. } => Toward::Machine,
            Dest::Hosted(_) => Toward::Hosted,
        }
    }

    /// Where it is from a team cloud account `account` keeps: that
    /// account is the team's home, any other hosting one a cloud, and
    /// this computer or a box a machine.
    pub(crate) fn toward_hosted(&self, account: &str) -> link_agent_team::moving::Toward {
        use link_agent_team::moving::Toward;
        match self {
            Dest::Hosted(to) if to == account => Toward::Home,
            Dest::Hosted(_) => Toward::Hosted,
            Dest::Here | Dest::Box(_) | Dest::New(_) | Dest::Image { .. } => Toward::Machine,
        }
    }
}

/// Box `name`'s installation ID, as this computer knows it (`lnk box list
/// --here`): what a copy noted there is checked by, so another box made
/// under the name is never taken for it. None when it isn't known yet.
pub(crate) fn box_machine(name: &str) -> Option<String> {
    boxes(true)
        .ok()?
        .into_iter()
        .find(|b| b.name == name)
        .and_then(|b| b.machine())
}

/// What [`choose`] makes of where `lnk agent move` goes.
#[derive(Debug, PartialEq)]
pub(crate) enum Chosen {
    /// That place.
    Is(Dest),
    /// This box, which you didn't name and this computer didn't make:
    /// asked about first.
    Found(String),
}

/// Where `to` is: this machine, a box of yours (`--on`), else a cloud
/// account you connected (`--cloud`): its only box, or a new one when it
/// has none. A box this computer didn't make, found by its tags, is taken
/// only by `--on` its name or a yes on a terminal (`agent` names the agent
/// moving).
pub(crate) fn destination(out: &mut Out, to: &Target, agent: &str) -> Result<Dest, Report> {
    let name = match to {
        Target::Here => return Ok(Dest::Here),
        Target::On(name) => name.as_str(),
        Target::Cloud { cloud, zone } => {
            if zone.is_some() {
                return Err(Report::error(sentence!(
                    "An agent moves to a cloud, not to a zone of it: {} picks one.",
                    cmd!("lnk box create {} --zone <zone>", Name::new("cloud", cloud))
                ))
                .exit(Exit::Usage));
            }
            cloud.as_str()
        }
    };
    let clouds = clouds();
    // An account that hosts agents has no box: the agent goes to it.
    if matches!(to, Target::Cloud { .. })
        && clouds.iter().any(|c| c == name)
        && link_agent_place::account(name).is_some_and(|a| hosted::runs_agents(&a))
    {
        return Ok(Dest::Hosted(name.to_string()));
    }
    // The boxes this computer knows first (finding the agent may just
    // have asked the clouds); the clouds only for one it doesn't.
    let chosen = match boxes(true) {
        Ok(known) if known.iter().any(|b| b.name == name) => choose(to, &known, &clouds)?,
        _ => choose(to, &boxes(false)?, &clouds)?,
    };
    let found = match chosen {
        Chosen::Is(dest) => return Ok(dest),
        Chosen::Found(name) => name,
    };
    let tags = "a box this computer didn't make, yours only by its tags in the cloud account, \
                which anyone who can tag machines there can set";
    let move_by_name = cmd!(
        "lnk agent move --on {} --agent {}",
        Name::new("box", &found),
        Name::new("agent", agent)
    );
    if !out.can_ask() {
        let report = Report::error(sentence!(
            "{} is a cloud account of yours, and {} is {}.",
            Name::new("cloud", name),
            Name::new("box", &found),
            tags
        ))
        .why(prose!("Run this on a terminal to be asked."));
        return Err(report
            .exit(Exit::Usage)
            .next(move_by_name, "move to it by its name"));
    }
    out.block(Report::warning(sentence!(
        "{} is {}.",
        Name::new("box", &found),
        tags
    )));
    let moves = out.ask(
        Question::confirm(
            sentence!(
                "Move {} to {}?",
                Name::new("agent", agent),
                Name::new("box", &found)
            ),
            false,
        )
        .why(prose!("No stops here, and nothing moves.")),
    )?;
    if !moves {
        return Err(
            Report::error(sentence!("Not moving {}.", Name::new("agent", agent)))
                .exit(Exit::Usage)
                .next(move_by_name, "move to it by its name"),
        );
    }
    Ok(Dest::Box(found))
}

/// [`destination`], given your boxes and your cloud accounts' names: for
/// `--on`, the box named so; for `--cloud`, the only box in that account,
/// else a new one there when it has none. A box this computer didn't
/// make, the only one in its account, is [`Chosen::Found`].
pub(crate) fn choose(to: &Target, boxes: &[Listed], clouds: &[String]) -> anyhow::Result<Chosen> {
    let to = match to {
        Target::On(name) => {
            ensure!(
                boxes.iter().any(|b| b.name == *name),
                "no box named {name:?}: lnk box list lists your boxes"
            );
            return Ok(Chosen::Is(Dest::Box(name.clone())));
        }
        Target::Cloud { cloud, .. } => cloud.as_str(),
        Target::Here => return Ok(Chosen::Is(Dest::Here)),
    };
    ensure!(
        clouds.iter().any(|c| c == to),
        "no cloud account named {to:?}: lnk cloud list lists your cloud accounts"
    );
    let mut in_cloud: Vec<&Listed> = boxes.iter().filter(|b| b.account == to).collect();
    in_cloud.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    match in_cloud[..] {
        [one] if one.made_here() => Ok(Chosen::Is(Dest::Box(one.name.clone()))),
        [one] => Ok(Chosen::Found(one.name.clone())),
        [] => Ok(Chosen::Is(Dest::New(to.to_string()))),
        _ => {
            let names: Vec<&str> = in_cloud.iter().map(|b| b.name.as_str()).collect();
            bail!(
                "you have {} boxes in {to}: {}. Move to one by its name: lnk agent move --on {}",
                names.len(),
                names.join(", "),
                names[0]
            )
        }
    }
}

/// Your boxes (`lnk box list --json`), only those this computer knows
/// with `here` (`--here`, no cloud asked).
pub(crate) fn boxes(here: bool) -> anyhow::Result<Vec<Listed>> {
    let output = link_core::lnk()
        .args(["box", "list"])
        .args(here.then_some("--here"))
        .arg("--json")
        .stdin(Stdio::null())
        .output()
        .within("running lnk box list")?;
    ensure!(
        output.status.success(),
        "lnk box list failed: {}",
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    serde_json::from_slice(&output.stdout).within("reading lnk box list --json")
}

/// Your cloud accounts' names (`lnk cloud list --json`); none when that
/// can't say.
pub(crate) fn clouds() -> Vec<String> {
    #[derive(Deserialize)]
    struct Account {
        name: String,
    }
    link_core::lnk()
        .args(["cloud", "list", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<Account>>(&o.stdout).ok())
        .map_or_else(Vec::new, |a| a.into_iter().map(|a| a.name).collect())
}

/// A new box in cloud account `account` (`lnk box create <account>`), from
/// your `image` there if given, else in one that has none of yours; its
/// name. It's billed, so `lnk box create` asks before making it, as it
/// always does, unless the move was given `yes` (`--yes`).
pub(crate) fn start_box(
    out: &mut Out,
    account: &str,
    image: Option<&str>,
    yes: bool,
) -> anyhow::Result<String> {
    out.block(Note(match image {
        Some(image) => sentence!(
            "A box in {} from your image {}: {} makes it, billed by the cloud at its rates.",
            Name::new("cloud", account),
            Foreign::new(image),
            cmd!(
                "lnk box create {} --image {}",
                Name::new("cloud", account),
                Foreign::new(image)
            )
        ),
        None => sentence!(
            "You have no box in {}: {} makes one, billed by the cloud at its rates.",
            Name::new("cloud", account),
            cmd!("lnk box create {}", Name::new("cloud", account))
        ),
    }));
    out.hand_over();
    let started = link_core::lnk()
        .args(["box", "create", "--cloud", account])
        .args(image.map(|i| ["--image", i]).into_iter().flatten())
        .args(yes.then_some("--yes"))
        .status()
        .within("running lnk box create")?;
    ensure!(
        started.success(),
        "no box made in {account}, so the agent stayed where it was (lnk agent move --yes makes one without asking)"
    );
    // The newest there: the one just made.
    boxes(true)?
        .into_iter()
        .filter(|b| b.account == account)
        .max_by_key(|b| b.created)
        .map(|b| b.name)
        .within(format!("lnk box create {account} made no box there"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boxes this computer made (a `created` time), plus those found in
    /// a cloud by their tags.
    fn listed(made: &[(&str, &str)], found: &[(&str, &str)]) -> Vec<Listed> {
        let made = made.iter().map(|b| (b, 1));
        let found = found.iter().map(|b| (b, 0));
        made.chain(found)
            .map(|((name, account), created)| Listed {
                name: name.to_string(),
                account: account.to_string(),
                created,
                ..Listed::default()
            })
            .collect()
    }

    /// A cloud picks its only box, starts one when it has none, and asks
    /// which when it has several; `--on` a box's name is that box.
    #[test]
    fn a_cloud_names_its_only_box() {
        let clouds = ["aws".to_string(), "gcp".to_string()];
        let boxes = listed(&[("aws-1", "aws"), ("gcp-1", "gcp"), ("gcp-2", "gcp")], &[]);
        let cloud = |name: &str| Target::Cloud {
            cloud: name.into(),
            zone: None,
        };
        let pick = |to: &Target| choose(to, &boxes, &clouds);
        assert_eq!(
            pick(&cloud("aws")).unwrap(),
            Chosen::Is(Dest::Box("aws-1".into()))
        );
        assert_eq!(
            pick(&Target::On("gcp-2".into())).unwrap(),
            Chosen::Is(Dest::Box("gcp-2".into()))
        );
        let several = pick(&cloud("gcp")).unwrap_err().to_string();
        assert!(
            several.contains("2 boxes in gcp: gcp-1, gcp-2"),
            "{several}"
        );
        let none = choose(&cloud("aws"), &listed(&[("gcp-1", "gcp")], &[]), &clouds).unwrap();
        assert_eq!(none, Chosen::Is(Dest::New("aws".into())));
        let unknown = pick(&cloud("azure")).unwrap_err().to_string();
        assert!(unknown.contains("no cloud account"), "{unknown}");
        let no_box = pick(&Target::On("azure".into())).unwrap_err().to_string();
        assert!(no_box.contains("no box named"), "{no_box}");
        // A box named like a cloud is that box under --on, and the cloud's
        // own box under --cloud.
        let named = listed(&[("aws", "work"), ("aws-1", "aws")], &[]);
        assert_eq!(
            choose(&Target::On("aws".into()), &named, &clouds).unwrap(),
            Chosen::Is(Dest::Box("aws".into()))
        );
        assert_eq!(
            choose(&cloud("aws"), &named, &clouds).unwrap(),
            Chosen::Is(Dest::Box("aws-1".into()))
        );
    }

    /// A box found only by its tags, which anyone who can tag machines in
    /// a shared account can plant, is never taken for its cloud without
    /// asking; `--on` its own name it is.
    #[test]
    fn a_box_found_by_its_tags_is_asked_about() {
        let clouds = ["aws".to_string(), "gcp".to_string()];
        let boxes = listed(&[("gcp-1", "gcp")], &[("aws-1", "aws")]);
        let cloud = |name: &str| Target::Cloud {
            cloud: name.into(),
            zone: None,
        };
        let pick = |to: &Target| choose(to, &boxes, &clouds).unwrap();
        assert_eq!(pick(&cloud("aws")), Chosen::Found("aws-1".into()));
        assert_eq!(
            pick(&Target::On("aws-1".into())),
            Chosen::Is(Dest::Box("aws-1".into()))
        );
        assert_eq!(pick(&cloud("gcp")), Chosen::Is(Dest::Box("gcp-1".into())));
        // Among several, it's named as the others are.
        let several = choose(
            &cloud("aws"),
            &listed(&[("aws-2", "aws")], &[("aws-1", "aws")]),
            &clouds,
        );
        let several = several.unwrap_err().to_string();
        assert!(
            several.contains("2 boxes in aws: aws-1, aws-2"),
            "{several}"
        );
    }

    #[test]
    fn a_box_gets_what_a_model_call_goes_through() {
        assert_eq!(box_modules(false), ["agent", "secret", "model"]);
        assert!(box_modules(true).contains(&"storage"));
    }
}
