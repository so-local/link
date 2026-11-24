//! `lnk storage push`, `clean` and `delete`: files sent to every cloud and
//! checked, then the local or the cloud copies removed.

use super::*;
use connecting::show_password;
use link_format::look::Mark;

/// `folder`'s bucket in each of `accounts`, making those missing (an
/// owner's folder gets one, encrypted, on its first push). A bucket is
/// billed by its cloud, so one is made only after a yes (`yes`, `--yes`).
fn buckets_for_push(
    out: &mut Out,
    rclone: &Rclone,
    clouds: &mut Clouds,
    accounts: &[Account],
    folder: &Folder,
    yes: bool,
) -> anyhow::Result<Vec<Cloud>> {
    let mut billed = Vec::new();
    for account in accounts {
        if account.cloud(&folder.id).is_none()
            && account.serves(&folder.id)
            && connect::bucket_based(rclone, account)?
        {
            billed.push(account.name.as_str());
        }
    }
    if !billed.is_empty() {
        let question = sentence!(
            "Make a bucket for {} in {}, billed by {}?",
            Foreign::new(&folder.label()),
            Foreign::new(&billed.join(" and ")),
            if billed.len() == 1 {
                "that cloud"
            } else {
                "each cloud"
            }
        );
        if !confirm(out, question, yes)? {
            return Err(declined(sentence!("Nothing was sent.")));
        }
    }
    let mut made: Option<String> = None;
    let mut made_in = Vec::new();
    for account in accounts {
        if account.cloud(&folder.id).is_some() || !account.serves(&folder.id) {
            continue;
        }
        let stored = clouds
            .clouds
            .iter_mut()
            .find(|a| a.name == account.name)
            .within("the cloud went away")?;
        out.block(Note(sentence!(
            "Making a bucket for {} in {}{}.",
            Foreign::new(&folder.label()),
            Name::new("storage", account.name.clone()),
            if folder.is_owned() { ", encrypted" } else { "" }
        )));
        // One password per owner, whichever cloud its buckets are in.
        let kept = match folder.is_owned() {
            true => connect::owner_password(&folder.id)?,
            false => None,
        };
        let password = match (&made, &kept) {
            (Some(password), _) => Password::Given(password),
            (None, Some(obscured)) => Password::Obscured(obscured),
            (None, None) => Password::New,
        };
        let new = connect::add_bucket(
            rclone,
            stored,
            &folder.id,
            None,
            None,
            folder.is_owned(),
            password,
            &mut asking(out, yes),
        )?;
        let stored = stored.clone();
        clouds.save()?;
        if let (Some(owner), Some(bucket)) = (&folder.owner, stored.buckets.get(&folder.id)) {
            connect::label(&stored, &bucket.name, &owner.id, &owner.name);
        }
        if made.is_none() && new.is_some() {
            made = new;
        }
        made_in.push(account.name.as_str());
    }
    if let Some(password) = made {
        show_password(out, &made_in.join(" and "), &folder.label(), &password);
    }
    let names: Vec<&str> = accounts.iter().map(|a| a.name.as_str()).collect();
    Ok(clouds
        .clouds
        .iter()
        .filter(|a| names.contains(&a.name.as_str()))
        .filter_map(|a| a.cloud(&folder.id))
        .collect())
}

