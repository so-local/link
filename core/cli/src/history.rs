//! The core's history: the modules renamed and removed, which no longer
//! exist to answer for themselves, and its own retired spellings, in the schema
//! every module's `history.toml` keeps ([`link_spec::history`]).

use std::sync::LazyLock;

pub use link_spec::history::*;

pub static HISTORY: LazyLock<History> = LazyLock::new(|| {
    History::parse(include_str!("../history.toml")).expect("the built-in command history is valid")
});

/// The release's history, as `--link-history` answers it: the core's,
/// with each module's retired spellings, as the registry holds them.
pub fn release() -> Wire {
    let registry = crate::registry();
    HISTORY.release_wire(
        registry
            .iter()
            .map(|(module, resolved)| (module.as_str(), &resolved.history)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_history_holds_to_the_commands_and_modules_today() {
        HISTORY
            .validate(
                &crate::surface(),
                &crate::surface(),
                &crate::modules(),
                &link_version::version(),
            )
            .unwrap();
        assert_eq!(
            HISTORY.check_retirement(&link_version::version()),
            Vec::<String>::new()
        );
    }
}
