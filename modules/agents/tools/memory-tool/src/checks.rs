//! The check hooks the tool's declarations name, on the arguments its
//! parser reads: each rule, once, is its contract's.

use crate::cli::MemoryToolServeManyArgs;

pub(crate) fn embedding(args: &MemoryToolServeManyArgs) -> Result<(), String> {
    link_memory_tool_contract::embedding_together(args.embed.is_some(), args.embed_url.is_some())
}

/// `serve`'s: an address needs its model, its contract's rule, while a
/// model alone is served for it.
pub(crate) fn embed(args: &crate::cli::MemoryToolServeArgs) -> Result<(), String> {
    link_memory_tool_contract::address_needs_model(args.embed.is_some(), args.embed_url.is_some())
}

/// `bridge`'s, its contract's rule.
pub(crate) fn bridge(args: &crate::cli::MemoryToolBridgeArgs) -> Result<(), String> {
    let value = serde_json::to_value(args).map_err(|e| e.to_string())?;
    let args: link_memory_tool_contract::tool::MemoryToolBridgeArgs =
        serde_json::from_value(value).map_err(|e| e.to_string())?;
    link_memory_tool_contract::bridge_flags(&args)
}