pub(super) fn push(
    out: &mut Out,
    args: &[String],
    to: &Only,
    options: &ops::Push,
    yes: bool,
) -> Ran {
    let folders = touched(out, files::folders_with_hosted()?);
    let (hosted, groups): (Vec<_>, Vec<_>) = by_folder(&folders, args)?
        .into_iter()
        .partition(|(folder, _)| folder.hosted.is_some());
    // A folder a cloud account keeps: through that account.
    for (folder, paths) in &hosted {
        heading(out, folder);
        let pushed = link_storage::hosted::push(folder, paths)?;
        print_hosted(out, folder, &pushed, "sent to");
    }
    if groups.is_empty() {
        return Ok(Done::shown());
    }
    let _lock = files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let accounts = clouds.pick(to)?;
    if options.archive {
        out.block(
            Report::warning(sentence!(
                "The archive tier costs about $1 per TB a month, with two catches."
            ))
            .why(prose!(
                "Reading files back takes up to 12 hours on aws, and costs money on both. Deleting them within 180 days (aws) or 365 days (gcp) is charged anyway."
            )),
        );
        if !confirm(out, sentence!("Archive them?"), yes)? {
            return Err(declined(sentence!("Nothing was sent.")));
        }
    }
    let rclone = Rclone::new()?;
    // An account past what it may keep reads only: nothing is sent.
    for account in &accounts {
        if let Some((_, message)) = rclone.reads_only(&account.name)? {
            return Err(said(
                Report::error(sentence!(
                    "{} takes nothing more.",
                    Name::new("storage", account.name.clone())
                ))
                .why(Prose::from(Sentence::adopt(&message)))
                .next(cmd!("lnk storage list"), phrase!("see your clouds")),
            ));
        }
    }
    let mut all_same = true;
    for (folder, paths) in &groups {
        heading(out, folder);
        let targets = buckets_for_push(out, &rclone, &mut clouds, &accounts, folder, yes)?;
        apart(&targets, &folders)?;
        let mut manifest = Manifest::open(&folder.id)?;
        let result = ops::push(
            &rclone,
            &folder.root,
            &mut manifest,
            &targets,
            paths,
            options,
        );
        manifest.save()?;
        let reports = result?;
        print_checks(
            out,
            &reports,
            "lnk storage push --replace replaces the copy there with the file here",
        );
        all_same &= reports.iter().all(Checked::all_same);
    }
    if all_same {
        Ok(Done::success(sentence!("Every file is in every cloud."))
            .next(
                cmd!("lnk storage list --tree"),
                phrase!("see where each file is"),
            )
            .next(
                cmd!("lnk storage clean <folder>"),
                phrase!("free the space the checked files take here"),
            ))
    } else {
        Err(said(
            Report::error(sentence!("Not every file is in every cloud yet."))
                .next(
                    cmd!("lnk storage push --replace"),
                    phrase!("replace the copies that differ"),
                )
                .next(
                    cmd!("lnk storage list --tree"),
                    phrase!("see which are missing"),
                ),
        ))
    }
}

/// What a push or a pull through the cloud account that keeps `folder`
/// did: the files that went (`verb` its copy there), and those the same
/// already.
pub(super) fn print_hosted(
    out: &mut Out,
    folder: &Folder,
    moved: &link_storage::hosted::Moved,
    verb: &str,
) {
    let account = folder.hosted.as_deref().unwrap_or_default();
    let shown: Vec<String> = moved
        .sent
        .iter()
        .take(5)
        .map(|p| link_format::clean(p))
        .collect();
    let more = match moved.sent.len() {
        n if n > 5 => format!(" and {} more", n - 5),
        _ => String::new(),
    };
    out.block(Success::new(sentence!(
        "{} {} its copy on {}{}, and {} the same there already.",
        Count::new(moved.sent.len() as u64, "file", "files"),
        verb,
        Foreign::new(account),
        match shown.is_empty() {
            true => String::new(),
            false => format!(" ({}{more})", shown.join(", ")),
        },
        moved.same
    )));
}

