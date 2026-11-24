//! What a box asks of a cloud: its CPUs, memory, GPUs, processor, disk,
//! OS, tags, region and machine type, every default filled in. A box
//! spec (the box module's) is read into it, and an adapter matches it to
//! its cloud's sizes. Every value is typed where it's read, since what's
//! asked may be someone else's: nothing reaches a shell or a cloud's CLI
//! as written.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use anyhow::ensure;
use link_actions::value::Words;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use super::{Arch, MachineSize, OSES, Os};

/// The most tags a spec gives a box, so Link's own always fit (AWS takes 50).
pub const MAX_TAGS: usize = 40;

/// What a spec asks of a box in one cloud, every default filled in: as
/// `lnk box check --plan <kind> --json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Want {
    pub cpus: Range<u32>,
    pub memory: Range<Size>,
    pub gpus: Range<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model: Option<GpuModel>,
    pub arch: Arch,
    pub disk: Size,
    pub disk_type: DiskType,
    pub os: Os,
    pub spot: bool,
    #[serde(with = "tag_map")]
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<CloudName>,
    /// A pinned machine type: no matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<CloudName>,
    /// Whether the machine type is matched to CPUs, memory, GPUs and an
    /// arm64 processor; else it's `size`, or the adapter's default, as
    /// before specs.
    pub matching: bool,
}

impl Want {
    /// An error for what no cloud box can be.
    pub fn check_buildable(&self) -> anyhow::Result<()> {
        ensure!(
            OSES.contains(&self.os.word()),
            "{:?} isn't an OS a cloud box can have: {}",
            self.os.word(),
            OSES.join(", ")
        );
        ensure!(
            self.arch == Arch::X8664
                || (self.gpus.min.unwrap_or(0) == 0 && self.gpu_model.is_none()),
            "arch = \"arm64\" with GPUs: a box with GPUs is x86_64, the processor of the images \
             with NVIDIA's driver Link uses"
        );
        Ok(())
    }

    /// The tags as the adapter's `Spec` takes them.
    pub fn tag_map(&self) -> BTreeMap<String, String> {
        self.tags
            .iter()
            .map(|t| (t.key.clone(), t.value.clone()))
            .collect()
    }
}

/// A region, zone or machine type, in the clouds' own characters:
/// lowercase letters, digits, dashes and dots, at most 63, never
/// starting like an option.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CloudName(String);

impl CloudName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CloudName {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        let plain = !text.is_empty()
            && text.len() <= 63
            && text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        match plain {
            true => Ok(Self(text)),
            false => Err(format!(
                "{:?} isn't a cloud's region or machine type: lowercase letters, digits, dashes \
                 and dots",
                link_format::clean(&text)
            )),
        }
    }
}

impl From<CloudName> for String {
    fn from(name: CloudName) -> String {
        name.0
    }
}

impl FromStr for CloudName {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        Self::try_from(text.to_string())
    }
}

impl fmt::Display for CloudName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How fast a box's disk is and a GPU's model: generated from the
/// contract's declarations (`types.toml`).
pub use crate::declared::{DiskType, GpuModel};

/// A box's disk when nothing names how fast.
impl Default for DiskType {
    fn default() -> Self {
        DiskType::Balanced
    }
}

/// One of your tags: `team=research`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

impl Tag {
    pub fn new(key: &str, value: &str) -> Result<Tag, String> {
        if key.starts_with("lnk-") || key == "Name" {
            return Err(format!(
                "the tag {key:?} is Link's own: name yours otherwise"
            ));
        }
        if key.to_ascii_lowercase().starts_with("aws:") {
            return Err(format!(
                "the tag {key:?} is AWS's own: name yours otherwise"
            ));
        }
        if !key.starts_with(|c: char| c.is_ascii_lowercase()) || super::check_label(key).is_err() {
            return Err(format!(
                "{:?} can't name a tag: lowercase letters, digits and dashes, starting with a \
                 letter, at most 63",
                link_format::clean(key)
            ));
        }
        if super::check_label(value).is_err() {
            return Err(format!(
                "{:?} can't be a tag's value: lowercase letters, digits and dashes, at most 63",
                link_format::clean(value)
            ));
        }
        Ok(Tag {
            key: key.into(),
            value: value.into(),
        })
    }
}

