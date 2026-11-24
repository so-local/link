//! `lnk storage connect`, `create`, `disconnect`, `delete`, `show`
//! and `list`: the clouds files go to, connected and listed. They read
//! and change settings only, so they work from `clouds.toml`, never
//! asking a module which folders it keeps: each bucket there is keyed by
//! its folder's key, `personal` or an owner's ID, which names the owner
//! ([`files::label_of`]). Only `connect --owner-bucket` without
//! `--owner` asks, to find the one owner here.

use super::*;

/// Shows an encryption password Link just made, once: while the command
/// runs, for the person to save.
pub(super) fn show_password(out: &mut Out, cloud: &str, label: &str, password: &str) {
    out.now(
        Section::new(phrase!(
            "Encryption password for {} in {}",
            Foreign::new(label),
            Foreign::new(cloud)
        ))
        .with(Verbatim::new().line(password)),
    );
    out.block(Note(sentence!(
        "Save it now, in a password manager: without it those files can't be read, by anyone. It's also kept in {}.",
        Where::new("~/.lnk/safe/storage")
    )));
}

/// `lnk storage create` and `connect`: `yes` (`--yes`) says yes to a
/// bucket `create` makes, which its cloud bills.
pub(super) fn connect_cloud(out: &mut Out, kind: Kind, options: Options, yes: bool) -> Ran {
    let personal = files::personal()?;
    let _lock = files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let rclone = Rclone::new()?;
    if kind == Kind::Rclone {
        // rclone asks its own questions on the terminal.
        let _ = out.hand_over();
    }
    let connected = connect::connect(
        &rclone,
        std::slice::from_ref(&personal),
        &clouds.clouds,
        kind,
        options,
        &mut asking(out, yes),
    )?;
    let account = connected.account;
    clouds.clouds.push(account.clone());
    clouds.save()?;
    for (name, password) in &connected.new_passwords {
        show_password(
            out,
            &account.name,
            &files::label_of(name),
            password.expose(),
        );
    }
    let mut buckets = Fields::new();
    let mut held = Vec::new();
    for key in account.folders() {
        let Some(cloud) = account.cloud(key) else {
            continue;
        };
        buckets = buckets.row(
            Foreign::new(&files::label_of(key)).as_str(),
            Foreign::new(&cloud.describe()),
        );
        let mut manifest = Manifest::open(key)?;
        let count = ops::refresh(&rclone, &mut manifest, &cloud)?;
        manifest.save()?;
        if count > 0 {
            held.push(count);
        }
    }
    if !buckets.is_empty() {
        out.block(buckets);
    }
    for count in held {
        out.block(Note(sentence!(
            "It already holds {}.",
            Count::new(count as u64, "file", "files")
        )));
    }
    std::fs::create_dir_all(&personal.root).within("making your files' folder")?;
    Ok(Done::success(sentence!(
        "Connected {}, with {}.",
        Name::new("storage", account.name.clone()),
        Foreign::new(&connected.login)
    ))
    .next(
        cmd!("lnk storage push <folder>"),
        phrase!("send what you put in {}", Where::new(personal.root.clone())),
    )
    .next(
        cmd!("lnk storage list --tree"),
        phrase!("see what a cloud holds"),
    )
    .next(cmd!("lnk storage pull"), phrase!("get what it holds")))
}

