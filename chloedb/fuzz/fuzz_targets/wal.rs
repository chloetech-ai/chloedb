#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| chloedb_fuzz::wal(data));
