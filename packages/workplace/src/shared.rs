//! The shared flag types, re-exported beside [`crate::Slots`] from
//! `link-spec`, which parses every command line, and held here to
//! `shared.toml`.

pub use link_spec::shared::*;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Args, Command};

    /// Each type's flags are exactly its `shared.toml` group's, typed as
    /// it declares them.
    #[test]
    fn each_type_is_its_shared_groups_flags() {
        let types: [(&str, Command); 4] = [
            ("output", Output::augment_args(Command::new("x"))),
            ("quiet", Quiet::augment_args(Command::new("x"))),
            ("confirm", Confirm::augment_args(Command::new("x"))),
            ("request", Request::augment_args(Command::new("x"))),
        ];
        for (group, command) in types {
            let declared = &link_component::shared::types()[group];
            let longs: Vec<&str> = command
                .get_arguments()
                .filter_map(|a| a.get_long())
                .collect();
            let wanted: Vec<&str> = declared.flags.keys().map(String::as_str).collect();
            assert_eq!(longs, wanted, "{group}");
            for arg in command.get_arguments() {
                let flag = &declared.flags[arg.get_long().unwrap()];
                let env = arg.get_env().map(|e| e.to_string_lossy().into_owned());
                assert_eq!(env, flag.environment, "{group}");
                let takes = arg.get_action().takes_values();
                assert_eq!(
                    takes,
                    flag.r#type != link_component::ValueType::Boolean,
                    "{group}"
                );
            }
            let id = command.get_groups().next().unwrap().get_id().to_string();
            assert_eq!(id, format!("{GROUP_PREFIX}{group}"));
        }
    }
}
