//! The check hooks the declarations name, on the arguments this crate
//! generates: the protocol's, and Link Memory's own commands' (`tool`).

use crate::declared::MemoryServeManyArgs;
use crate::tool::{MemoryToolBridgeArgs, MemoryToolServeArgs, MemoryToolServeManyArgs};

/// What `serve-many` says of its model, as either declaration of it
/// generates it.
pub(crate) trait Embedding {
    fn flags(&self) -> (bool, bool);
}

impl Embedding for MemoryServeManyArgs {
    fn flags(&self) -> (bool, bool) {
        (self.embed.is_some(), self.embed_url.is_some())
    }
}

impl Embedding for MemoryToolServeManyArgs {
    fn flags(&self) -> (bool, bool) {
        (self.embed.is_some(), self.embed_url.is_some())
    }
}

pub(crate) fn embedding(args: &impl Embedding) -> Result<(), String> {
    let (embed, url) = args.flags();
    crate::embedding_together(embed, url)
}

pub(crate) fn embed(args: &MemoryToolServeArgs) -> Result<(), String> {
    crate::address_needs_model(args.embed.is_some(), args.embed_url.is_some())
}

pub(crate) fn bridge(args: &MemoryToolBridgeArgs) -> Result<(), String> {
    crate::bridge_flags(args)
}
