//! Every public spec kind and its reference page: what the specs index
//! lists, and what a command of each kind binds its field flags by.

use link_spec::Exported;

/// A kind and its page, which `link-actions-surfaces` writes from the
/// kind's declaration.
pub struct KindPage {
    pub kind: Exported,
    /// Its reference page, where it has one yet.
    pub page: &'static str,
    /// The binding a command of this kind takes its field flags by.
    pub binding: fn() -> link_spec::Binding,
}

/// Kind `T`'s page.
fn page<T: link_spec::Kind>(page: &'static str) -> KindPage {
    KindPage {
        kind: Exported::of::<T>(),
        page,
        binding: link_spec::Binding::of::<T>,
    }
}

/// Every public kind and its reference page. Every kind a command binds
/// is here (a test holds it), and the specs index lists each whose page
/// exists yet.
pub fn kinds() -> Vec<KindPage> {
    vec![
        page::<link_bundle::cli::Bundle>("modules/tooling/bundle/docs/config/specs/bundle.md"),
        page::<link_agent_spec::Spec>("modules/agents/agent/docs/config/specs/agent.md"),
        page::<link_box::spec::BoxSpec>("modules/compute/box/docs/config/specs/box.md"),
        page::<link_env_contract::file::Env>("modules/compute/env/docs/config/specs/env.md"),
        page::<link_policy::Policy>("packages/policy/docs/config/specs/policy.md"),
        page::<link_sandbox_client::Spec>("modules/security/sandbox/docs/config/specs/sandbox.md"),
        page::<link_agent_spec::TeamFile>("modules/agents/agent/docs/config/specs/team.md"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    #[test]
    fn every_kind_a_command_binds_has_its_page_here() {
        let listed: Vec<String> = kinds().into_iter().map(|k| k.kind.kind).collect();
        for described in link_cli::registry().values() {
            for (path, command) in &described.commands {
                if let Some(kind) = &command.spec {
                    assert!(
                        listed.contains(kind),
                        "lnk {path} binds {kind}, which kinds() lacks"
                    );
                }
            }
        }
    }

    #[test]
    fn every_kind_with_a_page_is_in_the_specs_index_by_its_title() {
        let index = std::fs::read_to_string(root().join(crate::inventories::SPECS)).unwrap();
        for kind in kinds() {
            let Ok(text) = std::fs::read_to_string(root().join(kind.page)) else {
                continue;
            };
            let title = crate::inventories::title_of(&text).unwrap();
            assert!(index.contains(&format!("[{title}]")), "{title}");
        }
    }
}
