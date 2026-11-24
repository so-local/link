//! Monotonic host identities, outside automatic and delegated ranges.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use link_sandbox_contract::box_workers;

use super::{journal, records, root};

const FIRST_ID: u32 = 1_000_000;
const LAST_ID: u32 = 2_147_483_646;

fn save_counter(next: u32) -> anyhow::Result<()> {
    let path = journal().join("next-id");
    link_files::write_private(&path, next.to_string().as_bytes())?;
    Ok(())
}

/// Provision once, before ordinary-box configuration becomes visible. Losing
/// the durable counter is an error; IDs must never be reused, even after reboot.
pub(super) fn provision() -> anyhow::Result<()> {
    root()?;
    let records = records()?;
    let _lock = records.lock(libc::LOCK_EX)?;
    if !journal().join("next-id").exists() {
        ensure!(
            !box_workers::settings_path().exists(),
            "worker identity counter is missing; restore its durable state before running work"
        );
        save_counter(automatic_floor()?)?;
    }
    Ok(())
}

fn automatic_floor() -> anyhow::Result<u32> {
    let mut limits = BTreeMap::from([
        ("UID_MAX", 60000u32),
        ("GID_MAX", 60000),
        ("SYS_UID_MAX", 999),
        ("SYS_GID_MAX", 999),
        ("SUB_UID_MAX", 600_100_000),
        ("SUB_GID_MAX", 600_100_000),
        ("SUB_UID_COUNT", 65536),
        ("SUB_GID_COUNT", 65536),
    ]);
    for line in std::fs::read_to_string("/etc/login.defs")?.lines() {
        let mut words = line.split_whitespace();
        if let Some(key) = words.next()
            && let Some(limit) = limits.get_mut(key)
        {
            *limit = words
                .next()
                .context("missing automatic identity limit")?
                .parse()?;
        }
    }
    // Be conservative if a shadow version treats MAX as a range's start.
    let mut max = FIRST_ID - 1;
    for key in ["UID_MAX", "GID_MAX", "SYS_UID_MAX", "SYS_GID_MAX"] {
        max = max.max(limits[key]);
    }
    for (max_key, count_key) in [
        ("SUB_UID_MAX", "SUB_UID_COUNT"),
        ("SUB_GID_MAX", "SUB_GID_COUNT"),
    ] {
        max = max.max(
            limits[max_key]
                .checked_add(limits[count_key])
                .context("automatic identity range is too large")?,
        );
    }
    let first = max
        .checked_add(1)
        .context("automatic identity range is too large")?;
    ensure!(
        first <= LAST_ID,
        "no worker identity range remains outside automatic allocation"
    );
    Ok(first)
}

pub(super) fn reserve_ids(count: usize) -> anyhow::Result<Vec<u32>> {
    ensure!(count > 0, "the controller has no primary group");
    for line in std::fs::read_to_string("/etc/nsswitch.conf")?.lines() {
        if let Some((database, provider)) = line
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .split_once(':')
            && database.trim().eq_ignore_ascii_case("subid")
        {
            ensure!(
                provider.trim() == "files",
                "worker identities require files-backed subordinate ID allocation"
            );
        }
    }
    let mut next: u32 = std::fs::read_to_string(journal().join("next-id"))
        .context("the durable worker identity counter is missing")?
        .parse()
        .context("invalid worker identity counter")?;
    ensure!(
        (FIRST_ID..=LAST_ID + 1).contains(&next),
        "invalid worker identity counter"
    );
    next = next.max(automatic_floor()?);
    let mut used = std::collections::BTreeSet::new();
    for file in ["/etc/passwd", "/etc/group"] {
        for line in std::fs::read_to_string(file)?.lines() {
            if let Some(id) = line.split(':').nth(2).and_then(|id| id.parse::<u32>().ok()) {
                used.insert(id);
            }
        }
    }
    let mut ids = Vec::new();
    let mut delegated = Vec::new();
    for file in ["/etc/subuid", "/etc/subgid"] {
        let contents = match std::fs::read_to_string(file) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for line in contents.lines().filter(|line| !line.trim().is_empty()) {
            let mut fields = line.split(':');
            fields.next();
            let start: u64 = fields
                .next()
                .context("invalid subordinate identity range")?
                .parse()?;
            let count: u64 = fields
                .next()
                .context("invalid subordinate identity range")?
                .parse()?;
            delegated.push((
                start,
                start
                    .checked_add(count)
                    .context("invalid subordinate identity range")?,
            ));
        }
    }
    while ids.len() < count {
        ensure!(
            next <= LAST_ID,
            "worker identity space exhausted; no work was run"
        );
        if let Some(end) = delegated
            .iter()
            .filter(|(start, end)| *start <= u64::from(next) && u64::from(next) < *end)
            .map(|(_, end)| *end)
            .max()
        {
            next = end
                .try_into()
                .context("delegated identity range exhausts worker identities")?;
            continue;
        }
        if !used.contains(&next) {
            ids.push(next);
        }
        next += 1;
    }
    // Reserve durably before any account or group exists. Deletion never
    // rewinds this file: retained idmapped directory references cannot become
    // a later worker's path-traversal authority.
    save_counter(next)?;
    Ok(ids)
}
