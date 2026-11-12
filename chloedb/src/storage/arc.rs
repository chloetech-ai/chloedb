//! Log objects (`STRARC01`): numbered transactions as a database ships
//! them ([`crate::storage::ship`]), kept as one object in an object store.
//!
//! ```text
//! header   "STRARC01" | version: u32 = 1 | flags: u32 = 0
//!          | stream: [u8; 16]   whose transactions these are (a tenant's
//!                               incarnation, [`crate::tenant`])
//!          | db_id: u64 | generation: u64
//!          | first_seq: u64 | last_seq: u64 | count: u32
//!          | features_len: u32 | features, comma-separated
//! entries  count × ( term: u64 | seq: u64 | commit_ms: u64 | len: u32 | bytes )
//!          bytes: one transaction as [`crate::storage::Database::log_from`]
//!          gives it (its WAL records, its commit record last)
//! footer   crc32 of everything before it: u32 | "STRARC01"
//! ```
//!
//! All integers are little-endian. The entries are in order, their
//! numbers without gaps from `first_seq` to `last_seq`, and their terms
//! do not fall. A reader refuses an object that names a feature it does
//! not know, a version other than 1, or flags it does not know, and one
//! whose checksum, counts or numbers disagree: it is applied whole
//! ([`crate::storage::Database::apply_shipped`]) or not at all.
//!
//! The format is experimental until the design it belongs to is accepted
//! (`docs/design/0.7.0-file-per-agent.md`).

use std::sync::Arc;

use crate::error::{corrupt, Error, Result};
use crate::storage::manifest::SeqNo;

const MAGIC: &[u8; 8] = b"STRARC01";
const VERSION: u32 = 1;
/// Magic, version, flags, stream, db id, generation, first, last, count,
/// features' length.
const HEADER: usize = 8 + 4 + 4 + 16 + 8 + 8 + 8 + 8 + 4 + 4;
const FOOTER: usize = 4 + 8;

/// The feature of transactions that carry a term and a sequence number,
/// which every one here does.
pub const SEQ_NO: &str = "seq_no";

/// One transaction of a log object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub seq: SeqNo,
    /// When it was committed, in milliseconds since 1970 by the clock of
    /// the node that committed it (0: not known).
    pub commit_ms: u64,
    /// Its WAL records, ending with its commit.
    pub bytes: Arc<[u8]>,
}

/// A log object: whose transactions it holds, and the transactions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogObject {
    pub stream: [u8; 16],
    pub db_id: u64,
    pub generation: u64,
    pub entries: Vec<Entry>,
}

impl LogObject {
    /// The numbers of its first and last transactions; `None` when empty.
    pub fn range(&self) -> Option<(u64, u64)> {
        Some((self.entries.first()?.seq.seq, self.entries.last()?.seq.seq))
    }