/// What each cloud's check found, `fix` saying what to do about copies
/// that differ.
pub(super) fn print_checks(out: &mut Out, reports: &[Checked], fix: &str) {
    const SHOWN: usize = 5;
    let list = |paths: &[String]| {
        let mut text = paths
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if paths.len() > SHOWN {
            text += &format!(" and {} more", paths.len() - SHOWN);
        }
        text
    };
    let mut table = Table::new(
        ["Cloud", "Checked", "What the check found"],
        sentence!("Nothing was checked."),
    );
    for report in reports {
        let mut parts = Vec::new();
        if report.unchanged > 0 {
            parts.push(format!("{} unchanged since checked", report.unchanged));
        }
        if !report.missing.is_empty() {
            parts.push(format!(
                "{} missing ({})",
                report.missing.len(),
                list(&report.missing)
            ));
        }
        if !report.differs.is_empty() {
            let verb = if report.differs.len() == 1 {
                "differs"
            } else {
                "differ"
            };
            parts.push(format!(
                "{} {verb} ({}; {fix})",
                report.differs.len(),
                list(&report.differs)
            ));
        }
        if !report.errors.is_empty() {
            let paths: Vec<String> = report
                .errors
                .iter()
                .map(|(p, why)| format!("{p}: {why}"))
                .collect();
            parts.push(format!("{} not checked ({})", paths.len(), list(&paths)));
        }
        if !report.waiting.is_empty() {
            parts.push(format!(
                "{} waiting for their restore ({})",
                report.waiting.len(),
                list(&report.waiting)
            ));
        }
        let mark = match report.all_same() {
            true => Mark::Success,
            false => Mark::Error,
        };
        table.row([
            Value::from(Name::new("storage", report.cloud.clone())),
            Value::from(mark),
            Value::from(Foreign::new(&format!(
                "{} checked{}{}",
                report.same.len(),
                if parts.is_empty() { "" } else { "; " },
                parts.join("; ")
            ))),
        ]);
    }
    out.block(table);
}

pub(super) fn clean(out: &mut Out, args: &[String], dry_run: bool, yes: bool) -> Ran {
    let folders = touched(out, files::folders()?);
    let groups = by_folder(&folders, args)?;
    let accounts = Clouds::load()?.pick(&Only::default())?;
    // Before anything is deleted in any folder.
    if !dry_run {
        for (folder, _) in &groups {
            folder.running.refuse("deleting files")?;
        }
    }
    let mut deleted = 0;
    for (folder, paths) in &groups {
        heading(out, folder);
        let missing: Vec<&str> = accounts
            .iter()
            .filter(|a| a.serves(&folder.id) && a.cloud(&folder.id).is_none())
            .map(|a| a.name.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(said(
                Report::error(sentence!(
                    "Nothing was deleted: {} has no copy of {} yet.",
                    Foreign::new(&missing.join(" and ")),
                    Foreign::new(&folder.label())
                ))
                .exit(Exit::Usage)
                .next(cmd!("lnk storage push"), phrase!("send them first")),
            ));
        }
        let clouds = Clouds::buckets(&accounts, &folder.id);
        apart(&clouds, &folders)?;
        let rclone = Rclone::new()?;
        let mut manifest = Manifest::open(&folder.id)?;
        let result = ops::plan_clean(&rclone, &folder.root, &mut manifest, &clouds, paths);
        manifest.save()?;
        let plan = result?;
        print_checks(
            out,
            &plan.checks,
            "lnk storage push --replace sends the file here in its place",
        );
        if !plan.safe() {
            return Err(said(
                Report::error(sentence!(
                    "Nothing was deleted: not every cloud has a checked copy of every file."
                ))
                .next(cmd!("lnk storage push"), phrase!("send them first")),
            ));
        }
        let what = format!("{} files ({})", plan.files.len(), human(plan.bytes()));
        if dry_run {
            out.block(Text(prose!(
                "It is safe to delete here: {}.",
                Foreign::new(&what)
            )));
            continue;
        }
        let names: Vec<&str> = clouds.iter().map(|c| c.name.as_str()).collect();
        if !confirm(
            out,
            sentence!(
                "Delete {} of {} ({}) from this machine, when {} each have a checked copy?",
                Foreign::new(&what),
                Foreign::new(&folder.label()),
                Where::new(folder.root.clone()),
                Foreign::new(&names.join(" and "))
            ),
            yes,
        )? {
            return Err(declined(sentence!("Nothing was deleted.")));
        }
        // The answer may have taken a while: its owner may have started.
        if folder.is_owned() {
            files::running(&folder.id)?.refuse("deleting files")?;
        }
        let kept = ops::apply_clean(&folder.root, &plan)?;
        deleted += plan.files.len() - kept.len();
        if !kept.is_empty() {
            out.block(Note(sentence!(
                "Kept {} that changed after the check, for example {}.",
                Count::new(kept.len() as u64, "file", "files"),
                Foreign::new(&kept[0])
            )));
        }
    }
    Ok(match dry_run {
        true => Done::shown().next(cmd!("lnk storage clean"), phrase!("delete them")),
        false => Done::success(sentence!(
            "Deleted {} here.",
            Count::new(deleted as u64, "file", "files")
        ))
        .next(cmd!("lnk storage pull"), phrase!("get them back")),
    })
}

