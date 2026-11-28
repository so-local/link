//! `lnk-storage`: the storage module's commands. `lnk` runs it for `lnk
//! storage ...`.

use std::path::Path;

use link_format::{
    Command as Next, Count, Done, Exit, Fields, Foreign, Name, Note, Out, Path as Where, Phrase,
    Product, Prose, Question, Refuser, Report, Section, Sentence, Success, Table, Text, Value,
    Verbatim, WithinExt, cmd, phrase, prose, sentence,
};
use link_storage::cli::{self, Command};
use link_storage::clouds::{Account, Cloud, Clouds, Kind, Only, check_name};
use link_storage::connect::{self, Asked, Options, Password};
use link_storage::engine::Download;
use link_storage::files::{self, Folder, Owner, PERSONAL, human};
use link_storage::manifest::{Locked, Manifest};
use link_storage::ops::{self, Checked};
use link_storage::owners;
use link_storage::rclone::{self, Rclone};
use link_storage::said::{self, said};

mod connecting;
mod pulling;
mod pushing;
use connecting::{connect_cloud, disconnect, list_clouds, remove_cloud, show_cloud};
use pulling::{pull, restore, tree};
use pushing::{clean, delete, push};

/// What a command ran: its result, or an error that is a [`said::Said`]
/// report or the words of something that failed.
type Ran = anyhow::Result<Done>;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[
        link_storage::clouds::saved,
        link_storage::ids::saved,
        link_storage::manifest::saved,
        link_storage::rclone::keychain_saved,
    ]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// `result` ended: the report it carries, else a failure headed
/// `headline`, with `next` where it names none of its own.
fn ended(result: Ran, headline: Sentence, next: Next, why: Phrase) -> Result<Done, Report> {
    result.map_err(|err| {
        let report = said::report(err, headline);
        match report.next.is_empty() {
            true => report.next(next, why),
            false => report,
        }
    })
}

/// A mistake in what was typed: `headline`, perhaps its `why`, and what
/// to try.
fn usage(headline: Sentence, next: Next, why: Phrase) -> Report {
    Report::error(headline).exit(Exit::Usage).next(next, why)
}

/// The person said no: nothing happened.
fn declined(says: Sentence) -> anyhow::Error {
    said(Report::refusal(Refuser::Person, says))
}

