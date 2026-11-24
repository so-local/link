//! `lnk box image`: a box's disk saved as an image in its cloud account,
//! and new boxes made from one (`lnk box create <cloud> --image`). The
//! adapter's `image-save`, `image-list` and `image-remove` do the cloud's
//! part; an adapter without them makes no image, and no box from one.

use anyhow::ensure;
use link_box::said::{Asks, failed, said, usage, words};
use link_box::state::Boxes;
use link_box::{account, call, call_with, need_owner, ssh};
use link_cloud_contract::{self as cloud, Account, FoundImage, Request};
use link_format::clean;
use link_format::{
    Done, Exit, Fields, Name, Note, Out, Question, Refuser, Report, Steps, Table, Value, WithinExt,
    cmd, phrase, prose, sentence,
};
use link_secretive::Duplicate;

/// `owner`'s images in `account`, as its adapter lists them (every
/// place). An adapter that can't is said to be one without images.
pub fn images(account: &Account, owner: &str) -> anyhow::Result<Vec<FoundImage>> {
    let request = Request {
        account: account.duplicate(),
        owner: Some(owner.to_string()),
        ..Request::default()
    };
    cloud::call(&account.kind, "image-list", &request)
        .map(|reply| reply.images.unwrap_or_default())
        .map_err(|err| {
            said(
                failed(
                    sentence!(
                        "Couldn't list images in {}.",
                        Name::new("account", &account.name)
                    ),
                    err,
                )
                .next(
                    cmd!("lnk upgrade"),
                    "upgrade the adapter, since an older one makes no images",
                ),
            )
        })
}

