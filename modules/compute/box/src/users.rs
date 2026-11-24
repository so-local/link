//! `lnk box user add|list|remove`, run on a box as root: a Linux user of
//! the box for each person, made with their own home, their key let in
//! only to their sandbox (`link_sandbox_contract::box_users`, which says where root
//! keeps them and how a session enters).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, ensure};
use link_box::cli::{UserAddArgs as UserAdd, UserRemoveArgs as UserRemove};
use link_box::said::{Asks, said, usage};
use link_format::{
    Done, Name, Out, Path as Shown, Question, Refuser, Report, Table, Text, Value, Verbatim,
    WithinExt, cmd, phrase, prose, sentence,
};
use link_sandbox_contract::box_users::{self, BoxUser, GROUP, Places};

/// One of `lnk box user`'s commands.
pub enum UserCommand {
    Add(UserAdd),
    List,
    Remove(UserRemove),
}

/// `lnk box user ...`.
pub fn user(out: &mut Out, command: UserCommand) -> anyhow::Result<Done> {
    let places = Places::here();
    // A box user is a login on a machine of its own: a box here is a
    // folder of this one.
    if let Some(name) = link_box::native::running_in() {
        return Err(usage(
            sentence!(
                "Couldn't run lnk box user in {}, which is a box here.",
                Name::new("box", &name)
            ),
            prose!(
                "A box's users are a machine's own logins, and a box here is a folder of this machine."
            ),
            cmd!("lnk box create <cloud>"),
            "make a box with users of its own in a cloud account",
        ));
    }
    if !link_machine::machine::kind()?.is_box() {
        return Err(usage(
            sentence!("Couldn't run lnk box user on this computer."),
            prose!("A box's users are a box's, and this machine is a computer."),
            cmd!("lnk box ssh <box>"),
            "open a shell on a box, then run sudo lnk box user there",
        ));
    }
    match command {
        UserCommand::List => list(out, &places),
        UserCommand::Add(add) => {
            as_root(&places)?;
            self::add(out, &places, add)
        }
        UserCommand::Remove(remove) => {
            as_root(&places)?;
            self::remove(out, &places, remove)
        }
    }
}

/// An error unless this runs as root, who alone makes users: or in a
/// test's stand-in box.
fn as_root(places: &Places) -> anyhow::Result<()> {
    // SAFETY: geteuid has no failure and touches no memory.
    let root = unsafe { libc::geteuid() } == 0;
    if !root && places.root == Path::new("/") {
        return Err(usage(
            sentence!("Couldn't change a box's users without root."),
            prose!("A box's users are root's to make."),
            cmd!("sudo lnk box user"),
            "run it as root",
        ));
    }
    Ok(())
}

