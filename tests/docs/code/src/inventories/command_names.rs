//! Chat command names and aliases come from the same action declarations.

use std::path::Path;

use link_component::actions::{Actions, Surface, command_name};
use link_docs_check::Tree;

use super::{Items, entry};

pub const COMMANDS: &str =
    "modules/agents/harnesses/link/docs/agents/harnesses/link/commands/namespace-commands.md";
pub const ALIASES: &str =
    "modules/agents/harnesses/link/docs/agents/harnesses/link/commands/aliases.md";

fn collect(
    commands: &mut Items,
    aliases: &mut Items,
    actions: &Actions,
    source: &Path,
) -> Result<(), String> {
    for (path, action) in actions {
        if action.hidden() || !action.on(Surface::Command) {
            continue;
        }
        let target = format!("/{}", command_name(path));
        if commands.contains_key(&target) {
            return Err(format!("duplicate namespace command {target}"));
        }
        entry(commands, target.clone(), source);
        for alias in &action.aliases {
            if !alias.starts_with('/') || alias.len() == 1 || alias.chars().any(char::is_whitespace)
            {
                return Err(format!("invalid command alias {alias:?}"));
            }
            let prefix = format!("{alias} → ");
            if aliases.keys().any(|name| name.starts_with(&prefix)) {
                return Err(format!("duplicate command alias {alias}"));
            }
            entry(aliases, format!("{prefix}{target}"), source);
        }
    }
    Ok(())
}

fn validate(commands: &Items, aliases: &Items) -> Result<(), String> {
    for mapping in aliases.keys() {
        let (alias, target) = mapping.split_once(" → ").ok_or("invalid alias mapping")?;
        if alias == "/help" || commands.contains_key(alias) {
            return Err(format!("command alias {alias} shadows a primary command"));
        }
        if !commands.contains_key(target) {
            return Err(format!("command alias {alias} has no target {target}"));
        }
    }
    Ok(())
}

pub fn catalog(tree: &Tree) -> Result<(Items, Items), String> {
    let mut commands = Items::new();
    let mut aliases = Items::new();
    // These are the registries consumed by Link Harness's command discovery.
    for owner in ["agent", "model"] {
        let source = tree
            .root
            .join(format!("modules/agents/{owner}/actions.toml"));
        let text = tree
            .text(&source)
            .ok_or_else(|| format!("{}: missing registry", source.display()))?;
        let actions = link_component::actions::parse(text).map_err(|error| error.to_string())?;
        collect(&mut commands, &mut aliases, &actions, &source)?;
    }
    validate(&commands, &aliases)?;
    Ok((commands, aliases))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventories::{render, replace};

    fn fixture() -> Actions {
        link_component::actions::parse(
            r#"schema = 1

[actions."agent.chat.create"]
description = "Start a conversation"
role = "secondary"
surfaces = []
route = { bridge = "harness", callers = ["owner"], path = "chat create", command = true, names = ["/clear", "/new"] }

[actions."agent.agent.list"]
description = "CLI only"
role = "secondary"
"#,
        )
        .unwrap()
    }

    #[test]
    fn plumbing_actions_and_aliases_stay_out_of_command_inventories() {
        let mut actions = fixture();
        actions.get_mut("chat create").unwrap().role = link_component::actions::Role::Plumbing;
        let mut commands = Items::new();
        let mut aliases = Items::new();
        collect(
            &mut commands,
            &mut aliases,
            &actions,
            Path::new("actions.toml"),
        )
        .unwrap();
        assert!(commands.is_empty());
        assert!(aliases.is_empty());
    }

    #[test]
    fn declarations_drive_both_references_and_alias_retargeting() {
        let source = Path::new("/repo/actions.toml");
        let mut commands = Items::new();
        let mut aliases = Items::new();
        let mut actions = fixture();
        collect(&mut commands, &mut aliases, &actions, source).unwrap();
        validate(&commands, &aliases).unwrap();
        assert_eq!(
            commands.keys().map(String::as_str).collect::<Vec<_>>(),
            ["/chat:create"]
        );
        assert_eq!(aliases.len(), 2);
        let page = Path::new("/repo/docs/aliases.md");
        let primary = render("namespace-commands", page, &commands);
        let first = render("command-aliases", page, &aliases);
        assert!(first.contains("[`/clear` → `/chat:create`](../actions.toml)"));
        assert!(!first.contains("/old") && !first.contains("/ls"));
        let action = actions.remove("chat create").unwrap();
        actions.insert("chat resume".into(), action);
        commands.clear();
        aliases.clear();
        collect(&mut commands, &mut aliases, &actions, source).unwrap();
        let changed = render("command-aliases", page, &aliases);
        assert_ne!(render("namespace-commands", page, &commands), primary);
        assert!(changed.contains("`/clear` → `/chat:resume`"));
        assert_ne!(replace(&first, "command-aliases", &changed).unwrap(), first);
        let forged = changed.replace("- [`/clear`", "- [`/bogus` → `/missing`]\n- [`/clear`");
        assert_ne!(
            replace(&forged, "command-aliases", &changed).unwrap(),
            forged
        );
        aliases.clear();
        assert_ne!(render("command-aliases", page, &aliases), changed);
    }

    #[test]
    fn ambiguous_aliases_and_missing_targets_are_refused() {
        let source = Path::new("/repo/actions.toml");
        let mut actions = fixture();
        let mut commands = Items::new();
        let mut aliases = Items::new();
        actions
            .get_mut("chat create")
            .unwrap()
            .aliases
            .push("/clear".into());
        assert!(collect(&mut commands, &mut aliases, &actions, source).is_err());
        // A duplicate canonical name across registries is also ambiguous.
        assert!(collect(&mut commands, &mut Items::new(), &fixture(), source).is_err());
        aliases.clear();
        entry(&mut aliases, "/chat:create → /chat:create".into(), source);
        assert!(validate(&commands, &aliases).is_err());
        aliases.clear();
        entry(&mut aliases, "/clear → /missing".into(), source);
        assert!(validate(&commands, &aliases).is_err());
        aliases.clear();
        entry(&mut aliases, "/help → /chat:create".into(), source);
        assert!(validate(&commands, &aliases).is_err());
    }
}
