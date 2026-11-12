# Substring index (`sub.*` sections): format, version 1

Status: **version 1, written and read by `chloedb/src/substring/`.** The
format is a one-way door from the first release whose code writes it:
until then it may still change where measurements say so, and after that
only as a new `version`, `fold` or `encoding`, which rule 1 lets an older
reader skip.

A model of the structures below runs in `substring_proto.py` (beside this
file): it builds them for random collections and checks rank, access,
counting, anchored patterns and locating against brute force
(`python3 substring_proto.py 400`: 400 collections, 183 kB of text). It is
a slow reference for the rules, not an implementation: it keeps the
structures in memory and writes no sections. The Rust code's own tests
(`src/substring/tests.rs`, `tests/substring_index.rs`) check the sections
against brute force and the row evaluator's `LIKE`.

## What the index is for

A per-field, per-segment index that finds a byte string anywhere in a
field's values, without a tokenizer: `col LIKE '%lit%'`, `ILIKE`, anchored
`LIKE 'lit%'` and `'%lit'`, equality, and the number of occurrences of a
string in a segment. It is an
FM-index: the Burrows-Wheeler transform of the values, in a wavelet tree,
with a sample of the suffix array that gives documents.

It is an acceleration structure and nothing else. Every answer it gives
can be had by scanning the stored values, so:

> **Rule 1.** Whatever a reader does not understand in a field's `sub`
> sections (a meta `version`, a `fold`, a part's `encoding`) makes it
> ignore that index, or that part, and scan those documents instead. It is
> never an error and never a different answer. Inconsistent data in a
> version the reader does understand is corruption, as in any section.

A segment without the sections (written before the option was set, or by
a build that skipped them) is scanned in the same way, and so is the
memtable.

## Opting in

- Schema: `body TEXT OPTIONS(substring = true)`, or `"substring": true` in
  the JSON schema, on a single-valued `TEXT` or `KEYWORD` field. The field
  need not be `indexed`, and must be `stored`: the stored values answer
  for a segment without the index, and check a candidate the index alone
  does not settle.
- Format 3 feature: `substring:1`, declared by a table with such a field.
  A version of ChloeDB from before it refuses the database by naming the
  feature. The reason is the schema option, which an older version would
  drop; the sections themselves could be ignored under rule 1.
- Build options (not part of the format): `substring.sample` (default 32)
  and `substring.part_bytes` (default 64 MiB).

## The indexed text

The values are indexed as their **bytes, exactly as stored**: no case
folding and no Unicode normalization.

This changes the first design, which indexed the NFKC-normalized,
lowercased text. Two reasons:

1. A substring index over folded text only stays correct if folding is a
   map from each character to a string, so that folding a value's
   substring gives a substring of the folded value. Lowercasing is such a
   map. NFKC is not: it composes across characters. `e` followed by U+0301
   becomes `é`, and a search for `e` in the composed bytes misses a value
   that `LIKE '%e%'` matches.
2. Nothing about Unicode tables is then frozen into the file. `LIKE` is
   answered from the index with no check against the stored value.
   `ILIKE` is answered at query time: the backward search branches over
   the case variants of each pattern character and keeps the ranges that
   are not empty, which are as many as the spellings the text really has
   (`error`, `Error`, `ERROR`), with a cap after which the query scans.

`fold` in the meta section is `"none"`. A later version can add a named
per-character fold; under rule 1 an older reader then scans.

**Documents and parts.** The documents of a segment that have a value for
the field (not NULL), in docID order, are cut into consecutive *parts* of
at most `substring.part_bytes` bytes of text (one document larger than
that is a part of its own). Each part is an independent index. Parts
bound the memory of a build, which sorts the suffixes of one part at a
time (about 6 bytes of memory per byte of text with 32-bit suffixes), and
they are the unit a later merge could carry over without sorting again
(below).

Inside a part, document `o` (its *ordinal*, from 0) has value `v_o`, and
the part's text is

```text
T = FF v_0 FF v_1 FF ... FF v_(D-1) FF          n = |T| = D + 1 + sum |v_o|
```

`FF` is the byte 0xFF, which never occurs in UTF-8, so no match crosses a
document. A separator on both sides of every value makes anchors plain
patterns: `LIKE 'abc%'` is the pattern `FF abc`, `LIKE '%abc'` is
`abc FF`, `= 'abc'` is `FF abc FF`, and `= ''` is `FF FF`. A NULL has no
ordinal and nothing in `T`, so it matches nothing, as in SQL. A part's
`n` is at most 2^32 − 2.

**The transform.** Suffixes of `T` are compared as unsigned bytes, and the
empty suffix is the smallest. Row `r` of the sorted suffixes (0 ≤ r ≤ n)
starts at text position `SA[r]`; row 0 is the empty suffix. `L[r]` is the
byte before the suffix, `T[SA[r] − 1]`. The row with `SA[r] = 0` has no
byte before it: its number is `primary`, and it is left out, so the
stored `L` has `n` bytes and row `r` is at index `r − (r > primary)`.
`C[c]` is 1 plus the number of bytes of `T` smaller than `c`.

`T` determines `L`. How a writer sorts suffixes is not part of the format.

## Sections

For a field with key `f` ([`Field::key`]), all integers little-endian:

| section | contents |
|---|---|
| `sub.f.meta` | JSON: version, fold, totals, and one entry per part |
| `sub.f.p<k>.bwt` | part `k`: counts, the tree's shape and the levels of the wavelet tree over `L` |
| `sub.f.p<k>.sa` | part `k`: the sampled rows and the ordinal of each |
| `sub.f.p<k>.docs` | part `k`, optional: the docID of each ordinal |

### `sub.f.meta`

```json
{
  "version": 1,
  "fold": "none",
  "docs": 1998213,
  "bytes": 912345678,
  "parts": [
    { "encoding": "hq4", "n": 67108801, "docs": 146002, "first_doc": 0,
      "primary": 5512907, "sample": 32, "samples": 2243377, "runs": 31876511 }
  ]
}
```

- `docs` and `bytes`: the documents with a value, and the bytes of their
  values, over all parts.
- Per part: `encoding` (`"hq4"`, the only one in version 1), `n`, `docs`
  (`D`), `primary`, `sample` (`s`, at least 1), `samples` (`m`), and
  `runs`, the number of runs of equal bytes in `L`. `runs` is not read by
  queries: it is there so that a later version can see which parts are
  repetitive enough for a run-length encoding (n / runs is about 3 on
  English prose, and thousands on versioned or templated text).
- `first_doc`: ordinal `o` is docID `first_doc + o`. A part with a `docs`
  section has no `first_doc`.

### `sub.f.p<k>.bwt`: the wavelet tree

**Shape.** Each byte value `c` that occurs in `T` has a code of `len[c]`
base-4 digits, 1 ≤ len ≤ 16, with the sum of 4^(−len[c]) at most 1. A
writer picks the lengths (a 4-ary Huffman code, shortened where it would
pass 16). The lengths alone fix the tree:

- Depth `k` has `I[k]` internal nodes, numbered from 0 left to right, and
  `I[0] = 1`. Node `i` of depth `k` has four child slots at depth `k + 1`,
  numbered `4i` to `4i + 3`.
- At depth `k + 1`, slots `0 .. I[k+1]` are internal nodes, the next
  slots are leaves, holding the bytes with `len = k + 1` in increasing
  byte order, and any slots after those are empty. `I[k+1]` follows from
  the lengths, bottom up: `I[k] = ceil((I[k+1] + leaves at depth k+1) / 4)`.
- The digits of `c`'s code are the path to its leaf.

Internal nodes come before leaves at every depth. That is what keeps the
levels simple: a level holds only the positions that go on, and the
children of a level's nodes follow each other in the next level in the
same order.

**Levels.** Level `k` (0 ≤ k < K, `K` the largest `len`) is a sequence of
`n_k` digits: for every index of `L` whose byte has `len > k`, digit `k`
of its code, ordered by the internal node of depth `k` it is in, and
inside a node by index in `L`. Level 0 holds all `n` positions. Node `i`
of depth `k` occupies `start[k][i] .. start[k][i+1]` of level `k`, and
for an internal child in slot `q = 4i + d`:

```text
start[k+1][q] = start[k][i] + (digits smaller than d in node i)
```

**Layout.**

| offset | type | field |
|---|---|---|
| 0 | u32 | `K`, the number of levels (1 to 16) |
| 4 | u32 | `header_len`: bytes before the first level, a multiple of 128 |
| 8 | u64 | `n` |
| 16 | u64 | `primary` |
| 24 | 256 × u32 | `count[c]`: occurrences of byte `c` in `T` |
| 1048 | 256 × u8 | `len[c]`, 0 for a byte that does not occur |
| 1304 | `K` × (u64, u64) | per level: `n_k`, and the byte offset of its blocks from the section's start, a multiple of 128 |
| then | per level, `I[k] + 1` × 4 × u32 | per internal node `i` and once more for the end of the level: `start[k][i]`, and the number of digits 0, 1 and 2 in level `k` before `start[k][i]` |
| | | zero padding to `header_len` |

The node table is redundant with the levels. It is stored so that a
reader finds a node's start and its counts without touching the level's
pages, which would cost a read per node when a cold segment is opened.

Level `k` is `floor(n_k / 448) + 1` blocks of 128 bytes (sixteen u64
words), so that a rank at position `n_k` has a block:

| words | contents |
|---|---|
| 0 | `c0` in the low 32 bits, `c1` in the high 32: digits 0 and 1 in the level before this block |
| 1 | `c2` in the low 32 bits; the high 32 are zero |
| 2 to 15 | 448 digits, two bits each: position `p` of the block at bits `2 (p mod 32)` and up of word `2 + p div 32`; positions past `n_k` are 0 |

Digits 3 before a position are the position minus the other three
counts. A rank reads one block, which is two adjacent cache lines.

### `sub.f.p<k>.sa`: samples

A row is *marked* when its suffix starts at one of these text positions,
for the document with ordinal `o` whose value is at `a .. a + |v_o|`:

- `a − 1`, the separator before it, and
- `a + i·s` for every `i ≥ 0` with `a + i·s < a + |v_o|`.

The mark carries `o`. The last separator and the empty suffix are not
marked. So from any row inside a document, at most `s − 1` steps back
through the text reach a marked row of the same document, and a row at a
separator (a pattern that starts with `FF`) is marked itself.

Rows are grouped in buckets of 512 (`R = ceil((n + 1) / 512)` buckets).

| offset | type | field |
|---|---|---|
| 0 | u64 | `m`, the number of marked rows |
| 8 | u32 | `w`, the bits of an ordinal: `bit_width(D − 1)`, at least 1 |
| 12 | u32 | zero |
| 16 | (`R` + 1) × u32 | `dir[b]`: marked rows in buckets before `b`; `dir[R] = m` |
| then, 8-byte aligned | u64 words | `m` entries of `9 + w` bits, entry `e` at bit `e (9 + w)`, low bits first, in row order: the low 9 bits are the row mod 512, the next `w` the ordinal |

Whether row `r` is marked, and its ordinal: look at entries
`dir[r >> 9] .. dir[(r >> 9) + 1]` for one whose low 9 bits are `r & 511`.
With `s = 32` a bucket holds 16 entries on average, in one or two cache
lines.

A sample holds a document, not a text position. Queries need documents,
and an ordinal takes about 18 bits where a position takes 26 or more. The
price: the index cannot say where in a value a match is. Highlighting
finds it again in the stored value.

### `sub.f.p<k>.docs`: ordinals to docIDs

Absent when ordinal `o` is docID `first_doc + o` (the part's documents are
consecutive docIDs, none NULL between them).

| offset | type | field |
|---|---|---|
| 0 | u64 | `D` |
| 8 | u32 | `wd`, the bits of an entry: `bit_width(n_docs)` of the segment |
| 12 | u32 | zero |
| 16 | u64 words | `D` entries of `wd` bits: the docID of each ordinal, or all ones for none |

"None" is for a part carried into a segment that no longer has the
document. Version 1's writer rebuilds every part at a merge, so it never
writes "none": it writes the section only for a part with NULL documents
between its own (the ordinals then skip docIDs). Version 1's reader reads
"none" all the same, so that a later writer can keep a part's `bwt` and
`sa` sections as they are across a merge and write only a new `docs`
section. Sorting suffixes is nearly all of the build's cost, and segments
are merged about log_8 of their final size times.

## Reading

`rank(c, r)`, the occurrences of byte `c` in rows before `r`: let
`x = r − (r > primary)`, the node be the root and `o = x`. For each digit
`d` of `c`'s code at level `k`, with the node's `start` and its count
`base_d` of digit `d` before `start` (both from the header):
`o = rank_d(level k, start + o) − base_d`, then go to the child in slot
`4i + d`. The `o` at the leaf is the answer. One block per level.

`access(r)`, the byte of row `r` and its rank, is the same walk, reading
the digit at `start + o` from the block it ranks in.

Counting a pattern `P` is the backward search: start with all rows
`[0, n + 1)`, and for each byte `c` of `P` from the last to the first,
`lo = C[c] + rank(c, lo)` and `hi = C[c] + rank(c, hi)`. `hi − lo` is the
number of occurrences of `P` in the part. `P` holds `FF` only as its first
or its last byte, and is never `FF` alone.

Listing documents: for each row of `[lo, hi)`, while the row is not
marked, `(c, j) = access(row)` and `row = C[c] + j`; then the mark's
ordinal, through the part's `first_doc` or `docs`, is a docID. More than
`s − 1` steps without a mark is corruption. A reader drops ordinals
without a document and applies the segment's deletions, as for any index.

A count from `hi − lo` alone is exact for occurrences in the part's text.
It is a count of rows only where each document has one occurrence and the
segment has no deletions and no ordinal without a document, which
anchored patterns give (`FF abc`: one per document).

## What a reader checks

At open, from the meta and the headers only: section lengths against `n`,
`K`, `n_k`, `m`, `R`, `w`, `wd` and `D`; `primary ≤ n`; the counts sum to
`n`; the lengths satisfy the sum above and give `I[0] = 1`; the node
table's starts rise to `n_k`; offsets are multiples of 128 and inside the
section. Nothing at open reads a level's blocks or the entries. Every
index a query computes from the data is checked against its array before
it is used, and walks are bounded by `s`, so a damaged section gives an
error or a wrong answer, never a read outside the section or a loop.

## Size and cost

Estimates, not benchmarks; the builder's pull request measures them and
names the machine. The levels cost `2 · 16/14` bits per digit, so the
average code length decides. Measured on byte counts of three kinds of
text in this repository (2026-11-12):

| text | bytes | order-0 entropy | 4-ary Huffman | levels |
|---|---|---|---|---|
| `docs/**/*.md` (prose) | 1.1 MB | 4.96 bits | 2.54 digits | 5.8 bits a byte, 0.73 n |
| `chloedb/src/**/*.rs` | 5.9 MB | 4.85 bits | 2.47 digits | 5.7 bits, 0.71 n |
| JSON reports | 46 MB | 4.34 bits | 2.24 digits | 5.1 bits, 0.64 n |

Samples at `s = 32` with 18-bit ordinals: 27 bits for each 32 bytes of
text plus the directory, about 0.9 bits a byte, 0.11 n. Two more entries
for each document matter only for very short values. So about 0.75 to
0.85 n in all for text like the above, 0.70 to 0.79 n at `s = 64`.

Work, in cache-missing reads: a rank is one block per level, about 2.5
levels on prose, so a pattern byte costs about 5 and counting a 10-byte
pattern about 50. Listing one occurrence costs on average `s / 2` steps
of about 4.5 reads each (the walk, and the bucket). That is some tens of
microseconds a document at `s = 32`: fine for thousands of occurrences,
too slow for millions. The planner knows the count before it lists, so
for a pattern with too many occurrences it scans, or lists only as far as
a `LIMIT` needs.

## What is and is not frozen

Frozen by the first release that writes it: the text `T` and its
separators, the section names and layouts above, how lengths give the
tree, the marking rule, and rule 1.

Free: the sampling rate and the part size (both recorded per part), the
code lengths a writer picks, how it sorts suffixes, when a merge rebuilds
or carries a part, and everything about planning queries.

Left open on purpose, each as a new `encoding`, `fold` or section that an
older reader skips under rule 1:

- a run-length encoding for repetitive parts, chosen by `n / runs`;
- lists of documents precomputed for the suffix-tree nodes with many
  occurrences, stored as ChloeDB posting lists, so that listing an
  unselective pattern costs its documents and not its occurrences (the
  idea of precomputed document lists in the document-retrieval
  literature, cited here from memory: Muthukrishnan 2002 and Sadakane
  2007 for listing, Gagie and others 2017 for the precomputed lists);
- a token variant (the alphabet is token ids, not bytes) for n-gram
  counts over a named tokenizer;
- positions inside a value, as a second sample array.

## What version 1's code does with it

Not part of the format, and free to change:

- **Build** (`build.rs`): at a flush and at a merge, per field, the values
  in docID order, cut into parts of `substring.part_bytes`; suffixes
  sorted by libsais; a 4-ary Huffman code, its counts halved until no code
  is longer than 16 digits. A memtable's segment has no index.
- **`LIKE`** (`like.rs`): a pattern with one literal and `%` only at its
  ends (`lit`, `lit%`, `%lit`, `%lit%`), case-sensitive, is answered from
  the index alone. Any other pattern, and every `ILIKE`, takes its
  candidates from the literal piece with the fewest rows (a piece with no
  row means no match) and tests each candidate's stored value with the
  row evaluator's rule. `ILIKE` searches the spellings of each character
  that lowercase to the pattern's, keeping at most 64 ranges; past that
  the part is scanned.
- **Listing or scanning**: a part where the pattern has more candidates
  than one for each 4096 bytes of its text is scanned (one for each 1024
  bytes when the index answers alone and no value is read, and for an
  `ILIKE`, whose scan folds every value's case), since listing
  costs about `sample / 2` steps a row. The numbers are from
  `results/substring/`.
- **Not yet**: `ESCAPE`; a function that returns the count or a position
  (`SubstringIndex::count` gives a segment's count from Rust); carrying a
  part across a merge; listing only as far as a `LIMIT` needs; any of the
  encodings left open above.

## Decisions

The five choices this format makes, each open to the owner's review until
a release writes it:

1. **Raw bytes, no folding.** `LIKE` is exact from the index and `ILIKE`
   branches at query time.
2. **Samples as buckets of (row, ordinal) entries**: about 0.9 bits a
   byte and two reads a step. The alternatives were a plain bitvector with
   rank (about 1.9 bits a byte with the ordinals, one read) or marks
   interleaved in level 0's blocks (about 2.4 bits, no extra read).
3. **Parts with an ordinal-to-docID map**, read from version 1 so that a
   later writer can carry parts across merges without sorting again.
4. **Documents, not positions, in the samples.** Highlighting finds the
   match again in the stored value.
5. **Lists for unselective patterns are left for later**, as an encoding
   an older reader skips. Listing millions of occurrences is the weak
   point until then, and the scan rule above bounds it.