/// What a hidden command prints for the program that runs it (`export`,
/// `keys`, `label`, `find`, `restore`): JSON on stdout, with or without
/// `--json`, since that output is its interface, read by the module that
/// keeps an owner and never shown to a person.
fn plumbing(out: &mut Out, answer: &serde_json::Value) {
    out.reply(answer);
    if !out.is_json() {
        out.data(format!("{answer}\n").as_bytes());
    }
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    // `lnk uninstall` and `lnk module remove` (`link_core::uninstall`).
    let command = cli::parse();
    if let Command::Uninstall(given) = &command {
        let args = given;
        use link_core::uninstall as u;
        let kept = u::Kept {
            parts: vec![
            u::Part {
                name: "personal".into(),
                about: "your files in ~/Link/Files, including any never pushed to a cloud".into(),
                paths: vec![u::files(link_files::layout::Home::Files)?],
                keys: vec![],
                first: "lnk storage push copies your files to your clouds first.".into(),
                secrets: None,
                credentials: vec![],
            },
            u::Part {
                name: "buckets".into(),
                about: "which bucket is which, and the passwords of encrypted buckets: without them, what you pushed can't be read by anyone".into(),
                paths: vec![u::settings(link_files::layout::Hidden::Storage)?],
                keys: vec!["storage/".into()],
                credentials: link_storage::files::config_dir()
                    .map(|dir| link_storage::rclone::held(&link_storage::rclone::config_in(&dir)))
                    .unwrap_or_default(),
                ..u::Part::default()
            },
            ],
            ..u::Kept::default()
        };
        u::run(args, &kept, || Ok(()), |_| Ok(()))?;
        return Ok(Done::shown());
    }
    match command {
        Command::Uninstall(_) => unreachable!("uninstall handled above"),
        // Offered on REST alone, served by a cloud: no command
        // reads them here.
        Command::FileList(_)
        | Command::FileRead(_)
        | Command::FileShow(_)
        | Command::FileWrite(_)
        | Command::FolderShow(_) => unreachable!("no command reads a REST action"),
        Command::StorageCreate(create) => {
            let yes = create.yes;
            let (kind, options) = options(create, None, None)?;
            ended(
                connect_cloud(out, kind, options, yes),
                sentence!("The cloud could not be connected."),
                cmd!("lnk storage list"),
                phrase!("see the clouds that are connected"),
            )
        }
        Command::StorageConnect(connect) => {
            let cli::StorageConnectArgs {
                bucket,
                owner_bucket,
                owner,
                ..
            } = connect.clone();
            // Which cloud and how to sign in: what create takes too.
            let cloud = cli::StorageCreateArgs {
                kind: connect.kind,
                path: connect.path,
                setting: connect.setting,
                name: connect.name,
                region: connect.region,
                profile: connect.profile,
                project: connect.project,
                endpoint: connect.endpoint,
                key_id: connect.key_id,
                encrypt: connect.encrypt,
                cloud: connect.cloud,
                yes: false,
            };
            let owner_bucket = match owner_bucket {
                Some(bucket) => Some((owner_id(owner.as_deref())?, bucket)),
                None => None,
            };
            let (kind, options) = options(cloud, Some(bucket), owner_bucket)?;
            ended(
                connect_cloud(out, kind, options, false),
                sentence!("The cloud could not be connected."),
                cmd!("lnk storage list"),
                phrase!("see the clouds that are connected"),
            )
        }
        Command::StorageDisconnect(cli::StorageDisconnectArgs { name, yes }) => ended(
            disconnect(out, &name, yes),
            sentence!("The cloud could not be disconnected."),
            cmd!("lnk storage list"),
            phrase!("see the clouds that are connected"),
        ),
        Command::StorageDelete(cli::StorageDeleteArgs {
            name,
            everywhere,
            yes,
        }) => ended(
            remove_cloud(out, &name, everywhere, yes),
            sentence!("The cloud's buckets could not be deleted."),
            cmd!("lnk storage list"),
            phrase!("see the clouds that are connected"),
        ),
        Command::StorageShow(cli::StorageShowArgs { name, .. }) => ended(
            show_cloud(out, &name),
            sentence!("The cloud could not be shown."),
            cmd!("lnk storage list"),
            phrase!("see the clouds that are connected"),
        ),
        Command::StorageList(cli::StorageListArgs {
            tree: true,
            path,
            level,
            dirs_only,
            refresh,
            json: _,
        }) => ended(
            tree(
                out,
                path.as_deref(),
                level.map(|l| usize::try_from(l).unwrap_or(usize::MAX)),
                dirs_only,
                refresh,
            ),
            sentence!("The clouds' files could not be shown."),
            cmd!("lnk storage list"),
            phrase!("check the clouds"),
        ),
        Command::StorageList(_) => ended(
            list_clouds(out),
            sentence!("The clouds could not be listed."),
            cmd!("lnk storage create <type>"),
            phrase!("connect one"),
        ),
        Command::StoragePush(cli::StoragePushArgs {
            paths,
            cloud,
            remote,
            archive,
            replace,
            recheck,
            yes,
            flags,
        }) => {
            let options = ops::Push {
                replace,
                archive,
                flags,
                all: recheck,
            };
            ended(
                push(
                    out,
                    &paths,
                    &Only {
                        clouds: cloud,
                        remotes: remote,
                    },
                    &options,
                    yes,
                ),
                sentence!("The files could not be pushed."),
                cmd!("lnk storage list"),
                phrase!("check the clouds they go to"),
            )
        }
        Command::StoragePull(cli::StoragePullArgs {
            paths,
            cloud,
            remote,
            replace,
            yes,
            flags,
        }) => ended(
            pull(
                out,
                &paths,
                &Only {
                    clouds: cloud.into_iter().collect(),
                    remotes: remote.into_iter().collect(),
                },
                &Download { replace, flags },
                yes,
            ),
            sentence!("The files could not be pulled."),
            cmd!("lnk storage list --tree"),
            phrase!("see what the clouds hold"),
        ),
        Command::StorageClean(cli::StorageCleanArgs { paths, dry, yes }) => ended(
            clean(out, &paths, dry, yes),
            sentence!("The files could not be cleaned."),
            cmd!("lnk storage list --tree"),
            phrase!("see which files are safe in your clouds"),
        ),
        Command::StorageExport(cli::StorageExportArgs { owner }) => {
            let owner = owner_named(&owner)?;
            let exported = owners::export(&owner)
                .and_then(|e| Ok(serde_json::to_value(e)?))
                .map_err(|err| {
                    said::failed(sentence!("Its buckets could not be exported."), err)
                })?;
            plumbing(out, &exported);
            Ok(Done::shown())
        }
        Command::StorageAdopt(cli::StorageAdoptArgs { owner }) => ended(
            owner_named(&owner)
                .map_err(said)
                .and_then(|owner| adopt(out, &owner)),
            sentence!("The buckets could not be adopted."),
            cmd!("lnk storage list"),
            phrase!("see the clouds that are connected"),
        ),
        Command::StorageLeave(cli::StorageLeaveArgs { owner }) => ended(
            owner_named(&owner)
                .map_err(said)
                .and_then(|owner| owners::delete(&owner))
                .map(|()| Done::shown()),
            sentence!("The buckets could not be deleted."),
            cmd!("lnk storage list"),
            phrase!("see the clouds that are connected"),
        ),
        Command::StorageKeys(cli::StorageKeysArgs { owner }) => {
            let owner = owner_named(&owner)?;
            let keys = owners::keys(&owner)
                .and_then(|k| Ok(serde_json::to_value(k)?))
                .map_err(|err| said::failed(sentence!("Its keys could not be made."), err))?;
            plumbing(out, &keys);
            Ok(Done::shown())
        }
        Command::StorageRevoke(cli::StorageRevokeArgs { owner }) => {
            let owner = owner_named(&owner)?;
            ended(
                owners::revoke(&owner).map(|()| Done::shown()),
                sentence!("Its keys could not be revoked."),
                cmd!("lnk storage list"),
                phrase!("see the clouds that are connected"),
            )
        }
        Command::StorageLabel(cli::StorageLabelArgs { owner, labels }) => {
            let owner = owner_named(&owner)?;
            let labelled = owners::label(&owner, &labels)
                .and_then(|l| Ok(serde_json::to_value(l)?))
                .map_err(|err| {
                    said::failed(sentence!("Its buckets could not be labelled."), err)
                })?;
            plumbing(out, &labelled);
            Ok(Done::shown())
        }
        Command::StorageRestore(cli::StorageRestoreArgs { owner, folder }) => ended(
            restore(out, &owner, &folder),
            sentence!("The folder could not be restored."),
            cmd!("lnk cloud list"),
            phrase!("see the cloud accounts its buckets are in"),
        ),
        Command::StorageFind(cli::StorageFindArgs { owner }) => {
            let found = match owner {
                Some(owner) => owners::find_named(&owner).map_err(|err| {
                    said::failed(sentence!("That doesn't name an owner."), err).exit(Exit::Usage)
                })?,
                None => owners::find_all(),
            };
            let found = serde_json::to_value(found).within("reading what was found")?;
            plumbing(out, &found);
            Ok(Done::shown())
        }
        // Its own help, not rclone's: `--help` alone never installs rclone.
        Command::StorageRclone(cli::StorageRcloneArgs { args })
            if matches!(args.as_slice(), [] | [_])
                && args.iter().all(|a| a == "-h" || a == "--help") =>
        {
            let mut cli = cli::command();
            cli.build();
            let rclone = cli
                .find_subcommand_mut("rclone")
                .expect("lnk storage rclone");
            out.data(rclone.render_long_help().to_string().as_bytes());
            out.data(
                b"\nlnk storage rclone help is rclone's own help (it installs rclone first).\n",
            );
            Ok(Done::shown())
        }
        Command::StorageRclone(cli::StorageRcloneArgs { args }) => {
            // rclone writes to the terminal itself.
            let rclone = Rclone::new()
                .map_err(|err| said::failed(sentence!("rclone could not be made ready."), err))?;
            let _ = out.hand_over();
            let code = rclone
                .passthrough(&args)
                .map_err(|err| said::failed(sentence!("rclone could not run."), err))?;
            Ok(Done::passed(code))
        }
        Command::StorageRemove(cli::StorageRemoveArgs {
            paths,
            cloud,
            remote,
            everywhere,
            yes,
        }) => ended(
            delete(
                out,
                &paths,
                &Only {
                    clouds: cloud,
                    remotes: remote,
                },
                everywhere,
                yes,
            ),
            sentence!("The files could not be removed."),
            cmd!("lnk storage list --tree"),
            phrase!("see what the clouds hold"),
        ),
    }
}

