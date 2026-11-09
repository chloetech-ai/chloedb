//! Settings read from the environment, under both of their names.
//!
//! ChloeDB was called Strata, and its settings were named `STRATA_*`. Each
//! one is now also read as `CHLOEDB_*` (`CHLOEDB_SIMD` for `STRATA_SIMD`),
//! and the new name wins when both are set. The code still names a setting
//! by its old name; [`var`] and [`var_os`] look up the new one first.
//!
//! A program whose settings are read by something other than these two
//! functions (clap's `env = "STRATA_..."`, or `std::env` directly) calls
//! [`adopt_new_names`] first thing in `main`, before any thread starts.

use std::ffi::OsString;

/// The prefix settings were named by before the rename.
pub const OLD: &str = "STRATA_";
/// The prefix settings are named by now.
pub const NEW: &str = "CHLOEDB_";

/// The new name of a setting given by its old one: `CHLOEDB_SIMD` for
/// `STRATA_SIMD`; `None` for a name without the old prefix.
pub fn new_name(old: &str) -> Option<String> {
    old.strip_prefix(OLD).map(|rest| format!("{NEW}{rest}"))
}

/// The setting `name` (its old name, `STRATA_...`): its new name's value
/// when that is set, else the old one's.
pub fn var_os(name: &str) -> Option<OsString> {
    new_name(name).and_then(std::env::var_os).or_else(|| std::env::var_os(name))
}

/// [`var_os`] as text, as [`std::env::var`] gives it.
pub fn var(name: &str) -> Result<String, std::env::VarError> {
    match var_os(name) {
        Some(v) => v.into_string().map_err(std::env::VarError::NotUnicode),
        None => Err(std::env::VarError::NotPresent),
    }
}

/// Copies every `CHLOEDB_X` set into `STRATA_X`, so that code reading the
/// old names (clap's `env` attributes among them) sees the new ones too.
///
/// It changes the process's environment, which is safe only while no other
/// thread reads it: call it first thing in `main`, before a runtime starts.
pub fn adopt_new_names() {
    let new: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let rest = k.to_str()?.strip_prefix(NEW)?;
            (!rest.is_empty()).then(|| (OsString::from(format!("{OLD}{rest}")), v))
        })
        .collect();
    for (k, v) in new {
        std::env::set_var(k, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, since the environment is the process's: tests in parallel
    // would see each other's settings.
    #[test]
    fn both_names_are_read_and_the_new_one_wins() {
        assert_eq!(new_name("STRATA_SIMD").as_deref(), Some("CHLOEDB_SIMD"));
        assert_eq!(new_name("HOME"), None);

        std::env::remove_var("CHLOEDB_ENV_TEST_A");
        std::env::set_var("STRATA_ENV_TEST_A", "old");
        assert_eq!(var("STRATA_ENV_TEST_A").as_deref(), Ok("old"));
        std::env::set_var("CHLOEDB_ENV_TEST_A", "new");
        assert_eq!(var("STRATA_ENV_TEST_A").as_deref(), Ok("new"));
        std::env::set_var("CHLOEDB_ENV_TEST_B", "b");
        assert_eq!(var_os("STRATA_ENV_TEST_B"), Some(OsString::from("b")));
        assert!(var("STRATA_ENV_TEST_C").is_err());

        adopt_new_names();
        assert_eq!(std::env::var("STRATA_ENV_TEST_A").as_deref(), Ok("new"));
        assert_eq!(std::env::var("STRATA_ENV_TEST_B").as_deref(), Ok("b"));
        for k in ["STRATA_ENV_TEST_A", "CHLOEDB_ENV_TEST_A", "STRATA_ENV_TEST_B", "CHLOEDB_ENV_TEST_B"] {
            std::env::remove_var(k);
        }
    }
}
