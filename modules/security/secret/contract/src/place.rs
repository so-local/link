//! Where a copy of a service's own secrets was given: what a machine's
//! note of a copy away says, and where `lnk secret delete` reaches it.

use serde::{Deserialize, Serialize};

/// Where a copy of a service's own secrets was given: a box of yours, by
/// its name and its installation ID, or a cloud account, by its name and the
/// owner the copy went there as. What the note of a copy away says, and
/// where `lnk secret delete` reaches it.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    /// A cloud account, not a box.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cloud: bool,
    pub name: String,
    /// A box's installation ID: none for a box that
    /// didn't answer, or an account. A box made again under the name has
    /// another. Kept and written as `resource: "box:<id>"`.
    #[serde(
        default,
        rename = "resource",
        with = "link_machine::machine::box_field",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<String>,
    /// An account's owner the copy went there as (`link_core::owner::account_owner`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl Place {
    /// A box of yours, by its name and, when it's known, its installation ID.
    pub fn of_box(name: &str, id: Option<String>) -> Place {
        Place {
            name: name.to_string(),
            id,
            ..Place::default()
        }
    }

    /// A cloud account, the copy there as `owner`.
    pub fn of_account(name: &str, owner: Option<String>) -> Place {
        Place {
            cloud: true,
            name: name.to_string(),
            owner,
            ..Place::default()
        }
    }

    /// A box's installation ID as output writes it, `box:<id>`, when it's known.
    pub fn resource(&self) -> Option<String> {
        self.id.as_deref().map(link_machine::machine::box_resource)
    }

    /// Whether `other` is the same place by its kind and name: a box made
    /// again under the name is, until its installation ID says otherwise.
    pub fn is(&self, other: &Place) -> bool {
        self.cloud == other.cloud && self.name == other.name
    }

    /// An error unless its name is a box's or an account's, its ID a
    /// machine's and its owner an owner.
    pub fn check(&self) -> anyhow::Result<()> {
        link_machine::names::check_shape(&self.name)?;
        if let Some(id) = &self.id {
            link_machine::names::check_id(id)?;
        }
        if let Some(owner) = &self.owner {
            link_machine::owner::check_owner(owner)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boxs_machine_is_kept_as_a_box_resource() {
        let place = Place::of_box("aws-1", Some("a1b2c3d4e5f6".into()));
        let saved = toml::to_string(&place).unwrap();
        assert!(
            saved.contains(r#"resource = "box:a1b2c3d4e5f6""#),
            "{saved}"
        );
        assert_eq!(toml::from_str::<Place>(&saved).unwrap(), place);
        assert_eq!(place.resource().as_deref(), Some("box:a1b2c3d4e5f6"));
        for bare in [
            "name = \"aws-1\"\nresource = \"a1b2c3d4e5f6\"",
            "name = \"aws-1\"\nresource = \"installation:a1b2c3d4e5f6\"",
            "name = \"aws-1\"\nid = \"a1b2c3d4e5f6\"",
        ] {
            assert!(toml::from_str::<Place>(bare).is_err(), "{bare}");
        }
    }
}
