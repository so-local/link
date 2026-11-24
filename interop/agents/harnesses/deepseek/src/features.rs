//! The features DeepSeek Harness honors ([`link_harness_contract::harness::features`]): its own
//! repository instructions and tools, each its own way; the persona and
//! skills of the agent's spec ([`crate::identity`]); and the groups and
//! streaming of the connections Link carries to its endpoint, and the
//! schedules Link's scheduler fires into it, as the agent's spec says.

use link_harness_contract::harness::features::{Feature, Honor, Honors};

/// What its contract says it honors.
pub fn honors() -> Honors {
    use Feature::*;
    let native = [
        RepoInstructions,
        Subagents,
        Shell,
        FileRead,
        FileEdit,
        CodeSearch,
        Diffs,
    ];
    let native = native.into_iter().map(|feature| (feature, Honor::Native));
    // Its schedules are Link's scheduler's, fired into it as a prompt
    // from them.
    let carried = [Persona, Skills, Groups, Streaming, Schedules, Proactive]
        .into_iter()
        .map(|f| (f, Honor::Spec));
    native.chain(carried).collect()
}
