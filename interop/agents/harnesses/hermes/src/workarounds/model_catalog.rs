//! Hermes 0.23.0 through 0.23.3 build the ACP model picker synchronously at session/new.
//! Its supposedly non-blocking inventory also downloads models.dev on a
//! cold cache. Use its cache-only inventory path and carry that option
//! through the registry read; ordinary CLI refresh keeps live discovery.
//! Hosted relay metadata must not probe local model servers, and an owned
//! gateway must not be replaced with a detached daemon by its frontend, nor
//! leave a detached shutdown diagnostic behind when Link stops it.
//! The upstream reproducer is recorded in the adapter's decisions page.

use anyhow::ensure;
use link_format::WithinExt;
use serde::Deserialize;
use std::{path::Path, sync::OnceLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixes {
    patches: Vec<Patch>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    path: String,
    before: String,
    after: String,
}

fn fixes() -> &'static Fixes {
    static FIXES: OnceLock<Fixes> = OnceLock::new();
    FIXES.get_or_init(|| {
        toml::from_str(include_str!("model_catalog.toml")).expect("catalogue fixes")
    })
}

/// Applied while sealing the staged payload (`seal`), before the upstream
/// ACP process can ever start from it. Validate every file
/// before writing any; a changed upstream refuses the fix
/// rather than silently leaving session creation dependent on a network.
pub(crate) fn apply(code: &Path) -> anyhow::Result<()> {
    let mut changed = std::collections::BTreeMap::new();
    for patch in &fixes().patches {
        let path = code.join(&patch.path);
        let text = match changed.get(&path) {
            Some(text) => String::clone(text),
            None => std::fs::read_to_string(&path).within(format!(
                "reading Hermes's catalogue source {}",
                path.display()
            ))?,
        };
        if text.matches(&patch.after).count() == 1
            && !text.replacen(&patch.after, "", 1).contains(&patch.before)
        {
            continue;
        }
        ensure!(
            text.matches(&patch.before).count() == 1 && !text.contains(&patch.after),
            "Hermes's catalogue source {} changed; reinstall the pinned Hermes release",
            path.display()
        );
        changed.insert(path, text.replacen(&patch.before, &patch.after, 1));
    }
    for (path, text) in changed {
        link_files::write_private(&path, text.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_picker_covers_hidden_metadata_reads_and_restores_live_discovery() {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return;
        }
        let patch = |path: &str, needle: &str| {
            fixes()
                .patches
                .iter()
                .find(|p| p.path == path && p.before.contains(needle))
                .unwrap()
        };
        let scope = patch("agent/models_dev.py", "from utils");
        let fetch = patch("agent/models_dev.py", "global _models_dev_cache");
        let picker = patch("acp_adapter/model_catalog.py", "def build_model_state");
        let script = include_str!("picker-test.py")
            .replace(
                "__SCOPE__",
                &serde_json::to_string(&scope.after.replace(&scope.before, "")).unwrap(),
            )
            .replace("__FETCH__", &serde_json::to_string(&fetch.after).unwrap())
            .replace("__PICKER__", &serde_json::to_string(&picker.after).unwrap());
        let home = link_test_kit::TempDir::new("hermes-picker-metadata");
        let output = link_test_kit::Deadline::new(
            "cached picker metadata",
            std::time::Duration::from_secs(30),
        )
        .output(link_test_kit::env::command("python3", &home).args(["-I", "-c", &script]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The model address Link hands Hermes, with or without `/v1`, is
    /// Link's local-model route, never probed; another local server, or
    /// none handed, is probed as Hermes would; the hosted relay keeps its
    /// own test.
    #[test]
    fn links_local_route_is_known_by_the_address_link_hands_hermes() {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return;
        }
        let helpers = fixes()
            .patches
            .iter()
            .find(|p| p.after.contains("def _link_local_route"))
            .unwrap();
        let script = format!(
            "from urllib.parse import urlparse\nimport os\n{}\n{}",
            helpers.after,
            r#"
os.environ.pop("LNK_HERMES_MODEL_URL", None)
assert not _link_local_route("http://127.0.0.1:11434/v1")
os.environ["LNK_HERMES_MODEL_URL"] = "http://127.0.0.1:11434/v1"
assert _link_local_route("http://127.0.0.1:11434/v1")
assert _link_local_route("http://127.0.0.1:11434")
assert _link_local_route("http://127.0.0.1:11434/v1/")
assert not _link_local_route("http://127.0.0.1:8080/v1")
os.environ["LNK_HERMES_MODEL_URL"] = "http://127.0.0.1:9/_link/model"
assert not _link_local_route("http://127.0.0.1:9/_link/model")
"#
        );
        let home = link_test_kit::TempDir::new("hermes-local-route");
        let output = link_test_kit::Deadline::new(
            "Link's local route recognized",
            std::time::Duration::from_secs(30),
        )
        .output(link_test_kit::env::command("python3", &home).args(["-I", "-c", &script]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fixture(code: &Path) {
        let mut files = std::collections::BTreeMap::<String, String>::new();
        for patch in &fixes().patches {
            files
                .entry(patch.path.clone())
                .or_default()
                .push_str(&format!("# unrelated source\n{}\n", patch.before));
        }
        for (name, text) in files {
            let path = code.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    #[test]
    fn configuration_selects_cached_catalogues_and_preserves_other_source() {
        let home = link_test_kit::TempDir::new("hermes-cached-catalogues");
        fixture(&home);
        apply(&home).unwrap();
        apply(&home).unwrap();
        for patch in &fixes().patches {
            let text = std::fs::read_to_string(home.join(&patch.path)).unwrap();
            assert!(text.contains(&patch.after), "{} lost its fix", patch.path);
            assert!(text.contains("# unrelated source"));
        }
    }

    #[test]
    fn unexpected_or_ambiguous_source_refuses_without_partial_edits() {
        for ambiguous in [false, true] {
            let home = link_test_kit::TempDir::new("hermes-changed-catalogues");
            fixture(&home);
            let first = &fixes().patches[0];
            let patch = fixes()
                .patches
                .iter()
                .find(|p| p.path != first.path)
                .unwrap();
            let unexpected = if ambiguous {
                format!("{}\n{}\n", patch.before, patch.before)
            } else {
                "# new upstream\n".into()
            };
            std::fs::write(home.join(&patch.path), unexpected).unwrap();
            assert!(apply(&home).is_err());
            assert!(
                std::fs::read_to_string(home.join(&first.path))
                    .unwrap()
                    .contains(&first.before)
            );
        }
    }

    #[test]
    fn hosted_metadata_uses_inference_and_preserves_native_and_external_endpoints() {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return;
        }
        let home = link_test_kit::TempDir::new("hermes-metadata-routing");
        let patch = fixes()
            .patches
            .iter()
            .find(|p| {
                p.path == "agent/model_metadata.py" && p.before.contains("def _normalize_base_url")
            })
            .unwrap();
        let script = [
            "from urllib.parse import urlparse\nfrom typing import Optional\n",
            patch.after.as_str(),
            fixes()
                .patches
                .iter()
                .find(|p| p.before.contains("def _query_ollama_api_show("))
                .unwrap()
                .after
                .as_str(),
            "\n_ANTHROPIC_COMPATIBLE_HOSTS = {'api.anthropic.com'}\n",
            fixes()
                .patches
                .iter()
                .find(|p| p.path == "agent/auxiliary_client.py")
                .unwrap()
                .after
                .as_str(),
            include_str!("metadata-test.py"),
        ]
        .concat();
        let output = link_test_kit::Deadline::new(
            "hosted metadata guards",
            std::time::Duration::from_secs(30),
        )
        .output(link_test_kit::env::command("python3", &home).args(["-I", "-c", &script]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn an_owned_gateway_stops_without_a_detached_diagnostic() {
        let patch = fixes()
            .patches
            .iter()
            .find(|p| p.path == "gateway/run_bootstrap.py")
            .unwrap();
        assert!(patch.before.contains("_best_effort(_diagnostic,"));
        assert!(!patch.after.contains("_diagnostic"));
        assert!(patch.after.contains("_best_effort(_log_context,"));
    }

    #[test]
    fn managed_frontend_waits_for_admission_without_spawning_or_hiding_refusals() {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return;
        }
        let home = link_test_kit::TempDir::new("hermes-managed-admission");
        let patch = fixes()
            .patches
            .iter()
            .find(|p| p.path == "hermes_cli/gateway_client.py")
            .unwrap();
        let script = include_str!("gateway-test.py").replace(
            "# MANAGED_BODY",
            &patch
                .after
                .lines()
                .map(|line| line.strip_prefix("    ").unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let output = link_test_kit::Deadline::new(
            "managed gateway admission",
            std::time::Duration::from_secs(30),
        )
        .output(link_test_kit::env::command("python3", &home).args(["-I", "-c", &script]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn tool_schema_metadata_is_local_without_changing_real_vision_routing() {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return;
        }
        let home = link_test_kit::TempDir::new("hermes-schema-metadata");
        let source = fixes()
            .patches
            .iter()
            .find(|p| {
                p.path == "agent/image_routing.py" && p.before.contains("def _probe_models_dev")
            })
            .unwrap()
            .after
            .clone();
        let lookup = fixes()
            .patches
            .iter()
            .find(|p| p.path == "agent/image_routing.py" && p.before.contains("caps ="))
            .unwrap()
            .after
            .clone();
        let scoped = fixes()
            .patches
            .iter()
            .find(|p| p.path == "tools/registry.py")
            .unwrap()
            .after
            .clone();
        let gate = fixes()
            .patches
            .iter()
            .find(|p| p.path == "tools/vision_tools.py")
            .unwrap()
            .after
            .clone();
        // The fragment keeps the upstream method's indentation; run it at
        // the top level, whatever depth upstream nests the call at.
        let indent = scoped
            .lines()
            .find(|line| !line.trim().is_empty())
            .map_or(0, |line| line.len() - line.trim_start().len());
        let script = include_str!("schema-test.py")
            .replace(
                "__LOOKUP__",
                &format!(
                    "{}    from agent.models_dev import get_model_capabilities\n{}",
                    source, lookup
                ),
            )
            .replace(
                "__SCOPED__",
                &serde_json::to_string(
                    &scoped
                        .lines()
                        .map(|s| s.get(indent..).unwrap_or(s.trim_start()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .unwrap(),
            )
            .replace("__GATE__", &serde_json::to_string(&gate).unwrap());
        let output = link_test_kit::Deadline::new(
            "schema-only model metadata",
            std::time::Duration::from_secs(30),
        )
        .output(link_test_kit::env::command("python3", &home).args(["-I", "-c", &script]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
