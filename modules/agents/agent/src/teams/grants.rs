//! What a member is granted of a team's shared folder: what `--write`
//! and `--read` name, and nothing for membership alone.
use super::*;

/// What a member is granted of the shared folder: `--write` and
/// `--read`, or none when it names neither. Membership alone grants no
/// file access. A folder in both is refused, since writing a folder
/// includes reading it.
pub(super) fn grant_of(write: &[String], read: &[String]) -> Result<Option<TeamFolders>, Report> {
    if write.is_empty() && read.is_empty() {
        return Ok(None);
    }
    if let Some(both) = write.iter().find(|name| read.contains(name)) {
        return Err(Report::error(sentence!(
            "{} and {} both name {}: writing a folder includes reading it.",
            Foreign::new("--write"),
            Foreign::new("--read"),
            Foreign::new(both)
        ))
        .exit(Exit::Usage));
    }
    let grant = TeamFolders {
        write: write.to_vec(),
        read: read.to_vec(),
    };
    grant.check("--write and --read")?;
    Ok(Some(grant))
}

/// None of the shared folder: what membership alone grants.
pub(super) fn no_grant() -> TeamFolders {
    TeamFolders {
        write: Vec::new(),
        read: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::grant_of;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// Membership alone grants none of the folder: no flag is no grant,
    /// and the flags given are the grant.
    #[test]
    fn a_member_gets_only_what_its_flags_name() {
        assert!(grant_of(&[], &[]).unwrap().is_none());
        let grant = grant_of(&names(&["drafts"]), &names(&["sources"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            (grant.write, grant.read),
            (names(&["drafts"]), names(&["sources"]))
        );
        let whole = grant_of(&[], &names(&["."])).unwrap().unwrap();
        assert!(whole.whole() && !whole.writes_whole());
    }

    /// One folder written and read is refused, saying writing includes
    /// reading.
    #[test]
    fn a_folder_written_and_read_is_refused() {
        let refused = grant_of(&names(&["notes"]), &names(&["notes"])).unwrap_err();
        let said = refused.headline.to_string();
        assert!(
            said.contains("writing a folder includes reading it"),
            "{said}"
        );
        assert!(said.contains("notes"), "{said}");
    }
}
