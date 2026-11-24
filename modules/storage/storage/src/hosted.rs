//! The second engine: an owned folder a cloud account keeps, behind
//! that cloud's own key. `lnk storage push` and `pull` for it are the
//! module's that answers [`HOME`] (the one that keeps its owner), through
//! that account: never rclone, and never a key here.

use anyhow::bail;
use link_format::WithinExt;

use crate::files::Folder;

pub use link_storage_contract::Moved;
use link_storage_contract::{HOME, Home, Owned, Way};

/// Pushes `paths` of hosted `folder` (relative to its folder here, `""`
/// for all of it) to the account that keeps it: only what's new or
/// changed goes.
pub fn push(folder: &Folder, paths: &[String]) -> anyhow::Result<Moved> {
    home(Way::Push, folder, paths)
}

/// Pulls hosted `folder` from the account that keeps it into its folder
/// here, what `patterns` match (none: all of it), replacing what differs.
pub fn pull(folder: &Folder, patterns: &[String]) -> anyhow::Result<Moved> {
    home(Way::Pull, folder, patterns)
}

/// [`HOME`] asked for `folder` and `paths`, of the one module that
/// answers it: what moved. What it says on stderr is shown as it comes.
fn home(way: Way, folder: &Folder, paths: &[String]) -> anyhow::Result<Moved> {
    let account = folder.hosted.as_deref().within("not a hosted folder")?;
    let owner = folder.owner.as_ref().within("not an owner's folder")?;
    let asked = Home {
        way,
        owned: Owned {
            owner: owner.id.clone(),
            name: owner.name.clone(),
            root: folder.root.clone(),
            running: None,
            account: Some(account.to_string()),
        },
        paths: paths.to_vec(),
    };
    let Some(answerer) = HOME.answerers().into_iter().next() else {
        bail!(
            "no module here reaches {}'s folder on {account}",
            owner.name
        );
    };
    answerer.reply(&HOME, &asked, std::process::Stdio::inherit())
}
