//! Your boxes, as `lnk box list --json` lists them: what the box module
//! writes, and what every other module reads, from this one type. Each
//! reader runs `lnk box list --json` itself, since the core runs no
//! module's commands.

use link_cloud_contract::prices::Price;
use serde::{Deserialize, Serialize};

/// A box of yours: one this computer knows, or one a connected cloud
/// account has tagged as yours.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    #[serde(default)]
    pub tags: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    pub name: String,
    /// The cloud account it's in.
    #[serde(default)]
    pub account: String,
    /// Its cloud's kind (`aws`, `gcp`).
    #[serde(default)]
    pub kind: String,
    /// Its region or zone.
    #[serde(default)]
    pub place: String,
    /// Its cloud's instance ID, which a box made again under the name
    /// doesn't have.
    #[serde(default)]
    pub id: String,
    /// Its installation ID; empty while Link doesn't know it. Written
    /// `"resource": "box:<id>"`, left out while it's empty or not an ID
    /// ([`resource`]).
    #[serde(
        default,
        rename = "resource",
        with = "resource",
        skip_serializing_if = "resource::unknown"
    )]
    pub machine_id: String,
    #[serde(default)]
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// `running`, `stopped`, ...
    #[serde(default)]
    pub state: String,
    /// When this computer made it, in Unix seconds; 0 for one found in a
    /// cloud.
    #[serde(default)]
    pub created: u64,
    /// `here`, or `cloud` for one found just now.
    #[serde(default)]
    pub found: String,
    /// A spot box its cloud stopped, as its adapter says.
    #[serde(default)]
    pub stopped_by_cloud: bool,
}

/// The kind a box's ID is written with: `box:<its installation ID>`.
pub use link_machine::machine::BOX_KIND as KIND;

/// A box's installation ID as `lnk box list --json` writes it, `box:<id>`,
/// left out while it's unknown or a cloud's tag holds no ID; read back
/// refusing any other form.
pub mod resource {
    use serde::{Deserialize, Deserializer, Serializer};

    use link_machine::machine::{box_of, box_resource};

    /// Whether `machine_id` is no ID, so it's left out.
    pub fn unknown(machine_id: &str) -> bool {
        !link_id::is_candidate(machine_id)
    }

    pub fn serialize<S: Serializer>(machine_id: &str, serializer: S) -> Result<S::Ok, S::Error> {
        match link_id::is_candidate(machine_id) {
            true => serializer.serialize_str(&box_resource(machine_id)),
            false => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        match Option::<String>::deserialize(deserializer)? {
            None => Ok(String::new()),
            Some(text) => box_of(&text)
                .map(str::to_string)
                .ok_or_else(|| serde::de::Error::custom(format!("{text:?} isn't box:<id>"))),
        }
    }
}

impl Listed {
    /// Whether this computer made it. Any other is yours by its tags
    /// alone, which anyone who can tag machines in the account can set.
    pub fn made_here(&self) -> bool {
        self.created != 0
    }

    /// Its installation ID, when it's known and one.
    pub fn machine(&self) -> Option<String> {
        Some(self.machine_id.clone()).filter(|m| link_machine::machine::check_id(m).is_ok())
    }

    /// Its ID as output writes it, `box:<installation ID>`, when it's known.
    pub fn resource(&self) -> Option<String> {
        self.machine()
            .as_deref()
            .map(link_machine::machine::box_resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_machine_id_is_written_as_a_box_resource() {
        let listed = Listed {
            name: "aws-1".into(),
            machine_id: "a1b2c3d4e5f6".into(),
            ..Default::default()
        };
        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(json["resource"], "box:a1b2c3d4e5f6");
        assert!(json.get("machine_id").is_none());
        assert_eq!(serde_json::from_value::<Listed>(json).unwrap(), listed);
        let unknown = serde_json::to_value(Listed::default()).unwrap();
        assert!(unknown["resource"].is_null());
        assert_eq!(
            serde_json::from_value::<Listed>(unknown)
                .unwrap()
                .machine_id,
            ""
        );
        let tagged = Listed {
            machine_id: "not an id".into(),
            ..Default::default()
        };
        assert!(serde_json::to_value(&tagged).unwrap()["resource"].is_null());
        let bare = serde_json::json!({"name": "aws-1", "resource": "a1b2c3d4e5f6"});
        assert!(serde_json::from_value::<Listed>(bare).is_err());
    }
}