pub(super) fn disconnect(out: &mut Out, name: &str, yes: bool) -> Ran {
    let personal = files::personal()?;
    let _lock = files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let account = clouds
        .get(name)
        .cloned()
        .ok_or_else(|| said(not_connected(name)))?;
    let (mut manifests, only) = only_copies(&account, &personal)?;
    if !only.is_empty() {
        out.block(Note(sentence!(
            "{} have their only copy in {}, for example {}.",
            Count::new(only.len() as u64, "file", "files"),
            Name::new("storage", name),
            Foreign::new(&only[0])
        )));
        out.block(Note(sentence!(
            "They stay there, but lnk storage won't know about them until you connect it again."
        )));
        if !confirm(out, sentence!("Disconnect anyway?"), yes)? {
            return Err(declined(sentence!("Nothing was disconnected.")));
        }
    }
    forget(&mut clouds, &account, &mut manifests)?;
    let kind = account
        .backend
        .clone()
        .unwrap_or_else(|| account.kind.name().to_string());
    let mut again = format!("lnk storage connect {kind}");
    if let Some(path) = &account.path {
        again += &format!(" --path {path}");
    }
    if account.name != kind {
        again += &format!(" --name {}", account.name);
    }
    if let Some(bucket) = account.buckets.get(PERSONAL) {
        again += &format!(" --bucket {}", bucket.name);
    }
    // An owner is named by its ID's id, which `--owner` takes in place
    // of its name.
    let owned = account
        .folders()
        .into_iter()
        .find_map(|key| Some((files::owner_of(key)?, account.buckets.get(key)?)));
    if let Some((owner, bucket)) = owned {
        again += &format!(
            " --owner {}:{} --owner-bucket {}",
            owner.id.kind(),
            owner.id.id(),
            bucket.name
        );
    }
    Ok(Done::success(sentence!(
        "Disconnected {}, and nothing in it was deleted.",
        Name::new("storage", name)
    ))
    .next(
        Next::typed(again.split_whitespace().map(str::to_string)),
        phrase!("connect it again"),
    ))
}

/// Each manifest of a folder `account` has a bucket for, held to forget
/// it, and the files whose only good copy is in it. Only your files'
/// folder is known here (`personal`): an owner's file no other cloud has
/// counts, here or not, since where its folder is goes unasked.
fn only_copies(account: &Account, personal: &Folder) -> anyhow::Result<(Vec<Locked>, Vec<String>)> {
    let mut manifests = Vec::new();
    let mut only = Vec::new();
    for key in account.folders() {
        let manifest = Manifest::open(key)?;
        let root = (key == PERSONAL).then_some(personal.root.as_path());
        only.extend(ops::only_in(root, &manifest, &account.name));
        manifests.push(manifest);
    }
    Ok((manifests, only))
}

/// No cloud by `name` is connected.
fn not_connected(name: &str) -> Report {
    usage(
        sentence!("No cloud {} is connected.", Foreign::new(name)),
        cmd!("lnk storage list"),
        phrase!("see the clouds that are connected"),
    )
}

/// `lnk storage delete <cloud>`: the cloud's buckets deleted, everything
/// in them, and the cloud forgotten. A path is refused, never read as a
/// cloud.
pub(super) fn remove_cloud(out: &mut Out, name: &str, everywhere: bool, yes: bool) -> Ran {
    let files_are = |report: Report| {
        said(report.next(
            cmd!("lnk storage remove {}", name),
            phrase!("remove files from your clouds"),
        ))
    };
    if check_name(name).is_err() {
        return Err(files_are(
            usage(
                sentence!("{} is no cloud's name.", Foreign::new(name)),
                cmd!("lnk storage list"),
                phrase!("list the clouds"),
            )
            .why(prose!("The command delete deletes a cloud's buckets.")),
        ));
    }
    let personal = files::personal()?;
    let _lock = files::config_lock()?;
    let mut clouds = Clouds::load()?;
    let account = clouds
        .get(name)
        .cloned()
        .ok_or_else(|| files_are(not_connected(name)))?;
    // A file of yours named like the cloud: the cloud's buckets go, said
    // first, and only when you're asked.
    if let Some(path) = a_path(std::slice::from_ref(&personal), name) {
        if yes {
            return Err(files_are(
                usage(
                    sentence!(
                        "{} is a cloud, and {} a file of yours.",
                        Name::new("storage", name),
                        Foreign::new(&path)
                    ),
                    cmd!("lnk storage delete {}", name),
                    phrase!("be asked before the cloud's buckets are deleted"),
                )
                .why(prose!(
                    "Removing the cloud deletes its buckets, so leave out --yes to be asked."
                )),
            ));
        }
        out.block(
            Report::warning(sentence!(
                "This deletes the cloud {}'s buckets, not {}.",
                Foreign::new(name),
                Foreign::new(&path)
            ))
            .next(
                cmd!("lnk storage remove {}", name),
                phrase!("remove the file from your clouds instead"),
            ),
        );
    }
    let (mut manifests, only) = only_copies(&account, &personal)?;
    if !only.is_empty() && !everywhere {
        return Err(said(
            Report::error(sentence!(
                "{} have their only copy in {}, for example {}.",
                Count::new(only.len() as u64, "file", "files"),
                Name::new("storage", name),
                Foreign::new(&only[0])
            ))
            .exit(Exit::Usage)
            .next(cmd!("lnk storage pull"), phrase!("pull them first"))
            .next(
                cmd!("lnk storage delete {} --everywhere", name),
                phrase!("delete them anyway"),
            ),
        ));
    }
    let buckets: Vec<Cloud> = account
        .buckets
        .keys()
        .filter_map(|folder| account.cloud(folder))
        .collect();
    let mut listed = Table::new(["Bucket"], sentence!("It has no buckets."));
    for cloud in &buckets {
        listed.row([Foreign::new(&cloud.describe())]);
    }
    out.now(listed);
    let question = match buckets.len() {
        1 => sentence!("Delete this bucket and everything in it? It can't be undone."),
        _ => sentence!("Delete these buckets and everything in them? It can't be undone."),
    };
    if !confirm(out, question, yes)? {
        return Err(declined(sentence!("Nothing was deleted.")));
    }
    let rclone = Rclone::new()?;
    // Each bucket's key goes before it does: none outlives it.
    connect::delete_buckets(&account, &mut connect::revoke_key, &mut |cloud| {
        rclone.delete_bucket(cloud)?;
        out.block(Note(sentence!("Deleted {}.", Foreign::new(&cloud.bucket))));
        Ok(())
    })?;
    // The buckets are gone: their IDs are never given again.
    link_storage::ids::retire(account.buckets.values().map(|b| b.id.as_str()))?;
    forget(&mut clouds, &account, &mut manifests)?;
    Ok(Done::success(sentence!(
        "Deleted {}'s buckets and forgot it.",
        Name::new("storage", name)
    )))
}

