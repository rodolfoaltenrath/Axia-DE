//! Kernel memory accounting. Available memory is an estimate, not MemFree + cache.
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub fn parse(text: &str) -> Value {
    let fields: BTreeMap<_, _> = text
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let bytes = rest
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)?;
            Some((key, bytes))
        })
        .collect();
    let n = |key| *fields.get(key).unwrap_or(&0);
    let total = n("MemTotal");
    let free = n("MemFree").min(total);
    let raw_cache = n("Buffers")
        .saturating_add(n("Cached"))
        .saturating_add(n("SReclaimable"))
        .saturating_sub(n("Shmem"));
    let avail = fields
        .get("MemAvailable")
        .copied()
        .unwrap_or_else(|| free.saturating_add(raw_cache))
        .min(total);
    json!({"total":total,"free":free,"avail":avail,"used":total-avail,
        "cache":raw_cache.min(avail),"swapTotal":n("SwapTotal"),
        "swapUsed":n("SwapTotal").saturating_sub(n("SwapFree")),
        "dirty":n("Dirty"),"writeback":n("Writeback")})
}
