//! Who called, as chloedb-server says in the headers it sets on what it
//! forwards to an app (`x-chloedb-principal`, `x-chloedb-scope`, and for a
//! key limited to tables `x-chloedb-tables`), which come with this app's
//! token. The app says it checks its callers (`"checks_callers": true` in
//! its `app.json`), so the server forwards it keys limited to tables and
//! changes from keys of write scope, and it refuses what they may not do:
//!
//! - a workbook has an owner, the key that made it (by its name): a key of
//!   every table with the read scope opens every workbook, a key limited to
//!   tables opens its own and those shared; changing one takes the write
//!   scope and being its owner, or an admin of every table (a workbook from
//!   before owners is an admin's to change);
//! - what reads or writes the cluster's tables (a query block, a table
//!   copied into a workbook, a sheet written out as a table) runs with this
//!   app's token narrowed to the caller's tables (`x-chloedb-as-tables`),
//!   so the server checks it as the caller's own key.
//!
//! A request without the headers (a program calling the app itself, with
//! its token) may do anything, as before the server sent them.

use axum::http::HeaderMap;

use crate::book::Ext;

/// 0 feed, 1 read, 2 write, 3 admin.
fn level(scope: &str) -> u8 {
    match scope.trim().to_ascii_lowercase().as_str() {
        "admin" => 3,
        "write" => 2,
        "read" => 1,
        _ => 0,
    }
}

