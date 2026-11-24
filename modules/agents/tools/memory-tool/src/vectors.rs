//! Each embedded passage's vector, kept in two files beside the index,
//! never loaded whole: a binary code, one bit a dimension, which a search
//! by meaning compares with every other by counting differing bits, from
//! a file it maps rather than reads ([`scan`]); and a small vector, one
//! byte a dimension, read only to rescore the best few hundred
//! ([`read_small`]). A passage's vectors sit at its slot, the same in both
//! files; a slot let go is taken by the next passage embedded, so the files
//! never need rewriting whole.
//!
//! Vectors keep the first `dimensions` of the model's
//! ([`crate::opinions::Opinions::dimensions`]): a model trained to be cut
//! (Matryoshka) keeps most of what it knows in its first; any other's are
//! turned by a fixed random rotation first, so each kept dimension holds
//! an even share ([`Cut`]).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

/// How a model's vectors are cut to the dimensions kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// Its first dimensions, as a model trained to be cut keeps them.
    First,
    /// Its first after a fixed random rotation.
    Rotated,
}

impl Cut {
    /// How `model`'s vectors are cut: first, for a model `models.toml`
    /// says was trained to be cut.
    pub fn of(model: &str) -> Cut {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Models {
            cut: Vec<String>,
        }
        static MODELS: std::sync::LazyLock<Models> = std::sync::LazyLock::new(|| {
            toml::from_str(include_str!("models.toml")).expect("models.toml lists models")
        });
        let name = model.split(':').next().unwrap_or(model);
        match MODELS.cut.iter().any(|cut| cut == name) {
            true => Cut::First,
            false => Cut::Rotated,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Cut::First => "first",
            Cut::Rotated => "rotated",
        }
    }
}

/// The bytes before each small vector: the passage it's of (a slot taken
/// again since a search's snapshot is never taken for another's), and
/// the scale its bytes were made with.
const HEADER: usize = 16;

/// The vectors an index keeps: of what model, how many dimensions, cut how,
/// and in which files.
#[derive(Clone, Debug)]
pub struct Kept {
    pub model: String,
    pub dimensions: usize,
    pub cut: Cut,
    codes: PathBuf,
    small: PathBuf,
}

/// The files of the vectors of the index `file`: its codes' and its small
/// vectors', beside it.
pub fn files_of(file: &Path) -> [PathBuf; 2] {
    [".codes", ".small"].map(|end| {
        let mut path = file.as_os_str().to_owned();
        path.push(end);
        PathBuf::from(path)
    })
}

impl Kept {
    /// The vectors of the index `file`, of `model`, `dimensions` of them
    /// kept (rounded down to whole bytes of code, 8 at least).
    pub fn new(file: &Path, model: &str, dimensions: u32) -> Kept {
        let [codes, small] = files_of(file);
        Kept {
            model: model.to_string(),
            dimensions: ((dimensions as usize) / 8).max(1) * 8,
            cut: Cut::of(model),
            codes,
            small,
        }
    }

    /// The bytes of a code.
    fn code_bytes(&self) -> usize {
        self.dimensions / 8
    }

    /// The bytes of a small vector's slot.
    fn small_bytes(&self) -> usize {
        HEADER + self.dimensions
    }

