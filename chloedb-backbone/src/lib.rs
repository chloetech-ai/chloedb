//! The internet's backbone on ChloeDB: every BGP update RIPE RIS hears,
//! checked against RPKI and CAIDA as it arrives, beside PeeringDB's
//! exchanges, RIPE Atlas's measurements and IODA's outages; alert rules as
//! SQL, grouped into incidents; a page in NetOps' shape (a worklist, a map
//! that zooms from the world to a prefix, the stack of a country).

pub mod api;
pub mod atlas;
pub mod feed;
pub mod fetch;
pub mod load;
pub mod mrt;
pub mod outages;
pub mod pages;
pub mod pmap;
pub mod refdata;
pub mod rules;
pub mod schema;
pub mod strata;
