//! The app's page per action: a route at `<namespace>/<resource>/<verb>`
//! that renders one action component, which the app names, with its
//! action ID, so a page holds nothing but that ID.

use std::path::PathBuf;

use link_actions::{Action, Catalog, Surface};

use crate::{Files, config::SETTINGS};

/// The component every page renders: its name, and the module it is
/// imported from.
#[derive(Clone, Copy, Debug)]
pub struct Component<'a> {
    pub name: &'a str,
    pub import: &'a str,
}

/// Every page, in a folder the generator owns whole.
pub fn generate(catalog: &Catalog, component: Component) -> anyhow::Result<Files> {
    let mut files = Files::new();
    files.own("");
    for action in crate::offered(catalog, Surface::App) {
        files.insert(path(action), page(action, component))?;
    }
    Ok(files)
}

/// A page's path from the app's route folder: `env/approval/add.tsx`.
pub fn path(action: &Action) -> PathBuf {
    let id = &action.id;
    PathBuf::from(id.namespace())
        .join(id.resource())
        .join(format!("{}.{}", id.verb(), SETTINGS.app.extension))
}

fn page(action: &Action, component: Component) -> String {
    let id = serde_json::to_string(action.id.as_str()).expect("a string");
    let import = serde_json::to_string(component.import).expect("a string");
    format!(
        "// {marker}\nimport {{ {name} }} from {import};\n\n\
         export default function Page() {{\n  return <{name} action={{{id}}} />;\n}}\n",
        marker = crate::marker(),
        name = component.name,
    )
}