impl FromStr for Tag {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let (key, value) = text
            .split_once('=')
            .ok_or_else(|| format!("{:?} isn't a tag: key=value", link_format::clean(text)))?;
        Tag::new(key, value)
    }
}

/// An amount of memory or disk, in MiB, written `"8 GB"` (GiB, as both
/// clouds count memory), `"512 MB"` or `"1 TB"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size {
    pub mb: u64,
}

impl Size {
    pub fn gb(gb: u64) -> Size {
        Size { mb: gb * 1024 }
    }

    /// Whole GB, rounded up: what a disk is made with.
    pub fn whole_gb(self) -> u64 {
        self.mb.div_ceil(1024)
    }
}

impl FromStr for Size {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let bad = || {
            format!(
                "{:?} isn't a size: a number and its unit, as \"8 GB\" (GiB), \"512 MB\" or \"1 TB\"",
                link_format::clean(text)
            )
        };
        let text = text.trim();
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .ok_or_else(bad)?;
        let (number, unit) = text.split_at(split);
        let per = match unit.trim() {
            "MB" => 1,
            "GB" => 1024,
            "TB" => 1024 * 1024,
            _ => return Err(bad()),
        };
        if number.is_empty() || number.len() > 12 || number.matches('.').count() > 1 {
            return Err(bad());
        }
        let value: f64 = number.parse().map_err(|_| bad())?;
        let mb = (value * per as f64).round();
        if mb < 1.0 || mb > (1u64 << 40) as f64 {
            return Err(bad());
        }
        Ok(Size { mb: mb as u64 })
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.mb.is_multiple_of(1024) {
            write!(f, "{} GB", self.mb / 1024)
        } else if self
            .mb
            .checked_mul(100)
            .is_some_and(|hundredths| hundredths.is_multiple_of(1024))
        {
            write!(f, "{} GB", self.mb as f64 / 1024.0)
        } else {
            write!(f, "{} MB", self.mb)
        }
    }
}

impl Serialize for Size {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(OneVisitor::<Size>(std::marker::PhantomData))
    }
}

/// What a range's ends are: a count or a size.
pub trait Amount: Copy + Ord + fmt::Display + Serialize + Send + Sync + 'static {
    /// Written as a TOML integer.
    fn from_int(value: i64) -> Result<Self, String>;
    /// Written as text: in a file's string, or on the command line.
    fn from_text(text: &str) -> Result<Self, String>;
    /// What it is, for messages: `CPUs`.
    const WHAT: &'static str;
}

impl Amount for u32 {
    fn from_int(value: i64) -> Result<Self, String> {
        u32::try_from(value)
            .ok()
            .filter(|v| *v <= 4096)
            .ok_or_else(|| format!("{value} is no count Link takes"))
    }
    fn from_text(text: &str) -> Result<Self, String> {
        let value: i64 = text
            .trim()
            .parse()
            .map_err(|_| format!("{:?} isn't a count", link_format::clean(text)))?;
        u32::from_int(value)
    }
    const WHAT: &'static str = "a count";
}

impl Amount for Size {
    fn from_int(value: i64) -> Result<Self, String> {
        Err(format!(
            "{value} has no unit: write a size as \"{value} GB\" (GiB)"
        ))
    }
    fn from_text(text: &str) -> Result<Self, String> {
        text.parse()
    }
    const WHAT: &'static str = "a size";
}

/// What a range is: `{ min, max }`, either left out, or one value meaning
/// exactly that. On the command line `4`, `2..8`, `2..` or `..8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

impl<T: Amount> Range<T> {
    pub fn exactly(value: T) -> Self {
        Range {
            min: Some(value),
            max: Some(value),
        }
    }

    pub fn contains(&self, value: T) -> bool {
        self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max)
    }

    /// It, unless it has no end or its min is over its max.
    pub fn checked(self) -> Result<Self, String> {
        match (self.min, self.max) {
            (None, None) => Err("a range needs its min, its max or both".into()),
            (Some(min), Some(max)) if min > max => {
                Err(format!("a range's min ({min}) is over its max ({max})"))
            }
            _ => Ok(self),
        }
    }
}

