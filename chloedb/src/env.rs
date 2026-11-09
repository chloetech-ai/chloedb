//! Settings read from the environment, under both of their names.
//!
//! ChloeDB was called Strata, and its settings were named `STRATA_*`. They
//! are named `CHLOEDB_*` now (`CHLOEDB_SIMD`), and each one is still read
//! under its old name (`STRATA_SIMD`) when the new one is not set. The code
//! names a setting by its new name; [`var`] and [`var_os`] fall back to the
//! old one.
//!
//! A program whose settings are read by something other than these two
//! functions (clap's `env = "CHLOEDB_..."`, or `std::env` directly) calls
//! [`adopt_old_names`] first thing in `main`, before any thread starts.

use std::ffi::OsString;

/// The prefix settings were named by before the rename.
pub const OLD: &str = "STRATA_";
/// The prefix settings are named by now.
pub const NEW: &str = "CHLOEDB_";

/// The old name of a setting given by its new one: `STRATA_SIMD` for
/// `CHLOEDB_SIMD`; `None` for a name without the new prefix.
pub fn old_name(new: &str) -> Option<String> {
    new.strip_prefix(NEW).map(|rest| format!("{OLD}{rest}"))
}

/// The setting `name` (its new name, `CHLOEDB_...`): its value when set,
/// else its old name's.
pub fn var_os(name: &str) -> Option<OsString> {
    std::env::var_os(name).or_else(|| old_name(name).and_then(std::env::var_os))
}

/// [`var_os`] as text, as [`std::env::var`] gives it.
pub fn var(name: &str) -> Result<String, std::env::VarError> {
    match var_os(name) {
        Some(v) => v.into_string().map_err(std::env::VarError::NotUnicode),
        None => Err(std::env::VarError::NotPresent),
    }
}

/// Copies every `STRATA_X` set into `CHLOEDB_X` when that one is not set,
/// so that code reading the new names (clap's `env` attributes among them)
/// sees the old ones too, and the new name wins when both are set.
///
/// It changes the process's environment, which is safe only while no other
/// thread reads it: call it first thing in `main`, before a runtime starts.
pub fn adopt_old_names() {
    let old: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let rest = k.to_str()?.strip_prefix(OLD)?;
            let new = format!("{NEW}{rest}");
            (!rest.is_empty() && std::env::var_os(&new).is_none()).then(|| (OsString::from(new), v))
        })
        .collect();
    for (k, v) in old {
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
        assert_eq!(old_name("CHLOEDB_SIMD").as_deref(), Some("STRATA_SIMD"));
        assert_eq!(old_name("HOME"), None);

        std::env::remove_var("CHLOEDB_ENV_TEST_A");
        std::env::set_var("STRATA_ENV_TEST_A", "old");
        assert_eq!(var("CHLOEDB_ENV_TEST_A").as_deref(), Ok("old"));
        std::env::set_var("CHLOEDB_ENV_TEST_A", "new");
        assert_eq!(var("CHLOEDB_ENV_TEST_A").as_deref(), Ok("new"));
        std::env::set_var("STRATA_ENV_TEST_B", "b");
        assert_eq!(var_os("CHLOEDB_ENV_TEST_B"), Some(OsString::from("b")));
        assert!(var("CHLOEDB_ENV_TEST_C").is_err());

        adopt_old_names();
        assert_eq!(std::env::var("CHLOEDB_ENV_TEST_A").as_deref(), Ok("new"));
        assert_eq!(std::env::var("CHLOEDB_ENV_TEST_B").as_deref(), Ok("b"));
        for k in ["STRATA_ENV_TEST_A", "CHLOEDB_ENV_TEST_A", "STRATA_ENV_TEST_B", "CHLOEDB_ENV_TEST_B"] {
            std::env::remove_var(k);
        }
    }
}