/// Image `name` of `owner`'s in `account`.
pub fn find(account: &Account, owner: &str, name: &str) -> anyhow::Result<FoundImage> {
    cloud::check_image(name)?;
    let found: Vec<FoundImage> = images(account, owner)?
        .into_iter()
        .filter(|i| i.name == name)
        .collect();
    let account_name = Name::new("account", &account.name);
    match found.len() {
        0 => Err(usage(
            sentence!(
                "Couldn't find an image named {} in {}.",
                Value::from(name.to_string()),
                &account_name
            ),
            prose!("No image of yours there has that name."),
            cmd!("lnk box image list {}", &account_name),
            "list its images",
        )),
        1 => Ok(found.into_iter().next().expect("one")),
        _ => Err(said(
            Report::error(sentence!(
                "Couldn't tell which image named {} in {} you mean.",
                Value::from(name.to_string()),
                &account_name
            ))
            .why(prose!(
                "There are {} of them: {}. Delete all but one.",
                found.len(),
                found
                    .iter()
                    .map(|i| format!("{} in {}", clean(&i.id), clean(&i.place)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .next(
                cmd!(
                    "lnk box image delete {} {}",
                    Value::from(name.to_string()),
                    &account_name
                ),
                "delete one",
            ),
        )),
    }
}

/// Which image a new box starts from: by its name, or the newest ready
/// one saved with a hash.
pub enum Pick<'a> {
    Named(&'a str),
    Hashed(&'a str),
}

/// The image `pick` names, of `owner`'s in `account`.
pub fn pick(account: &Account, owner: &str, pick: Pick) -> anyhow::Result<FoundImage> {
    match pick {
        Pick::Named(name) => find(account, owner, name),
        Pick::Hashed(hash) => hashed(account, owner, hash)?
            .into_iter()
            .find(|i| i.state == "ready")
            .ok_or_else(|| {
                let account_name = Name::new("account", &account.name);
                usage(
                    sentence!(
                        "Couldn't find a ready image with the hash {} in {}.",
                        Value::from(hash.to_string()),
                        &account_name
                    ),
                    prose!("None of your images there has been saved with it."),
                    cmd!(
                        "lnk box image list {} --hash {}",
                        &account_name,
                        Value::from(hash.to_string())
                    ),
                    "list the images with it",
                )
            }),
    }
}

/// `owner`'s images in `account` saved with `hash`, newest first.
fn hashed(account: &Account, owner: &str, hash: &str) -> anyhow::Result<Vec<FoundImage>> {
    cloud::check_image_hash(hash)?;
    let mut found: Vec<FoundImage> = images(account, owner)?
        .into_iter()
        .filter(|i| i.hash() == hash)
        .collect();
    // Google Cloud's times carry an offset, which changes across a DST
    // change, so they're compared as instants, never as text. A time
    // that isn't one sorts last.
    found.sort_by_key(|i| std::cmp::Reverse(instant(i.created())));
    Ok(found)
}

/// `time`, an RFC 3339 time (`2026-03-08T01:30:00.123-08:00`, or with
/// `Z`), as seconds and nanoseconds since 1970 in UTC; none when it
/// isn't one.
fn instant(time: &str) -> Option<(i64, u32)> {
    let num = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let (date, rest) = time.split_once(['T', 't', ' '])?;
    let mut date = date.splitn(3, '-');
    let (year, month, day) = (num(date.next()?)?, num(date.next()?)?, num(date.next()?)?);
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut clock = clock.splitn(3, ':');
    let (hour, minute, second) = (
        num(clock.next()?)?,
        num(clock.next()?)?,
        num(clock.next()?)?,
    );
    let nanos = match fraction {
        "" => 0,
        digits => {
            num(digits)?;
            let digits = &digits[..digits.len().min(9)];
            u32::try_from(num(digits)? * 10_i64.pow(9 - digits.len() as u32)).ok()?
        }
    };
    let offset = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (h, m) = zone[1..].split_once(':')?;
            sign * (num(h)? * 3600 + num(m)? * 60)
        }
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from 1970-01-01 to the date, in the proleptic Gregorian
    // calendar (Howard Hinnant's `days_from_civil`).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((
        days * 86_400 + hour * 3600 + minute * 60 + second - offset,
        nanos,
    ))
}

/// Where other tools usually leave secrets on a box, each found one
/// printed as `<path>\t<what>`, its path from the home folder. A file
/// with a token written as `${VARIABLE}` (read from the environment)
/// isn't one. Link's modules say their own ([`kept_secrets`]).
const SECRETS: &str = r#"cd "$HOME" || exit 1
found() { printf '%s\t%s\n' "$1" "$2"; }
for f in .git-credentials .config/git/credentials; do
  [ -s "$f" ] && found "$f" "git credentials"
done
[ -f .netrc ] && grep -Eq '(^|[[:space:]])password[[:space:]]+[^[:space:]]' .netrc && found .netrc "a password for git and other logins"
[ -f .config/gh/hosts.yml ] && grep -q 'oauth_token' .config/gh/hosts.yml && found .config/gh/hosts.yml "GitHub CLI's login"
for f in .ssh/id_*; do
  case "$f" in *.pub) ;; *) [ -f "$f" ] && found "$f" "an SSH private key" ;; esac
done
find . -maxdepth 4 \( -name node_modules -o -name .git -o -name .venv \) -prune -o -type f \
  \( -name .npmrc -o -name .pypirc -o -name .yarnrc.yml \) -print 2>/dev/null | while IFS= read -r f; do
  f=${f#./}
  case "$f" in
    *.npmrc) grep -Eq '(_authToken|_auth|_password)[[:space:]]*=[[:space:]]*[^[:space:]$]' "$f" && found "$f" "an npm token" ;;
    *.yarnrc.yml) grep -Eq 'npmAuth(Token|Ident)[[:space:]]*:[[:space:]]*[^[:space:]$]' "$f" && found "$f" "an npm token" ;;
    *.pypirc) grep -Eiq '^[[:space:]]*password[[:space:]]*[=:][[:space:]]*[^[:space:]$]' "$f" && found "$f" "a PyPI password" ;;
  esac
done
for f in .aws/credentials .aws/sso/cache .aws/cli/cache; do
  [ -n "$(ls -A "$f" 2>/dev/null)" ] && found "$f" "the AWS CLI's login"
done
for f in .config/gcloud/credentials.db .config/gcloud/access_tokens.db .config/gcloud/application_default_credentials.json .config/gcloud/legacy_credentials; do
  [ -n "$(ls -A "$f" 2>/dev/null)" ] && found "$f" "gcloud's login"
