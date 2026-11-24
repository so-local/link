//! What tests hold a command's Nexts to: its declarations.

/// Each action's declared `next` and `fail`, keyed by its command after
/// `lnk` (`agent start`), each a template a Next fits: an `lnk` command's
/// words and a placeholder for what follows them. An action naming itself
/// is also `retry`, and one its group runs by default is also named by its
/// group (`lnk agent chat`).
pub(crate) fn navigation() -> toml::Value {
    let declared: toml::Table = toml::from_str(include_str!("../actions.toml")).unwrap();
    let actions = declared["actions"].as_table().unwrap();
    let templates = |own: &str, id: &toml::Value| {
        let text = id.as_str().unwrap();
        let id: link_actions::names::ActionId = text.parse().unwrap();
        let command = id.command();
        let mut named = vec![command.clone()];
        if actions.get(text).and_then(|a| a.get("default")) == Some(&toml::Value::Boolean(true)) {
            named.push(command.rsplit_once(' ').unwrap().0.to_string());
        }
        let mut named: Vec<_> = named
            .into_iter()
            .map(|command| toml::Value::String(format!("{command} <arg>")))
            .collect();
        if text == own {
            named.push(toml::Value::String(link_format::nexts::RETRY.into()));
        }
        named
    };
    let mut by_path = toml::Table::new();
    for (own, action) in actions {
        let id: link_actions::names::ActionId = own.parse().unwrap();
        let mut table = toml::Table::new();
        for part in ["next", "fail"] {
            let ids = action.get(part).and_then(|v| v.as_array()).cloned();
            let named = ids
                .unwrap_or_default()
                .iter()
                .flat_map(|id| templates(own, id))
                .collect();
            table.insert(part.into(), toml::Value::Array(named));
        }
        by_path.insert(id.typed().join(" "), toml::Value::Table(table));
    }
    toml::Value::Table(by_path)
}
