//! `lnk storage list --tree`, `pull` and `restore`: what the clouds hold, brought
//! back.

use super::*;
use link_format::WithinExt;
use pushing::{print_checks, print_hosted};

pub(super) fn tree(
    out: &mut Out,
    path: Option<&str>,
    level: Option<usize>,
    dirs_only: bool,
    refresh: bool,
) -> Ran {
    let folders = touched(out, files::folders()?);
    let shown: Vec<(Folder, String)> = match path {
        Some(path) => by_folder(&folders, &[path.to_string()])?
            .into_iter()
            .map(|(folder, mut paths)| (folder, paths.remove(0)))
            .collect(),
        None => folders.iter().map(|f| (f.clone(), String::new())).collect(),
    };
    let accounts = Clouds::load()?.clouds;
    let mut by_name = serde_json::Map::new();
    let mut drawn = 0;
    for (folder, prefix) in &shown {
        let clouds = Clouds::buckets(&accounts, &folder.id);
        let mut manifest = Manifest::open(&folder.id)?;
        if refresh && !clouds.is_empty() {
            apart(&clouds, &folders)?;
            let rclone = Rclone::new()?;
            for cloud in &clouds {
                ops::refresh(&rclone, &mut manifest, cloud)
                    .within(format!("listing {}", cloud.name))?;
            }
            manifest.save()?;
        }
        let rows = ops::tree(&folder.root, &manifest, &clouds, prefix)?;
        by_name.insert(folder.id.clone(), serde_json::to_value(&rows)?);
        if out.is_json() {
            continue;
        }
        let label = tilde(&folder.root.join(prefix));
        if rows.is_empty() {
            // With no path, a folder that's empty everywhere isn't drawn.
            if path.is_some() {
                out.block(Note(sentence!(
                    "There is nothing in {} yet.",
                    Foreign::new(&label)
                )));
            }
            continue;
        }
        let mut drawing = String::new();
        if drawn > 0 {
            drawing.push('\n');
        }
        drawn += 1;
        let names: Vec<String> = clouds.iter().map(|c| c.name.clone()).collect();
        // A tree drawn like `tree`'s: a document of its own.
        drawing += &link_storage::tree::render(&rows, &names, &label, prefix, level, dirs_only);
        out.data(drawing.as_bytes());
    }
    out.reply(&serde_json::Value::Object(by_name));
    if path.is_none() && drawn == 0 && !out.is_json() {
        out.block(Note(sentence!(
            "There is nothing yet in {}.",
            Where::new(folders[0].root.clone())
        )));
        return Ok(Done::shown().next(
            cmd!("lnk storage push <folder>"),
            phrase!("send what you put there"),
        ));
    }
    Ok(Done::shown())
}