/// `--owner <kind>:<id>` of a plumbing command, as the program that
/// runs it names the owner: by its ID, never by a name.
fn owner_named(text: &str) -> Result<Owner, Report> {
    files::parse_owner(text).map_err(|err| {
        said::failed(
            sentence!("{} isn't an owner's ID.", Foreign::new(text)),
            err,
        )
        .exit(Exit::Usage)
        .next(
            cmd!("lnk storage find"),
            phrase!("see the owners your buckets are labelled with"),
        )
    })
}

/// `lnk storage adopt --owner <id>`: what to adopt on stdin, put in
/// place.
fn adopt(out: &mut Out, owner: &Owner) -> Ran {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
        .within("reading what to adopt")?;
    let adopting: owners::Adopting = serde_json::from_str(&text).within("reading what to adopt")?;
    link_actions::value::Validate::validate(&adopting, "")
        .map_err(|refused| anyhow::anyhow!(refused.why))
        .within("reading what to adopt")?;
    for line in owners::adopt(&Rclone::new()?, owner, &adopting, None)? {
        out.block(Note(Sentence::adopt(&line)));
    }
    Ok(Done::shown())
}

/// The ID of the owner `named` names (`<kind>:<name>`), as the module
/// that keeps it says, or with none, of the only owner with a folder
/// here.
fn owner_id(named: Option<&str>) -> Result<Owner, Report> {
    if let Some(named) = named {
        return owners::resolve(named)
            .map(|(owner, _)| owner)
            .map_err(|err| {
                said::failed(
                    sentence!("{} names no owner here.", Foreign::new(named)),
                    err,
                )
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk storage list"),
                    phrase!("see the folders each cloud backs up"),
                )
            });
    }
    let owned = files::owned()
        .map_err(|err| said::failed(sentence!("The folders here could not be listed."), err))?;
    match owned.as_slice() {
        [one] => Ok(one.owner.as_ref().expect("an owner's folder").id.clone()),
        [] => Err(usage(
            sentence!("No folder here is an owner's."),
            cmd!("lnk storage connect <type> --owner <kind>:<name> --owner-bucket <bucket>"),
            phrase!("name the owner the bucket is for"),
        )),
        _ => Err(usage(
            sentence!("More than one folder here is an owner's."),
            cmd!("lnk storage connect <type> --owner <kind>:<name> --owner-bucket <bucket>"),
            phrase!("name the one you mean"),
        )),
    }
}

