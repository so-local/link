//! A move's hold on an agent, on the machine it leaves:
//! `~/.lnk/safe/agents/transfers/moving/<id>.claim`, beside the journals
//! of moves this machine runs, holding the ID of the one move whose
//! switch has the agent. `lnk agent pause --move <id>` takes it, and
//! `pause`, `state-send`, `left` and `resume` of any other move are
//! refused while it's held, so two computers moving one agent at once
//! never both switch it: the second is refused at its pause, and its
//! take-back never starts the agent the first is switching. `resume` and
//! `left` of its own move let go of it, and so does any start of the
//! agent here, which ends a pause.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::bail;
use link_format::WithinExt;

/// Where agent `id`'s hold is kept.
fn path(id: &str) -> PathBuf {
    link_files::layout::Hidden::Moving
        .path()
        .join(format!("{id}.claim"))
}

/// The move holding the agent at hold `path`, if one does. A hold this
/// machine can't read is refused, never taken for none.
fn held(path: &Path) -> anyhow::Result<Option<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).within(format!("reading {}", path.display())),
    };
    let move_id = text.trim().to_string();
    link_agent_contract::moving::check_move_id(&move_id)
        .within(format!("reading {}", path.display()))?;
    Ok(Some(move_id))
}

/// An error unless agent `name` (`id`) is held here by no move, or by
/// `move_id`'s.
pub(crate) fn check(id: &str, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    check_at(&path(id), name, move_id)
}

fn check_at(path: &Path, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    match held(path)? {
        Some(holder) if Some(holder.as_str()) != move_id => bail!(
            "{name} is held here by another move ({holder}), from another lnk agent move: \
             that move finishes or takes it back, or a start of {name} here ends it"
        ),
        _ => Ok(()),
    }
}

/// Agent `name` (`id`) held here by move `move_id` (`lnk agent pause
/// --move`): taken once, by one move alone, even when two ask at once.
/// With no move, only checked: held by none.
pub(crate) fn take(id: &str, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    if move_id.is_some() {
        link_files::create_private_dir(&link_files::layout::Hidden::Moving.path())?;
    }
    take_at(&path(id), name, move_id)
}

fn take_at(path: &Path, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    let Some(move_id) = move_id else {
        return check_at(path, name, None);
    };
    link_agent_contract::moving::check_move_id(move_id)?;
    let made = std::os::unix::fs::OpenOptionsExt::mode(
        std::fs::OpenOptions::new().write(true).create_new(true),
        0o600,
    )
    .open(path);
    match made {
        Ok(mut file) => file
            .write_all(move_id.as_bytes())
            .within(format!("writing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            check_at(path, name, Some(move_id))
        }
        Err(e) => Err(e).within(format!("writing {}", path.display())),
    }
}

/// Move `move_id`'s hold on agent `name` (`id`) let go of (`resume`,
/// `left`): an error, changing nothing, while another move holds it.
pub(crate) fn release(id: &str, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    release_at(&path(id), name, move_id)
}

fn release_at(path: &Path, name: &str, move_id: Option<&str>) -> anyhow::Result<()> {
    check_at(path, name, move_id)?;
    let _ = std::fs::remove_file(path);
    Ok(())
}

/// Whatever holds agent `id` here let go of: a start ends a pause.
pub(crate) fn end(id: &str) {
    let _ = std::fs::remove_file(path(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "0123456789abcdef0123456789abcdef";
    const TWO: &str = "fedcba9876543210fedcba9876543210";

    /// One move holds an agent: another's pause, send, left and resume
    /// are refused while it does, its own pass, and its resume lets go.
    #[test]
    fn one_move_holds_an_agent_at_a_time() {
        let fixture = link_test_kit::TempDir::new("claim-one");
        let path = fixture.canonicalize().unwrap().join("a1.claim");
        take_at(&path, "main", None).unwrap();
        take_at(&path, "main", Some(ONE)).unwrap();
        take_at(&path, "main", Some(ONE)).unwrap();
        let refused = take_at(&path, "main", Some(TWO)).unwrap_err();
        assert!(format!("{refused:#}").contains(ONE), "{refused:#}");
        assert!(take_at(&path, "main", None).is_err());
        assert!(check_at(&path, "main", Some(TWO)).is_err());
        assert!(release_at(&path, "main", Some(TWO)).is_err());
        assert!(path.exists());
        check_at(&path, "main", Some(ONE)).unwrap();
        release_at(&path, "main", Some(ONE)).unwrap();
        take_at(&path, "main", Some(TWO)).unwrap();
        // One it can't read is refused, never taken for none.
        std::fs::write(&path, "not a move").unwrap();
        assert!(check_at(&path, "main", None).is_err());
    }
}
