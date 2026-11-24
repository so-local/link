//! A job's profile as `lnk env here` reads it: a sample of the job's
//! processes at each stage's edges and every while, each measure named as
//! the measure contract names it, said back line by line (`sample
//! <json>`) wherever the run is.

use serde::{Deserialize, Serialize};

use crate::stage::Timeout;

/// A run of stages profiled: how often it's sampled, beside each stage's
/// start and end.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiling {
    pub every: Timeout,
}

/// How a sample was read: the run's control group of its own, or, with
/// none, the tree of processes under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reading {
    Group,
    Tree,
}

/// The run's processes at one moment: each measure none when it can't be
/// read there, never zero.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// When, in nanoseconds since 1970.
    pub at: u64,
    /// The stage running.
    pub stage: String,
    pub reading: Reading,
    /// Bytes in use (`link.memory.used`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    /// The most bytes used so far, a group's own count (`link.memory.peak`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<u64>,
    /// Core-seconds so far (`link.cpu.used`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    /// Bytes read from disk so far (`link.disk.read`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<u64>,
    /// Bytes written to disk so far (`link.disk.written`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<u64>,
}