pub(super) fn pull(
    out: &mut Out,
    args: &[String],
    from: &Only,
    options: &Download,
    yes: bool,
) -> Ran {
    let folders = touched(out, files::folders_with_hosted()?);
    let (hosted, groups): (Vec<_>, Vec<_>) = by_folder(&folders, args)?
        .into_iter()
        .partition(|(folder, _)| folder.hosted.is_some());
    // A folder a cloud account keeps: from that account.
    for (folder, patterns) in &hosted {
        heading(out, folder);
        let patterns: Vec<String> = patterns.iter().filter(|p| !p.is_empty()).cloned().collect();
        let pulled = link_storage::hosted::pull(folder, &patterns)?;
        print_hosted(out, folder, &pulled, "brought from");
    }
    if groups.is_empty() {
        return Ok(Done::shown());
    }
    let accounts = Clouds::load()?.pick(from)?;
    // Its owner could swap a folder for a link while files arrive.
    for (folder, _) in &groups {
        folder.running.refuse("pulling files")?;
    }
    let mut all_same = true;
    let mut restoring = false;
    for (folder, patterns) in &groups {
        heading(out, folder);
        let clouds = Clouds::buckets(&accounts, &folder.id);
        apart(&clouds, &folders)?;
        let mut manifest = Manifest::open(&folder.id)?;
        let plan = ops::plan_pull(&folder.root, &manifest, &clouds, patterns)?;
        if !plan.here.is_empty() {
            out.block(Note(sentence!(
                "{} are here already.",
                Count::new(plan.here.len() as u64, "file", "files")
            )));
        }
        if !plan.refused.is_empty() {
            out.block(Report::warning(sentence!(
                "Not pulling {}, for example {}, since a folder on the way to them here is a link, which would lead outside the folder.",
                Count::new(plan.refused.len() as u64, "file", "files"),
                Foreign::new(&plan.refused[0])
            )));
            all_same = false;
        }
        for (cloud, files) in &plan.restore {
            out.block(Report::warning(sentence!(
                "{} are in {}'s archive tier: unless restored already, they're restored first, which takes up to 12 hours and costs money.",
                Count::new(files.len() as u64, "file", "files"),
                Name::new("storage", cloud.name.clone())
            )));
        }
        let costly: Vec<&Cloud> = plan
            .download
            .iter()
            .filter(|(c, _)| matches!(c.kind, Kind::Aws | Kind::Gcp))
            .map(|(c, _)| c)
            .collect();
        if !costly.is_empty() && plan.bytes > 1_000_000_000 {
            let names: Vec<&str> = costly.iter().map(|c| c.name.as_str()).collect();
            out.block(Report::warning(sentence!(
                "Downloading {} from {} is charged by the cloud, about $90 per TB.",
                link_format::Size(plan.bytes),
                Foreign::new(&names.join(" and "))
            )));
        }
        if (!plan.restore.is_empty() || (!costly.is_empty() && plan.bytes > 1_000_000_000))
            && !confirm(out, sentence!("Go ahead?"), yes)?
        {
            return Err(declined(sentence!("Nothing was pulled.")));
        }
        if plan.download.is_empty() && plan.restore.is_empty() {
            continue;
        }
        let rclone = Rclone::new()?;
        let result = ops::apply_pull(&rclone, &folder.root, &mut manifest, &plan, options);
        manifest.save()?;
        let reports = result?;
        print_checks(
            out,
            &reports,
            "lnk storage pull --replace replaces the file here with the copy",
        );
        restoring |= reports.iter().any(|r| !r.waiting.is_empty());
        all_same &= reports.iter().all(Checked::all_same);
    }
    if restoring {
        out.block(Note(sentence!(
            "Restores were asked for, and take up to 12 hours."
        )));
    }
    if all_same {
        Ok(Done::success(sentence!("Every file came back.")).next(
            cmd!("lnk storage list --tree"),
            phrase!("see where each file is"),
        ))
    } else if restoring {
        Err(said(
            Report::error(sentence!("Not every file came back yet."))
                .next(
                    cmd!("lnk storage pull"),
                    phrase!("run the same pull again once the restores are done"),
                )
                .next(
                    cmd!("lnk storage list --tree"),
                    phrase!("see which are missing"),
                ),
        ))
    } else {
        Err(said(
            Report::error(sentence!("Not every file came back."))
                .next(
                    cmd!("lnk storage pull --replace"),
                    phrase!("replace the files that differ"),
                )
                .next(
                    cmd!("lnk storage list --tree"),
                    phrase!("see which are missing"),
                ),
        ))
    }
}

