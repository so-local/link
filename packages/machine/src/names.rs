//! Identity names and immutable IDs.
use anyhow::bail;
pub const NAME_MAX: usize = 30;
/// An error unless `id` is one [`link_id::candidate`] could have drawn.
pub fn check_id(id: &str) -> anyhow::Result<()> {
    match link_id::is_candidate(id) {
        true => Ok(()),
        false => bail!(
            "{:?} isn't an ID ({} lowercase letters and digits)",
            link_format::clean(id),
            link_id::limits().candidate_length
        ),
    }
}

/// Link's own words where a command takes a name, which no agent or
/// machine can be named: `here` (this machine) and `all` (every one).
pub const WORDS: &[&str] = &["here", "all"];

/// An error unless `name` can name an agent or a machine: lowercase
/// letters, digits and dashes, starting with a letter, at most
/// [`NAME_MAX`]; not one of Link's [`WORDS`].
pub fn check_name(name: &str) -> anyhow::Result<()> {
    check_shape(name)?;
    if WORDS.contains(&name) {
        bail!(
            "{name:?} is one of Link's words where a name goes ({}): pick another name",
            WORDS.join(", ")
        );
    }
    Ok(())
}

/// [`check_name`] but for Link's [`WORDS`]: the shape of a name that
/// is a cloud account's, a module's or a place's, where the words are
/// not refused.
pub fn check_shape(name: &str) -> anyhow::Result<()> {
    let plain = name.len() <= NAME_MAX
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !plain {
        bail!(
            "{:?} isn't a name: lowercase letters, digits and dashes, starting with a letter, \
             at most {NAME_MAX}",
            link_format::clean(name)
        );
    }
    Ok(())
}

/// A host name made into a name ([`check_name`]): lowercase, its first
/// label (`Danas-MBP.local` is `danas-mbp`), other characters as
/// dashes; `computer` when nothing is left.
pub fn name_from_host(host: &str) -> String {
    let first = host.trim().split('.').next().unwrap_or_default();
    let mut name = String::new();
    for c in first.chars().map(|c| c.to_ascii_lowercase()) {
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() {
            c
        } else {
            '-'
        };
        if !(c == '-' && (name.is_empty() || name.ends_with('-'))) {
            name.push(c);
        }
    }
    let name = name.trim_start_matches(|c: char| !c.is_ascii_lowercase());
    let mut name: String = name.chars().take(NAME_MAX).collect();
    while name.ends_with('-') {
        name.pop();
    }
    match check_name(&name) {
        Ok(()) => name,
        Err(_) => "computer".into(),
    }
}