impl<T: Amount> fmt::Display for Range<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self.min, self.max) {
            (Some(min), Some(max)) if min == max => write!(f, "{min}"),
            (Some(min), Some(max)) => write!(f, "{min} to {max}"),
            (Some(min), None) => write!(f, "at least {min}"),
            (None, Some(max)) => write!(f, "at most {max}"),
            (None, None) => write!(f, "any"),
        }
    }
}

impl<T: Amount> FromStr for Range<T> {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let end = |part: &str| match part.trim() {
            "" => Ok(None),
            part => T::from_text(part).map(Some),
        };
        let range = match text.split_once("..") {
            None => Range::exactly(T::from_text(text.trim())?),
            Some((min, max)) => Range {
                min: end(min)?,
                max: end(max)?,
            },
        };
        range.checked()
    }
}

impl<T: Amount> Serialize for Range<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match (self.min, self.max) {
            (Some(min), Some(max)) if min == max => min.serialize(s),
            _ => {
                let mut map = s.serialize_map(None)?;
                if let Some(min) = self.min {
                    map.serialize_entry("min", &min)?;
                }
                if let Some(max) = self.max {
                    map.serialize_entry("max", &max)?;
                }
                map.end()
            }
        }
    }
}

/// One value of `T`: an integer or a string, as `T` takes it.
struct OneVisitor<T>(std::marker::PhantomData<T>);

impl<'de, T: Amount> Visitor<'de> for OneVisitor<T> {
    type Value = T;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(T::WHAT)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        T::from_int(v).map_err(E::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        T::from_int(i64::try_from(v).unwrap_or(i64::MAX)).map_err(E::custom)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        T::from_text(v).map_err(E::custom)
    }
}

/// A range's end in a file: a count or a size, as `T` takes it.
pub struct End<T>(pub T);

impl<'de, T: Amount> Deserialize<'de> for End<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(OneVisitor::<T>(std::marker::PhantomData))
            .map(End)
    }
}

struct RangeVisitor<T>(std::marker::PhantomData<T>);

impl<'de, T: Amount> Visitor<'de> for RangeVisitor<T> {
    type Value = Range<T>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}, or {{ min, max }}", T::WHAT)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Range<T>, E> {
        T::from_int(v).map(Range::exactly).map_err(E::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Range<T>, E> {
        self.visit_i64(i64::try_from(v).unwrap_or(i64::MAX))
    }
    // One amount, or the text form `2..4`, a flag's and a file's alike.
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Range<T>, E> {
        v.parse().map_err(E::custom)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Range<T>, A::Error> {
        let mut range = Range {
            min: None,
            max: None,
        };
        while let Some(key) = map.next_key::<String>()? {
            let end = match key.as_str() {
                "min" => &mut range.min,
                "max" => &mut range.max,
                other => return Err(de::Error::unknown_field(other, &["min", "max"])),
            };
            if end.is_some() {
                return Err(de::Error::duplicate_field("min or max"));
            }
            *end = Some(map.next_value::<End<T>>()?.0);
        }
        range.checked().map_err(de::Error::custom)
    }
}

impl<'de, T: Amount> Deserialize<'de> for Range<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(RangeVisitor(std::marker::PhantomData))
    }
}

/// Tags as a TOML table: `tags = { team = "research" }`.
pub mod tag_map {
    use super::*;

