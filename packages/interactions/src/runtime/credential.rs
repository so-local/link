//! The binding credential is serialized only into the protected coordinator store.
use link_secretive::Secret;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(super) fn serialize<S: serde::Serializer>(
    secret: &Arc<Secret>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    secret.expose().serialize(serializer)
}

pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Secret>, D::Error> {
    Secret::deserialize(deserializer).map(Arc::new)
}
