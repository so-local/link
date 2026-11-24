//! Bounded, recipient-sealed streams. A fresh ChaCha20-Poly1305 secret
//! is HPKE sealed in the header; numbered records authenticate their
//! length and order, including a required empty final record. A failed
//! writer never manufactures a successful end.

use std::io::{self, Read, Write};

use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};

use crate::{About, Recipient, SealTo, Sealed, Secret};

/// The most plaintext held for one record.
pub const CHUNK: usize = 64 * 1024;
const MAGIC: &[u8; 8] = b"LNKSEAL1";
const HEADER_MAX: usize = 4096;
const TAG: usize = 16;

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "sealed stream refused: wrong recipient, damaged or incomplete",
    )
}

fn cipher(bytes: &Secret<Vec<u8>>) -> io::Result<aead::LessSafeKey> {
    aead::UnboundKey::new(&aead::CHACHA20_POLY1305, bytes.expose())
        .map(aead::LessSafeKey::new)
        .map_err(|_| refused())
}

fn nonce(number: u64) -> aead::Nonce {
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(&number.to_be_bytes());
    aead::Nonce::assume_unique_for_key(nonce)
}

fn aad(number: u64, length: u32) -> [u8; 12] {
    let mut aad = [0; 12];
    aad[..8].copy_from_slice(&number.to_be_bytes());
    aad[8..].copy_from_slice(&length.to_be_bytes());
    aad
}

/// A stream's sender. Call `finish` only after the complete payload has
/// been written successfully; dropping it leaves an incomplete stream.
pub struct Sender<W> {
    out: W,
    cipher: aead::LessSafeKey,
    number: u64,
    pending: Secret<Vec<u8>>,
    failed: bool,
}

impl<W: Write> Sender<W> {
    pub fn new(mut out: W, to: &SealTo, about: About) -> io::Result<Self> {
        let mut bytes = Secret::new(vec![0; 32]);
        SystemRandom::new()
            .fill(bytes.expose_mut())
            .map_err(|_| refused())?;
        let cipher = cipher(&bytes)?;
        let sealed = Sealed::seal(&bytes, to, about).map_err(|_| refused())?;
        let header = serde_json::to_vec(&sealed).map_err(|_| refused())?;
        if header.len() > HEADER_MAX {
            return Err(refused());
        }
        out.write_all(MAGIC)?;
        out.write_all(&(header.len() as u32).to_be_bytes())?;
        out.write_all(&header)?;
        Ok(Self {
            out,
            cipher,
            number: 0,
            pending: Secret::new(Vec::with_capacity(CHUNK + TAG)),
            failed: false,
        })
    }

    fn record(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(refused());
        }
        // Reserve the nonce before attempting any I/O: retrying a failed
        // write must never encrypt another plaintext under the same nonce.
        let number = self.number;
        self.number = number.checked_add(1).ok_or_else(refused)?;
        self.failed = true;
        let length = self.pending.expose().len() as u32;
        self.cipher
            .seal_in_place_append_tag(
                nonce(number),
                aead::Aad::from(aad(number, length)),
                self.pending.expose_mut(),
            )
            .map_err(|_| refused())?;
        self.out.write_all(&length.to_be_bytes())?;
        self.out.write_all(self.pending.expose())?;
        // Clear through the secret wrapper, so consumed plaintext and tags
        // are wiped before the allocation is reused.
        self.pending = Secret::new(Vec::with_capacity(CHUNK + TAG));
        self.failed = false;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<W> {
        if !self.pending.expose().is_empty() {
            self.record()?;
        }
        self.record()?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for Sender<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(refused());
        }
        let size = bytes.len().min(CHUNK - self.pending.expose().len());
        self.pending.expose_mut().extend_from_slice(&bytes[..size]);
        if self.pending.expose().len() == CHUNK {
            self.record()?;
        }
        Ok(size)
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.expose().is_empty() {
            self.record()?;
        }
        self.out.flush()
    }
}

/// A stream's receiver. Only authenticated records become readable.
/// Reading to EOF also verifies the final record and refuses trailing data.
pub struct Receiver<R> {
    input: R,
    cipher: aead::LessSafeKey,
    number: u64,
    plain: Secret<Vec<u8>>,
    at: usize,
    ended: bool,
    failed: bool,
}