fn add(out: &mut Out, places: &Places, add: UserAdd) -> anyhow::Result<Done> {
    let UserAdd {
        name,
        public_key,
        sandbox,
        yes,
    } = add;
    box_users::check_name(&name)?;
    let key = box_users::public_key(&public_key)?;
    let known = box_users::load(places, &name)?;
    if known.is_none() && user_exists(&name) {
        return Err(usage(
            sentence!(
                "Couldn't add {}, who is a user of this box already.",
                Name::new("user", &name)
            ),
            prose!("They are not a user lnk box user add made."),
            cmd!("lnk box user list"),
            "list the users it made",
        ));
    }
    let shown = Name::new("user", &name);
    let folder = match &known {
        Some(user) => user.folder.clone(),
        None => places.home(&name),
    };
    install_programs(places)?;
    // Their spec, read as it would be in their session (`~` their home),
    // kept absolute, checked and shown, all before anything is made.
    let staged = places.users().join(format!("adding-{name}"));
    let _ = std::fs::remove_dir_all(&staged);
    root_dir(&staged)?;
    let result = (|| {
        let absolute = as_them(places, &folder, &["spec", "show", "--spec"], &sandbox, &[])
            .within("reading their sandbox's spec")?;
        let spec = staged.join(format!("{name}.toml"));
        root_file(&spec, absolute.as_bytes())?;
        let folder_flag = folder.display().to_string();
        let opens = as_them(
            places,
            &folder,
            &["enter", "--check", "--spec"],
            &spec,
            &["--folder", &folder_flag],
        )?;
        let mut opens_lines = Verbatim::new().lead(phrase!(
            "With {}, {}'s sandbox, besides their folder ({}), they open",
            Shown::new(&sandbox),
            &shown,
            Shown::new(&folder)
        ));
        for line in opens.lines() {
            opens_lines = opens_lines.line(link_format::clean(line));
        }
        out.now(opens_lines);
        if !out.asks(Question::confirm(sentence!("Add them?"), true).yes(yes))? {
            return Err(said(Report::refusal(
                Refuser::Person,
                sentence!("Didn't add {}.", &shown),
            )));
        }
        Ok(spec)
    })();
    let spec = match result {
        Ok(spec) => spec,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(err);
        }
    };
    if known.is_none() {
        host(link_secretive::process::command("groupadd").args(["-f", GROUP]))?;
        host(
            link_secretive::process::command("useradd")
                .args(["--create-home", "--home-dir"])
                .arg(&folder)
                .args(["--shell", "/bin/sh", "--user-group", "--groups", GROUP])
                .arg(&name),
        )?;
    }
    let user = BoxUser {
        folder: folder.clone(),
        key,
    };
    let dir = places.user(&name);
    root_dir(&dir)?;
    std::fs::rename(&spec, places.spec(&name)).within("keeping their spec")?;
    let _ = std::fs::remove_dir_all(&staged);
    root_file(
        &dir.join("user.toml"),
        link_files::saved::print(&user)?.as_bytes(),
    )?;
    let line = box_users::line(places, &name, &user)?;
    root_file(&places.keys(&name), format!("{line}\n").as_bytes())?;
    sshd_reads_them(out, places)?;
    out.block(Text(prose!(
        "They write only {}; scp and sftp work there.",
        Shown::new(&folder)
    )));
    Ok(
        Done::success(sentence!("{} can SSH in, into their sandbox.", &shown))
            .next(cmd!("lnk box user list"), "list the users"),
    )
}

fn list(out: &mut Out, places: &Places) -> anyhow::Result<Done> {
    let mut users = Vec::new();
    for name in box_users::names(places)? {
        if let Some(user) = box_users::load(places, &name)? {
            users.push((name, user));
        }
    }
    let listed: Vec<serde_json::Value> = users
        .iter()
        .map(|(name, user)| {
            serde_json::json!({
                "name": name,
                "folder": user.folder,
                "key": user.key,
                "sandbox": places.spec(name),
            })
        })
        .collect();
    out.reply(&link_actions::answer::output::<link_box::cli::Users>(
        &listed,
    )?);
    let mut table = Table::new(
        ["Name", "Folder", "Key", "Sandbox"],
        sentence!("No users on this box."),
    );
    for (name, user) in &users {
        let tail = &user.key[user.key.len().saturating_sub(8)..];
        table.row([
            Name::new("user", name).into(),
            Shown::new(&user.folder).into(),
            Value::from(format!("...{tail}")),
            Shown::new(places.spec(name)).into(),
        ]);
    }
    out.block(table);
    Ok(Done::shown().next(
        cmd!("sudo lnk box user add <name> --public-key <key> --sandbox <spec>"),
        "add one",
    ))
}

fn remove(out: &mut Out, places: &Places, remove: UserRemove) -> anyhow::Result<Done> {
    let UserRemove {
        name,
        delete_folder,
    } = remove;
    let Some(user) = box_users::load(places, &name)? else {
        return Err(usage(
            sentence!(
                "Couldn't find a user named {} on this box.",
                Name::new("user", &name)
            ),
            prose!("No user was added by that name."),
            cmd!("lnk box user list"),
            "list the users",
        ));
    };
    // Their key first: nothing lets them in while the rest goes.
    std::fs::remove_file(places.keys(&name)).within("removing their key")?;
    let mut userdel = link_secretive::process::command("userdel");
    if delete_folder {
        userdel.arg("--remove");
    }
    host(userdel.arg(&name))?;
    if !delete_folder {
        keep_for_root(&user.folder)?;
    }
    std::fs::remove_dir_all(places.user(&name)).within("removing what root kept of them")?;
    let _ = out;
    Ok(match delete_folder {
        true => Done::success(sentence!(
            "{} can't SSH in anymore, and their folder is gone.",
            Name::new("user", &name)
        )),
        false => Done::success(sentence!(
            "{} can't SSH in anymore; their folder stays at {}, root's alone.",
            Name::new("user", &name),
            Shown::new(&user.folder)
        )),
    }
    .next(cmd!("lnk box user list"), "list the users"))
}

