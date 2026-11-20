//! A post's features and richness read exactly as the Python app reads them,
//! on posts from a real Parley (tests/fixtures/craft.json, written by it).

use serde_json::Value;

#[test]
fn craft_matches_python() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/craft.json")).unwrap();
    let mut bad = Vec::new();
    for c in &cases {
        let got = chloedb_parley::evolve::craft(c["kind"].as_str().unwrap(), c["body"].as_str().unwrap_or(""), c["svg"].as_str(), c["app"].as_str());
        if got != c["want"] {
            bad.push(format!("{}\n  want {}\n  got  {}", c["body"].as_str().unwrap_or("").chars().take(80).collect::<String>(), c["want"], got));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.iter().take(5).cloned().collect::<Vec<_>>().join("\n"));
}