done
for f in .azure/msal_token_cache.json .azure/accessTokens.json .azure/msal_token_cache.bin; do
  [ -s "$f" ] && found "$f" "the Azure CLI's login"
done
[ -f .docker/config.json ] && grep -q '"auth"' .docker/config.json && found .docker/config.json "a container registry's login"
[ -f .kube/config ] && grep -Eq '(token|client-key-data|password):' .kube/config && found .kube/config "a Kubernetes login"
exit 0
"#;

/// The secrets on the box at `ip`: where other tools leave theirs
/// ([`SECRETS`]), `~/<path> (<what>)`, and what Link's modules there
/// keep ([`kept_secrets`]).
fn secrets(ip: &str) -> anyhow::Result<Vec<String>> {
    let said = ssh::script_output(ip, SECRETS)?;
    let mut found: Vec<String> = said
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(path, what)| format!("~/{} ({})", clean(path), clean(what)))
        .collect();
    found.extend(kept_secrets(ip)?);
    Ok(found)
}

/// What each of Link's modules on the box at `ip` keeps that an image
/// must never carry, as each says (the box's `lnk uninstall --parts
/// --json`, each part's secret paths): each that holds anything, `<path>
/// (lnk's <part>)`. Every installed module is checked. A box whose lnk doesn't say refuses, so a
/// module that moves its keys never makes this pass.
fn kept_secrets(ip: &str) -> anyhow::Result<Vec<String>> {
    let kept: link_core::uninstall::Kept = on_box(ip, "uninstall --parts --json")
        .and_then(|said| Ok(serde_json::from_value(said)?))
        .within(
            "the box's lnk didn't say what its modules keep (an lnk too early to say?): \
             run lnk box run <box> -- upgrade first",
        )?;
    let script = held_script(&kept)?;
    let said = ssh::script_output(ip, &script)?;
    Ok(said
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(path, part)| format!("{} (lnk's {})", clean(path), clean(part)))
        .collect())
}

/// A script printing `<path>\t<part>` for each of `kept`'s secret paths
/// that holds anything: a folder with anything in it, or a file that
/// isn't empty. One it can't look into (a folder above it, or itself,
/// not this user's to search or read: a keeper's store, which is another
/// user's) is taken as holding something, so the check fails closed. A
/// path that isn't absolute, or holds a character no path of Link's
/// does, refuses it.
fn held_script(kept: &link_core::uninstall::Kept) -> anyhow::Result<String> {
    let quote = |text: &str| format!("'{}'", text.replace('\'', r"'\''"));
    let mut script = String::from(
        "found() { printf '%s\\t%s\\n' \"$1\" \"$2\"; }\n\
         hidden() { d=$(dirname \"$1\"); while [ \"$d\" != / ]; do \
           if [ -e \"$d\" ] && ! [ -x \"$d\" ]; then return 0; fi; d=$(dirname \"$d\"); done; return 1; }\n\
         held() { hidden \"$1\" && return 0; \
           if [ -d \"$1\" ]; then { [ -r \"$1\" ] && [ -x \"$1\" ]; } || return 0; \
           [ -n \"$(ls -A \"$1\" 2>/dev/null)\" ]; \
           else [ -s \"$1\" ]; fi; }\n",
    );
    for part in &kept.parts {
        ensure!(
            !part.name.chars().any(char::is_control),
            "the box's lnk named a part that isn't plain"
        );
        for path in part.secret_paths() {
            let text = path.to_string_lossy();
            ensure!(
                path.is_absolute() && !text.chars().any(char::is_control),
                "the box's lnk named a path that isn't plain: {}",
                clean(&text)
            );
            script.push_str(&format!(
                "held {p} && found {p} {name}\n",
                p = quote(&text),
                name = quote(&part.name)
            ));
        }
    }
    script.push_str("exit 0\n");
    Ok(script)
}

