//! SHA-256, the one hash Link names things and checks downloads by: of
//! bytes held ([`sha256`]), of what a reader reads ([`sha256_of`]), as
//! lowercase hex, or as its 32 bytes ([`sha256_bytes`]); and the
//! identity of typed content ([`ContentId`], [`content_digest`]).

use std::io::{self, Read};

mod content;
pub use content::{ContentId, content_digest};

/// The SHA-256 of `bytes`, in lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    hex(&sha256_bytes(bytes))
}

/// The SHA-256 of `bytes`, its 32 bytes.
pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut out = [0; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// The SHA-256 of what `input` reads, in lowercase hex.
pub fn sha256_of(mut input: impl Read) -> io::Result<String> {
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let n = match input.read(&mut buffer) {
            // Interrupted by a signal: tried again.
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            n => n?,
        };
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(hex(context.finish().as_ref()))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTHING: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn of_bytes_as_the_standard_says() {
        assert_eq!(sha256(b""), NOTHING);
        assert_eq!(sha256(b"abc"), ABC);
        assert_eq!(hex(&sha256_bytes(b"abc")), ABC);
    }

    #[test]
    fn of_a_reader_as_of_its_bytes_across_its_reads() {
        assert_eq!(sha256_of(io::empty()).unwrap(), NOTHING);
        assert_eq!(sha256_of(&b"abc"[..]).unwrap(), ABC);
        // Longer than one read's buffer, and read a byte at a time.
        let long: Vec<u8> = (0..200_000u32).map(|n| n as u8).collect();
        assert_eq!(sha256_of(long.as_slice()).unwrap(), sha256(&long));
        struct Dribble<'a>(&'a [u8]);
        impl Read for Dribble<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = self.0.len().min(1).min(out.len());
                out[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        assert_eq!(sha256_of(Dribble(b"abc")).unwrap(), ABC);
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
