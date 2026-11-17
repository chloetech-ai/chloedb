//! Image Explorer on ChloeDB: 12.4 million public-domain and CC0 pictures
//! (PD12M) as rows, each with its CLIP vector, searched by words, by a
//! photo, by a detail of a picture, by a picture plus words, by colors and
//! by theme. Every search is one SQL statement against chloedb-server; the
//! app turns words and photos into vectors and makes thumbnails.

pub mod api;
pub mod clip;
pub mod colors;
pub mod load;
pub mod pd12m;
pub mod prepare;
pub mod schema;
pub mod strata;
pub mod themes;
pub mod thumbs;

/// A token for a server this app starts itself.
pub fn random_token() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    } else {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        b = (t ^ ((std::process::id() as u128) << 64)).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}
