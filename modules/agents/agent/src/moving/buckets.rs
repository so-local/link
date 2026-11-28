//! The storage module's part of a move and a backup: a cloud to back up
//! to, its JSON, and an agent's buckets put in place where it lands.

use std::io::Write;
use std::process::Stdio;

use link_format::{
    Exit, Foreign, Out, Question, Report, Sentence, WithinExt, cmd, prose, sentence,
};

/// A cloud of the storage module's to back up to, for `doing`: when you
/// have none (`lnk storage list`), one made in a terminal, as you say
/// (`lnk storage create`), else what makes one said.
pub(crate) fn a_cloud(out: &mut Out, doing: &str) -> Result<(), Report> {
    let none = bucket_run(&["list", "--json"])
        .and_then(|clouds| clouds.as_array().map(Vec::is_empty))
        .unwrap_or(false);
    if !none {
        return Ok(());
    }
    let makes = || {
        Report::error(sentence!(
            "{} goes to a bucket in your clouds, and you have none yet.",
            doing
        ))
        .exit(Exit::Usage)
        .next(
            cmd!("lnk storage create <type>"),
            "make one (aws, gcp, s3 or drive --path <folder>)",
        )
    };
    if !out.can_ask() {
        return Err(makes());
    }
    let answer = out.ask(
        Question::text(sentence!(
            "Make one in: aws, gcp, s3 or drive <path> (empty: not now)?"
        ))
        .why(prose!(
            "{} goes to a bucket in your clouds, and you have none yet.",
            doing
        ))
        .default(""),
    )?;
    let words: Vec<&str> = answer.split_whitespace().collect();
    if words.is_empty() {
        return Err(makes());
    }
    out.hand_over();
    let made = link_core::lnk()
        .args(["storage", "create"])
        .args(backend_args(&words))
        .status()
        .within("running lnk storage create")?;
    if !made.success() {
        return Err(makes());
    }
    Ok(())
}

/// What `lnk storage create` takes for the words you answered: a
/// type, then a `drive`'s folder or an rclone type's `key=value`
/// settings.
fn backend_args(words: &[&str]) -> Vec<String> {
    let mut args = vec![words[0].to_string()];
    for rest in &words[1..] {
        match (words[0], rest.contains('=')) {
            ("drive", _) => args.extend(["--path".to_string(), rest.to_string()]),
            (_, true) => args.extend(["--setting".to_string(), rest.to_string()]),
            (_, false) => args.push(rest.to_string()),
        }
    }
    args
}

/// `lnk storage <args>`'s JSON; none without the storage module, when it
/// fails, or when it says null.
pub(crate) fn bucket_run(args: &[&str]) -> Option<serde_json::Value> {
    link_core::find_module("storage")?;
    let output = link_core::lnk()
        .arg("storage")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()
        .filter(|v| !v.is_null())
}

/// After a move to a box that didn't finish, or was taken back: the key
/// made for that box (`lnk storage keys`, which replaces the one before)
/// revoked, and, where agent `id` stays on a box (`stays_on`), a new one
/// made and put in place there, so its backups go on. Here, it backs up
/// with your accounts, and needs none. Said when it can't be.
pub(crate) fn key_back(out: &mut Out, id: &str, stays_on: Option<&str>) {
    let Ok(owner) = link_agent::owners::agent_flag(id) else {
        return;
    };
    if link_core::find_module("storage").is_none()
        || bucket_run(&["export", &owner[0], &owner[1]]).is_none()
    {
        return;
    }
    let Some(box_name) = stays_on else {
        let revoked = link_core::lnk()
            .args(["storage", "revoke"])
            .args(&owner)
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !revoked {
            out.block(Report::warning(sentence!(
                "The key made for the move to reach your agent's buckets may still work: {} \
                 revokes it.",
                Foreign::new(&format!("lnk storage revoke {}", owner.join(" ")))
            )));
        }
        return;
    };
    let given = super::machines::bucket_credentials(id).is_some_and(|credentials| {
        let mut adopting = serde_json::json!({}).into();
        with_credentials(&mut adopting, &credentials);
        let mut command = link_core::lnk();
        command
            .args(["box", "run", box_name, "--", "storage", "adopt"])
            .args(&owner)
            .stdout(Stdio::null());
        adopt(command, &adopting)
    });
    if !given {
        out.block(
            Report::warning(sentence!(
                "Your agent's backups on {} are off: the key the move made replaced its own, and \
                 no new one could be put in place there.",
                Foreign::new(box_name)
            ))
            .next(
                cmd!("lnk agent move --on {} --agent <agent>", box_name),
                "move it there again",
            ),
        );
    }
}

/// Agent `id`'s buckets put in place here (`lnk storage adopt`), from
/// what the stream and the header carry; said when they can't be.
pub(crate) fn adopt_buckets(
    out: &mut Out,
    id: &str,
    bucket: Option<&serde_json::Value>,
    credentials: Option<&link_secretive::Secret<link_secretive::Json>>,
) {
    if bucket.is_none() && credentials.is_none() {
        return;
    }
    let off = |out: &mut Out, why: &str| {
        out.block(Report::warning(sentence!("Its backups are off here.")).why(Sentence::adopt(why)))
    };
    if let Err(err) = link_core::ensure("storage", "Keeping its backups") {
        return off(out, &format!("{err:#}"));
    }
    let mut adopting: link_secretive::Secret<link_secretive::Json> = bucket
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
        .into();
    // Credentials come only in the header, never in what the stream says
    // of its buckets; `lnk storage adopt` takes a credential only as `keys`
    // make it.
    if let Some(object) = adopting.expose_mut().as_object_mut() {
        object.remove("credentials");
    }
    if let Some(credentials) = credentials {
        with_credentials(&mut adopting, credentials);
    }
    let owner = match link_agent::owners::agent_flag(id) {
        Ok(owner) => owner,
        Err(err) => return off(out, &format!("{err:#}")),
    };
    let mut command = link_core::lnk();
    command.args(["storage", "adopt"]).args(owner);
    if !adopt(command, &adopting) {
        off(out, "lnk storage adopt failed (above)");
    }
}

/// `credentials`, as `lnk storage keys` made them, put in `adopting`.
fn with_credentials(
    adopting: &mut link_secretive::Secret<link_secretive::Json>,
    credentials: &link_secretive::Secret<link_secretive::Json>,
) {
    adopting.expose_mut()["credentials"] = credentials.duplicate().into_exposed().0;
}

/// Whether `command`, a `lnk storage adopt`, took `adopting` on its
/// standard input and succeeded.
fn adopt(
    mut command: std::process::Command,
    adopting: &link_secretive::Secret<link_secretive::Json>,
) -> bool {
    command
        .stdin(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut input) = child.stdin.take() {
                let text = link_secretive::Secret::new(serde_json::to_vec(adopting.expose())?);
                input.write_all(text.expose())?;
            }
            child.wait()
        })
        .is_ok_and(|s| s.success())
}
