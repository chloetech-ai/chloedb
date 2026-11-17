//! Sound Explorer on ChloeDB: an app of ChloeDB's command center, and a page
//! of its own. Every recording and every ten seconds of it is a row with
//! the vector of how it sounds (CLAP, run by ChloeDB's server); searches,
//! moments, the clip someone hums, scenes, drifts and the map are SQL over
//! them.

pub mod api;
pub mod labels;
pub mod load;
pub mod schema;
pub mod sound;
pub mod strata;
pub mod vecs;
