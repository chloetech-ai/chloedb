//! Experiments on ChloeDB: simulated users with the attributes a report
//! segments by, six click models whose every chance is exact, ChloeDB's
//! logging policies (exploration with exact propensities, as the rank log
//! logs them) and off-policy estimators checked against the truth the
//! simulator knows. An app of the command center (`serve`, later), a
//! client of chloedb-server over HTTP.

pub mod ab;
pub mod app;
pub mod check;
pub mod click;
pub mod estimate;
pub mod interleave;
pub mod judge;
pub mod live;
pub mod personas;
pub mod policy;
pub mod repeat;
pub mod server;
pub mod sim;
pub mod site;
pub mod source;
pub mod stats;
pub mod study;
pub mod trial;
pub mod tune;
pub mod users;
