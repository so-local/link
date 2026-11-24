//! rclone's crypt format, one object at a time: what Home is in a bucket
//! when it's kept behind a key of its own. Each object's contents and
//! each part of its name are encrypted as rclone's `crypt` remote
//! encrypts them, with its standard name encryption and its default
//! salt, from a password, so a bucket written here is read by rclone
//! given the same password, and the other way round:
//!
//! - **The keys**: scrypt (N 16384, r 8, p 1) of the password, 80 bytes:
//!   the contents' key, the names' key, and the names' tweak.
//! - **The contents**: `RCLONE\0\0`, a random 24-byte nonce, then the
//!   plaintext in blocks of 64 KiB, each sealed with NaCl's secretbox
//!   (XSalsa20 and Poly1305, its 16-byte tag first) under the nonce,
//!   which counts up by one, little-endian, after each.
//! - **The names**: each part between `/` padded (PKCS#7) to whole
//!   16-byte blocks, encrypted with EME over AES-256 and the tweak, and
//!   written in base32hex, lowercase, unpadded.
//!
//! Nothing here holds a key past the [`Cipher`] made from it, and
//! nothing prints one.

use aes::Aes256;
use aes::cipher::{Block, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use anyhow::{Context, bail, ensure};
use link_secretive::Secret;
use poly1305::Poly1305;
use salsa20::XSalsa20;
use salsa20::cipher::{KeyIvInit, StreamCipher};
use std::io::Read;

/// What every object starts with.
const MAGIC: &[u8; 8] = b"RCLONE\0\0";

/// The nonce after it.
const NONCE: usize = 24;

/// The header: [`MAGIC`] and the nonce.
pub const HEADER: usize = MAGIC.len() + NONCE;

/// A block of plaintext, and what sealing adds to it.
pub const BLOCK: usize = 64 << 10;
const TAG: usize = 16;

/// rclone's salt when no second password is set.
const SALT: [u8; 16] = [
    0xA8, 0x0D, 0xF4, 0x3A, 0x8F, 0xBD, 0x03, 0x08, 0xA7, 0xCA, 0xB8, 0x3E, 0x58, 0x1F, 0x86, 0xB1,
];

/// NaCl's secretbox, as rclone seals each block: XSalsa20's first 32
/// bytes of stream key Poly1305, the rest encrypt, and the 16-byte tag
/// goes first. Never printed, and wiped when dropped.
struct SecretBox(Secret<[u8; 32]>);

impl SecretBox {
    /// The stream past its first 32 bytes, and the authenticator they key.
    fn start(&self, nonce: &[u8; NONCE]) -> (XSalsa20, Poly1305) {
        let mut stream = XSalsa20::new(self.0.expose().into(), nonce.into());
        let mut key = Secret::new([0u8; 32]);
        stream.apply_keystream(key.expose_mut());
        let mac = Poly1305::new(key.expose().into());
        (stream, mac)
    }

    /// `plain`, sealed: the tag, then the ciphertext.
    fn seal(&self, nonce: &[u8; NONCE], plain: &[u8]) -> Vec<u8> {
        let (mut stream, mac) = self.start(nonce);
        let mut out = vec![0u8; TAG + plain.len()];
        out[TAG..].copy_from_slice(plain);
        stream.apply_keystream(&mut out[TAG..]);
        let tag = mac.compute_unpadded(&out[TAG..]);
        out[..TAG].copy_from_slice(&tag);
        out
    }

    /// What `sealed` holds, or nothing when its tag doesn't match.
    fn open(&self, nonce: &[u8; NONCE], sealed: &[u8]) -> Option<Vec<u8>> {
        let (tag, body) = sealed.split_at_checked(TAG)?;
        let (mut stream, mac) = self.start(nonce);
        let expected = mac.compute_unpadded(body);
        // In constant time: a tag is never compared byte by byte.
        if !bool::from(subtle::ConstantTimeEq::ct_eq(&expected[..], tag)) {
            return None;
        }
        let mut out = body.to_vec();
        stream.apply_keystream(&mut out);
        Some(out)
    }
}

/// base32hex, lowercase, as rclone writes names.
const BASE32: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

/// The keys made from one password. Never printed.
pub struct Cipher {
    data: SecretBox,
    names: Aes256,
    tweak: [u8; 16],
}

impl std::fmt::Debug for Cipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cipher { .. }")
    }
}

