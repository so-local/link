//! The checks the package's declarations name, each a rule the declared
//! shape alone can't say.

/// A Policy Spec's own rules, beyond its shape: its name, its version,
/// and grants whose actions and resources are of one kind.
pub(crate) fn policy(policy: &crate::Policy) -> Result<(), String> {
    policy.check().map_err(|e| format!("{e:#}"))
}

/// An action pattern reads as the actions it allows.
pub(crate) fn action_pattern(pattern: &crate::ActionPattern) -> Result<(), String> {
    pattern.0.parse::<crate::Actions>().map(drop)
}

/// A resource pattern reads as the resources it reaches.
pub(crate) fn resource_pattern(pattern: &crate::ResourcePattern) -> Result<(), String> {
    pattern.0.parse::<crate::Resources>().map(drop)
}