impl<R: Read> Receiver<R> {
    pub fn new(mut input: R, recipient: &Recipient, about: &About) -> io::Result<Self> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(refused());
        }
        let mut length = [0; 4];
        input.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > HEADER_MAX {
            return Err(refused());
        }
        let mut header = vec![0; length];
        input.read_exact(&mut header)?;
        let sealed: Sealed<Secret<Vec<u8>>> =
            serde_json::from_slice(&header).map_err(|_| refused())?;
        let bytes = recipient
            .open_sealed(&sealed, about)
            .map_err(|_| refused())?;
        Ok(Self {
            input,
            cipher: cipher(&bytes)?,
            number: 0,
            plain: Secret::default(),
            at: 0,
            ended: false,
            failed: false,
        })
    }

    fn record(&mut self) -> io::Result<()> {
        self.failed = true;
        let mut length = [0; 4];
        self.input.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length);
        if length as usize > CHUNK {
            return Err(refused());
        }
        self.plain = Secret::new(vec![0; length as usize + TAG]);
        self.input.read_exact(self.plain.expose_mut())?;
        self.cipher
            .open_in_place(
                nonce(self.number),
                aead::Aad::from(aad(self.number, length)),
                self.plain.expose_mut(),
            )
            .map_err(|_| refused())?;
        self.number = self.number.checked_add(1).ok_or_else(refused)?;
        self.plain.expose_mut().truncate(length as usize);
        self.at = 0;
        if length == 0 {
            let mut extra = [0];
            if self.input.read(&mut extra)? != 0 {
                return Err(refused());
            }
            self.ended = true;
        }
        self.failed = false;
        Ok(())
    }
}

impl<R: Read> Read for Receiver<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if into.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(refused());
        }
        if self.at == self.plain.expose().len() && !self.ended {
            self.record()?;
        }
        let size = into.len().min(self.plain.expose().len() - self.at);
        into[..size].copy_from_slice(&self.plain.expose()[self.at..self.at + size]);
        self.at += size;
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about() -> About {
        About {
            owner: "transfer".into(),
            kind: "state".into(),
            version: 1,
        }
    }
    fn sealed(recipient: &Recipient) -> Vec<u8> {
        let mut out = Sender::new(Vec::new(), recipient.seal_to(), about()).unwrap();
        out.write_all(b"device and harness secrets").unwrap();
        out.finish().unwrap()
    }
    fn open(bytes: &[u8], recipient: &Recipient) -> io::Result<Vec<u8>> {
        let mut input = Receiver::new(bytes, recipient, &about())?;
        let mut plain = Vec::new();
        input.read_to_end(&mut plain)?;
        Ok(plain)
    }

    #[test]
    fn only_the_recipient_reads_a_complete_untampered_stream() {
        let recipient = Recipient::new();
        let bytes = sealed(&recipient);
        assert!(!bytes.windows(7).any(|w| w == b"secrets"));
        assert_eq!(
            open(&bytes, &recipient).unwrap(),
            b"device and harness secrets"
        );
        assert!(open(&bytes, &Recipient::new()).is_err());
        for end in 0..bytes.len() {
            assert!(open(&bytes[..end], &recipient).is_err(), "cut at {end}");
        }
        let mut bent = bytes.clone();
        let end = bent.len() - TAG - 5;
        bent[end] ^= 1;
        assert!(open(&bent, &recipient).is_err());
        let mut extra = bytes;
        extra.push(0);
        assert!(open(&extra, &recipient).is_err());
    }

    #[test]
    fn large_streams_hold_only_one_record_and_refuse_reordering() {
        let recipient = Recipient::new();
        let mut out = Sender::new(Vec::new(), recipient.seal_to(), about()).unwrap();
        let header = out.out.len();
        for n in 0..1024 {
            out.write_all(&vec![n as u8; CHUNK]).unwrap();
            assert!(out.pending.expose().len() <= CHUNK);
        }
        let mut bytes = out.finish().unwrap();
        let mut input = Receiver::new(&bytes[..], &recipient, &about()).unwrap();
        assert_eq!(
            io::copy(&mut input, &mut io::sink()).unwrap(),
            (1024 * CHUNK) as u64
        );
        let size = 4 + CHUNK + TAG;
        let (first, rest) = bytes[header..].split_at_mut(size);
        first.swap_with_slice(&mut rest[..size]);
        assert!(open(&bytes, &recipient).is_err());
    }

    #[test]
    fn a_failed_writer_cannot_retry_a_nonce_or_finish() {
        struct Fails {
            allowed: usize,
        }
        impl Write for Fails {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.allowed == 0 {
                    return Err(io::Error::other("interrupted"));
                }
                let size = bytes.len().min(self.allowed);
                self.allowed -= size;
                Ok(size)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let recipient = Recipient::new();
        let mut out = Sender::new(Fails { allowed: 8192 }, recipient.seal_to(), about()).unwrap();
        assert!(out.write_all(&vec![7; CHUNK]).is_err());
        let used = out.number;
        assert!(out.write_all(b"retry with different plaintext").is_err());
        assert_eq!(out.number, used);
        assert!(out.finish().is_err());
    }
}
