//! The generators that write each surface from the catalog. Each reads a
//! [`Catalog`](link_actions::Catalog) and nothing else, writes its surface's
//! files, and adds no behavior, so what a surface can do is what the
//! declarations say. [`Files`] checks what they write for freshness.

pub mod completion;
mod config;
mod files;
pub mod modules;
pub mod openapi;
pub mod pages;
pub mod reference;
pub mod rust;
mod shapes;
pub mod tools;
pub mod typescript;

pub use files::{Files, Problem};

use link_actions::{Action, Arg, Surface};

/// The marker line every generated file carries.
pub fn marker() -> &'static str {
    &config::SETTINGS.generator.marker
}

/// The actions offered on `surface`, in ID order.
pub(crate) fn offered(
    catalog: &link_actions::Catalog,
    surface: Surface,
) -> impl Iterator<Item = &Action> {
    catalog
        .actions()
        .filter(move |action| action.offered_on(surface))
}

/// An action's runtime arguments as a surface other than the command line
/// takes them: core's printing-only shared ones left out, and a bound
/// action's own spec file, since that surface holds the spec itself.
pub(crate) fn args(action: &Action) -> impl Iterator<Item = (&String, &Arg)> {
    let bound = action.spec.is_some() || action.held.is_some();
    action.args.iter().filter(move |(name, arg)| {
        !(bound && **name == link_actions::core().spec)
            && arg
                .cli
                .shared
                .as_ref()
                .is_none_or(|shared| !config::SETTINGS.surfaces.cli_only.contains(shared))
    })
}

/// An action's runtime arguments as REST takes them: those it accepts.
pub(crate) fn rest_args(action: &Action) -> impl Iterator<Item = (&String, &Arg)> {
    args(action).filter(|(name, arg)| action.rest_accepts(name, arg))
}

/// An action's runtime arguments as agents' tools take them: also without
/// core's shared ones only a person answers.
pub(crate) fn tool_args(action: &Action) -> impl Iterator<Item = (&String, &Arg)> {
    args(action).filter(|(_, arg)| {
        arg.cli
            .shared
            .as_ref()
            .is_none_or(|shared| !config::SETTINGS.surfaces.person_only.contains(shared))
    })
}

#[cfg(test)]
link_test_kit::program!(unit);
