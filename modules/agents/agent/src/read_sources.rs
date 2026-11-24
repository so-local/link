//! Managed read origins handed to a harness by its host. A service name
//! or a response field can never establish this authority.

use link_agent::settings::Settings;
use link_harness_wire::evidence::{ManagedReadSource, ReadProvider, ReadRegistration};

use crate::defined::DefinedTool;

/// Verifies the current native definitions before recording any authority.
/// An unavailable or malformed inventory leaves every source unverified.
pub(crate) fn registrations(
    agent: &Settings,
    handed: &link_harness_contract::harness::Settings,
) -> Vec<ReadRegistration> {
    let Ok(definitions) = crate::defined::list_tools() else {
        return vec![];
    };
    let (Ok(secret), Ok(owner)) = (agent.url_secret(), agent.owner()) else {
        return vec![];
    };
    let endpoint =
        |name: &str| link_sandbox_client::routes::tool_url(agent.proxy_port(), name, &secret);
    let source = |provider, definition: String, argv_sha256| ManagedReadSource {
        provider,
        owner: owner.clone(),
        agent: agent.key(),
        definition,
        argv_sha256,
    };
    let registration = |server: &str, endpoint: String, source| ReadRegistration {
        server: server.into(),
        operation: "read".into(),
        endpoint,
        source,
    };
    let mut registrations = Vec::new();
    if let Some(name) = crate::run_file::tool(agent, "file")
        && let Some(native) = unique(&definitions, &name)
        && let Some(digest) = crate::run_file::read_fingerprint(agent, &native.argv)
    {
        let source = source(ReadProvider::File, name.clone(), digest);
        // Native environment starts can reach File directly. Configured
        // starts generally reach the host's namespace over that same tool.
        registrations.push(registration("file", endpoint(&name), source.clone()));
        if let Some((_, namespace)) = crate::namespaces::handed(agent)
            .into_iter()
            .find(|(namespace, _)| namespace == "file")
            && let Some(outer) = unique(&definitions, &namespace)
            && valid_namespace(agent, outer, &name)
            && handed.tools.get("file") == Some(&endpoint(&namespace))
        {
            registrations.push(registration("file", endpoint(&namespace), source));
        }
    }
    if let Some(name) = agent.memory_tool()
        && let Some(native) = unique(&definitions, &name)
        && let Some(digest) = crate::memory::read_fingerprint(agent, &native.argv)
        && handed.memory.as_ref() == Some(&endpoint(&name))
    {
        registrations.push(registration(
            "memory",
            endpoint(&name),
            source(ReadProvider::Memory, name, digest),
        ));
    }
    registrations
}

fn unique<'a>(definitions: &'a [DefinedTool], name: &str) -> Option<&'a DefinedTool> {
    let mut found = definitions.iter().filter(|tool| tool.name == name);
    let tool = found.next()?;
    found.next().is_none().then_some(tool)
}

fn valid_namespace(agent: &Settings, actual: &DefinedTool, native: &str) -> bool {
    let path = crate::namespaces::file(agent, "file");
    let Some(program) = link_core::find_module("sandbox") else {
        return false;
    };
    let expected = [
        program.display().to_string(),
        "mcp-namespace".into(),
        path.display().to_string(),
    ];
    if actual.argv != expected {
        return false;
    }
    let Some(definition) = std::fs::read(&path).ok().and_then(|bytes| {
        link_files::saved::parse_json::<link_sandbox_client::namespace::Namespace>(&bytes).ok()
    }) else {
        return false;
    };
    if definition.check().is_err() {
        return false;
    }
    let Some(read) = definition.actions.get("read") else {
        return false;
    };
    !read.broadcast
        && read.targets.len() == 1
        && read
            .targets
            .values()
            .any(|target| target.server == native && target.tool == "read")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_or_missing_definitions_never_establish_identity() {
        let tool = |name| {
            serde_json::from_value::<DefinedTool>(serde_json::json!({
                "name": name, "argv": ["/native/file"], "ask": "none", "allow": ["read"],
            }))
            .unwrap()
        };
        assert!(unique(&[], "file").is_none());
        assert!(unique(&[tool("other")], "file").is_none());
        assert!(unique(&[tool("file")], "file").is_some());
        assert!(unique(&[tool("file"), tool("file")], "file").is_none());
    }
}
