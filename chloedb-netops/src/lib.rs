//! Network operations on ChloeDB: an app of chloedb-server, as the crawler
//! and the spreadsheets are. It keeps a network's records, telemetry
//! rollups, alerts, incidents and changes in ChloeDB tables and answers
//! every screen with SQL, which each response carries so the page can show
//! it. A simulator plays a carrier's network into those tables, so the app
//! has something to watch; everything it shows is synthetic.

pub mod agents;
pub mod api;
pub mod daily;
pub mod debug;
pub mod hub;
pub mod graph;
pub mod leadership;
pub mod schema;
pub mod seed;
pub mod sim;
pub mod strata;
pub mod telemetry;
pub mod world;
