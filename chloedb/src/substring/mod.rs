//! The substring index: finds a byte string anywhere in a field's values,
//! without a tokenizer.
//!
//! A field with `OPTIONS(substring = true)` gets, in each segment, an
//! FM-index of its values: the Burrows-Wheeler transform of their bytes in
//! a 4-ary Huffman-shaped wavelet tree, and a sample of the suffix array
//! that gives documents. `LIKE` and `ILIKE` on the field are answered from
//! it ([`crate::query`]); a segment without one (a memtable's, or one whose
//! index this version does not read) is scanned, with the same answer.
//!
//! The format is specified in `chloedb/notes/substring.md`, section by
//! section, and `chloedb/notes/substring_proto.py` is a slow model of it.
//! In short:
//!
//! - The values are indexed as their bytes, with no folding. A segment's
//!   documents that have a value are cut into consecutive *parts* of at
//!   most `substring.part_bytes` bytes, each an index of its own.
//! - A part's text is `FF v_0 FF v_1 ... FF v_(D-1) FF`: 0xFF never occurs
//!   in UTF-8, so no match crosses a document, and a pattern that starts
//!   or ends with it is anchored.
//! - `sub.<field>.meta` (JSON) lists the parts; `sub.<field>.p<k>.bwt` is
//!   part `k`'s wavelet tree, `sub.<field>.p<k>.sa` its sampled rows with
//!   the document ordinal of each, and the optional `sub.<field>.p<k>.docs`
//!   the docID of each ordinal.
//!
//! [`build`] writes the sections, [`read`] reads them and [`like`] turns a
//! `LIKE` pattern into searches of them.

pub mod build;
pub mod like;
pub mod read;

pub use build::{field_builder, SubstringBuilder};
pub use like::Like;
pub use read::SubstringIndex;

use serde::{Deserialize, Serialize};

/// The format 3 feature of a table with a substring field.
pub const FEATURE: &str = "substring:1";

/// The byte between values: never part of UTF-8.
pub const SEP: u8 = 0xFF;

/// The version of the sections this module writes and reads.
pub const VERSION: u64 = 1;

/// A part's encoding in version 1: a Huffman-shaped 4-ary wavelet tree.
pub const ENCODING: &str = "hq4";

/// Version 1 indexes the bytes as they are.
pub const FOLD: &str = "none";

/// Rows per bucket of the samples.
pub const BUCKET: u64 = 512;

/// Digits per block of a level, and the block's bytes.
pub const BLOCK_DIGITS: usize = 448;
pub const BLOCK_BYTES: usize = 128;
const BLOCK_WORDS: usize = BLOCK_BYTES / 8;

/// The longest code, in digits.
pub const MAX_LEN: usize = 16;

/// Where the per-level table of a `bwt` section starts.
const LEVELS_AT: usize = 24 + 256 * 4 + 256;

/// The most bytes of text in a part (`substring.part_bytes`'s limit): the
/// suffixes are sorted as 32-bit numbers.
pub const MAX_PART_BYTES: usize = 1 << 30;

/// `substring.part_bytes` by default.
pub const DEFAULT_PART_BYTES: usize = 64 << 20;

/// `substring.sample` by default.
pub const DEFAULT_SAMPLE: u32 = 32;

/// A field's section `part` (`meta`, `p0.bwt`, ...).
pub fn section(key: &str, part: &str) -> String {
    format!("sub.{key}.{part}")
}

/// `sub.<field>.meta`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub version: u64,
    pub fold: String,
    /// The documents with a value, over all parts.
    pub docs: u64,
    /// The bytes of their values.
    pub bytes: u64,
    pub parts: Vec<PartMeta>,
}