/// `lnk box image create <box> <image> [--hash <hash>]`.
pub fn create(
    out: &mut Out,
    name: &str,
    image: &str,
    hash: Option<&str>,
    yes: bool,
) -> anyhow::Result<Done> {
    cloud::check_image(image)?;
    if let Some(hash) = hash {
        cloud::check_image_hash(hash)?;
    }
    let shown = Name::new("box", name);
    let image_name = Value::from(image.to_string());
    let machine = Boxes::load()?.pick(Some(name))?.clone();
    if machine.id.is_empty() {
        return Err(usage(
            sentence!("Couldn't save {}, which isn't made yet.", &shown),
            prose!("Another lnk box create is making it, or stopped before it could."),
            cmd!("lnk box list"),
            "list your boxes",
        ));
    }
    let account = account(&machine.account)?;
    let account_name = Name::new("account", &machine.account);
    let owner = need_owner()?;
    let taken = images(&account, &owner)?;
    if let Some(same) = taken.iter().find(|i| i.name == image) {
        return Err(said(
            Report::error(sentence!(
                "Couldn't save the image {}, since one is in {} already.",
                &image_name,
                &account_name
            ))
            .why(prose!(
                "The image {} has the name.",
                Value::from(clean(&same.id))
            ))
            .next(
                cmd!("lnk box image delete {} {}", &image_name, &account_name),
                "delete it first",
            )
            .next(
                cmd!("lnk box image create {} <image>", &shown),
                "or save under another name",
            )
            .exit(Exit::Usage),
        ));
    }
    if let Ok(reply) = call(&machine, "state")
        && let Some(state) = reply.state
    {
        let by_cloud = reply.stopped_by_cloud;
        Boxes::update(|boxes| {
            if let Some(saved) = boxes.get_mut(name)
                && saved.id == machine.id
            {
                saved.seen(state, by_cloud);
            }
        })?;
    }
    let machine = Boxes::load()?.pick(Some(name))?.clone();
    if link_box::state::stopped(&machine.state) {
        return Err(usage(
            sentence!("Couldn't save {}, which is stopped.", &shown),
            prose!("Its lnk is signed out before it's saved, so it must run."),
            cmd!("lnk box start {}", &shown),
            "start it first",
        ));
    }
    let ip = crate::reachable(out, Some(name))?;
    // What runs on the box would run on each box from the image too, as
    // the same agent, with its keys and connections.
    // Any answer but a list (an SSH failure, an agent's settings it
    // can't read) refuses it: what can't be checked isn't saved.
    let agents = agents_on(&ip).within(format!(
        "asking {name}'s lnk which agents are on it, before it's saved"
    ))?;
    if !agents.is_empty() {
        let names: Vec<Value> = agents
            .iter()
            .filter_map(|a| {
                a["name"]
                    .as_str()
                    .map(|n| Name::new("agent", clean(n)).into())
            })
            .collect();
        return Err(said(
            Report::error(sentence!("Couldn't save {}, which has agents on it.", &shown))
                .why(prose!(
                    "An image would carry each of {}, its keys and connections with it, and each box from it would run them too, so save a box before an agent comes, or move them off it first.",
                    Value::List(names)
                ))
                .next(cmd!("lnk agent move here --agent <agent>"), "move an agent off it")
                .exit(Exit::Usage),
        ));
    }
    // What a setup left on the disk would be in every box from it.
    let found = secrets(&ip).within(format!("looking for secrets on {name} before it's saved"))?;
    if !found.is_empty() {
        return Err(said(
            Report::error(sentence!(
                "Couldn't save {}, which holds secrets an image would carry to every box made from it.",
                &shown
            ))
            .why(prose!(
                "Remove them from it, then save it again; a setup stage in a sandbox of its own keeps them off the disk."
            ))
            .tail(link_format::Tail::last(&found.join("\n"), found.len()))
            .next(cmd!("lnk box ssh {}", &shown), "remove them there")
            .exit(Exit::Usage),
        ));
    }
    let disk = call(&machine, "describe")
        .ok()
        .and_then(|r| r.boxes.unwrap_or_default().into_iter().next())
        .and_then(|f| f.disk_gb);
    out.block(Note(sentence!(
        "The disk of {} is saved as the image {} in your {} account {} ({}).",
        &shown,
        &image_name,
        machine.kind.as_str(),
        &account_name,
        machine.place.as_str()
    )));
    out.now(
        Fields::new()
            .row(
                "Downtime",
                format!("{name} is stopped while it's saved, a few minutes, then started again"),
            )
            .row(
                "Cost",
                format!(
                    "a copy of the disk{}, billed to that account by {} every month until you remove it",
                    disk.map_or(String::new(), |gb| format!(" ({gb} GB)")),
                    clean(&machine.kind)
                ),
            ),
    );
    if !out.asks(Question::confirm(sentence!("Save it?"), true).yes(yes))? {
        return Err(said(Report::refusal(
            Refuser::Person,
            sentence!("Didn't save the image {}.", &image_name),
        )));
    }
    let mut steps = Steps::new(3);
    // A box made with a deadline ends at it, by a timer on its disk: left
    // out of the image, and put back once the box runs again.
    let timed = ssh::script_output(&ip, &crate::setup::has_deadline())
        .within("reading whether the box has a deadline")?
        .trim()
        == "timed";
    ssh::script_output(&ip, crate::setup::WITHOUT_DEADLINE)
        .within("leaving the box's deadline out of its image")?;
    // The freed blocks of what was deleted (the old secrets among them)
    // are dropped from the disk, where the cloud's copy can't read them.
    // A disk that doesn't take it keeps them.
    let _ = ssh::script_output(&ip, "sudo -n fstrim -a >/dev/null 2>&1 || true\n");
    out.step(&mut steps, phrase!("Stopping {}", &shown));
    call(&machine, "stop")?;
    ssh::close(&ip);
    Boxes::update(|boxes| {
        if let Some(saved) = boxes.get_mut(name)
            && saved.id == machine.id
        {
            saved.state = "stopped".into();
        }
    })?;
    let saving = out.wait_step(
        &mut steps,
        phrase!("Saving the disk of {} as {}", &shown, &image_name),
    );
    let mut tags = cloud::image_tags(image, &owner);
    if let Some(hash) = hash {
        tags.insert(cloud::IMAGE_HASH.into(), hash.to_string());
    }
    let saved = call_with(&machine, "image-save", |r| {
        r.image = Some(image.to_string());
        r.tags = tags;
    })
    .and_then(|r| {
        r.images
            .unwrap_or_default()
            .into_iter()
            .next()
            .within("the adapter didn't say which image it made")
    });
    drop(saving);
    // Started again whatever came of saving: it was running.
    out.step(&mut steps, phrase!("Starting {} again", &shown));
    let restarted = crate::restart(out, name).and_then(|ip| {
        // Its deadline, as its first boot set it, kept.
        if timed && let Some(until) = machine.deadline {
            ssh::script_output(
                &ip,
                &link_cloud_contract::first_boot::deadline_script(until),
            )
            .within("putting the box's deadline back")?;
        }
        Ok(ip)
    });
    let saved = match (saved, restarted) {
        (Ok(saved), Ok(_)) => saved,
        (Ok(_), Err(err)) => {
            return Err(said(
                failed(
                    sentence!(
                        "Saved the image {}, but {} didn't start again.",
                        &image_name,
                        &shown
                    ),
                    err,
                )
                .next(cmd!("lnk box start {}", &shown), "start it"),
            ));
        }
        (Err(err), Ok(_)) => {
            return Err(said(
                failed(
                    sentence!(
                        "Couldn't save the image, though {} is running again.",
                        &shown
                    ),
                    err,
                )
                .next(
                    cmd!("lnk box image list {}", &account_name),
                    "see which images exist",
                ),
            ));
        }
        (Err(err), Err(again)) => {
            return Err(said(
                failed(
                    sentence!(
                        "Couldn't save the image, and {} didn't start again.",
                        &shown
                    ),
                    err,
                )
                .why(words(&again))
                .next(cmd!("lnk box start {}", &shown), "start it"),
            ));
        }
    };
    Ok(Done::success(sentence!(
        "The image {} is saved as {}{}.",
        &image_name,
        Value::from(clean(&saved.id)),
        match saved.place.is_empty() {
            true => String::new(),
            false => format!(" in {}", clean(&saved.place)),
        }
    ))
    .next(
        cmd!("lnk box create {} --image {}", &account_name, &image_name),
        "make a new box from it",
    )
    .next(
        cmd!("lnk box image delete {} {}", &image_name, &account_name),
        "delete it",
    ))
}

