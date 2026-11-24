//! Who may do what: the Policy Spec and the one check of a principal's
//! action on a resource.
//!
//! A principal does an action on a resource; it's allowed when its
//! policy says so, and, for another account's resource, that resource's
//! `[access]` does too. Nothing else is allowed: there are no deny rules,
//! and every check goes through [`allows`].

mod check;
mod checks;
mod declared {
    //! The Policy Spec's shapes, generated from the package's
    //! declarations (`types.toml`).
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));
}
mod made;
mod name;
mod policy;
mod role;

pub use check::{
    Decision, Principal, Refused, Resource, allows, allows_all, allows_all_with_access,
};
pub use made::{Child, Made};
pub use name::{
    AccountId, Accounts, Action, Actions, Kind, PrincipalId, PrincipalKind, ResourceName, Resources,
};
pub use policy::{
    Access, ActionPattern, Exceeded, Grant, Limits, MAX_GRANTS, Policy, ResourcePattern, Usage,
    VERSION, Who, parse,
};
pub use role::{Roles, check_name as check_role_name};

#[cfg(test)]
mod whole_access_tests;

#[cfg(test)]
link_test_kit::program!(unit);