const SCOPES: [&str; 4] = ["feed", "read", "write", "admin"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caller {
    /// The key's name (`token` for the server's token); `None` when the
    /// server said nothing (a program with the app's token).
    pub name: Option<String>,
    /// Its scope (a key limited to tables: its widest grant's).
    pub scope: u8,
    /// A key limited to tables: its grants as the server sent them
    /// (`*=read,ana_*=admin`).
    pub tables: Option<String>,
}

impl Caller {
    pub fn of(h: &HeaderMap) -> Caller {
        let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
        let scope = get("x-chloedb-scope");
        let name = get("x-chloedb-principal").or_else(|| scope.as_ref().map(|_| "someone".to_string()));
        Caller { scope: scope.as_deref().map_or(3, level), name, tables: get("x-chloedb-tables").filter(|t| !t.trim().is_empty()) }
    }

    /// A program with the app's token: anything.
    pub fn direct(&self) -> bool {
        self.name.is_none()
    }

    pub fn limited(&self) -> bool {
        self.tables.is_some()
    }

    /// What it may do to every workbook: admin of every table.
    fn admin(&self) -> bool {
        self.direct() || (!self.limited() && self.scope >= 3)
    }

    fn owns(&self, ext: &Ext) -> bool {
        self.name.is_some() && ext.owner.is_some() && ext.owner == self.name
    }

    /// Whether it may open a workbook.
    pub fn sees(&self, ext: &Ext) -> bool {
        self.direct() || (!self.limited() && self.scope >= 1) || self.owns(ext) || ext.shared
    }

    /// Refuses (403) opening a workbook it may not.
    pub fn may_see(&self, ext: &Ext) -> Result<(), (u16, String)> {
        if self.sees(ext) {
            return Ok(());
        }
        Err((403, format!("the workbook is {}'s, and not shared: {} opens only its own and shared ones", ext.owner.as_deref().unwrap_or("an admin"), self.who())))
    }

    /// Refuses (403) a change to a workbook it may not change.
    pub fn may_change(&self, ext: &Ext) -> Result<(), (u16, String)> {
        self.may_make()?;
        if self.admin() || self.owns(ext) {
            return Ok(());
        }
        Err((403, format!("the workbook is {}'s: {} changes only its own (an admin of every table changes any)", ext.owner.as_deref().unwrap_or("an admin"), self.who())))
    }

    /// Refuses (403) making a workbook without the write scope.
    pub fn may_make(&self) -> Result<(), (u16, String)> {
        if self.direct() || self.scope >= 2 {
            return Ok(());
        }
        Err((403, format!("making or changing a workbook takes a key that may write; {} has the {} scope", self.who(), SCOPES[self.scope as usize])))
    }

    fn who(&self) -> String {
        format!("the key {:?}", self.name.as_deref().unwrap_or("?"))
    }

    /// The owner a workbook it makes gets.
    pub fn owner(&self) -> Option<String> {
        self.name.clone()
    }

    /// The grants a statement it runs is narrowed to (`x-chloedb-as-tables`):
    /// its own tables, or every table at its scope; `None` for a program
    /// with the app's token.
    pub fn as_tables(&self) -> Option<String> {
        if self.direct() {
            return None;
        }
        Some(match &self.tables {
            Some(t) => t.clone(),
            None if self.scope == 0 => String::new(),
            None => format!("*={}", SCOPES[self.scope as usize]),
        })
    }

    /// Whether a workbook is in a listing (`whose`: mine, shared or all).
    pub fn lists(&self, ext: &Ext, whose: &str) -> bool {
        self.sees(ext)
            && match whose {
                "mine" => self.owns(ext),
                "shared" => !self.owns(ext),
                _ => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(h: &[(&'static str, &str)]) -> Caller {
        let mut m = HeaderMap::new();
        for (k, v) in h {
            m.insert(*k, v.parse().unwrap());
        }
        Caller::of(&m)
    }

    fn ext(owner: Option<&str>, shared: bool) -> Ext {
        Ext { owner: owner.map(str::to_string), shared, ..Default::default() }
    }

    #[test]
    fn workbooks_by_their_owners_and_the_callers_keys() {
        let analyst = caller(&[("x-chloedb-principal", "ana"), ("x-chloedb-scope", "admin"), ("x-chloedb-tables", "*=read,ana_*=admin")]);
        let other = caller(&[("x-chloedb-principal", "bob"), ("x-chloedb-scope", "read"), ("x-chloedb-tables", "bob_*=read")]);
        let reader = caller(&[("x-chloedb-principal", "r"), ("x-chloedb-scope", "read")]);
        let writer = caller(&[("x-chloedb-principal", "w"), ("x-chloedb-scope", "write")]);
        let admin = caller(&[("x-chloedb-principal", "token"), ("x-chloedb-scope", "admin")]);
        let program = caller(&[]);
        let mine = ext(Some("ana"), false);
        let theirs = ext(Some("w"), false);
        let old = ext(None, false);
        // Opening.
        assert!(analyst.sees(&mine) && !analyst.sees(&theirs) && !analyst.sees(&old));
        assert!(analyst.sees(&ext(Some("w"), true)), "shared");
        assert!(reader.sees(&mine) && reader.sees(&old), "a key of every table reads every workbook");
        assert!(!other.sees(&mine));
        // Changing.
        assert!(analyst.may_change(&mine).is_ok());
        assert_eq!(analyst.may_change(&ext(Some("w"), true)).map_err(|e| e.0), Err(403), "shared is to open, not to change");
        assert_eq!(reader.may_make().map_err(|e| e.0), Err(403));
        assert!(writer.may_change(&theirs).is_ok());
        assert_eq!(writer.may_change(&mine).map_err(|e| e.0), Err(403));
        assert_eq!(writer.may_change(&old).map_err(|e| e.0), Err(403), "a workbook from before owners is an admin's");
        assert!(admin.may_change(&mine).is_ok() && program.may_change(&old).is_ok());
        assert_eq!(other.may_make().map_err(|e| e.0), Err(403));
        // What its statements are narrowed to.
        assert_eq!(analyst.as_tables().as_deref(), Some("*=read,ana_*=admin"));
        assert_eq!(reader.as_tables().as_deref(), Some("*=read"));
        assert_eq!(program.as_tables(), None);
        // Listings.
        assert!(analyst.lists(&mine, "mine") && !analyst.lists(&mine, "shared"));
        assert!(reader.lists(&mine, "shared") && !reader.lists(&mine, "mine"));
        assert_eq!(analyst.owner().as_deref(), Some("ana"));
    }
}