    /// Makes the index `db` hold these vectors: if it held another model's,
    /// or another number of them, every passage is let go of its slot to be
    /// embedded again, and the files are emptied.
    pub fn take_over(&self, db: &Connection) -> Result<()> {
        let held: Option<(String, i64, String)> = db
            .query_row("SELECT model, dimensions, cut FROM vectors", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        let ours = (
            self.model.clone(),
            self.dimensions as i64,
            self.cut.as_str().to_string(),
        );
        // Its files hold every slot given out, or the index was fetched
        // without them: then it starts over.
        let whole = || -> Result<bool> {
            let slots = slots(db)?;
            let long = |file: &Path, bytes: usize| {
                std::fs::metadata(file).map_or(0, |m| m.len()) >= slots * bytes as u64
            };
            Ok(long(&self.codes, self.code_bytes()) && long(&self.small, self.small_bytes()))
        };
        if held.as_ref() == Some(&ours) && whole()? {
            return Ok(());
        }
        let tx = db.unchecked_transaction()?;
        tx.execute("UPDATE passages SET slot = NULL WHERE slot IS NOT NULL", [])?;
        tx.execute("DELETE FROM free_slots", [])?;
        tx.execute("DELETE FROM meta WHERE key = 'slots'", [])?;
        tx.execute("DELETE FROM vectors", [])?;
        tx.execute(
            "INSERT INTO vectors (model, dimensions, cut) VALUES (?1, ?2, ?3)",
            params![ours.0, ours.1, ours.2],
        )?;
        for file in [&self.codes, &self.small] {
            open(file)?.set_len(0)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The vectors the index `db` (its file `file`) holds, of `model`: none
    /// when it holds none of it.
    pub fn held(db: &Connection, file: &Path, model: &str) -> Result<Option<Kept>> {
        let held: Option<(String, i64)> = db
            .query_row("SELECT model, dimensions FROM vectors", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        Ok(held
            .filter(|(held, _)| held == model)
            .map(|(_, dimensions)| Kept::new(file, model, dimensions as u32)))
    }

    /// `vector`, a model's, as kept: cut to the dimensions kept, its length
    /// made 1.
    pub fn reduce(&self, vector: &[f32]) -> Vec<f32> {
        let turned;
        let vector = match self.cut {
            Cut::First => vector,
            Cut::Rotated => {
                turned = rotate(vector);
                &turned
            }
        };
        let mut kept: Vec<f32> = vector.iter().copied().take(self.dimensions).collect();
        kept.resize(self.dimensions, 0.0);
        crate::semantic::normalized(kept)
    }

    /// Writes passage `passage`'s vector, `reduced` ([`Kept::reduce`]), at
    /// `slot` of both files.
    pub fn write(&self, slot: u64, passage: i64, reduced: &[f32]) -> Result<()> {
        use std::os::unix::fs::FileExt;
        let code = code(reduced);
        let (scale, bytes) = small(reduced);
        let mut slot_bytes = Vec::with_capacity(self.small_bytes());
        slot_bytes.extend(passage.to_le_bytes());
        slot_bytes.extend(scale.to_le_bytes());
        slot_bytes.resize(HEADER, 0);
        slot_bytes.extend(bytes.iter().map(|b| *b as u8));
        open(&self.codes)?.write_all_at(&code, slot * self.code_bytes() as u64)?;
        open(&self.small)?.write_all_at(&slot_bytes, slot * self.small_bytes() as u64)?;
        Ok(())
    }

    /// The `keep` slots, of the first `slots`, whose codes differ least
    /// from `query`'s ([`code`]): each with how many bits differ, fewest
    /// first. The codes' file is mapped, never read into memory.
    pub fn scan(&self, slots: u64, query: &[f32], keep: usize) -> Result<Vec<(u32, u64)>> {
        let asked = code(query);
        let bytes = self.code_bytes();
        let file = match std::fs::File::open(&self.codes) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err).context("opening the vectors' codes"),
        };
        let length = (file.metadata()?.len() / bytes as u64).min(slots) as usize * bytes;
        if length == 0 || keep == 0 {
            return Ok(Vec::new());
        }
        let mapped = Mapped::of(&file, length)?;
        let mut best: std::collections::BinaryHeap<(u32, u64)> =
            std::collections::BinaryHeap::with_capacity(keep + 1);
        for (slot, code) in mapped.bytes().chunks_exact(bytes).enumerate() {
            let differ: u32 = code
                .iter()
                .zip(&asked)
                .map(|(mine, theirs)| (mine ^ theirs).count_ones())
                .sum();
            if best.len() < keep {
                best.push((differ, slot as u64));
            } else if best.peek().is_some_and(|worst| differ < worst.0) {
                best.pop();
                best.push((differ, slot as u64));
            }
        }
        Ok(best.into_sorted_vec())
    }

    /// The small vector at `slot`, if it's passage `passage`'s: as floats.
    pub fn read_small(&self, slot: u64, passage: i64) -> Result<Option<Vec<f32>>> {
        use std::os::unix::fs::FileExt;
        let mut bytes = vec![0u8; self.small_bytes()];
        let file = match std::fs::File::open(&self.small) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).context("opening the small vectors"),
        };
        if file
            .read_exact_at(&mut bytes, slot * self.small_bytes() as u64)
            .is_err()
        {
            return Ok(None);
        }
        let of = i64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
        if of != passage {
            return Ok(None);
        }
        let scale = f32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
        Ok(Some(
            bytes[HEADER..]
                .iter()
                .map(|b| f32::from(*b as i8) * scale)
                .collect(),
        ))
    }

    /// The bytes a small vector's read takes.
    pub fn small_read(&self) -> usize {
        self.small_bytes()
    }
}

/// A slot for a vector: one let go, else a new one past the last.
pub fn take_slot(db: &Connection) -> Result<u64> {
    let free: Option<i64> = db
        .query_row(
            "DELETE FROM free_slots WHERE slot = (SELECT min(slot) FROM free_slots)
             RETURNING slot",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(slot) = free {
        return Ok(slot as u64);
    }
    let next: i64 = db
        .query_row("SELECT value FROM meta WHERE key = 'slots'", [], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or(0);
    db.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('slots', ?1)",
        [next + 1],
    )?;
    Ok(next as u64)
}