impl Cipher {
    /// The keys rclone makes from `password` with its default salt. An
    /// empty password is refused: rclone would use keys of zeros.
    pub fn new(password: &str) -> anyhow::Result<Self> {
        ensure!(!password.is_empty(), "a key for Home can't be empty");
        let params = scrypt::Params::new(14, 8, 1).context("scrypt's parameters")?;
        let mut key = Secret::new([0u8; 80]);
        scrypt::scrypt(password.as_bytes(), &SALT, &params, key.expose_mut())
            .context("making Home's keys")?;
        let data = SecretBox(key.expose_map(|k| {
            let mut data = [0u8; 32];
            data.copy_from_slice(&k[..32]);
            data
        }));
        let key = key.expose();
        let names = Aes256::new_from_slice(&key[32..64]).expect("a name key is 32 bytes");
        let mut tweak = [0u8; 16];
        tweak.copy_from_slice(&key[64..80]);
        Ok(Self { data, names, tweak })
    }

    /// Encrypts a reader in rclone's 64 KiB blocks, with bounded memory.
    pub fn encrypt_to(
        &self,
        input: &mut dyn std::io::Read,
        out: &mut dyn std::io::Write,
    ) -> anyhow::Result<()> {
        let mut nonce = [0u8; NONCE];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("no random nonce"))?;
        out.write_all(MAGIC)?;
        out.write_all(&nonce)?;
        loop {
            let mut block = Vec::with_capacity(BLOCK);
            (&mut *input).take(BLOCK as u64).read_to_end(&mut block)?;
            if block.is_empty() {
                break;
            }
            let sealed = self.data.seal(&nonce, block.as_slice());
            out.write_all(&sealed)?;
            increment(&mut nonce);
        }
        Ok(())
    }
    /// Opens one block at a time. A damaged block is refused before it is written.
    pub fn decrypt_to(
        &self,
        input: &mut dyn std::io::Read,
        out: &mut dyn std::io::Write,
    ) -> anyhow::Result<()> {
        let mut header = [0; HEADER];
        input.read_exact(&mut header)?;
        ensure!(
            &header[..MAGIC.len()] == MAGIC,
            "not an encrypted object of Home's"
        );
        let mut nonce = [0; NONCE];
        nonce.copy_from_slice(&header[MAGIC.len()..]);
        loop {
            let mut block = Vec::with_capacity(BLOCK + TAG);
            (&mut *input)
                .take((BLOCK + TAG) as u64)
                .read_to_end(&mut block)?;
            if block.is_empty() {
                break;
            }
            ensure!(
                block.len() > TAG,
                "an encrypted object of Home's is cut short"
            );
            let opened = self
                .data
                .open(&nonce, block.as_slice())
                .ok_or_else(|| anyhow::anyhow!("an object of Home's doesn't open with its key"))?;
            out.write_all(&opened)?;
            increment(&mut nonce);
        }
        Ok(())
    }

    /// `plain` encrypted, under a new random nonce.
    pub fn encrypt(&self, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("no random nonce"))?;
        self.encrypt_with(plain, nonce)
    }

    /// `plain` encrypted under `nonce`: [`Cipher::encrypt`]'s, for tests
    /// that need the same bytes twice.
    pub fn encrypt_with(&self, plain: &[u8], mut nonce: [u8; NONCE]) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(encrypted_size(plain.len() as u64) as usize);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        for block in plain.chunks(BLOCK) {
            let sealed = self.data.seal(&nonce, block);
            out.extend_from_slice(&sealed);
            increment(&mut nonce);
        }
        Ok(out)
    }

    /// What `sealed`, an object as [`Cipher::encrypt`] or rclone writes
    /// one, holds; an error for one that isn't, was changed, or is cut
    /// short inside a block. (As in rclone, an object cut at a block's
    /// end opens as its first blocks: an object is written whole, and
    /// what's read is checked against the size listed.)
    pub fn decrypt(&self, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
        ensure!(
            sealed.len() >= HEADER && sealed[..MAGIC.len()] == *MAGIC,
            "not an encrypted object of Home's"
        );
        let mut nonce = [0u8; NONCE];
        nonce.copy_from_slice(&sealed[MAGIC.len()..HEADER]);
        let body = &sealed[HEADER..];
        let mut out = Vec::with_capacity(body.len());
        for block in body.chunks(BLOCK + TAG) {
            ensure!(
                block.len() > TAG,
                "an encrypted object of Home's is cut short"
            );
            let opened = self
                .data
                .open(&nonce, block)
                .ok_or_else(|| anyhow::anyhow!("an object of Home's doesn't open with its key"))?;
            out.extend_from_slice(&opened);
            increment(&mut nonce);
        }
        Ok(out)
    }

    /// `name`, each part between `/` encrypted; empty parts stay empty.
    pub fn encrypt_name(&self, name: &str) -> String {
        name.split('/')
            .map(|part| self.encrypt_part(part))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// What [`Cipher::encrypt_name`] made `name` from; an error for a
    /// part that isn't an encrypted name of this key's.
    pub fn decrypt_name(&self, name: &str) -> anyhow::Result<String> {
        let parts: anyhow::Result<Vec<String>> = name
            .split('/')
            .map(|part| self.decrypt_part(part))
            .collect();
        Ok(parts?.join("/"))
    }

    fn encrypt_part(&self, part: &str) -> String {
        if part.is_empty() {
            return String::new();
        }
        let pad = 16 - part.len() % 16;
        let mut padded = part.as_bytes().to_vec();
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        base32(&eme(&self.names, &self.tweak, &padded, true))
    }

    fn decrypt_part(&self, part: &str) -> anyhow::Result<String> {
        if part.is_empty() {
            return Ok(String::new());
        }
        let bytes = unbase32(part).context("not an encrypted name")?;
        ensure!(
            !bytes.is_empty() && bytes.len() % 16 == 0 && bytes.len() <= 2048,
            "not an encrypted name"
        );
        let mut plain = eme(&self.names, &self.tweak, &bytes, false);
        let pad = *plain.last().unwrap_or(&0) as usize;
        ensure!(
            (1..=16).contains(&pad)
                && plain[plain.len() - pad..]
                    .iter()
                    .all(|b| *b as usize == pad),
            "not an encrypted name of this key's"
        );
        plain.truncate(plain.len() - pad);
        String::from_utf8(plain).context("not an encrypted name of this key's")
    }
}