    pub fn serialize<S: Serializer>(tags: &[Tag], s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(tags.len()))?;
        for tag in tags {
            map.serialize_entry(&tag.key, &tag.value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Tag>, D::Error> {
        let map = BTreeMap::<String, String>::deserialize(d)?;
        if map.len() > MAX_TAGS {
            return Err(de::Error::custom(format!(
                "{} tags: a box takes at most {MAX_TAGS} of yours",
                map.len()
            )));
        }
        map.iter()
            .map(|(k, v)| Tag::new(k, v).map_err(de::Error::custom))
            .collect()
    }
}

/// The word a declared choice is written as, in a file and on the command
/// line.
pub fn word<T: Words>(value: &T) -> String {
    value.word().to_string()
}

/// A word of `T`'s, from an adapter or a file.
pub fn from_word<T: Words>(text: &str) -> Option<T> {
    T::from_word(text)
}

/// Whether `size` fits `want`: within every `min` and `max`, the right
/// arch, GPU count and model.
pub fn fits(want: &Want, size: &MachineSize) -> bool {
    let model_fits = match &want.gpu_model {
        Some(model) => size.gpu_model() == word(model),
        None => true,
    };
    want.cpus.contains(size.cpus())
        && want.memory.contains(Size {
            mb: size.memory_mb(),
        })
        && want.gpus.contains(size.gpus())
        && model_fits
        && size.arch == word(&want.arch)
}

/// The sizes that fit `want`, best first: the smallest, by CPUs then
/// memory then GPUs; between equals the cheaper when they have a price,
/// then the adapter's order.
pub fn ranked<'a>(want: &Want, sizes: &'a [MachineSize]) -> Vec<&'a MachineSize> {
    let mut fitting: Vec<(usize, &MachineSize)> = sizes
        .iter()
        .enumerate()
        .filter(|(_, s)| fits(want, s))
        .collect();
    // Prices compared in cents: a total order. No price sorts after one.
    let cents = |s: &MachineSize| {
        s.price
            .filter(|p| p.is_finite() && *p >= 0.0)
            .map_or(u64::MAX, |p| (p * 100_000.0).round() as u64)
    };
    fitting.sort_by_key(|(i, s)| (s.cpus(), s.memory_mb(), s.gpus(), cents(s), *i));
    fitting.into_iter().map(|(_, s)| s).collect()
}

/// The best size for `want`, or why none fits: the nearest on each side.
pub fn pick<'a>(want: &Want, sizes: &'a [MachineSize]) -> anyhow::Result<&'a MachineSize> {
    match ranked(want, sizes).first() {
        Some(best) => Ok(best),
        None => anyhow::bail!("nothing there fits: {}", nearest(want, sizes)),
    }
}

