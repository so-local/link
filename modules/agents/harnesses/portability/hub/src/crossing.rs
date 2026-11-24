//! Crossing harnesses: a command of Link's namespaces, or a name another
//! harness gives one (`/chat:create`, `/new`, `/clear`), typed to a harness
//! that knows the action by another of its names, goes to it by that
//! one, so nothing an owner used is lost in the move.

use std::collections::HashSet;

use agent_client_protocol::schema::v1::{ContentBlock, PromptRequest, TextContent};
use link_component::actions::{Actions, Surface, command_name};

/// Each action an agent's commands offer: its name, then its aliases,
/// without their `/`, as the namespaces' modules' `actions.toml` say.
pub(crate) struct Names(Vec<Vec<String>>);

impl Names {
    /// Of Link's namespaces: the agent module's (`chat`, `skill`) and the
    /// model module's.
    pub fn link() -> Self {
        Self::of([
            link_agent_contract::actions(),
            link_model_contract::actions(),
        ])
    }

    pub fn of(actions: impl IntoIterator<Item = Actions>) -> Self {
        let names = actions
            .into_iter()
            .flatten()
            .filter(|(_, action)| action.on(Surface::Command))
            .map(|(path, action)| {
                std::iter::once(command_name(&path))
                    .chain(
                        action
                            .aliases
                            .iter()
                            .map(|a| a.trim_start_matches('/').to_string()),
                    )
                    .collect()
            })
            .collect();
        Self(names)
    }

    /// Whether `text` names a registered command whose spelling may need
    /// the session's advertised commands.
    pub fn recognizes(&self, text: &str) -> bool {
        text.trim()
            .strip_prefix('/')
            .and_then(|text| text.split_whitespace().next())
            .is_some_and(|name| self.0.iter().any(|names| names.iter().any(|n| n == name)))
    }

    /// `text` as the harness, which offers the commands `offered`, knows
    /// it: a command it offers, or one that isn't an action's, as it is;
    /// else as the first of the action's names it offers. None when it's
    /// already right, or it offers none of them.
    pub fn crossed(&self, text: &str, offered: &HashSet<String>) -> Option<String> {
        let typed = text.trim().strip_prefix('/')?;
        let (name, rest) = typed
            .split_once(char::is_whitespace)
            .map_or((typed, ""), |(name, rest)| (name, rest.trim()));
        if offered.contains(name) {
            return None;
        }
        let names = self
            .0
            .iter()
            .find(|names| names.iter().any(|n| n == name))?;
        let known = names.iter().find(|n| offered.contains(*n))?;
        Some(format!("/{known} {rest}").trim_end().to_string())
    }

    /// `prompt` crossed: a prompt of one text, a command, sent by the
    /// name the harness offers.
    pub fn cross(&self, prompt: &mut PromptRequest, offered: &HashSet<String>) {
        if let [ContentBlock::Text(said)] = prompt.prompt.as_mut_slice()
            && let Some(crossed) = self.crossed(&said.text, offered)
        {
            *said = TextContent::new(crossed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_command_goes_by_the_name_its_harness_knows() {
        let names = Names::link();
        // Claude Code's: a new conversation is its /clear.
        let claude = offered(&["clear", "model", "compact"]);
        assert_eq!(
            names.crossed("/chat:create", &claude).as_deref(),
            Some("/clear")
        );
        assert_eq!(names.crossed("/new", &claude).as_deref(), Some("/clear"));
        assert_eq!(
            names.crossed("/model:set  opus ", &claude).as_deref(),
            Some("/model opus")
        );
        // Codex's has /new: as it is, or to it.
        let codex = offered(&["new", "status"]);
        assert_eq!(names.crossed("/new", &codex), None);
        assert_eq!(
            names.crossed("/chat:create", &codex).as_deref(),
            Some("/new")
        );
        // Link Harness's names: as they are.
        assert_eq!(
            names.crossed("/chat:create", &offered(&["chat:create"])),
            None
        );
    }

    #[test]
    fn what_no_harness_name_fits_goes_as_it_is() {
        let names = Names::link();
        let claude = offered(&["clear"]);
        // Not an action's, offered nowhere, or not a command at all.
        assert_eq!(names.crossed("/deploy now", &claude), None);
        assert_eq!(names.crossed("/skill:write", &claude), None);
        assert_eq!(names.crossed("new", &claude), None);
    }
}
