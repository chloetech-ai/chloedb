//! The fuzz targets' bodies (`fuzz/src/lib.rs`) over pseudo-random
//! inputs, so that CI runs them on stable without cargo fuzz. Longer runs:
//! `cargo +nightly fuzz run <target>` in `chloedb/fuzz`.

#[path = "../fuzz/src/lib.rs"]
#[allow(dead_code)]
mod harness;

/// `n` inputs of 1 to `max` bytes, the same on every run (CHLOEDB_FUZZ_RUNS
/// scales `n`).
fn inputs(seed: u64, n: usize, max: usize) -> impl Iterator<Item = Vec<u8>> {
    let scale: f64 = chloedb::env::var("CHLOEDB_FUZZ_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        x >> 33
    };
    (0..(n as f64 * scale) as usize).map(move |_| {
        let len = 1 + next() as usize % max;
        (0..len).map(|_| next() as u8).collect()
    })
}

#[test]
fn the_queries_run_on_the_intact_seed() {
    harness::intact();
}

#[test]
fn damaged_loom_lists() {
    for data in inputs(1, 3000, 40) {
        harness::loom(&data);
    }
}

#[test]
fn damaged_segments() {
    for data in inputs(2, 300, 30) {
        harness::segment(&data);
    }
}

#[test]
fn damaged_wals() {
    for data in inputs(3, 1200, 30) {
        harness::wal(&data);
    }
}

#[test]
fn damaged_manifests() {
    for data in inputs(4, 3000, 30) {
        harness::manifest(&data);
    }
    // Found by the nightly fuzzer: a freelist whose counts overflow when
    // summed, which decoding added up unchecked.
    harness::manifest(&[0x01, 0x8f, 0x10, 0x00, 0x61, 0x6d, 0x65, 0xff]);
}

#[test]
fn arbitrary_sql() {
    for q in harness::QUERIES {
        harness::sql(q.as_bytes());
    }
    for data in inputs(5, 2000, 80) {
        // Mutated queries: a known query with bytes of `data` spliced in.
        let q = harness::QUERIES[data[0] as usize % harness::QUERIES.len()].as_bytes();
        let at = data.get(1).map_or(0, |&b| b as usize % q.len());
        let mut m = q[..at].to_vec();
        m.extend_from_slice(&data[2.min(data.len())..data.len().min(8)]);
        m.extend_from_slice(&q[(at + data.len() % 7).min(q.len())..]);
        harness::sql(&m);
        harness::sql(&data);
    }
}

#[test]
fn damaged_media_and_arbitrary_media_values() {
    for v in [r#"{"uri":"a.png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB"}"#, r#"{"data":"AAAA"}"#, r#"{"uri":"file:///etc/passwd"}"#, "https://cdn/a.png", r#"{"uri":"x","data":"!!"}"#] {
        harness::media(&[[0].as_slice(), v.as_bytes()].concat());
    }
    for data in inputs(7, 4000, 60) {
        harness::media(&data);
    }
}

#[test]
fn arbitrary_text_through_every_analyzer() {
    for t in ["The Quick brown foxes", "東京都庁に行きました。", "서울특별시에 갑니다.", "ภาษาไทยง่ายมาก", "हिन्दी भाषा", "مَكْتَبَة", "ＣｈｌｏｅＤＢ ３.１４ ﬁle café I’ll", ""] {
        harness::analyzer(&[[0].as_slice(), t.as_bytes()].concat());
    }
    for data in inputs(8, 1500, 48) {
        harness::analyzer(&data);
    }
}

#[test]
fn patterns_on_intact_and_damaged_substring_indexes() {
    // Intact: every pattern's answer is checked against a scan.
    for data in inputs(9, 6000, 14) {
        let how = data[0] & 0xf1 | 0x30;
        harness::substring(&[&[how][..], &data[1..]].concat());
        // Short runs of the frequent characters between `%`: patterns
        // that match something.
        let likely: Vec<u8> = data[1..].iter().map(|b| if b % 4 == 0 { 21 } else { b % 8 }).collect();
        harness::substring(&[&[how][..], &likely].concat());
    }
    for p in ["%", "", "_", "%a%", "%İ%", "%i\u{307}%", "%σ", "ς%", "%É_東%", "a%b%n", "%\u{0}%", "%%_%%"] {
        for how in [0xf2, 0xf3] {
            harness::substring(&[&[how][..], p.as_bytes()].concat());
        }
    }
    // An input with its first byte alone: the empty pattern, which the
    // fuzzer found to panic as `ILIKE` in a build that checks overflow.
    for how in [0xd0, 0xd1, 0x00, 0x01, 0x03] {
        harness::substring(&[how]);
    }
    // Damaged: an error or an answer.
    for data in inputs(10, 6000, 40) {
        harness::substring(&[&[data[0] | 4][..], &data[1..]].concat());
    }
}

#[test]
fn arbitrary_text_queries() {
    for q in ["alpha", "\"bravo charlie\" -delta", "(echo OR golf) AND NOT kilo", "lim* india~2", "a'b", ""] {
        harness::text_query(q.as_bytes());
    }
    let alphabet = b"abcdeghiklo ()\"'*~-+:^0123|&!";
    for data in inputs(6, 2000, 40) {
        harness::text_query(&data);
        let mapped: Vec<u8> = data.iter().map(|b| alphabet[*b as usize % alphabet.len()]).collect();
        harness::text_query(&mapped);
    }
}
