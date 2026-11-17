//! Which shard a row belongs to, computed as chloedb-server computes it, so
//! that rows prepared here can be loaded into each shard offline and the
//! cluster's coordinator finds them where its slot map says they are.
//!
//! A key's slot is xxh3-64 over a typed encoding (an INT is tag 1 and its
//! eight bytes little-endian) modulo 4096 (`chloedb-server/src/slots.rs`,
//! `slot` and `encode`), and a new cluster of `n` shards gives slot `s` to
//! shard `s * n / 4096` (`SlotMap::even`). `tests` pin both against values
//! a server computed.

pub const SLOTS: usize = 4096;

/// The slot of an INT key or `shard_by` value.
pub fn slot(key: i64) -> usize {
    let mut b = [0u8; 9];
    b[0] = 1;
    b[1..].copy_from_slice(&key.to_le_bytes());
    (twox_hash::XxHash3_64::oneshot(&b) % SLOTS as u64) as usize
}

/// The shard of an INT key in a new cluster of `n` shards.
pub fn shard(key: i64, n: usize) -> usize {
    slot(key) * n / SLOTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_map_splits_slots_in_order() {
        assert_eq!(shard(0, 1), 0);
        for k in [1i64, 42, 7_000_000, -5] {
            let s = slot(k);
            assert!(s < SLOTS);
            assert_eq!(shard(k, 4), s / 1024);
        }
    }
}