/// How large `plain` bytes are encrypted.
pub fn encrypted_size(plain: u64) -> u64 {
    let blocks = plain.div_ceil(BLOCK as u64);
    HEADER as u64 + plain + blocks * TAG as u64
}

/// How large an object of `sealed` bytes is decrypted; none for a size
/// no object has.
pub fn decrypted_size(sealed: u64) -> Option<u64> {
    let body = sealed.checked_sub(HEADER as u64)?;
    let blocks = body.div_ceil((BLOCK + TAG) as u64);
    let last = body - (blocks.saturating_sub(1)) * (BLOCK + TAG) as u64;
    (blocks == 0 || last > TAG as u64).then(|| body - blocks * TAG as u64)
}

/// The nonce after `nonce`: one more, little-endian.
fn increment(nonce: &mut [u8; NONCE]) {
    for byte in nonce.iter_mut() {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            return;
        }
    }
}

fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut buffer, mut bits) = (0u32, 0);
    for byte in bytes {
        buffer = (buffer << 8) | *byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let Some(value) = BASE32.iter().position(|b| *b == c.to_ascii_lowercase()) else {
            bail!("not base32hex");
        };
        buffer = (buffer << 5) | value as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    // What's left over is padding: zeros, fewer than a byte.
    ensure!(bits < 5 && buffer & ((1 << bits) - 1) == 0, "not base32hex");
    Ok(out)
}