    /// The bytes of its transactions, without the framing.
    pub fn payload_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.bytes.len()).sum()
    }

    /// Checks that the entries follow one another: numbers without gaps,
    /// terms that do not fall.
    fn check(&self) -> Result<()> {
        for w in self.entries.windows(2) {
            if w[1].seq.seq != w[0].seq.seq + 1 || w[1].seq.term < w[0].seq.term {
                return Err(corrupt(format!(
                    "log object: transaction {} of term {} does not follow {} of term {}",
                    w[1].seq.seq, w[1].seq.term, w[0].seq.seq, w[0].seq.term
                )));
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.check()?;
        if self.entries.len() > u32::MAX as usize || self.entries.iter().any(|e| e.bytes.len() > u32::MAX as usize) {
            return Err(Error::Schema("log object: too many transactions, or one too large".into()));
        }
        let (first, last) = self.range().unwrap_or((0, 0));
        let mut b = Vec::with_capacity(HEADER + SEQ_NO.len() + self.entries.len() * 28 + self.payload_bytes() + FOOTER);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&self.stream);
        b.extend_from_slice(&self.db_id.to_le_bytes());
        b.extend_from_slice(&self.generation.to_le_bytes());
        b.extend_from_slice(&first.to_le_bytes());
        b.extend_from_slice(&last.to_le_bytes());
        b.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        b.extend_from_slice(&(SEQ_NO.len() as u32).to_le_bytes());
        b.extend_from_slice(SEQ_NO.as_bytes());
        for e in &self.entries {
            b.extend_from_slice(&e.seq.term.to_le_bytes());
            b.extend_from_slice(&e.seq.seq.to_le_bytes());
            b.extend_from_slice(&e.commit_ms.to_le_bytes());
            b.extend_from_slice(&(e.bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(&e.bytes);
        }
        let crc = crc32fast::hash(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b.extend_from_slice(MAGIC);
        Ok(b)
    }

    pub fn decode(b: &[u8]) -> Result<LogObject> {
        let bad = |what: &str| corrupt(format!("log object: {what}"));
        if b.len() < HEADER + FOOTER || &b[..8] != MAGIC || &b[b.len() - 8..] != MAGIC {
            return Err(bad("not a log object (STRARC01), or cut short"));
        }
        let body = &b[..b.len() - FOOTER];
        let crc = u32::from_le_bytes(b[b.len() - FOOTER..b.len() - 8].try_into().expect("4 bytes"));
        if crc32fast::hash(body) != crc {
            return Err(bad("its checksum does not match"));
        }
        let u32_at = |at: usize| u32::from_le_bytes(body[at..at + 4].try_into().expect("4 bytes"));
        let u64_at = |at: usize| u64::from_le_bytes(body[at..at + 8].try_into().expect("8 bytes"));
        if u32_at(8) != VERSION {
            return Err(Error::Schema(format!("log object of version {}: this version of ChloeDB reads version {VERSION}", u32_at(8))));
        }
        if u32_at(12) != 0 {
            return Err(Error::Schema(format!("log object with flags {:#x}, which this version of ChloeDB does not know", u32_at(12))));
        }
        let stream: [u8; 16] = body[16..32].try_into().expect("16 bytes");
        let (db_id, generation, first, last) = (u64_at(32), u64_at(40), u64_at(48), u64_at(56));
        let (count, features_len) = (u32_at(64) as usize, u32_at(68) as usize);
        let mut at = HEADER;
        let features = body.get(at..at + features_len).ok_or_else(|| bad("its features run past its end"))?;
        let features = std::str::from_utf8(features).map_err(|_| bad("its features are not text"))?;
        if let Some(f) = features.split(',').filter(|f| !f.is_empty()).find(|f| *f != SEQ_NO) {
            return Err(Error::Schema(format!("log object with feature {f:?}, which this version of ChloeDB does not know")));
        }
        at += features_len;
        let mut entries = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            let head = body.get(at..at + 28).ok_or_else(|| bad("a transaction's header runs past its end"))?;
            let term = u64::from_le_bytes(head[..8].try_into().expect("8 bytes"));
            let seq = u64::from_le_bytes(head[8..16].try_into().expect("8 bytes"));
            let commit_ms = u64::from_le_bytes(head[16..24].try_into().expect("8 bytes"));
            let len = u32::from_le_bytes(head[24..28].try_into().expect("4 bytes")) as usize;
            at += 28;
            let bytes = body.get(at..at + len).ok_or_else(|| bad("a transaction runs past its end"))?;
            at += len;
            entries.push(Entry { seq: SeqNo { term, seq }, commit_ms, bytes: bytes.into() });
        }
        if at != body.len() {
            return Err(bad("bytes follow its last transaction"));
        }
        let o = LogObject { stream, db_id, generation, entries };
        if o.range().unwrap_or((0, 0)) != (first, last) {
            return Err(bad("its transactions are not those its header names"));
        }
        o.check()?;
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object() -> LogObject {
        let e = |term, seq, bytes: &[u8]| Entry { seq: SeqNo { term, seq }, commit_ms: 1_700_000_000_000 + seq, bytes: bytes.into() };
        LogObject { stream: [7; 16], db_id: 42, generation: 3, entries: vec![e(1, 5, b"five"), e(1, 6, b""), e(3, 7, b"seven, of a later term")] }
    }

    #[test]
    fn a_log_object_reads_back_as_written() {
        let o = object();
        let b = o.encode().unwrap();
        assert_eq!(LogObject::decode(&b).unwrap(), o);
        assert_eq!(o.range(), Some((5, 7)));
        let empty = LogObject { stream: [1; 16], ..LogObject::default() };
        assert_eq!(LogObject::decode(&empty.encode().unwrap()).unwrap(), empty);
    }

    #[test]
    fn a_damaged_or_cut_log_object_is_refused() {
        let b = object().encode().unwrap();
        for cut in [0, 7, HEADER, b.len() - 1, b.len() - FOOTER] {
            assert!(LogObject::decode(&b[..cut]).is_err(), "cut at {cut}");
        }
        for at in [9, 20, 50, HEADER + 3, b.len() - FOOTER - 2, b.len() - 10] {
            let mut d = b.clone();
            d[at] ^= 0x40;
            assert!(LogObject::decode(&d).is_err(), "a bit flipped at {at}");
        }
        // More bytes after the footer: the footer is no longer at the end.
        let mut longer = b.clone();
        longer.push(0);
        assert!(LogObject::decode(&longer).is_err());
    }

    #[test]
    fn transactions_out_of_order_are_not_written() {
        let mut o = object();
        o.entries.swap(0, 1);
        assert!(o.encode().is_err());
        let mut gap = object();
        gap.entries.remove(1);
        assert!(gap.encode().is_err());
        let mut older = object();
        older.entries[2].seq.term = 0;
        assert!(older.encode().is_err());
    }
}
