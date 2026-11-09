//! Settings read from the environment, under both of their names.
//!
//! ChloeCtx was called Engram, and its settings were named `ENGRAM_*`. They
//! are named `CHLOECTX_*` now (`CHLOECTX_HOME`), and each one is still read
//! under its old name (`ENGRAM_HOME`) when the new one is not set: the
//! programs call [`adopt_old_names`] first thing in `main`, and the code
//! reads the new names only.

use std::ffi::OsString;

/// The prefix settings were named by before the rename.
pub const OLD: &str = "ENGRAM_";
/// The prefix settings are named by now.
pub const NEW: &str = "CHLOECTX_";

/// The old name of a setting given by its new one: `ENGRAM_HOME` for
/// `CHLOECTX_HOME`; `None` for a name without the new prefix.
pub fn old_name(new: &str) -> Option<String> {
    new.strip_prefix(NEW).map(|rest| format!("{OLD}{rest}"))
}

/// Copies every `ENGRAM_X` set into `CHLOECTX_X` when that one is not set,
/// so that code reading the new names sees the old ones too, and the new
/// name wins when both are set.
///
/// It changes the process's environment, which is safe only while no other
/// thread reads it: call it first thing in `main`.
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

/// A setting in the `env` block of a Claude Code settings file, under its
/// new name or, in a file an earlier install wrote, its old one.
pub fn in_settings<'a>(settings: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    let env = settings.get("env")?;
    env.get(name).or_else(|| env.get(old_name(name)?)).and_then(|v| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, since the environment is the process's: tests in parallel
    // would see each other's settings.
    #[test]
    fn old_names_are_adopted_and_the_new_one_wins() {
        assert_eq!(old_name("CHLOECTX_HOME").as_deref(), Some("ENGRAM_HOME"));
        assert_eq!(old_name("HOME"), None);

        std::env::remove_var("CHLOECTX_ENV_TEST_A");
        std::env::remove_var("CHLOECTX_ENV_TEST_B");
        std::env::set_var("ENGRAM_ENV_TEST_A", "old");
        std::env::set_var("ENGRAM_ENV_TEST_B", "old");
        std::env::set_var("CHLOECTX_ENV_TEST_B", "new");
        adopt_old_names();
        assert_eq!(std::env::var("CHLOECTX_ENV_TEST_A").as_deref(), Ok("old"));
        assert_eq!(std::env::var("CHLOECTX_ENV_TEST_B").as_deref(), Ok("new"));

        let s = serde_json::json!({"env": {"ENGRAM_DISTILL_MODEL": "haiku"}});
        assert_eq!(in_settings(&s, "CHLOECTX_DISTILL_MODEL"), Some("haiku"));
        let s = serde_json::json!({"env": {"ENGRAM_DISTILL_MODEL": "haiku", "CHLOECTX_DISTILL_MODEL": "sonnet"}});
        assert_eq!(in_settings(&s, "CHLOECTX_DISTILL_MODEL"), Some("sonnet"));
        assert_eq!(in_settings(&serde_json::json!({}), "CHLOECTX_DISTILL_MODEL"), None);
    }
}