/// Whether the machine has a user named `name`.
fn user_exists(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: a NUL-ended name; the entry it points to isn't kept.
    !unsafe { libc::getpwnam(name.as_ptr()) }.is_null()
}

/// What root's copy of `lnk-sandbox` prints for `lnk sandbox <args>
/// <file> <more>`, run as a session of theirs would be: `HOME` their
/// folder, nothing else of this user's.
fn as_them(
    places: &Places,
    folder: &Path,
    args: &[&str],
    file: &Path,
    more: &[&str],
) -> anyhow::Result<String> {
    let output = link_secretive::process::command(places.sandbox())
        .args(args)
        .arg(file)
        .args(more)
        .env_clear()
        .env("HOME", folder)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .within("running lnk-sandbox")?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}",
            link_format::clean(why.trim().trim_start_matches("Error: "))
        );
    }
    Ok(text)
}

/// Root's copies of `lnk` and `lnk-sandbox` (those this `lnk` runs), in
/// a folder every user reaches and none writes: the box's own user's home
/// is closed to the rest. Replaced when they differ, so each `add`
/// brings them up to this `lnk`'s.
fn install_programs(places: &Places) -> anyhow::Result<()> {
    let lnk = link_core::lnk().get_program().to_owned();
    let lnk = Path::new(&lnk)
        .canonicalize()
        .within("can't find lnk, which a user's key runs")?;
    let sandbox =
        link_core::find_module("sandbox").within("the sandbox module isn't installed here")?;
    let dir = places.programs();
    root_dir(&dir)?;
    for (from, name) in [(lnk, "lnk"), (sandbox, "lnk-sandbox")] {
        let to = dir.join(name);
        let new = std::fs::read(&from).within(format!("reading {}", from.display()))?;
        if std::fs::read(&to).is_ok_and(|old| old == new) {
            continue;
        }
        root_write(&to, &new, 0o755)?;
    }
    Ok(())
}

/// sshd's drop-in for box users, written if it isn't, and sshd told.
fn sshd_reads_them(out: &mut Out, places: &Places) -> anyhow::Result<()> {
    let path = places.sshd();
    let text = places.sshd_text();
    if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        root_dir(dir)?;
    }
    root_file(&path, text.as_bytes())?;
    let reloaded = ["ssh", "sshd"].into_iter().any(|unit| {
        link_secretive::process::command("systemctl")
            .args(["reload", unit])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    if !reloaded {
        out.block(
            Report::warning(sentence!(
                "Couldn't reload sshd after writing {}.",
                Shown::new(&path)
            ))
            .why(prose!(
                "Until it reads the file again, a user's key opens nothing."
            )),
        );
    }
    Ok(())
}

/// Runs one of the machine's own commands (`useradd`, ...), its output
/// said only when it fails.
/// A removed user's kept folder, made root's and closed to everyone else:
/// the next user `useradd` makes may be given the removed one's user ID,
/// and would own their files as they were.
fn keep_for_root(folder: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    host(
        link_secretive::process::command("chown")
            .args(["-R", "--no-dereference", "0:0"])
            .arg(folder),
    )?;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
        .within("closing their folder")
}

fn host(command: &mut Command) -> anyhow::Result<()> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .within(format!("running {:?}", command.get_program()))?;
    ensure!(
        output.status.success(),
        "{:?} failed: {}",
        command.get_program(),
        link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    Ok(())
}

/// A folder of root's, readable by all, written by root alone.
fn root_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path).within(format!("making {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// A file of root's, readable by all.
fn root_file(path: &Path, text: &[u8]) -> anyhow::Result<()> {
    root_write(path, text, 0o644)
}

/// `path` written whole with `mode`, in one rename: never half there.
fn root_write(path: &Path, text: &[u8], mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let new = path.with_extension(format!("new-{}", std::process::id()));
    let _ = std::fs::remove_file(&new);
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&new)?;
        file.write_all(text)?;
        file.sync_all()?;
        std::fs::rename(&new, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    written.within(format!("writing {}", path.display()))
}