/// Removes `account` from Link: its keys and encryption, its entry, and
/// what the manifests say it holds. Nothing in the cloud changes.
fn forget(clouds: &mut Clouds, account: &Account, manifests: &mut [Locked]) -> anyhow::Result<()> {
    let config = rclone::config_in(&files::config_dir()?);
    rclone::remove_section(&config, &account.name)?;
    for folder in account.buckets.keys() {
        if let Some(cloud) = account.cloud(folder) {
            rclone::remove_section(&config, &cloud.crypt_section())?;
        }
    }
    clouds.clouds.retain(|c| c.name != account.name);
    clouds.save()?;
    for manifest in manifests {
        manifest.forget_cloud(&account.name);
        manifest.save()?;
    }
    Ok(())
}

/// `lnk storage show <name>`: one cloud, its type and where it is, then
/// a section for each folder it holds, its bucket and its encryption,
/// never a credential: the state `lnk issue create` puts in an issue.
pub(super) fn show_cloud(out: &mut Out, name: &str) -> Ran {
    let clouds = Clouds::load()?;
    let Some(account) = clouds.clouds.iter().find(|a| a.name == name) else {
        return Err(said(not_connected(name)));
    };
    out.block(about(account));
    let config = rclone::config_in(&files::config_dir()?);
    for key in account.folders() {
        let Some(cloud) = account.cloud(key) else {
            continue;
        };
        let stats = Stats::of(&Manifest::open(key)?.manifest, &cloud.name);
        let mut fields = Fields::new().row("Bucket", bucket(&cloud));
        if let Some(bucket) = account.buckets.get(key) {
            fields = fields.row("ID", Foreign::new(&bucket.resource()));
        }
        fields = fields.row("Encryption", encryption(&cloud, true));
        if cloud.encrypted {
            let kept = rclone::kept(&config, &cloud.crypt_section(), "password");
            fields = fields.row(
                "Password",
                match kept {
                    Some(rclone::Kept::File(path)) => Value::from(Where::new(path)),
                    Some(kept @ rclone::Kept::Keychain(_)) => {
                        Value::from(Foreign::new(&kept.describe()))
                    }
                    None => Value::from("missing here"),
                },
            );
        }
        fields = fields
            .row("In rclone", Foreign::new(&cloud.remote()))
            .row("Files", Count::new(stats.files, "file", "files"))
            .row("Size", link_format::Size(stats.bytes));
        if let Some(checked) = stats.checked {
            fields = fields.row("Last checked", link_format::Time(checked));
        }
        out.block(Section::new(phrase!("{}", holds(key))).with(fields));
    }
    if let Some(unmade) = unmade(account) {
        out.block(
            Section::new(phrase!("{}", holds(&unmade)))
                .with(Fields::new().row("Bucket", "none yet, made on the first push")),
        );
    }
    Ok(Done::shown().next(cmd!("lnk storage list"), phrase!("see every cloud")))
}

