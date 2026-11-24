//! Trusted source bindings and execution admission.
use super::route::Tools;
use anyhow::ensure;
use serde_json::Value;

/// Current source policy, work and sandbox state are checked under both dispatch fences.
pub(super) fn application(
    tools: &Tools,
    target: &str,
    message: &Value,
    reviewed: Option<crate::bridges::asks::Reviewed>,
) -> anyhow::Result<Option<link_interactions::sources::Application>> {
    ensure!(
        !link_sandbox_proxy::wire::is_stopped(&tools.dir)
            && !link_sandbox_proxy::wire::paused(&tools.dir)
                .iter()
                .any(|(name, _)| name == &tools.sandbox),
        "sandbox stopped or paused before dispatch"
    );
    let work = work_of(tools, message).map_err(anyhow::Error::msg)?;
    let payload = serde_json::to_vec(message)?;
    if let Some(reviewed) = reviewed {
        return Ok(Some(reviewed.claim(&payload, true)?));
    }
    if let Some(work) = work
        && matches!(
            message["method"].as_str(),
            Some("tools/call" | "resources/read" | "prompts/get")
        )
    {
        let call = message["params"]["_meta"]["link.local/call"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source call has no host call identity"))?;
        return Ok(Some(crate::bridges::asks::store(&tools.dir)?.policy_call(
            &work,
            &link_interactions::SourceId::new("sandbox").map_err(anyhow::Error::msg)?,
            &format!(
                    "{target}/{}",
                    message["params"]["name"]
                        .as_str()
                        .unwrap_or_else(|| message["method"].as_str().unwrap_or_default())
                ),
            call,
            &payload,
        )?));
    }
    Ok(None)
}

/// Validate host metadata at the trusted source, including on calls whose policy does not ask.
pub(super) fn work_of(
    tools: &Tools,
    message: &Value,
) -> Result<Option<link_interactions::WorkRef>, String> {
    let meta = &message["params"]["_meta"];
    let store = crate::bridges::asks::store(&tools.dir).map_err(|err| format!("{err:#}"))?;
    let Some(token) = meta["link.local/work"].as_str() else {
        if matches!(
            message["method"].as_str(),
            Some("tools/call" | "resources/read" | "prompts/get")
        ) && let Some(asking) = &tools.asking
            && store
                .has_agent_work(&asking.agent)
                .map_err(|e| e.to_string())?
        {
            return Err(
                "this agent requires an authenticated work binding for source calls".into(),
            );
        }
        return Ok(None);
    };
    store
        .source_work(
            token,
            tools.asking.as_ref().map(|a| a.agent.as_str()),
            meta["link.local/thread"].as_str().unwrap_or_default(),
            meta["link.local/attempt"].as_str().unwrap_or_default(),
            meta["link.local/control_revision"]
                .as_u64()
                .ok_or("source call has no control revision")?,
        )
        .map(Some)
        .map_err(|e| e.to_string())
}