/// `lnk storage remove <paths>`: files deleted from your clouds. A cloud's
/// name that's no file of yours is refused.
pub(super) fn delete(
    out: &mut Out,
    args: &[String],
    from: &Only,
    everywhere: bool,
    yes: bool,
) -> Ran {
    let folders = touched(out, files::folders()?);
    let all = Clouds::load()?;
    for arg in args {
        if all.get(arg).is_some() && a_path(&folders, arg).is_none() {
            return Err(said(
                usage(
                    sentence!(
                        "{} is a cloud, and no file of yours.",
                        Name::new("storage", arg.clone())
                    ),
                    cmd!("lnk storage remove {}", arg.as_str()),
                    phrase!("delete the cloud's buckets"),
                )
                .why(prose!("The command remove removes files from your clouds.")),
            ));
        }
    }
    let groups = by_folder(&folders, args)?;
    let targets = all.pick(from)?;
    let mut deleted = false;
    for (folder, patterns) in &groups {
        heading(out, folder);
        let every = Clouds::buckets(&all.clouds, &folder.id);
        let chosen = Clouds::buckets(&targets, &folder.id);
        apart(&every, &folders)?;
        let rclone = Rclone::new()?;
        let mut manifest = Manifest::open(&folder.id)?;
        let plan = ops::plan_remove(&rclone, &folder.root, &manifest, &every, &chosen, patterns)?;
        if !plan.last_copies.is_empty() && !everywhere {
            return Err(said(
                Report::error(sentence!(
                    "{} would have no copy left anywhere, for example {}.",
                    Count::new(plan.last_copies.len() as u64, "file", "files"),
                    Foreign::new(&plan.last_copies[0])
                ))
                .exit(Exit::Usage)
                .next(cmd!("lnk storage pull"), phrase!("pull them first"))
                .next(
                    cmd!("lnk storage remove --everywhere"),
                    phrase!("delete them anyway"),
                ),
            ));
        }
        if !plan.archived.is_empty() {
            out.block(Report::warning(sentence!(
                "{} are in an archive tier: deleting them before its minimum period (180 days on aws, 365 on gcp) is charged anyway.",
                Count::new(plan.archived.len() as u64, "file", "files")
            )));
        }
        let summary: Vec<String> = plan
            .delete
            .iter()
            .map(|(c, files)| format!("{} from {}", files.len(), c.name))
            .collect();
        let question = match plan.last_copies.is_empty() {
            true => sentence!("Delete {}?", Foreign::new(&summary.join(", "))),
            false => sentence!(
                "Delete {}, when {} will be gone for good?",
                Foreign::new(&summary.join(", ")),
                Count::new(plan.last_copies.len() as u64, "file", "files")
            ),
        };
        if !confirm(out, question, yes)? {
            return Err(declined(sentence!("Nothing was deleted.")));
        }
        let result = ops::apply_remove(&rclone, &mut manifest, &plan);
        manifest.save()?;
        result?;
        deleted = true;
    }
    Ok(match deleted {
        true => Done::success(sentence!("Deleted.")).next(
            cmd!("lnk storage list --tree"),
            phrase!("see what the clouds hold now"),
        ),
        false => Done::shown(),
    })
}
