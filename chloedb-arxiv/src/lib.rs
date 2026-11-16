//! arXiv on ChloeDB: an app of ChloeDB's command center, and a page of its
//! own. Every paper is a row; search, questions, topics, trends, people,
//! citations, recommendations and digests are SQL over them.

pub mod answer;
pub mod born;
pub mod cite;
pub mod claims;
pub mod classics;
pub mod code;
pub mod collab;
pub mod api;
pub mod ask;
pub mod concepts;
pub mod digest;
pub mod fulltext;
pub mod gaps;
pub mod labs;
pub mod live;
pub mod load;
pub mod pages;
pub mod paper;
pub mod prepare;
pub mod raw;
pub mod related;
pub mod schema;
pub mod search;
pub mod spread;
pub mod strata;
pub mod understand;
pub mod watch;

/// 128 random bits as hex, from /dev/urandom (the time and the process id
/// where there is none): a server's token, a team feed's link.
pub fn random_token() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    } else {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        b = (t ^ (std::process::id() as u128) << 64).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}
