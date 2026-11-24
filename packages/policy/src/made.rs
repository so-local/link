//! A child dies with its maker: what each principal made, by its maker,
//! so whatever removes a principal removes what it made too, and what
//! that made. Data a service keeps and acts on; nothing here removes.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::ensure;
use serde::{Deserialize, Serialize};

use crate::name::{PrincipalId, ResourceName};

/// What a principal makes: another principal (a session's service,
/// a subagent's) or a resource (a box).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Child {
    Principal(PrincipalId),
    Resource(ResourceName),
}

/// Every child, under its one maker.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Made(BTreeMap<PrincipalId, BTreeSet<Child>>);

impl Made {
    /// `child`, made by `maker`. Refused when it has a maker already, or
    /// when it made `maker`, so every child has one maker and none is its
    /// own.
    pub fn record(&mut self, maker: PrincipalId, child: Child) -> anyhow::Result<()> {
        ensure!(
            self.maker_of(&child).is_none(),
            "{} has a maker already",
            name(&child)
        );
        if let Child::Principal(made) = &child {
            ensure!(
                *made != maker
                    && !self
                        .ended_with(made)
                        .contains(&Child::Principal(maker.clone())),
                "{made} made {maker}, so {maker} can't make it"
            );
        }
        self.0.entry(maker).or_default().insert(child);
        Ok(())
    }

    /// Who made `child`, if it was recorded.
    pub fn maker_of(&self, child: &Child) -> Option<&PrincipalId> {
        self.0
            .iter()
            .find(|(_, children)| children.contains(child))
            .map(|(maker, _)| maker)
    }

    /// What `maker` made, directly.
    pub fn by(&self, maker: &PrincipalId) -> impl Iterator<Item = &Child> {
        self.0.get(maker).into_iter().flatten()
    }

    /// Everything that ends with `maker`: what it made, and what that
    /// made, each after what it made, so removing them in order never
    /// leaves a child without its maker.
    pub fn ended_with(&self, maker: &PrincipalId) -> Vec<Child> {
        let mut out = Vec::new();
        self.descend(maker, &mut out);
        out
    }

    fn descend(&self, maker: &PrincipalId, out: &mut Vec<Child>) {
        for child in self.by(maker) {
            if let Child::Principal(made) = child {
                self.descend(made, out);
            }
            out.push(child.clone());
        }
    }

    /// `maker` and all it made forgotten, as they're removed; returns
    /// what [`Made::ended_with`] does.
    pub fn end(&mut self, maker: &PrincipalId) -> Vec<Child> {
        let ended = self.ended_with(maker);
        self.0.remove(maker);
        for child in &ended {
            if let Child::Principal(made) = child {
                self.0.remove(made);
            }
        }
        for children in self.0.values_mut() {
            children.remove(&Child::Principal(maker.clone()));
        }
        self.0.retain(|_, children| !children.is_empty());
        ended
    }
}

fn name(child: &Child) -> String {
    match child {
        Child::Principal(principal) => principal.to_string(),
        Child::Resource(resource) => resource.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> PrincipalId {
        text.parse().unwrap()
    }

    fn service(text: &str) -> Child {
        Child::Principal(p(text))
    }

    fn boxed(text: &str) -> Child {
        Child::Resource(text.parse().unwrap())
    }

    #[test]
    fn a_child_ends_with_its_maker_and_its_own_children_first() {
        let mut made = Made::default();
        made.record(p("user:ownereqf7slq"), service("service:s1qiwywwj1pe"))
            .unwrap();
        made.record(p("service:s1qiwywwj1pe"), boxed("box:b1"))
            .unwrap();
        made.record(p("service:s1qiwywwj1pe"), service("service:s1ci8fzi9wz7"))
            .unwrap();
        made.record(p("service:s1ci8fzi9wz7"), boxed("box:b2"))
            .unwrap();
        made.record(p("user:ownereqf7slq"), boxed("box:mine"))
            .unwrap();

        let ended = made.ended_with(&p("service:s1qiwywwj1pe"));
        assert_eq!(
            ended,
            [
                boxed("box:b2"),
                service("service:s1ci8fzi9wz7"),
                boxed("box:b1")
            ]
        );
        assert_eq!(made.end(&p("service:s1qiwywwj1pe")), ended);
        let left: Vec<_> = made.by(&p("user:ownereqf7slq")).cloned().collect();
        assert_eq!(left, [boxed("box:mine")]);
        assert_eq!(made.maker_of(&boxed("box:b2")), None);
    }

    #[test]
    fn one_maker_and_no_loop() {
        let mut made = Made::default();
        made.record(p("user:ownereqf7slq"), service("service:s1qiwywwj1pe"))
            .unwrap();
        made.record(p("service:s1qiwywwj1pe"), service("service:s23o280y9gpq"))
            .unwrap();
        assert!(
            made.record(p("user:otherbf4q36h"), service("service:s1qiwywwj1pe"))
                .is_err()
        );
        assert!(
            made.record(p("service:s23o280y9gpq"), service("user:ownereqf7slq"))
                .is_err()
        );
        assert!(
            made.record(p("service:s33ahp4o86op"), service("service:s33ahp4o86op"))
                .is_err()
        );
    }

    #[test]
    fn it_is_kept_as_toml() {
        let mut made = Made::default();
        made.record(p("user:ownereqf7slq"), service("service:s1qiwywwj1pe"))
            .unwrap();
        made.record(p("service:s1qiwywwj1pe"), boxed("box:b1"))
            .unwrap();
        let text = toml::to_string(&made).unwrap();
        assert_eq!(toml::from_str::<Made>(&text).unwrap(), made);
    }
}
