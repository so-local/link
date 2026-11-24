//! A spec as the field flags that write it (`--internet.hosts '*'`):
//! how Link says a grant, which `lnk sandbox run`, `lnk agent permission add` and
//! `deny` take back as typed. The flags are the spec's own, each its key
//! path, which `link_spec` generates from the spec's shape.

use super::spec;

/// `spec` as the field flags that write it, quoted as a shell would take
/// them (`--internet.hosts '*'`).
pub fn shown(spec: &spec::Spec) -> String {
    link_spec::flags_of(spec)
        .unwrap_or_default()
        .iter()
        .map(|word| {
            match word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:@=,".contains(c))
            {
                true => word.clone(),
                false => format!("'{}'", word.replace('\'', "'\\''")),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
