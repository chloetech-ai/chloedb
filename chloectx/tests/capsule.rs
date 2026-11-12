//! Capsules: `pack` cuts the chosen kinds of a store into one checked file,
//! redacted again; `open` refuses a file that does not match its manifest;
//! the mount's tools answer from it. Uses the hash embedder, so no model
//! files are needed.

use std::sync::Arc;

use chloectx::capsule::{self, PackOptions};
use chloectx::embed::HashEmbedder;
use chloectx::record::{Draft, Kind};
use chloectx::store::{Store, StoreOptions};
use serde_json::json;

fn github_token() -> String {
    format!("{}_{}", "ghp", "Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0FeDcBa98")
}

fn source(dir: &std::path::Path) -> Store {
    let store = Store::open(&dir.join("memory.strata"), Arc::new(HashEmbedder { dim: 32 }), StoreOptions { sync: false, ..Default::default() }).unwrap();
    store
        .append(
            "s1",
            1,
            1_700_000_000,
            vec![
                Draft::new(Kind::Prompt, "Purchase suggestions should order up to three times the reorder point."),
                Draft::new(Kind::Answer, "Understood: suggestions will order up to three times the reorder point."),
                Draft::new(Kind::Decision, "Purchase suggestions order up to 3x the reorder point, because the supplier ships weekly."),
                Draft::new(Kind::ToolResult, "raw test output: 42 passed, 0 failed").tag("Bash"),
            ],
        )
        .unwrap();
    // A record written before redaction knew a secret's shape: stored as it was.
    let text = format!("Never commit the deploy token {} to the repository.", github_token());
    let rec = chloectx::record::Record::from_draft(Draft::new(Kind::Preference, text), "s1", 1, store.last_seq() + 1, 1_700_000_000);
    let emb = store.embedder.embed(&[rec.embed_text().as_str()]).unwrap();
    store.bulk_insert(&[rec], emb).unwrap();
    store.flush().unwrap();
    store
}

fn holds(path: &std::path::Path, s: &str) -> bool {
    std::fs::read(path).unwrap().windows(s.len()).any(|w| w == s.as_bytes())
}

#[test]
fn a_capsule_holds_the_chosen_kinds_redacted_and_answers_questions() {
    let dir = tempfile::tempdir().unwrap();
    let store = source(dir.path());
    let out = dir.path().join("cap");
    let m = capsule::pack(&store, "hash:32", "/work/stockroom", &out, &PackOptions::default()).unwrap();

    assert_eq!(m.format, capsule::FORMAT);
    assert_eq!(m.title, "stockroom");
    assert_eq!(m.records, 4, "the tool result stays behind: {:?}", m.by_kind);
    assert!(!m.by_kind.contains_key("tool_result"));
    assert_eq!(m.redacted, 1);
    assert_eq!(m.dim, 32);
    assert!(!holds(&out.join("memory.strata"), &github_token()), "the token was redacted on the way out");
    assert!(!holds(&out.join("memory.strata"), "42 passed"));
    assert_eq!(chloectx::embed::file_sha256(&out.join("memory.strata")).unwrap(), m.sha256);
    assert!(capsule::pack(&store, "hash:32", "/work/stockroom", &out, &PackOptions::default()).is_err(), "an existing capsule is never overwritten");

    let caps = vec![capsule::open(&out, &dir.path().join("work"), Some(Arc::new(HashEmbedder { dim: 32 }))).unwrap()];
    assert_eq!(caps[0].store.len(), 4);

    let (text, err, ids) = capsule::call(&caps, "capsule_ask", &json!({"question": "how much should purchase suggestions order?"}));
    assert!(!err, "{text}");
    assert!(text.contains("three times the reorder point") || text.contains("3x the reorder point"), "{text}");
    assert!(!ids.is_empty());
    assert!(text.contains(&format!("sha256:{}", &m.sha256[..12])), "the answer names the capsule: {text}");

    let (text, err, _) = capsule::call(&caps, "capsule_sql", &json!({"sql": "SELECT kind, COUNT(*) FROM memory GROUP BY kind ORDER BY 2 DESC LIMIT 10"}));
    assert!(!err, "{text}");
    assert!(text.contains("decision | 1"), "{text}");

    let (text, err, _) = capsule::call(&caps, "capsule_sql", &json!({"sql": "DELETE FROM memory"}));
    assert!(err, "a capsule is read-only: {text}");

    let (text, err, ids) = capsule::call(&caps, "capsule_search", &json!({"query": "reorder point", "kinds": ["decision"]}));
    assert!(!err, "{text}");
    assert_eq!(ids.len(), 1, "{text}");

    let (text, err, _) = capsule::call(&caps, "capsule_ask", &json!({"question": "x", "capsule": "nope"}));
    assert!(err && text.contains("stockroom"), "{text}");

    // Mounting never changes the capsule.
    drop(caps);
    assert_eq!(chloectx::embed::file_sha256(&out.join("memory.strata")).unwrap(), m.sha256);
}

#[test]
fn a_capsule_that_does_not_match_its_manifest_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = source(dir.path());
    let out = dir.path().join("cap");
    capsule::pack(&store, "hash:32", "/work/p", &out, &PackOptions { kinds: vec![Kind::Decision], ..Default::default() }).unwrap();
    let path = out.join("memory.strata");
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    let e = capsule::open(&out, &dir.path().join("work"), Some(Arc::new(HashEmbedder { dim: 32 }))).err().expect("refused");
    assert!(e.to_string().contains("does not match its manifest"), "{e}");
}