/// One part of [`Meta`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartMeta {
    pub encoding: String,
    /// The bytes of the part's text, separators included.
    pub n: u64,
    /// Its documents.
    pub docs: u64,
    /// Ordinal `o` is docID `first_doc + o`; a part with a `docs` section
    /// has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_doc: Option<u64>,
    /// The row of the suffix that starts the text.
    pub primary: u64,
    /// A sample every this many bytes of a value.
    pub sample: u64,
    /// The sampled rows.
    pub samples: u64,
    /// Runs of equal bytes in the transform (not read by queries).
    pub runs: u64,
}

/// The shape of a part's wavelet tree, which the code lengths fix: at each
/// depth the internal nodes come first, then the leaves in byte order.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    /// The number of levels: the longest code.
    pub k: usize,
    /// Internal nodes at each depth, `0..=k` (none at depth `k`).
    pub internal: Vec<u32>,
    /// The bytes whose leaf is at each depth, in byte order.
    pub leaves: Vec<Vec<u8>>,
    /// Each byte's code, a digit per level.
    pub code: Box<[[u8; MAX_LEN]; 256]>,
    /// Each byte's code length, 0 for a byte without one.
    pub len: [u8; 256],
}

impl Shape {
    /// The tree of these code lengths, or why they are not a code.
    pub fn from_lens(len: &[u8; 256]) -> Result<Shape, String> {
        let k = *len.iter().max().unwrap() as usize;
        if k == 0 || k > MAX_LEN {
            return Err(format!("a longest code of {k} digits"));
        }
        let mut leaves = vec![Vec::new(); k + 1];
        for c in 0..256 {
            if len[c] > 0 {
                leaves[len[c] as usize].push(c as u8);
            }
        }
        let mut internal = vec![0u32; k + 1];
        for d in (0..k).rev() {
            internal[d] = (internal[d + 1] + leaves[d + 1].len() as u32).div_ceil(4);
        }
        if internal[0] != 1 {
            return Err("code lengths that are not a prefix code".to_string());
        }
        let mut code = Box::new([[0u8; MAX_LEN]; 256]);
        for d in 1..=k {
            for (j, &c) in leaves[d].iter().enumerate() {
                let mut slot = internal[d] + j as u32;
                for at in (0..d).rev() {
                    code[c as usize][at] = (slot & 3) as u8;
                    slot >>= 2;
                }
                debug_assert_eq!(slot, 0);
            }
        }
        Ok(Shape { k, internal, leaves, code, len: *len })
    }

    /// The internal node of depth `depth` that byte `c`'s code passes
    /// through (`depth` below its length).
    pub fn node_at(&self, c: u8, depth: usize) -> u32 {
        let l = self.len[c as usize] as usize;
        let j = self.leaves[l].binary_search(&c).expect("a byte with a code") as u32;
        (self.internal[l] + j) >> (2 * (l - depth))
    }
}

/// The starts of `text`'s suffixes in sorted order, compared as unsigned
/// bytes with a suffix that is a prefix of another before it (the empty
/// suffix is left out).
pub(crate) fn suffix_array(text: &[u8]) -> crate::error::Result<Vec<i32>> {
    let n = i32::try_from(text.len()).map_err(|_| crate::error::Error::Schema("a part of the substring index over 2 GiB".to_string()))?;
    let mut sa = vec![0i32; text.len()];
    if cfg!(miri) || text.len() < 2 {
        // No foreign code under Miri: sorted directly, for small texts.
        let mut order: Vec<i32> = (0..n).collect();
        order.sort_by(|&a, &b| text[a as usize..].cmp(&text[b as usize..]));
        return Ok(order);
    }
    // SAFETY: `text` and `sa` hold `n` elements each, as libsais asks.
    let r = unsafe { libsais_sys::libsais::libsais(text.as_ptr(), sa.as_mut_ptr(), n, 0, std::ptr::null_mut()) };
    if r != 0 {
        return Err(crate::error::Error::Schema(format!("sorting the suffixes of a substring index failed ({r})")));
    }
    Ok(sa)
}

#[cfg(test)]
mod tests;
