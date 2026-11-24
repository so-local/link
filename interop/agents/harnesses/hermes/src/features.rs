//! The features Hermes honors ([`link_harness_contract::harness::features`]): its persona,
//! skills, schedules and messages it sends first from the agent's spec
//! ([`crate::identity`]), the rest its own way. Its own scheduler is off,
//! since its results would reach no chat: Link's scheduler fires into it.

use link_harness_contract::harness::features::{Feature, Honor, Honors};

/// What its contract says it honors.
pub fn honors() -> Honors {
    use Feature::*;
    [
        RepoInstructions,
        Persona,
        Skills,
        Commands,
        ModelFallback,
        Images,
        Attachments,
        Groups,
        Streaming,
        Subagents,
        Schedules,
        Proactive,
        Shell,
        FileRead,
        FileEdit,
        CodeSearch,
        Diffs,
    ]
    .into_iter()
    .map(|feature| match feature {
        // Its schedules are Link's scheduler's, fired into it as a
        // prompt from them.
        Persona | Skills | Schedules | Proactive => (feature, Honor::Spec),
        _ => (feature, Honor::Native),
    })
    .collect()
}