/// The options for connecting `cloud`, to buckets that exist (`bucket`)
/// or new ones.
fn options(
    cloud: cli::StorageCreateArgs,
    bucket: Option<String>,
    owner_bucket: Option<(Owner, String)>,
) -> Result<(Kind, Options), Report> {
    let cli::StorageCreateArgs {
        kind,
        path,
        setting,
        name,
        region,
        profile,
        project,
        endpoint,
        key_id,
        encrypt,
        cloud: account,
        yes: _,
    } = cloud;
    let (kind, backend) = match kind.as_str() {
        "aws" => (Kind::Aws, None),
        "gcp" => (Kind::Gcp, None),
        "s3" => (Kind::S3, None),
        "drive" => (Kind::Drive, None),
        other => (Kind::Rclone, Some(other.to_string())),
    };
    if path.is_some() && kind != Kind::Drive {
        return Err(usage(
            sentence!("The flag --path is for a drive cloud."),
            cmd!("lnk storage create drive --path <folder>"),
            phrase!("connect one folder"),
        ));
    }
    if !setting.is_empty() && kind != Kind::Rclone {
        return Err(usage(
            sentence!(
                "{} takes options, not settings.",
                Product::new(format!("lnk storage create {}", kind.name()))
            ),
            cmd!("lnk storage create {} --help", kind.name()),
            phrase!("see its options"),
        ));
    }
    let settings = setting
        .iter()
        .map(|arg| {
            arg.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| {
                    usage(
                        sentence!("{} isn't a setting.", Foreign::new(arg)),
                        cmd!("lnk storage create <type> --setting <key>=<value>"),
                        phrase!("give each setting as key=value"),
                    )
                })
        })
        .collect::<Result<Vec<_>, Report>>()?;
    let mut options = Options {
        name,
        bucket,
        owner_bucket,
        region,
        profile,
        project,
        endpoint,
        key_id,
        encrypt,
        path,
        backend,
        settings,
        account: None,
    };
    // An aws or gcp cloud is a `lnk cloud` account, and named as it.
    if matches!(kind, Kind::Aws | Kind::Gcp) {
        if options.name.is_some() {
            return Err(usage(
                sentence!("An aws or gcp cloud is named by its account, never --name."),
                cmd!("lnk storage create {} --cloud <account>", kind.name()),
                phrase!("connect it as a cloud account"),
            ));
        }
        options.account = Some(account.unwrap_or_else(|| kind.name().to_string()));
    } else if account.is_some() {
        return Err(usage(
            sentence!("The flag --cloud is for aws and gcp clouds, each a cloud account."),
            cmd!("lnk storage create {} --name <name>", kind.name()),
            phrase!("name it instead"),
        ));
    }
    Ok((kind, options))
}

