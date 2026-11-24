//! Resolve a single group's short invocation using its declared operands.
use std::ffi::OsString;

/// `args` as `lnk` passes them: a program of one group (`lnk-example`)
/// run on its own (`lnk-example push`, `lnk-example --help`) has its group
/// put in, so it works as its usage, `lnk example <COMMAND>`, says. Its
/// hidden words beside its group at its root, such as `answer`
/// ([`crate::answers`]) or an adapter's contract words, are no group, and
/// a line starting with one is left as it is.
pub(super) fn grouped(
    command: &clap::Command,
    mut args: Vec<OsString>,
    own: Option<&str>,
) -> Vec<OsString> {
    // A root operand has its own parser. A valid one belongs to the root,
    // even when this program also offers a single command group.
    if let Some(first) = args.get(1) {
        let positional: Vec<_> = command
            .get_arguments()
            .filter(|arg| arg.is_positional())
            .cloned()
            .collect();
        if !positional.is_empty()
            && clap::Command::new("operand")
                .disable_help_flag(true)
                .args(positional)
                .try_get_matches_from([OsString::from("operand"), first.clone()])
                .is_ok()
        {
            return args;
        }
    }
    let groups: Vec<&str> = command
        .get_subcommands()
        // A backend's own group can be plumbing too. Its declared group
        // still provides the same direct invocation as a visible one.
        .filter(|c| {
            !c.is_hide_set() || own.and_then(|name| name.strip_prefix("lnk-")) == Some(c.get_name())
        })
        .map(|c| c.get_name())
        .collect();
    let words: Vec<&str> = command.get_subcommands().map(|c| c.get_name()).collect();
    if let [group] = groups.as_slice()
        && args.get(1).is_none_or(|first| {
            !words
                .iter()
                .chain(["-V", "--version"].iter())
                .any(|w| first == *w)
        })
    {
        args.insert(args.len().min(1), group.into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_operands_use_their_own_validator_before_the_implicit_group() {
        let command = clap::Command::new("lnk")
            .arg(
                clap::Arg::new("page").value_parser(clap::builder::PossibleValuesParser::new([
                    "https://example.com",
                ])),
            )
            .subcommand(clap::Command::new("group").subcommand(clap::Command::new("run")));
        let words = |given: &[&str]| given.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            grouped(&command, words(&["lnk", "https://example.com"]), None),
            words(&["lnk", "https://example.com"])
        );
        assert_eq!(
            grouped(&command, words(&["lnk", "run"]), None),
            words(&["lnk", "group", "run"])
        );
        assert_eq!(
            grouped(&command, words(&["lnk", "group", "run"]), None),
            words(&["lnk", "group", "run"])
        );
        assert!(
            command
                .clone()
                .try_get_matches_from(grouped(&command, words(&["lnk", "unknown"]), None))
                .is_err()
        );
    }
    #[test]
    fn a_hidden_owners_group_keeps_its_direct_invocation() {
        let command = clap::Command::new("lnk")
            .subcommand(
                clap::Command::new("widget")
                    .hide(true)
                    .subcommand(clap::Command::new("backends")),
            )
            .subcommand(clap::Command::new("supervise").hide(true));
        let words = |given: &[&str]| given.iter().map(OsString::from).collect::<Vec<_>>();
        let arranged = grouped(
            &command,
            words(&["lnk-widget", "backends"]),
            Some("lnk-widget"),
        );
        let parsed = command.clone().try_get_matches_from(arranged).unwrap();
        assert_eq!(parsed.subcommand().unwrap().0, "widget");
        assert_eq!(
            grouped(
                &command,
                words(&["lnk-widget", "supervise"]),
                Some("lnk-widget")
            ),
            words(&["lnk-widget", "supervise"])
        );
    }
}
