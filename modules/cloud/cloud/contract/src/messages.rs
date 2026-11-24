//! What a command reads and prints: its [`Request`] and its [`Reply`],
//! with what a reply finds (boxes, images, buckets).

use super::*;

/// One adapter command's input, generated from the contract's
/// declarations (`types.toml`).
pub use crate::declared::Request;

#[expect(
    clippy::derivable_impls,
    reason = "the type is generated, so derives nothing"
)]
impl Default for Request {
    fn default() -> Self {
        Self {
            account: Account::default(),
            bucket: None,
            expires: None,
            host_keys: None,
            id: None,
            image: None,
            labels: BTreeMap::new(),
            object: None,
            objects: None,
            owner: None,
            place: None,
            places: None,
            public_key: None,
            spec: None,
            tags: BTreeMap::new(),
        }
    }
}

impl Duplicate for Request {
    fn duplicate(&self) -> Self {
        Self {
            account: self.account.duplicate(),
            bucket: self.bucket.clone(),
            expires: self.expires,
            host_keys: self.host_keys.clone(),
            id: self.id.clone(),
            image: self.image.clone(),
            labels: self.labels.clone(),
            object: self.object.clone(),
            objects: self.objects.clone(),
            owner: self.owner.clone(),
            place: self.place.clone(),
            places: self.places.clone(),
            public_key: self.public_key.clone(),
            spec: self.spec.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// The most objects one `delete-objects` takes.
pub const MAX_DELETE_OBJECTS: usize = 1000;

/// What a reply finds: an image, a bucket and a box, generated from the
/// contract's declarations.
pub use crate::declared::{Found, FoundBucket, FoundImage};

impl Default for Found {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({"id": "", "name": "", "place": "", "state": ""}))
            .expect("the declared defaults")
    }
}

impl Default for FoundImage {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({"id": "", "name": "", "state": ""}))
            .expect("the declared defaults")
    }
}

/// One adapter command's output, generated from the contract's
/// declarations.
pub use crate::declared::Reply;

impl Duplicate for Reply {
    fn duplicate(&self) -> Self {
        Self {
            attachments: self.attachments,
            boxes: self.boxes.clone(),
            buckets: self.buckets.clone(),
            contents: self.contents.clone(),
            denied: self.denied.clone(),
            env: self.env.duplicate(),
            host_keys: self.host_keys.clone(),
            id: self.id.clone(),
            images: self.images.clone(),
            ip: self.ip.clone(),
            credential: self.credential.duplicate(),
            message: self.message.clone(),
            place: self.place.clone(),
            rates: self.rates.clone(),
            size: self.size.clone(),
            sizes: self.sizes.clone(),
            state: self.state.clone(),
            stopped_by_cloud: self.stopped_by_cloud,
            temporary: self.temporary,
            upload: self.upload.duplicate(),
            writes: self.writes,
        }
    }
}

/// A list a message carries: none when it's empty, as the wire leaves an
/// empty list out.
pub fn carried<T>(list: Vec<T>) -> Option<Vec<T>> {
    (!list.is_empty()).then_some(list)
}

/// Text a message carries: none when it's empty, as the wire leaves empty
/// text out.
pub fn said(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// What a list left out of a message reads as: none of it.
fn listed<T>(list: &Option<Vec<T>>) -> &[T] {
    list.as_deref().unwrap_or_default()
}

impl Request {
    /// `list`: where to look; none, everywhere the account may.
    pub fn places(&self) -> &[String] {
        listed(&self.places)
    }
    /// `save-hostkeys`: `<type> <key>` lines.
    pub fn host_keys(&self) -> &[String] {
        listed(&self.host_keys)
    }
    /// `delete-objects`: the objects to delete.
    pub fn objects(&self) -> &[String] {
        listed(&self.objects)
    }
}

impl Reply {
    /// `hostkeys`: `<type> <key>` lines.
    pub fn host_keys(&self) -> &[String] {
        listed(&self.host_keys)
    }
    /// `list`: the boxes found.
    pub fn boxes(&self) -> &[Found] {
        listed(&self.boxes)
    }
    /// `list`: the places skipped, denied there.
    pub fn denied(&self) -> &[String] {
        listed(&self.denied)
    }
    /// `buckets`: those found.
    pub fn buckets(&self) -> &[FoundBucket] {
        listed(&self.buckets)
    }
    /// `sizes`: the machine types a box can be there.
    pub fn sizes(&self) -> &[MachineSize] {
        listed(&self.sizes)
    }
    /// `image-list`: the images found; `image-save`: the one saved.
    pub fn images(&self) -> &[FoundImage] {
        listed(&self.images)
    }
    /// `add-key`: whether the key works for a minute only.
    pub fn temporary(&self) -> bool {
        self.temporary.unwrap_or_default()
    }
}

impl Found {
    /// Its `lnk-owner` tag; empty when the adapter reports none.
    pub fn owner(&self) -> &str {
        self.owner.as_deref().unwrap_or_default()
    }
}

impl FoundImage {
    /// When it was made, as the cloud says it; empty when it doesn't.
    pub fn created(&self) -> &str {
        self.created.as_deref().unwrap_or_default()
    }
    /// Its hash tag; empty for an image saved without one.
    pub fn hash(&self) -> &str {
        self.hash.as_deref().unwrap_or_default()
    }
}
