//! A module's tool actions, checked against its `[tools]`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, ensure};

use super::placeholders;
use crate::{Module, Surface, names::is_name, schema::Node};

/// What a fill gives, by its use: text in a description, or an
/// argument's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Text,
    Words,
}

/// `module`'s tool actions and `[tools]`: each tool table on an action
/// offered on `tool`, named once; each summary and fill on a tool
/// action's own argument; and every fill and condition declared once and
/// used, each fill one way.
pub(crate) fn check(module: &Module) -> anyhow::Result<()> {
    let tools = &module.tools;
    for (what, names) in [("fills", &tools.fills), ("conditions", &tools.conditions)] {
        let distinct: BTreeSet<_> = names.iter().collect();
        ensure!(distinct.len() == names.len(), "[tools] {what} repeats one");
    }
    let mut uses: BTreeMap<&str, Use> = BTreeMap::new();
    let mut conditions = BTreeSet::new();
    let mut names = BTreeSet::new();
    for action in module.actions.values() {
        let id = &action.id;
        for (arg, declared) in &action.args {
            let mut inner = 0;
            declared.schema.walk(&mut |schema| {
                if schema.meta.summary.is_some() || schema.meta.fill.is_some() {
                    inner += 1;
                }
            });
            let own = &declared.schema.meta;
            let at_top = usize::from(own.summary.is_some() || own.fill.is_some());
            ensure!(
                inner == at_top,
                "action {id}: argument {arg} holds x-link-summary or x-link-fill inside it: they mark a tool action's own argument"
            );
            ensure!(
                at_top == 0 || action.tool.is_some(),
                "action {id}: argument {arg}: x-link-summary and x-link-fill are a tool action's, and it has no tool table"
            );
        }
        let Some(tool) = &action.tool else {
            continue;
        };
        ensure!(
            action.offered_on(Surface::Tool),
            "action {id}: a tool table is an action's offered on tool: its surfaces name tool"
        );
        ensure!(
            is_name(&tool.name),
            "action {id}: tool name {:?} is a lowercase name",
            tool.name
        );
        ensure!(
            names.insert(tool.name.as_str()),
            "action {id}: tool name {} is another of the module's tool actions'",
            tool.name
        );
        if let Some(when) = &tool.when {
            ensure!(
                tools.conditions.contains(when),
                "action {id}: tool when {when:?} is one of [tools] conditions"
            );
            conditions.insert(when.as_str());
        }
        let mut places = Vec::new();
        let mut texts = vec![action.description.as_str()];
        for (arg, declared) in &action.args {
            let meta = &declared.schema.meta;
            if let Some(place) = meta.summary {
                ensure!(
                    !meta.sensitive,
                    "action {id}: argument {arg} is sensitive, so its call's title never shows it: no x-link-summary"
                );
                places.push(place);
            }
            if let Some(fill) = &meta.fill {
                ensure!(
                    matches!(declared.schema.node, Node::String(_)),
                    "action {id}: argument {arg}: x-link-fill gives a text's choices, so it's a string with no enum of its own"
                );
                ensure!(
                    tools.fills.contains(fill),
                    "action {id}: argument {arg}: x-link-fill {fill:?} is one of [tools] fills"
                );
                used(&mut uses, fill, Use::Words)?;
            }
            texts.extend(meta.description.as_deref());
        }
        places.sort_unstable();
        ensure!(
            places.iter().copied().eq(1..=u32::try_from(places.len())?),
            "action {id}: x-link-summary places are 1, 2 and on, each once"
        );
        for text in texts {
            for name in placeholders(text).map_err(|why| anyhow::anyhow!("action {id}: {why}"))? {
                ensure!(
                    tools.fills.iter().any(|fill| fill == name),
                    "action {id}: {{{name}}} is one of [tools] fills"
                );
                used(&mut uses, name, Use::Text)?;
            }
        }
    }
    if let Some(unused) = tools.fills.iter().find(|f| !uses.contains_key(f.as_str())) {
        bail!("[tools] fill {unused} is declared and nothing names it");
    }
    if let Some(unused) = tools
        .conditions
        .iter()
        .find(|c| !conditions.contains(c.as_str()))
    {
        bail!("[tools] condition {unused} is declared and no tool action is offered on it");
    }
    Ok(())
}

/// Records `fill` used as `how`: one fill is text or choices, never both.
fn used<'a>(uses: &mut BTreeMap<&'a str, Use>, fill: &'a str, how: Use) -> anyhow::Result<()> {
    match uses.insert(fill, how) {
        Some(before) if before != how => {
            bail!(
                "[tools] fill {fill} is a description's text and an argument's choices: one fill is one"
            )
        }
        _ => Ok(()),
    }
}