/// `args`, as paths in the folder each is in, grouped by folder, your
/// files first.
fn by_folder(folders: &[Folder], args: &[String]) -> anyhow::Result<Vec<(Folder, Vec<String>)>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| folders[0].root.clone());
    // The current folder comes with links resolved; a folder's path may
    // go through one (~/Link on another drive). Put it back in the
    // folder's own terms, so relative paths land in the right folder.
    let cwd = folders
        .iter()
        .filter_map(|f| {
            let real = f.root.canonicalize().ok()?;
            let rest = cwd.strip_prefix(&real).ok()?;
            (!cwd.starts_with(&f.root)).then(|| (real.components().count(), f.root.join(rest)))
        })
        .max_by_key(|(depth, _)| *depth)
        .map_or(cwd.clone(), |(_, cwd)| cwd);
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); folders.len()];
    for arg in args {
        let (index, path) = files::locate(folders, &cwd, arg)?;
        groups[index].push(path);
    }
    Ok(folders
        .iter()
        .cloned()
        .zip(groups)
        .filter(|(_, paths)| !paths.is_empty())
        .collect())
}

/// Where `arg` is, as `lnk storage remove` reads it, if it's a file or
/// folder of yours: here, or in a cloud as the manifest says.
fn a_path(folders: &[Folder], arg: &str) -> Option<String> {
    let (folder, paths) = by_folder(folders, &[arg.to_string()]).ok()?.pop()?;
    let path = paths.first()?;
    let full = folder.root.join(path);
    let here = std::fs::symlink_metadata(&full).is_ok();
    let there = || {
        Manifest::open(&folder.id).is_ok_and(|m| m.files.keys().any(|p| files::matches(path, p)))
    };
    (here || there()).then(|| tilde(&full))
}