/// What a cloud is, as `show` begins it: its name, type and where it
/// is.
fn about(account: &Account) -> Fields {
    let mut fields = Fields::new()
        .key("Name", Name::new("storage", account.name.clone()))
        .row("Type", Foreign::new(kind_of(account)));
    if let Some(path) = &account.path {
        fields = fields.row("Folder", Where::new(path));
    }
    if let Some(region) = &account.region {
        fields = fields.row("Region", Foreign::new(region));
    }
    if let Some(cloud) = &account.account {
        fields = fields.row("Cloud account", Name::new("account", cloud.clone()));
    }
    if let Some(owner) = &account.only_for {
        fields = fields.row("Only for", Foreign::new(&files::label_of(owner)));
    }
    fields
}

/// Its type as `create` named it: `aws`, `folder`, or rclone's word for
/// another.
fn kind_of(account: &Account) -> &str {
    account.backend.as_deref().unwrap_or(account.kind.name())
}

/// A bucket by its name in its cloud, or in a drive cloud, the folder
/// its files are in.
fn bucket(cloud: &Cloud) -> Value {
    match (cloud.kind, &cloud.path) {
        (Kind::Drive, Some(path)) => Value::from(Where::new(Path::new(path).join(&cloud.bucket))),
        _ => Value::from(Foreign::new(&cloud.bucket)),
    }
}

/// A cloud as a heading: its name, type and where it is.
fn heading_of(account: &Account) -> Phrase {
    let name = Name::new("storage", account.name.clone());
    let kind = Foreign::new(kind_of(account));
    match (&account.path, &account.region) {
        (Some(path), _) => phrase!("{} ({} at {})", name, kind, Where::new(path)),
        (None, Some(region)) => phrase!("{} ({} in {})", name, kind, Foreign::new(region)),
        (None, None) => phrase!("{} ({})", name, kind),
    }
}

/// Whether a bucket is encrypted, and how: `show` says that names are
/// too (`full`).
fn encryption(cloud: &Cloud, full: bool) -> Value {
    match (cloud.encrypted, full) {
        (true, true) => Value::from("rclone crypt, file and folder names too"),
        (true, false) => Value::from("rclone crypt"),
        (false, _) => Value::from("off"),
    }
}

/// The folder `key` a bucket holds: `Your files`, or an owner's.
fn holds(key: &str) -> Value {
    holds_in(key, &Default::default())
}

/// [`holds`], an owner named as `names` says, else by its ID.
fn holds_in(key: &str, names: &std::collections::BTreeMap<String, String>) -> Value {
    match (key == PERSONAL, files::owner_in(key, names)) {
        (true, _) => Value::from("Your files"),
        (false, Some(owner)) => Value::from(Foreign::new(&format!("{}'s files", owner.name))),
        (false, None) => Value::from(Foreign::new(&files::label_of(key))),
    }
}

/// The folder a cloud is for that has no bucket in it yet: your files,
/// or in a cloud only for one owner, that owner's.
fn unmade(account: &Account) -> Option<String> {
    let unmade = account.only_for.as_deref().unwrap_or(PERSONAL).to_string();
    (account.serves(&unmade) && account.cloud(&unmade).is_none()).then_some(unmade)
}

/// What a folder's manifest says one cloud holds of it.
struct Stats {
    files: u64,
    bytes: u64,
    /// When a copy there last matched the file here.
    checked: Option<std::time::SystemTime>,
}

impl Stats {
    fn of(manifest: &Manifest, cloud: &str) -> Self {
        let copies = manifest.files.values().filter_map(|e| e.clouds.get(cloud));
        let mut stats = Stats {
            files: 0,
            bytes: 0,
            checked: None,
        };
        let mut latest = None;
        for copy in copies {
            stats.files += 1;
            stats.bytes += copy.size;
            latest = latest.max(copy.checked);
        }
        stats.checked =
            latest.map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs));
        stats
    }
}