fn count(n: u32, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn cpus(range: &Range<u32>) -> String {
    match (range.min, range.max) {
        (Some(min), Some(max)) if min == max => count(min, "CPU", "CPUs"),
        _ => format!("{range} CPUs"),
    }
}

/// Why nothing fits `want`, by the nearest sizes on each side of what
/// failed: the GPUs, else the CPUs, else the memory.
pub fn nearest(want: &Want, sizes: &[MachineSize]) -> String {
    let arch = word(&want.arch);
    let model = want.gpu_model.as_ref().map(word);
    let with_gpus: Vec<&MachineSize> = sizes
        .iter()
        .filter(|s| s.arch == arch)
        .filter(|s| want.gpus.contains(s.gpus()))
        .filter(|s| model.as_ref().is_none_or(|m| s.gpu_model() == m))
        .collect();
    if with_gpus.is_empty() {
        let gpus = match (&model, want.gpus) {
            (Some(model), range) => format!("{range} {model} GPUs"),
            (None, range) => format!("{range} GPUs"),
        };
        let mut models: Vec<&str> = sizes
            .iter()
            .filter(|s| s.gpus() > 0)
            .map(|s| s.gpu_model())
            .collect();
        models.sort();
        models.dedup();
        return match models.is_empty() {
            true => format!("no machine there has {gpus}, and none has GPUs"),
            false => format!(
                "no machine there has {gpus}; its GPUs are {}",
                models.join(", ")
            ),
        };
    }
    let with_cpus: Vec<&MachineSize> = with_gpus
        .iter()
        .copied()
        .filter(|s| want.cpus.contains(s.cpus()))
        .collect();
    if with_cpus.is_empty() {
        let mut said = Vec::new();
        if let Some(min) = want.cpus.min
            && let Some(below) = with_gpus
                .iter()
                .map(|s| s.cpus())
                .filter(|c| *c < min)
                .max()
        {
            said.push(format!(
                "the largest under your min of {min} has {}",
                count(below, "CPU", "CPUs")
            ));
        }
        if let Some(max) = want.cpus.max
            && let Some(above) = with_gpus
                .iter()
                .map(|s| s.cpus())
                .filter(|c| *c > max)
                .min()
        {
            said.push(format!(
                "the smallest over your max of {max} has {}",
                count(above, "CPU", "CPUs")
            ));
        }
        return format!("none has {}: {}", cpus(&want.cpus), said.join("; "));
    }
    let mut said = Vec::new();
    let with = cpus(&want.cpus);
    if let Some(min) = want.memory.min
        && let Some(below) = with_cpus
            .iter()
            .map(|s| s.memory_mb())
            .filter(|m| *m < min.mb)
            .max()
    {
        said.push(format!(
            "the largest with {with} has {}, under your min of {min}",
            Size { mb: below }
        ));
    }
    if let Some(max) = want.memory.max
        && let Some(above) = with_cpus
            .iter()
            .map(|s| s.memory_mb())
            .filter(|m| *m > max.mb)
            .min()
    {
        said.push(format!(
            "the smallest with {with} has {}, over your max of {max}",
            Size { mb: above }
        ));
    }
    match said.is_empty() {
        true => format!("none has {with} and {} of memory", want.memory),
        false => said.join("; "),
    }
}

/// What a size is, for the question before making a box: `g6.xlarge: 4
/// CPUs, 16 GB, 1 nvidia-l4, x86_64`.
pub fn describe(size: &MachineSize) -> String {
    let mut parts = vec![
        count(size.cpus(), "CPU", "CPUs"),
        Size {
            mb: size.memory_mb(),
        }
        .to_string(),
    ];
    if size.gpus() > 0 {
        parts.push(format!(
            "{} {}",
            size.gpus(),
            link_format::clean(size.gpu_model())
        ));
    }
    parts.push(link_format::clean(&size.arch));
    format!("{}: {}", link_format::clean(&size.size), parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(name: &str, cpus: u32, gb: u64, gpus: u32, price: Option<f64>) -> MachineSize {
        MachineSize {
            size: name.into(),
            cpus: cpus.into(),
            memory_mb: gb * 1024,
            gpus: gpus.into(),
            gpu_model: (gpus > 0).then(|| "nvidia-l4".into()),
            arch: "x86_64".into(),
            price,
        }
    }

    fn want(json: serde_json::Value) -> Want {
        let mut want = serde_json::json!({
            "cpus": { "min": 1 }, "memory": { "min": "1 GB" }, "gpus": 0, "arch": "x86_64",
            "disk": "20 GB", "disk_type": "balanced", "os": "ubuntu-24.04", "spot": false,
            "tags": {}, "matching": true,
        });
        for (key, value) in json.as_object().unwrap() {
            want[key] = value.clone();
        }
        serde_json::from_value(want).unwrap()
    }

    #[test]
    fn ranges_and_sizes_as_text() {
        let r = |t: &str| t.parse::<Range<u32>>();
        assert_eq!(r("4").unwrap(), Range::exactly(4));
        assert_eq!(
            r("2..8").unwrap(),
            Range {
                min: Some(2),
                max: Some(8)
            }
        );
        assert_eq!(
            r("2..").unwrap(),
            Range {
                min: Some(2),
                max: None
            }
        );
        assert_eq!(
            r("..8").unwrap(),
            Range {
                min: None,
                max: Some(8)
            }
        );
        assert!(r("..").is_err() && r("8..2").is_err() && r("x").is_err());
        let m = "8GB..16 GB".parse::<Range<Size>>().unwrap();
        assert_eq!(m.min, Some(Size::gb(8)));
        assert_eq!(m.max, Some(Size::gb(16)));
        assert_eq!("0.5 GB".parse::<Size>().unwrap().mb, 512);
        assert_eq!(Size { mb: 512 }.to_string(), "0.5 GB");
        assert_eq!(Size { mb: 3840 }.to_string(), "3.75 GB");
        // As large as an adapter may say, without overflowing.
        assert_eq!(
            Size { mb: u64::MAX }.to_string(),
            format!("{} MB", u64::MAX)
        );
    }

    #[test]
    fn the_smallest_that_fits_and_the_cheaper_of_equals() {
        let sizes = [
            size("big", 8, 32, 0, None),
            size("a", 4, 16, 0, Some(0.20)),
            size("b", 4, 16, 0, Some(0.15)),
            size("c", 4, 16, 0, None),
            size("small", 2, 4, 0, None),
            size("gpu", 4, 16, 1, Some(0.8)),
        ];
        let w = want(serde_json::json!({ "cpus": { "min": 4 } }));
        let names: Vec<_> = ranked(&w, &sizes).iter().map(|s| &*s.size).collect();
        assert_eq!(names, ["b", "a", "c", "big"]);
        assert_eq!(pick(&w, &sizes).unwrap().size, "b");
        let w = want(serde_json::json!({ "gpus": 1 }));
        assert_eq!(pick(&w, &sizes).unwrap().size, "gpu");
    }

    #[test]
    fn nothing_fits_says_the_nearest() {
        let sizes = [size("s", 2, 4, 0, None), size("m", 4, 16, 0, None)];
        let err = pick(
            &want(serde_json::json!({ "cpus": 4, "memory": { "max": "8 GB" } })),
            &sizes,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("the smallest with 4 CPUs has 16 GB, over your max of 8 GB"),
            "{err}"
        );
        let err = pick(&want(serde_json::json!({ "cpus": 3 })), &sizes).unwrap_err();
        assert!(err.to_string().contains("has 2 CPUs"), "{err}");
        assert!(err.to_string().contains("has 4 CPUs"), "{err}");
        let err = pick(&want(serde_json::json!({ "gpus": 1 })), &sizes).unwrap_err();
        assert!(err.to_string().contains("none has GPUs"), "{err}");
    }

    #[test]
    fn a_size_says_what_it_is() {
        assert_eq!(
            describe(&size("g6.xlarge", 4, 16, 1, None)),
            "g6.xlarge: 4 CPUs, 16 GB, 1 nvidia-l4, x86_64"
        );
    }

    #[test]
    fn a_want_crosses_as_json_and_only_as_itself() {
        let mut sent = want(serde_json::json!({
            "cpus": { "min": 2, "max": 8 }, "gpus": 1, "gpu_model": "nvidia-l4",
            "tags": { "team": "research" }, "region": "us-east-1", "spot": true,
        }));
        sent.size = Some(CloudName::try_from("g6.xlarge".to_string()).unwrap());
        let json = serde_json::to_value(&sent).unwrap();
        assert_eq!(json["tags"], serde_json::json!({ "team": "research" }));
        assert_eq!(json["memory"], serde_json::json!({ "min": "1 GB" }));
        let plain = serde_json::to_value(want(serde_json::json!({}))).unwrap();
        assert!(plain.get("region").is_none() && plain.get("gpu_model").is_none());
        let back: Want = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, sent);
        let refused = |key: &str, value: serde_json::Value| {
            let mut bad = json.clone();
            bad[key] = value;
            serde_json::from_value::<Want>(bad).unwrap_err().to_string()
        };
        assert!(refused("colour", "red".into()).contains("unknown field"));
        assert!(refused("tags", serde_json::json!({ "lnk-box": "x" })).contains("Link's own"));
        assert!(
            refused("cpus", serde_json::json!({ "min": 8, "max": 2 })).contains("over its max")
        );
        assert!(refused("region", "US EAST".into()).contains("region"));
    }

    #[test]
    fn shared_guest_labels_do_not_expand_the_cloud_image_contract() {
        for os in [Os::Ubuntu2404, Os::Ubuntu2204] {
            let wanted = want(serde_json::json!({ "os": os.word() }));
            wanted.check_buildable().unwrap();
            let spec = super::super::Spec {
                os: os.word().into(),
                ..Default::default()
            };
            spec.check().unwrap();
        }
        for os in [Os::Debian13, Os::Macos] {
            let wanted = want(serde_json::json!({ "os": os.word() }));
            let error = wanted.check_buildable().unwrap_err().to_string();
            assert!(
                error.contains(os.word()) && error.contains("cloud box"),
                "{error}"
            );
            let spec = super::super::Spec {
                os: os.word().into(),
                ..Default::default()
            };
            assert!(spec.check().is_err());
        }
    }
}