/// EME (Halevi and Rogaway's ECB-mix-ECB), as rclone's names use it:
/// `data`, whole 16-byte blocks, encrypted (or with `forward` false,
/// decrypted) under `aes` and `tweak`.
fn eme(aes: &Aes256, tweak: &[u8; 16], data: &[u8], forward: bool) -> Vec<u8> {
    let m = data.len() / 16;
    let block = |out: &mut [u8], input: &[u8]| {
        let mut b = Block::<Aes256>::default();
        b.copy_from_slice(input);
        match forward {
            true => aes.encrypt_block(&mut b),
            false => aes.decrypt_block(&mut b),
        }
        out.copy_from_slice(&b);
    };
    // L_j: 2^j times the encryption of zeros.
    let mut table = Vec::with_capacity(m);
    let mut l = Block::<Aes256>::default();
    aes.encrypt_block(&mut l);
    let mut l: [u8; 16] = l.into();
    for _ in 0..m {
        l = times_two(&l);
        table.push(l);
    }
    let mut c = vec![0u8; data.len()];
    for j in 0..m {
        let pp = xor(&data[j * 16..(j + 1) * 16], &table[j]);
        block(&mut c[j * 16..(j + 1) * 16], &pp);
    }
    let mut mp = xor(&c[..16], tweak);
    for j in 1..m {
        mp = xor(&mp, &c[j * 16..(j + 1) * 16]);
    }
    let mut mc = [0u8; 16];
    block(&mut mc, &mp);
    let mut mm = xor(&mp, &mc);
    for j in 1..m {
        mm = times_two(&mm);
        let ccc = xor(&c[j * 16..(j + 1) * 16], &mm);
        c[j * 16..(j + 1) * 16].copy_from_slice(&ccc);
    }
    let mut ccc1 = xor(&mc, tweak);
    for j in 1..m {
        ccc1 = xor(&ccc1, &c[j * 16..(j + 1) * 16]);
    }
    c[..16].copy_from_slice(&ccc1);
    for j in 0..m {
        let mut cc = [0u8; 16];
        block(&mut cc, &c[j * 16..(j + 1) * 16]);
        c[j * 16..(j + 1) * 16].copy_from_slice(&xor(&cc, &table[j]));
    }
    c
}

fn times_two(input: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = input[0].wrapping_mul(2);
    if input[15] >= 128 {
        out[0] ^= 135;
    }
    for j in 1..16 {
        out[j] = input[j].wrapping_mul(2) | u8::from(input[j - 1] >= 128);
    }
    out
}

