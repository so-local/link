//! The core, `lnk`: what of it a test reads. The core itself is the
//! program (`main.rs`).

pub mod checks;
pub mod cli;
pub mod history;

/// Current syntax, projected from the same declarations used for dispatch.
pub fn surface() -> history::Surface {
    let actions = link_component::actions::parse(include_str!("../actions.toml"))
        .expect("the core actions are valid");
    let mut surface = history::Surface::new();
    let core = link_component::actions::commands(&actions);
    for commands in registry()
        .values()
        .map(|d| &d.commands)
        .chain(std::iter::once(&core))
    {
        for (path, command) in commands {
            if path == "root" {
                continue;
            }
            let mut flags = std::collections::BTreeMap::new();
            for option in command.options.values() {
                let takes = !matches!(option.r#type, link_component::ValueType::Boolean);
                if let Some(long) = &option.long {
                    flags.insert(format!("--{long}"), takes);
                }
                if let Some(short) = &option.short {
                    flags.insert(format!("-{short}"), takes);
                }
            }
            surface.insert(path.clone(), flags);
        }
    }
    surface
}

use std::collections::BTreeMap;

/// Every component the core knows, as its build resolved them from their
/// `module.toml`: what `lnk --link-registry` prints. Read once a process:
/// it's every module's commands, a third of a megabyte.
pub fn registry() -> &'static BTreeMap<String, link_component::Resolved> {
    static REGISTRY: std::sync::LazyLock<BTreeMap<String, link_component::Resolved>> =
        std::sync::LazyLock::new(|| {
            #[cfg(test)]
            REGISTRY_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/registry.json")))
                .expect("the build validated the component registry")
        });
    &REGISTRY
}

/// How many times this test process read the registry.
#[cfg(test)]
static REGISTRY_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The ids of every module the core knows.
pub fn modules() -> Vec<&'static str> {
    registry().keys().map(String::as_str).collect()
}

/// Internal command history query handled by the core: what an upgrade
/// asks the incoming core, for its module renames.
pub const HISTORY_FLAG: &str = link_core::descriptor::HISTORY_FLAG;

/// Internal component registry query handled by the core.
pub const REGISTRY_FLAG: &str = "--link-registry";

/// Internal documentation query handled by the core.
pub const DOCUMENTATION_FLAG: &str = "--link-documentation";

#[cfg(test)]
link_test_kit::program!(system);

/// Each command reads the registry once, however many readers ask for it:
/// the release's history, its surface and its modules each do.
#[test]
fn the_registry_is_read_once_a_process() {
    let modules = modules();
    let _ = history::release();
    let _ = surface();
    assert_eq!(registry().len(), modules.len());
    assert_eq!(REGISTRY_READS.load(std::sync::atomic::Ordering::SeqCst), 1);
}