/// `lnk <args>` on the box at `ip` (Link's own words, never a user's):
/// what it printed.
fn run_on_box(ip: &str, args: &str) -> anyhow::Result<String> {
    ssh::script_output(ip, &format!("\"$HOME/.lnk/exec/bin/lnk\" {args}\n"))
}

/// [`run_on_box`], its output read as JSON.
fn on_box(ip: &str, args: &str) -> anyhow::Result<serde_json::Value> {
    let said = run_on_box(ip, args)?;
    serde_json::from_str(said.trim()).within(format!("the box's lnk {args} didn't answer JSON"))
}

/// The agents on the box at ip, as its lnk agent list says. A box
/// without the harness module has none ([SECRETS] finds a folder an
/// agent left there).
fn agents_on(ip: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let modules: Vec<link_core::modules::Listed> =
        serde_json::from_value(on_box(ip, "module list --json")?)
            .within("the box's lnk module list --json didn't list its modules")?;
    let harness = modules
        .iter()
        .any(|p| matches!(p.name.as_str(), "agent" | "harness") && p.installed);
    if !harness {
        return Ok(Vec::new());
    }
    on_box(ip, "agent list --json --here")?["agents"]
        .as_array()
        .cloned()
        .within("the box's lnk agent list didn't list its agents")
}