fn xor(a: &[u8], b: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (o, (x, y)) in out.iter_mut().zip(a.iter().zip(b)) {
        *o = x ^ y;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nonce counts up little-endian, carrying across bytes and
    /// wrapping at its end.
    #[test]
    fn a_nonce_carries_across_bytes() {
        let mut nonce = [0u8; NONCE];
        nonce[0] = 0xff;
        increment(&mut nonce);
        assert_eq!(nonce[..2], [0, 1]);
        let mut nonce = [0xff; NONCE];
        nonce[NONCE - 1] = 0;
        increment(&mut nonce);
        assert_eq!(nonce[..NONCE - 1], [0; NONCE - 1]);
        assert_eq!(nonce[NONCE - 1], 1);
        let mut nonce = [0xff; NONCE];
        increment(&mut nonce);
        assert_eq!(nonce, [0; NONCE]);
    }

    /// The password the vectors in `testdata/rclone/` were made with:
    /// `rclone copy` of [`plain`]'s files to a crypt remote, rclone
    /// 1.75.1 (the version `lnk storage` pins), its standard name
    /// encryption and default salt.
    const PASSWORD: &str = "link-test-agent-key";

    /// Each file the vectors hold: its name, encrypted as rclone wrote it,
    /// and its plaintext.
    fn plain() -> Vec<(&'static str, &'static str, Vec<u8>)> {
        let report: Vec<u8> = (0..1800)
            .flat_map(|i| format!("Line {i} of the tax report, déjà vu.\n").into_bytes())
            .collect();
        vec![
            ("empty", "8pngj7nnbufljfvc11dllr2rcs", Vec::new()),
            (
                "memory/notes.md",
                "vnkjuiqaa23il5pa8fgmtqr5to/jhoj1ni738sc5isvhfs98r0bok",
                b"hello\n".to_vec(),
            ),
            (
                "work/block.bin",
                "g1pefk16f48m0inc5043joosv4/st4kj7aoafv1sot6qpq4dnh0tc",
                (0..65536u32).map(|i| ((i * 7 + 3) % 251) as u8).collect(),
            ),
            (
                "memory/tax/report 2025.txt",
                "vnkjuiqaa23il5pa8fgmtqr5to/9qgbfiokkma1hvreq1di5krk9s/urgk315ukr5o2msfum2jsnjm28",
                report,
            ),
            (
                "0123456789abcdef",
                "9h8uuucu8gmj9vj6i2cu5b7dd3ql7idj8ner4c9tpm875jm8edg0",
                b"sixteen".to_vec(),
            ),
        ]
    }

    fn vector(encrypted: &str) -> Vec<u8> {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/files/testdata/rclone");
        std::fs::read(dir.join(encrypted)).unwrap()
    }

    /// What rclone wrote opens here: every name and every file, the
    /// sizes as rclone's.
    #[test]
    fn rclones_objects_open_here() {
        let cipher = Cipher::new(PASSWORD).unwrap();
        for (name, encrypted, plain) in plain() {
            assert_eq!(cipher.encrypt_name(name), encrypted, "{name}");
            assert_eq!(cipher.decrypt_name(encrypted).unwrap(), name);
            let sealed = vector(encrypted);
            assert_eq!(
                sealed.len() as u64,
                encrypted_size(plain.len() as u64),
                "{name}"
            );
            assert_eq!(
                decrypted_size(sealed.len() as u64),
                Some(plain.len() as u64)
            );
            assert_eq!(cipher.decrypt(&sealed).unwrap(), plain, "{name}");
        }
    }

    /// What's written here is what rclone would write under the same
    /// nonce: its file's nonce, given, makes its bytes.
    #[test]
    fn objects_written_here_are_rclones() {
        let cipher = Cipher::new(PASSWORD).unwrap();
        for (name, encrypted, plain) in plain() {
            let sealed = vector(encrypted);
            let nonce: [u8; NONCE] = sealed[MAGIC.len()..HEADER].try_into().unwrap();
            assert_eq!(
                cipher.encrypt_with(&plain, nonce).unwrap(),
                sealed,
                "{name}"
            );
            let fresh = cipher.encrypt(&plain).unwrap();
            assert_ne!(fresh[MAGIC.len()..HEADER], sealed[MAGIC.len()..HEADER]);
            assert_eq!(cipher.decrypt(&fresh).unwrap(), plain);
        }
    }

    /// Another key, a changed byte, a block cut off: refused.
    #[test]
    fn what_isnt_its_own_is_refused() {
        let cipher = Cipher::new(PASSWORD).unwrap();
        let other = Cipher::new("another key").unwrap();
        let sealed = vector("vnkjuiqaa23il5pa8fgmtqr5to/jhoj1ni738sc5isvhfs98r0bok");
        assert!(other.decrypt(&sealed).is_err());
        assert!(other.decrypt_name("vnkjuiqaa23il5pa8fgmtqr5to").is_err());
        let mut changed = sealed.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(cipher.decrypt(&changed).is_err());
        assert!(cipher.decrypt(&sealed[..HEADER + TAG]).is_err());
        assert!(cipher.decrypt(b"plaintext").is_err());
        let block = vector("g1pefk16f48m0inc5043joosv4/st4kj7aoafv1sot6qpq4dnh0tc");
        let two = cipher.encrypt(&vec![1u8; BLOCK + 5]).unwrap();
        assert!(cipher.decrypt(&two[..two.len() - 20]).is_err());
        assert!(cipher.decrypt(&block).is_ok());
        assert!(Cipher::new("").is_err());
        assert_eq!(format!("{cipher:?}"), "Cipher { .. }");
        assert_eq!(decrypted_size(HEADER as u64 + 5), None);
    }
}
