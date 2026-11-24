//! Without `compact`, a conversation kept compact is refused.

use anyhow::bail;
use serde_json::Value;

use super::super::Reply;
use super::Dir;

pub fn thread(_: &Dir) -> Option<String> {
    None
}

pub fn read(_: &Dir, _: u64) -> anyhow::Result<Vec<Value>> {
    bail!("a conversation kept compact: this driver was built without compact")
}

pub fn last(_: &Dir) -> anyhow::Result<u64> {
    bail!("a conversation kept compact: this driver was built without compact")
}

pub fn append(_: &Dir, _: u64, _: &[u8]) -> anyhow::Result<Reply> {
    bail!("a conversation kept compact: this driver was built without compact")
}
