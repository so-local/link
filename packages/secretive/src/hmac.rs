//! The one HMAC (SHA-256, on `ring`) every signature Link makes or checks
//! is, and the one hex a signature is written in: an agent's proof, a
//! bridge's, a webhook's ([`crate::inbound`]). No other workspace signs or
//! decodes a signature itself.

use subtle::ConstantTimeEq;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// HMAC-SHA256 of `message` under `key`.
pub fn sign(key: &[u8], message: &[u8]) -> [u8; 32] {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let mut out = [0u8; 32];
    out.copy_from_slice(ring::hmac::sign(&key, message).as_ref());
    out
}

/// Whether `tag` is [`sign`] of `message` under `key`, compared in
/// constant time; never under an empty key, which signs nothing anyone
/// should trust.
pub fn verify(key: &[u8], message: &[u8], tag: &[u8]) -> bool {
    !key.is_empty() && bool::from(sign(key, message).as_slice().ct_eq(tag))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| {
            [
                HEX[(b >> 4) as usize] as char,
                HEX[(b & 15) as usize] as char,
            ]
        })
        .collect()
}

/// The bytes `hex` spells, either case; none unless every character is a
/// hex digit and there's an even number of them.
pub fn unhex(hex: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let hex = hex.as_bytes();
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_signs_as_rfc_4231_says() {
        // RFC 4231, test case 2.
        let tag = sign(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&tag),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(verify(b"Jefe", b"what do ya want for nothing?", &tag));
        assert!(!verify(b"Jefe", b"what do ya want for nothing!", &tag));
        assert!(!verify(
            b"Jefe",
            b"what do ya want for nothing?",
            &tag[..31]
        ));
        assert!(!verify(b"", b"", &sign(b"", b"")));
    }

    #[test]
    fn hex_goes_both_ways_and_refuses_what_isnt() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(unhex("000fA5ff"), Some(vec![0x00, 0x0f, 0xa5, 0xff]));
        assert_eq!(unhex(""), Some(vec![]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
        assert_eq!(unhex("+1"), None);
        assert_eq!(unhex("é0"), None);
    }
}