/// The kind of owner that's a group (a hosted team's): written under
/// `group`; every other kind (`agent`, `job`) is what runs, under
/// `workload`.
const GROUP_KIND: &str = "group";

/// A folder's owner as `lnk storage list --json` writes it: its ID under
/// the field naming its axis, and its name.
fn owner_json(owner: &crate::files::FolderOwner) -> serde_json::Value {
    let axis = match owner.id.kind().as_str() == GROUP_KIND {
        true => "group",
        false => "workload",
    };
    serde_json::json!({axis: owner.id, "name": owner.name})
}

/// `lnk storage list`: every cloud in `clouds.toml` and each bucket
/// it has, an owner's included whether or not its module is here.
pub(super) fn list_clouds(out: &mut Out) -> Ran {
    let clouds = Clouds::load()?;
    let mut manifests = std::collections::BTreeMap::new();
    for key in clouds.clouds.iter().flat_map(Account::folders) {
        if !manifests.contains_key(key) {
            manifests.insert(key, Manifest::open(key)?);
        }
    }
    // Each owner by its name, as its module says, else by its ID: asked
    // only when a bucket is an owner's, since asking starts its module.
    let owned = clouds
        .clouds
        .iter()
        .flat_map(Account::folders)
        .any(|key| files::owner_of(key).is_some());
    let names = match owned {
        true => files::names(),
        false => Default::default(),
    };
    let stats = |cloud: &Cloud| Stats::of(&manifests[cloud.folder.as_str()].manifest, &cloud.name);
    let list: Vec<_> = clouds
        .clouds
        .iter()
        .map(|account| {
            let buckets: Vec<serde_json::Value> = account
                .folders()
                .into_iter()
                .filter_map(|key| {
                    let cloud = account.cloud(key)?;
                    let Stats { files, bytes, .. } = stats(&cloud);
                    let resource = account.buckets.get(key)?.resource();
                    Some(serde_json::json!({
                        "resource": resource, "bucket": cloud.bucket,
                        "encrypted": cloud.encrypted, "remote": cloud.remote(),
                        "files": files, "bytes": bytes,
                        "owner": files::owner_in(key, &names).as_ref().map(owner_json),
                    }))
                })
                .collect();
            serde_json::json!({
                "name": account.name, "kind": account.kind, "backend": account.backend,
                "region": account.region, "buckets": buckets,
            })
        })
        .collect();
    out.reply(&exact(serde_json::Value::Array(list)));
    // A section for each cloud, its buckets in a table.
    for account in &clouds.clouds {
        let mut table = Table::new(
            ["Holds", "Bucket", "Encryption", "Files", "Size"],
            sentence!("It has no buckets yet."),
        )
        .right(3)
        .right(4);
        for key in account.folders() {
            let Some(cloud) = account.cloud(key) else {
                continue;
            };
            let stats = stats(&cloud);
            table.row([
                holds_in(key, &names),
                Value::from(Foreign::new(&cloud.bucket)),
                encryption(&cloud, false),
                Value::from(Count::new(stats.files, "file", "files")),
                Value::from(link_format::Size(stats.bytes)),
            ]);
        }
        if let Some(unmade) = unmade(account) {
            table.row([
                holds(&unmade),
                Value::from("none yet, made on the first push"),
                Value::from(""),
                Value::from(""),
                Value::from(""),
            ]);
        }
        out.block(Section::new(heading_of(account)).with(table));
    }
    let empty = clouds.clouds.is_empty();
    if empty {
        out.block(Text(prose!("No clouds are connected yet.")));
    }
    Ok(match empty {
        true => Done::shown().next(
            cmd!("lnk storage create <type>"),
            phrase!("connect aws, gcp, s3 or a folder"),
        ),
        false => Done::shown().next(
            cmd!("lnk storage list --tree"),
            phrase!("see what they hold"),
        ),
    })
}

/// `value` with every field that has no value left out, as its declared
/// shape says: a field it may leave out is absent, never null.
fn exact(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => fields
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k, exact(v)))
            .collect(),
        serde_json::Value::Array(items) => items.into_iter().map(exact).collect(),
        other => other,
    }
}
