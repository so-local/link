//! Content identity supplied by a trusted read or instruction-loading
//! boundary, apart from whether the agent followed that content.

use super::{CallRef, ReadAuthority};
use link_files::reading::{ContentDigest, ReadReceipt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SkillAnchor {
    Call {
        call: CallRef,
    },
    /// The owner event carrying this record, whose successful append
    /// supplies its sequence without a second, speculative identity.
    Owner,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SkillSource {
    Owned { path: String },
    Repository { path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillRoute {
    File,
    Memory,
    Invocation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillLoad {
    pub anchor: SkillAnchor,
    pub source: SkillSource,
    pub route: SkillRoute,
    pub authority: ReadAuthority,
    pub read: ReadReceipt,
    /// Exact skill text known to be in the model-facing view. None
    /// when clipping or another transformation prevents establishing it.
    pub supplied: Option<ContentDigest>,
    /// The final containing tool/owner message, after all formatting.
    pub model_view: ContentDigest,
    pub complete: bool,
    pub omissions: Vec<String>,
    /// Preserved separately from explicit invocation's expanded body.
    pub original_input: Option<String>,
}