/// `lnk storage restore`: the buckets of the owner `named` names
/// (`<kind>:<name>`, or its ID's id in place of the name, when two of
/// its kind have the name) adopted here, with the password the user
/// saved, and its files pulled into `into`. Only `{"owner", "buckets",
/// "labels"}` goes to stdout, for the module that keeps the owner; what
/// it says and asks goes to stderr.
pub(super) fn restore(out: &mut Out, named: &str, into: &Path) -> Ran {
    let found = owners::find_named(named)
        .map_err(|err| said(said::failed(sentence!("That doesn't name an owner."), err)))?;
    // A second bucket with the same labels, in one cloud, is refused:
    // it would take the owner's place.
    let found: Vec<owners::Found> = owners::one_per_cloud(found)?;
    let mut ids: Vec<&Owner> = found.iter().map(|b| &b.owner).collect();
    ids.sort();
    ids.dedup();
    let owner = match ids.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(said(
                usage(
                    sentence!(
                        "There is no bucket of {} in your cloud accounts.",
                        Foreign::new(named)
                    ),
                    cmd!("lnk cloud list"),
                    phrase!("see your cloud accounts"),
                )
                .why(prose!(
                    "Link finds an owner's buckets by the labels it gives them when it makes them."
                )),
            ));
        }
        many => {
            let ids: Vec<&str> = many.iter().map(|o| o.as_str()).collect();
            return Err(said(usage(
                sentence!(
                    "Buckets of {} are named {}.",
                    Count::new(many.len() as u64, "owner", "owners"),
                    Foreign::new(named)
                ),
                cmd!(
                    "lnk storage restore --owner <kind>:<id> --folder {}",
                    tilde(into).as_str()
                ),
                phrase!("pick one by its ID: {}", Foreign::new(&ids.join(", "))),
            )));
        }
    };
    let name = found
        .iter()
        .find(|b| b.owner == owner)
        .map(|b| b.name.clone())
        .unwrap_or_default();
    let buckets: std::collections::BTreeMap<String, owners::Carried> = found
        .iter()
        .filter(|b| b.owner == owner)
        .map(|b| {
            let carried = owners::Carried {
                name: b.bucket.clone(),
                resource: None,
            };
            (b.cloud.clone(), carried)
        })
        .collect();
    // What `lnk storage label` added to each, set with your accounts.
    let labels: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> =
        found
            .iter()
            .filter(|b| b.owner == owner)
            .map(|b| (b.cloud.clone(), b.labels.clone()))
            .collect();
    out.block(Note(sentence!(
        "Restoring {} from {}.",
        Foreign::new(&format!("{}:{name}", owner.kind())),
        Foreign::new(
            &buckets
                .iter()
                .map(|(cloud, bucket)| format!("{cloud} ({})", bucket.name))
                .collect::<Vec<_>>()
                .join(" and ")
        )
    )));
    let password = out
        .ask(Question::hidden(sentence!(
            "What is the encryption password of its files?"
        )))
        .map_err(said)?;
    // A box's way in: credentials reaching only its buckets; elsewhere the
    // account's own.
    // Its buckets as just found, not looked for again.
    let its: Vec<owners::Found> = found.into_iter().filter(|b| b.owner == owner).collect();
    let credentials = owners::keys_of(its.clone()).unwrap_or_default();
    let adopting = owners::Adopting {
        plain: Some(password),
        buckets: buckets.clone(),
        credentials,
        password: None,
    };
    for line in owners::adopt(&Rclone::new()?, &owner, &adopting, Some(its))? {
        out.block(Note(Sentence::adopt(&line)));
    }
    link_files::create_private_dir(into).within("making the folder for its files")?;
    // Its files, as `lnk storage pull <folder>` gets them, saying so on
    // stderr: stdout is the answer.
    let _ = out.hand_over();
    let me = std::env::current_exe().within("finding lnk-storage")?;
    let pulled = link_secretive::process::ours(me)
        .args(link_storage::cli::pull_again(into))
        .stdin(std::process::Stdio::inherit())
        .stdout(link_format::streams::child_stderr().within("handing it stderr")?)
        .status()
        .within("pulling its files")?;
    if !pulled.success() {
        return Err(said(
            Report::error(sentence!("Its files didn't all come back."))
                .why(prose!("The lines above say which."))
                .next(
                    cmd!("lnk storage pull {}", tilde(into).as_str()),
                    phrase!("try for the rest"),
                ),
        ));
    }
    // Each bucket by its name in its cloud, as its labels found it.
    let named: std::collections::BTreeMap<&String, &String> =
        buckets.iter().map(|(cloud, b)| (cloud, &b.name)).collect();
    plumbing(
        out,
        &serde_json::json!({"owner": owner, "buckets": named, "labels": labels}),
    );
    Ok(Done::shown())
}