/// `lnk box image list <cloud> [--hash <hash>]`.
pub fn list(out: &mut Out, account_name: &str, hash: Option<&str>) -> anyhow::Result<Done> {
    let account = account(account_name)?;
    let owner = need_owner()?;
    let found = match hash {
        Some(hash) => hashed(&account, &owner, hash)?,
        None => images(&account, &owner)?,
    };
    out.reply(&link_actions::answer::output::<link_box::cli::Images>(
        &found,
    )?);
    let shown = Name::new("account", account_name);
    let mut table = Table::new(
        ["Name", "Place", "Disk", "Arch", "State", "Made"],
        match hash {
            Some(hash) => sentence!(
                "No images with the hash {} in {}.",
                Value::from(hash.to_string()),
                &shown
            ),
            None => sentence!("No images in {}.", &shown),
        },
    );
    for image in &found {
        table.row([
            Value::from(clean(&image.name)),
            match image.place.is_empty() {
                true => Value::from("anywhere"),
                false => Value::from(clean(&image.place)),
            },
            image.disk_gb.map_or(Value::from("-"), |gb| {
                link_format::Size(gb * 1_073_741_824).into()
            }),
            image
                .arch
                .as_deref()
                .map_or(Value::from("-"), |a| Value::from(clean(a))),
            Value::from(clean(&image.state)),
            Value::from(clean(image.created().get(..10).unwrap_or(image.created()))),
        ]);
    }
    out.block(table);
    Ok(Done::shown().next(
        match found.first() {
            Some(first) => cmd!(
                "lnk box create {} --image {}",
                &shown,
                Value::from(clean(&first.name))
            ),
            None => cmd!("lnk box image create <box> <image>"),
        },
        match found.is_empty() {
            true => "save a box's disk as one",
            false => "make a box from the first",
        },
    ))
}

