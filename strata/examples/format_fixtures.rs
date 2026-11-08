//! Writes the fixtures of formats 2 and 3, of numbered writes and of rank
//! profiles that `tests/format_fixtures.rs` opens with every later version
//! of Strata (`tests/fixtures/README.md`), or with `functions` after the
//! directory, the set of stored functions:
//!
//! ```bash
//! cargo run --release --example format_fixtures -- /tmp/fixtures
//! cargo run --release --example format_fixtures -- /tmp/fixtures functions
//! ```

#[path = "../tests/fixtures/writer.rs"]
mod writer;

fn main() {
    let out = std::env::args().nth(1).expect("the directory to write the fixtures in");
    match std::env::args().nth(2).as_deref() {
        None => writer::write_all(std::path::Path::new(&out)),
        Some("functions") => writer::write_functions(std::path::Path::new(&out)),
        Some(other) => panic!("{other}: the second argument is functions, or none"),
    }
}