/// How many slots the index has given out.
pub fn slots(db: &Connection) -> Result<u64> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key = 'slots'", [], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
        .unwrap_or(0)
        .max(0) as u64)
}

/// A vector file, opened to read and write, made if it isn't there, the
/// user's alone.
fn open(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))
}

/// `vector`'s sign in each dimension, a bit each.
pub fn code(vector: &[f32]) -> Vec<u8> {
    vector
        .chunks(8)
        .map(|eight| {
            eight
                .iter()
                .enumerate()
                .fold(0u8, |byte, (bit, x)| byte | (u8::from(*x > 0.0) << bit))
        })
        .collect()
}

/// `vector` in a byte a dimension, and the scale that gives it back.
pub fn small(vector: &[f32]) -> (f32, Vec<i8>) {
    let most = vector.iter().fold(0f32, |most, x| most.max(x.abs()));
    if most == 0.0 {
        return (0.0, vec![0; vector.len()]);
    }
    let scale = most / 127.0;
    (
        scale,
        vector
            .iter()
            .map(|x| (x / scale).round().clamp(-127.0, 127.0) as i8)
            .collect(),
    )
}

/// `vector` turned by a fixed random rotation: each dimension's sign
/// flipped by a fixed sequence, then a Walsh-Hadamard transform over it,
/// padded to a power of two, so its length is kept and each dimension
/// holds an even share of it.
pub fn rotate(vector: &[f32]) -> Vec<f32> {
    let n = vector.len().next_power_of_two();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let x = vector.get(i).copied().unwrap_or(0.0);
            if state & 1 == 1 { -x } else { x }
        })
        .collect();
    let mut half = 1;
    while half < n {
        for start in (0..n).step_by(half * 2) {
            for i in start..start + half {
                let (a, b) = (out[i], out[i + half]);
                out[i] = a + b;
                out[i + half] = a - b;
            }
        }
        half *= 2;
    }
    let scale = 1.0 / (n as f32).sqrt();
    out.iter_mut().for_each(|x| *x *= scale);
    out
}

/// A file mapped to read: its pages are the system's page cache, never a
/// copy in this process.
struct Mapped {
    at: *mut libc::c_void,
    length: usize,
}

impl Mapped {
    fn of(file: &std::fs::File, length: usize) -> Result<Mapped> {
        use std::os::fd::AsRawFd;
        // SAFETY: a file this process opened, mapped to read, privately,
        // for `length` bytes it holds; unmapped when dropped.
        let at = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if at == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error()).context("mapping the vectors' codes");
        }
        Ok(Mapped { at, length })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `length` bytes mapped at `at`, alive while `self` is.
        unsafe { std::slice::from_raw_parts(self.at as *const u8, self.length) }
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: the mapping `of` made, unmapped once.
        unsafe { libc::munmap(self.at, self.length) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rotation_keeps_a_vectors_length_and_angles() {
        let a: Vec<f32> = (0..1000)
            .map(|i| ((i * 7 % 13) as f32 - 6.0) / 8.0)
            .collect();
        let b: Vec<f32> = (0..1000)
            .map(|i| ((i * 5 % 11) as f32 - 5.0) / 4.0)
            .collect();
        let dot = crate::semantic::dot;
        let (ra, rb) = (rotate(&a), rotate(&b));
        assert!((dot(&ra, &ra) - dot(&a, &a)).abs() / dot(&a, &a) < 1e-4);
        assert!((dot(&ra, &rb) - dot(&a, &b)).abs() / dot(&a, &a) < 1e-3);
    }

    #[test]
    fn a_code_is_signs_and_a_small_vector_gives_it_back() {
        assert_eq!(
            code(&[1.0, -1.0, 0.5, 0.0, -2.0, 3.0, 1.0, 1.0, 1.0]),
            [0b1110_0101, 1]
        );
        let v = [0.5f32, -0.25, 0.125, 0.0];
        let (scale, bytes) = small(&v);
        for (x, b) in v.iter().zip(bytes) {
            assert!((f32::from(b) * scale - x).abs() < 0.005);
        }
    }

    #[test]
    fn the_default_model_is_cut_at_its_first_dimensions() {
        assert_eq!(Cut::of(crate::semantic::DEFAULT_MODEL), Cut::First);
        assert_eq!(Cut::of("qwen3-embedding:0.6b"), Cut::First);
        assert_eq!(Cut::of("other-model"), Cut::Rotated);
    }
}