/// Refuses drive clouds whose backup folder has come to overlap one of
/// Link's folders since `connect`: each "copy" would be the file itself.
fn apart(clouds: &[Cloud], folders: &[Folder]) -> anyhow::Result<()> {
    for cloud in clouds {
        connect::check_apart(cloud, folders)?;
    }
    Ok(())
}

/// The folders a command that touches files works with, warning of each
/// module that didn't say which folders it keeps: they're left out, so a
/// path in one is in none of Link's folders.
fn touched(out: &mut Out, found: files::Found) -> Vec<Folder> {
    for unanswered in &found.unanswered {
        out.block(Report::warning(sentence!(
            "Leaving out the folders the {} module keeps: {}.",
            Foreign::new(&unanswered.module),
            Foreign::new(unanswered.why.trim_end_matches('.'))
        )));
    }
    found.folders
}

/// A note before a folder's results, when it isn't your files.
fn heading(out: &mut Out, folder: &Folder) {
    if folder.id != PERSONAL {
        out.block(Note(sentence!(
            "Now {} ({}).",
            Foreign::new(&folder.label()),
            Where::new(folder.root.clone())
        )));
    }
}

/// `~/...` for paths in the home folder.
fn tilde(path: &Path) -> String {
    match link_files::try_home_dir()
        .ok()
        .and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Asks yes or no; `yes` answers for the user. Without a terminal to ask
/// in, it's a report naming --yes.
fn confirm(out: &mut Out, question: Sentence, yes: bool) -> anyhow::Result<bool> {
    out.ask(Question::confirm(question, false).yes(yes))
        .map_err(said)
}

/// What the library asks of the person while connecting, asked with
/// `out`; `yes` (`--yes`) says yes to a bucket it makes.
fn asking(out: &mut Out, yes: bool) -> impl FnMut(Asked) -> anyhow::Result<String> + '_ {
    move |asked| match asked {
        Asked::MakeBucket(cloud) => {
            let question = sentence!(
                "Make a bucket for your files in {}, billed by that cloud?",
                Name::new("storage", cloud.to_string())
            );
            match confirm(out, question, yes)? {
                true => Ok(String::new()),
                false => Err(declined(sentence!("Made nothing."))),
            }
        }
        Asked::Endpoint => Ok(out
            .ask(Question::text(sentence!(
                "What is the storage's endpoint URL, such as {}?",
                Foreign::new("https://<account>.r2.cloudflarestorage.com")
            )))
            .map_err(said)?),
        Asked::KeyId(whose) => Ok(out
            .ask(Question::text(sentence!(
                "What is {} access key id?",
                Foreign::new(whose)
            )))
            .map_err(said)?),
        Asked::SecretKey(whose) => Ok(out
            .ask(Question::hidden(sentence!(
                "What is {} secret access key?",
                Foreign::new(whose)
            )))
            .map_err(said)?
            .expose_owned()),
        // A new bucket's password is made; a bucket that exists needs the
        // one saved when it was made.
        Asked::Password {
            reconnecting: false,
            ..
        } => Ok(String::new()),
        Asked::Password { owned, .. } => Ok(out
            .ask(Question::hidden(match owned {
                true => sentence!("What is the encryption password of the owner's bucket?"),
                false => sentence!("What is the encryption password you saved when it was made?"),
            }))
            .map_err(said)?
            .expose_owned()),
    }
}
