//! A whole JSON answer scanned for its top-level usage, which is kept;
//! nothing else is.

use super::*;

/// The keys of a whole answer whose values are kept: its usage
/// (Ollama's counts are at its top).
const KEPT: [&[u8]; 3] = [b"usage", b"prompt_eval_count", b"eval_count"];

/// A whole JSON answer, scanned byte by byte for its top-level
/// [`KEPT`] keys' values, which are kept; nothing else is.
#[derive(Debug, Default)]
pub(super) struct Scan {
    depth: u32,
    in_string: bool,
    escaped: bool,
    /// At the top, the next string is a key.
    expect_key: bool,
    /// The key being read, while it may be one of [`KEPT`].
    key: Option<Vec<u8>>,
    /// The last key read, if it's one of [`KEPT`].
    named: Option<&'static [u8]>,
    /// The value being kept, under its key.
    value: Option<(&'static [u8], Vec<u8>)>,
    /// What's kept, as JSON members: at most one each of [`KEPT`].
    kept: Vec<(&'static [u8], Vec<u8>)>,
    /// Not JSON as expected: nothing more is read.
    broken: bool,
}

impl Scan {
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.broken {
                return;
            }
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        // The value being kept takes every byte up to the `,` or `}` that
        // ends it at the top.
        let ends_value = !self.in_string && self.depth == 1 && (b == b',' || b == b'}');
        if let Some((_, value)) = &mut self.value {
            if ends_value {
                // One value a key, the last: an answer naming a key over
                // and over never grows what's kept.
                let (key, value) = self.value.take().expect("kept");
                self.kept.retain(|(k, _)| *k != key);
                self.kept.push((key, value));
            } else {
                value.push(b);
                if value.len() > MAX_USAGE {
                    self.value = None;
                }
            }
        }
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if b == b'\\' {
                self.escaped = true;
            } else if b == b'"' {
                self.in_string = false;
                if let Some(key) = self.key.take() {
                    self.named = KEPT.iter().find(|k| **k == key.as_slice()).copied();
                }
                return;
            }
            if let Some(key) = &mut self.key {
                key.push(b);
                if key.len() > 32 {
                    self.key = None;
                }
            }
            return;
        }
        match b {
            b'"' => {
                self.in_string = true;
                if self.depth == 1 && self.expect_key {
                    self.key = Some(Vec::new());
                    self.expect_key = false;
                }
            }
            b'{' | b'[' => {
                if self.depth == 0 && b == b'[' {
                    self.broken = true;
                }
                self.depth += 1;
                if self.depth == 1 {
                    self.expect_key = true;
                }
            }
            b'}' | b']' => {
                if self.depth == 0 {
                    self.broken = true;
                    return;
                }
                self.depth -= 1;
            }
            b',' if self.depth == 1 => self.expect_key = true,
            b':' if self.depth == 1 => {
                if let Some(key) = self.named.take() {
                    self.value = Some((key, Vec::new()));
                }
            }
            _ => {}
        }
    }

    /// The kept values, as one JSON object, if any.
    pub(super) fn usage(&mut self) -> Option<Vec<u8>> {
        if self.kept.is_empty() {
            return None;
        }
        let mut json = b"{".to_vec();
        for (i, (key, value)) in self.kept.iter().enumerate() {
            if i > 0 {
                json.push(b',');
            }
            json.push(b'"');
            json.extend_from_slice(key);
            json.extend_from_slice(b"\":");
            json.extend_from_slice(value);
        }
        json.push(b'}');
        Some(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A whole answer naming its usage over and over keeps one value of
    /// it, never one a time: what's kept stays bounded.
    #[test]
    fn a_key_given_over_and_over_is_kept_once() {
        let mut scan = Scan::default();
        scan.feed(b"{");
        for _ in 0..100_000 {
            scan.feed(br#""usage":{"input_tokens":1},"eval_count":2,"#);
        }
        scan.feed(br#""usage":{"input_tokens":3}}"#);
        assert_eq!(scan.kept.len(), 2);
        let kept: serde_json::Value = serde_json::from_slice(&scan.usage().unwrap()).unwrap();
        assert_eq!(kept, json!({"eval_count": 2, "usage": {"input_tokens": 3}}));
    }
}