/// `lnk box image delete <image> <cloud>`.
pub fn delete(out: &mut Out, account_name: &str, image: &str, yes: bool) -> anyhow::Result<Done> {
    let account = account(account_name)?;
    let owner = need_owner()?;
    let found = find(&account, &owner, image)?;
    let shown = Value::from(image.to_string());
    if !out.asks(
        Question::confirm(
            sentence!(
                "Delete the image {} and its copy of the disk? Boxes made from it stay.",
                &shown
            ),
            false,
        )
        .yes(yes),
    )? {
        return Err(said(
            Report::refusal(
                Refuser::Person,
                sentence!("Didn't delete the image {}.", &shown),
            )
            .next(
                cmd!("lnk box image list {}", Name::new("account", account_name)),
                "list its images",
            ),
        ));
    }
    let deleting = out.activity(phrase!("Deleting the image {}", &shown));
    let request = Request {
        account: account.duplicate(),
        place: Some(found.place.clone()).filter(|p| !p.is_empty()),
        image: Some(found.id.clone()),
        owner: Some(owner),
        ..Request::default()
    };
    cloud::call(&account.kind, "image-remove", &request)?;
    drop(deleting);
    Ok(Done::success(sentence!(
        "The image {} is gone, with its copy of the disk.",
        &shown
    ))
    .next(
        cmd!("lnk box image list {}", Name::new("account", account_name)),
        "list its images",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The secrets script on `home`: the paths it finds, sorted.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn found_in(home: &std::path::Path) -> Vec<String> {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(SECRETS)
            .env("HOME", home)
            .output()
            .unwrap();
        assert!(output.status.success());
        let said = String::from_utf8(output.stdout).unwrap();
        let mut found: Vec<String> = said
            .lines()
            .map(|l| l.split('\t').next().unwrap().to_string())
            .collect();
        found.sort_unstable();
        found
    }

    /// The secrets script, run on a home folder like a box's: what it
    /// finds, and what it leaves.
    #[test]
    fn secrets_are_found_where_they_usually_are() {
        let home = link_test_kit::TempDir::new("secrets");
        let put = |path: &str, text: &str| {
            let path = home.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // An empty home holds none.
        std::fs::create_dir_all(&home).unwrap();
        assert!(found_in(&home).is_empty());
        // Found.
        put(".git-credentials", "https://me:ghp_x@github.com\n");
        put(".ssh/id_ed25519", "-----BEGIN OPENSSH PRIVATE KEY-----\n");
        put("app/.npmrc", "//registry.npmjs.org/:_authToken=npm_abc\n");
        put(
            ".pypirc",
            "[pypi]\nusername = __token__\npassword = pypi-abc\n",
        );
        put(".aws/credentials", "[default]\naws_secret_access_key = x\n");
        put(".config/gcloud/application_default_credentials.json", "{}");
        put(
            ".docker/config.json",
            r#"{"auths": {"ghcr.io": {"auth": "eDp5"}}}"#,
        );
        // Not secrets: a key's public half, an empty file, a token read
        // from the environment, a dependency's own file, a CLI's settings.
        put(".ssh/id_ed25519.pub", "ssh-ed25519 AAAA me\n");
        put(".ssh/authorized_keys", "ssh-ed25519 AAAA me\n");
        put(".config/git/credentials", "");
        put(
            "web/.npmrc",
            "//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n",
        );
        put("app/node_modules/x/.npmrc", "_authToken=npm_theirs\n");
        put(".aws/config", "[default]\nregion = eu-west-2\n");
        // Link's own are its modules' to say (below).
        put(".lnk/safe/installation.toml", "id = \"abc\"\n");
        put(".lnk/safe/clouds/accounts.toml", "[[account]]\n");
        put(".lnk/safe/gcp/credentials/work.json", "{}");
        put(".lnk/safe/storage/rclone.conf", "[aws-x]\n");
        put(
            ".lnk/safe/boxes/id_ed25519",
            "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        );
        put("Link/Agents/.moved/ada-2026-10-01/notes.md", "hi");
        put(".lnk/data/agents/move/cy-2026-10-02/notes.md", "hi");
        put("Link/Agents/bob/notes.md", "hi");
        assert_eq!(
            found_in(&home),
            [
                ".aws/credentials",
                ".config/gcloud/application_default_credentials.json",
                ".docker/config.json",
                ".git-credentials",
                ".pypirc",
                ".ssh/id_ed25519",
                "app/.npmrc",
            ]
        );
    }

    /// What Link's modules keep, as each says: a secret path that holds
    /// anything is found, whatever its name, and one a module moves is
    /// found where it says; what isn't secret, or is empty, isn't; the
    /// login is included too.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn what_modules_keep_is_found_where_they_say() {
        use link_core::uninstall::{Kept, Part};
        let home = link_test_kit::TempDir::new("lnk-kept-secrets");
        let at = |p: &str| home.join(p);
        let put = |path: &str, text: &str| {
            std::fs::create_dir_all(at(path).parent().unwrap()).unwrap();
            std::fs::write(at(path), text).unwrap();
        };
        put(
            ".lnk/safe/storage/a new place/it's rclone.conf",
            "[aws-x]\n",
        );
        put(".lnk/tool/web/settings.toml", "name = \"web\"\n");
        put(".lnk/tool/secrets.toml", "");
        put(".lnk/safe/accounts/accounts.toml", "token = \"x\"\n");
        put("Link/Agents/bob/notes.md", "hi");
        std::fs::create_dir_all(at(".lnk/data/agents/move")).unwrap();
        let part = |name: &str, paths: &[&str], secrets: Option<&[&str]>| Part {
            name: name.into(),
            paths: paths.iter().map(|p| at(p)).collect(),
            secrets: secrets.map(|s| s.iter().map(|p| at(p)).collect()),
            ..Part::default()
        };
        // The box's user's own home, which it can look into.
        std::fs::set_permissions(&home, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        // A store in another user's home: an empty folder there, which
        // this user can't look into, is taken as holding something.
        std::fs::create_dir_all(at("keeper/store")).unwrap();
        std::fs::set_permissions(
            at("keeper"),
            std::os::unix::fs::PermissionsExt::from_mode(if unsafe { libc::geteuid() } == 0 {
                0o700
            } else {
                0o000
            }),
        )
        .unwrap();
        let kept = Kept {
            parts: vec![
                part("keeper", &["keeper/store"], None),
                part("buckets", &[".lnk/safe/storage"], None),
                part("tools", &[".lnk/tool"], Some(&[".lnk/tool/secrets.toml"])),
                part("relay", &[".lnk/safe/accounts/accounts.toml"], None),
                part(
                    "agents",
                    &["Link/Agents", ".lnk/data/agents/move", ".lnk/safe/agents"],
                    None,
                ),
            ],
            ..Kept::default()
        };
        // As a box's user, never root, who can look into everything:
        // nobody, when this runs as root.
        let mut command = match unsafe { libc::geteuid() } {
            0 => {
                let mut c = std::process::Command::new("setpriv");
                c.args(["--reuid=65534", "--regid=65534", "--clear-groups", "sh"]);
                c
            }
            _ => std::process::Command::new("sh"),
        };
        let output = command
            .arg("-c")
            .arg(held_script(&kept).unwrap())
            .output()
            .unwrap();
        std::fs::set_permissions(
            at("keeper"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        assert!(output.status.success());
        let said = String::from_utf8(output.stdout).unwrap();
        let found: Vec<(&str, &str)> = said.lines().filter_map(|l| l.split_once('\t')).collect();
        assert_eq!(
            found,
            [
                (at("keeper/store").to_str().unwrap(), "keeper"),
                (at(".lnk/safe/storage").to_str().unwrap(), "buckets"),
                (
                    at(".lnk/safe/accounts/accounts.toml").to_str().unwrap(),
                    "relay"
                ),
                (at("Link/Agents").to_str().unwrap(), "agents"),
            ]
        );
        // A path that isn't plain is refused, never run.
        let bad = Kept {
            parts: vec![part("x", &[], Some(&["a\nb"]))],
            ..Kept::default()
        };
        assert!(held_script(&bad).is_err());
        let relative = Kept {
            parts: vec![Part {
                name: "x".into(),
                paths: vec!["relative".into()],
                ..Part::default()
            }],
            ..Kept::default()
        };
        assert!(held_script(&relative).is_err());
    }
}

#[cfg(test)]
#[path = "image/times.rs"]
mod times;
