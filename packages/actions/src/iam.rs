//! What IAM reads of the catalog: what an action acts on, the resource
//! kinds a module declares and the roles it comes with. Version 1 reserves
//! `resource` and `[roles]` for IAM and checks their shape; nothing else
//! acts on them yet.

use std::collections::BTreeSet;

use anyhow::{Context, bail, ensure};

use crate::{ActionId, Catalog, Module, schema::Node};

/// What an action acts on: one resource of a kind, named by the argument
/// its `resource` says, or the project it runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On<'a> {
    Resource(&'a str),
    Project,
}

impl Catalog {
    /// What a declared action acts on: the kind of the ID its `resource`
    /// argument holds, or its project. None for an undeclared action.
    pub fn on(&self, id: &ActionId) -> Option<On<'_>> {
        let action = self.action(id)?;
        let Some(field) = &action.resource else {
            return Some(On::Project);
        };
        let schema = self.resolve(&action.args.get(field)?.schema).ok()?;
        match &schema.node {
            Node::String(text) => text.kind.as_deref().map(On::Resource),
            _ => None,
        }
    }

    /// A built-in role's actions: a template, given on a project by a grant.
    pub fn role(&self, module: &str, name: &str) -> Option<BTreeSet<ActionId>> {
        roles(self.module(module)?)
            .ok()?
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, a)| a)
    }
}

impl Module {
    /// Its resource kinds: each shape with `x-link-resource`.
    pub fn resource_kinds(&self) -> impl Iterator<Item = &str> {
        self.types
            .iter()
            .filter(|(_, d)| d.resource.is_some())
            .map(|(n, _)| n.as_str())
    }

    /// The names of the roles it comes with.
    pub fn role_names(&self) -> impl Iterator<Item = &str> {
        self.roles.keys().map(String::as_str)
    }
}

/// `[roles.<name>.<namespace>.<resource>]`, each verb naming `["*"]`.
pub(crate) fn roles(module: &Module) -> anyhow::Result<Vec<(String, BTreeSet<ActionId>)>> {
    let mut out = Vec::new();
    for (name, role) in &module.roles {
        ensure!(
            crate::names::is_name(name),
            "role {name:?} is a lowercase name"
        );
        let mut actions = BTreeSet::new();
        let role = role
            .as_table()
            .with_context(|| format!("role {name} is a table"))?;
        for (namespace, resources) in role {
            for (resource, verbs) in resources
                .as_table()
                .with_context(|| format!("role {name}: {namespace} is a table"))?
            {
                for (verb, on) in verbs
                    .as_table()
                    .with_context(|| format!("role {name}: {namespace}.{resource} is a table"))?
                {
                    let id = ActionId::new(namespace, resource, verb)?;
                    if on
                        .as_array()
                        .map(|a| a.len() == 1 && a[0].as_str() == Some("*"))
                        != Some(true)
                    {
                        bail!("role {name}: a built-in role covers {id} on [\"*\"] alone");
                    }
                    actions.insert(id);
                }
            }
        }
        out.push((name.clone(), actions));
    }
    Ok(out)
}

/// A built-in role names only actions its module declares, and never a
/// control action, since a role may be given to a run.
pub(crate) fn check(module: &Module) -> anyhow::Result<()> {
    for (name, actions) in roles(module)? {
        for id in actions {
            let action = module.actions.get(&id).with_context(|| {
                format!(
                    "role {name} names {id}, which module {} doesn't declare",
                    module.identity.id
                )
            })?;
            ensure!(
                !action.control,
                "role {name} names {id}, a control action, which no role a module ships holds"
            );
        }
    }
    for action in module.actions.values() {
        if let Some(field) = &action.resource {
            ensure!(
                action.args.contains_key(field),
                "action {}'s resource names its argument {field}",
                action.id
            );
        }
    }
    Ok(())
}
