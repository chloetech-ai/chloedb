//! The app's tables, all named `ea_*`. Three kinds, as Backbone's are:
//!
//! - **places**, which change slowly: a row a fragment of Major TOM's
//!   Sentinel-2 (3.84 km a side) with its SigLIP vector and what lies on
//!   it; a row a place a year, for how it changed; one table whose model
//!   turns the words of a search into a vector;
//! - **what the satellites see**: every fire pixel of every scan GOES made
//!   over the regions, kept a while, and the scans themselves;
//! - **what the app decides**: fires (the pixels grouped, with how their
//!   first hours went), alert rules (each a SQL statement), the alerts
//!   they raised, and where each feed stands.

/// SigLIP so400m's vectors: Major TOM's pictures, and the words of a
/// search through the same model.
pub const LOOK_DIM: usize = 1152;
pub const LOOK_MODEL: &str = "siglip-so400m-patch14-384";
/// A fire's first hours as numbers ([`crate::fires::shape`]).
pub const SHAPE_DIM: usize = crate::fires::SHAPE_DIM;
pub const SHAPE2_DIM: usize = crate::fires::SHAPE2_DIM;
pub const SHAPE1_DIM: usize = crate::fires::SHAPE1_DIM;

/// (table, CREATE TABLE statement, what it holds).
pub fn tables() -> Vec<(&'static str, String, &'static str)> {
    vec![
        ("ea_places", format!("CREATE TABLE IF NOT EXISTS ea_places (
  id KEYWORD PRIMARY KEY, region KEYWORD, country KEYWORD, adm1 KEYWORD, adm2 KEYWORD,
  town KEYWORD, town_km FLOAT, town_pop INT, lat FLOAT, lon FLOAT, cell KEYWORD, taken TIMESTAMP,
  product KEYWORD OPTIONS(indexed = false), kind KEYWORD,
  trees FLOAT, shrubs FLOAT, grass FLOAT, crops FLOAT, built FLOAT, bare FLOAT, snow FLOAT, water FLOAT,
  wetland FLOAT, mangrove FLOAT, moss FLOAT,
  elev INT, elev_min INT, elev_max INT, slope FLOAT, steep FLOAT, people INT,
  climate KEYWORD, climate_group KEYWORD, mine FLOAT, crop KEYWORD, crop_pct FLOAT,
  fire_years INT[], fire_px INT, last_fire INT,
  words TEXT OPTIONS(analyzer = 'english'), about TEXT OPTIONS(analyzer = 'english'),
  twin KEYWORD, twin_sim FLOAT, twin_score FLOAT, look VECTOR({LOOK_DIM})
) OPTIONS(cluster_by = 'region')"), "Places: Major TOM's fragments of Sentinel-2 (3.84 km a side), each with its SigLIP vector, its land cover (WorldCover), terrain (Copernicus DEM), people (GHSL), climate (Koppen-Geiger), mines, crops (CDL) and fire years (FIRMS)"),
        ("ea_words", format!("CREATE TABLE IF NOT EXISTS ea_words (
  id KEYWORD PRIMARY KEY, text TEXT, kind KEYWORD,
  emb VECTOR({LOOK_DIM}) OPTIONS(embed = 'text', model = '{LOOK_MODEL}')
)"), "Words the app keeps with their vectors (the land-cover names a change is read against); its model is what turns a search's words into a vector"),
        ("ea_years", format!("CREATE TABLE IF NOT EXISTS ea_years (
  id KEYWORD PRIMARY KEY, place KEYWORD, region KEYWORD, year INT, taken TIMESTAMP,
  scene KEYWORD OPTIONS(indexed = false), cloud FLOAT, ndvi FLOAT, label KEYWORD,
  look VECTOR({LOOK_DIM})
) OPTIONS(cluster_by = 'place')"), "A place a year: its look in one clear Sentinel-2 scene of that year's dry season, for how places changed"),
        ("ea_changes", format!("CREATE TABLE IF NOT EXISTS ea_changes (
  id KEYWORD PRIMARY KEY, region KEYWORD, lat FLOAT, lon FLOAT, town KEYWORD, adm1 KEYWORD,
  first INT, last INT, drift FLOAT, step_year INT, step FLOAT, from_label KEYWORD, to_label KEYWORD,
  arc KEYWORD OPTIONS(indexed = false),
  before VECTOR({LOOK_DIM}), after VECTOR({LOOK_DIM}), delta VECTOR({LOOK_DIM})
)"), "A place's change over the years: its first and last looks, how far it moved and in which year most"),
        ("ea_detections", "CREATE TABLE IF NOT EXISTS ea_detections (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, sat KEYWORD, region KEYWORD, lat FLOAT, lon FLOAT,
  mask INT, conf KEYWORD, mw FLOAT, area FLOAT, temp FLOAT, place KEYWORD, fire KEYWORD
) OPTIONS(cluster_by = 'fire')".to_string(), "Every fire pixel a GOES scan saw over a region (2 km at best), with its power, the place it fell on and the fire it belongs to"),
        ("ea_scans", "CREATE TABLE IF NOT EXISTS ea_scans (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, sat KEYWORD, region KEYWORD, pixels INT, mw FLOAT, fires INT,
  file KEYWORD OPTIONS(indexed = false), read_ms FLOAT
)".to_string(), "Each scan read: when, which satellite, how many fire pixels over the region and their power"),
        ("ea_fires", format!("CREATE TABLE IF NOT EXISTS ea_fires (
  id KEYWORD PRIMARY KEY, region KEYWORD, name TEXT OPTIONS(analyzer = 'simple'), town KEYWORD, adm1 KEYWORD, adm2 KEYWORD,
  place KEYWORD, lat FLOAT, lon FLOAT, start TIMESTAMP, last TIMESTAMP, state KEYWORD, seen_start BOOL,
  scans INT, pixels INT, peak_px INT, peak_mw FLOAT, mw_now FLOAT, mw_1h FLOAT, energy FLOAT,
  area_km2 FLOAT, spread_km FLOAT, hours FLOAT,
  people5 INT, trees FLOAT, shrubs FLOAT, grass FLOAT, built FLOAT, slope FLOAT, elev INT, climate KEYWORD,
  burned_before INT, gust FLOAT, wind FLOAT, wind_dir FLOAT, rh FLOAT,
  perimeter KEYWORD, first KEYWORD OPTIONS(indexed = false), outlook_km2 FLOAT, outlook_lo FLOAT, outlook_hi FLOAT,
  shape VECTOR({SHAPE_DIM}), shape2 VECTOR({SHAPE2_DIM}), shape1 VECTOR({SHAPE1_DIM})
)"), "Fires: a region's fire pixels grouped in space and time, with how big and fast they got, what they burned on, the weather at their start, and their first hours as a vector (shape) for finding the fires that began like them"),
        ("ea_perimeters", "CREATE TABLE IF NOT EXISTS ea_perimeters (
  id KEYWORD PRIMARY KEY, source KEYWORD, name TEXT OPTIONS(analyzer = 'simple'), complex KEYWORD, year INT,
  alarm TIMESTAMP, contained TIMESTAMP, acres FLOAT, km2 FLOAT, cause KEYWORD, agency KEYWORD, unit KEYWORD, inc KEYWORD,
  lat FLOAT, lon FLOAT, s FLOAT, n FLOAT, w FLOAT, e FLOAT, shape KEYWORD OPTIONS(indexed = false)
)".to_string(), "Fire perimeters: CAL FIRE's outlines of every fire since 1878, with name, dates, acres and cause"),
        ("ea_burns", "CREATE TABLE IF NOT EXISTS ea_burns (
  id KEYWORD PRIMARY KEY, place KEYWORD, perimeter KEYWORD, region KEYWORD, name KEYWORD, year INT,
  alarm TIMESTAMP, acres FLOAT, share FLOAT, cause KEYWORD
) OPTIONS(cluster_by = 'place')".to_string(), "Which fire burned which place, and the share of the square inside its perimeter"),
        ("ea_rules", "CREATE TABLE IF NOT EXISTS ea_rules (
  id KEYWORD PRIMARY KEY, title KEYWORD, severity KEYWORD, sql KEYWORD OPTIONS(indexed = false),
  about KEYWORD OPTIONS(indexed = false), enabled BOOL, last_run TIMESTAMP, last_ms FLOAT, last_rows INT
)".to_string(), "Alert rules: each a SQL statement over the fires, run every minute, whose rows are the alerts"),
        ("ea_alerts", "CREATE TABLE IF NOT EXISTS ea_alerts (
  id KEYWORD PRIMARY KEY, rule KEYWORD, severity KEYWORD, fire KEYWORD, region KEYWORD,
  title KEYWORD OPTIONS(indexed = false), detail KEYWORD OPTIONS(indexed = false),
  lat FLOAT, lon FLOAT, first_seen TIMESTAMP, last_seen TIMESTAMP, state KEYWORD
)".to_string(), "What the rules found: one row per fire and rule, open until the rule stops finding it"),
        ("ea_backtest", "CREATE TABLE IF NOT EXISTS ea_backtest (
  id KEYWORD PRIMARY KEY, region KEYWORD, window KEYWORD, hours FLOAT, fires INT,
  covered FLOAT, base_covered FLOAT, width FLOAT, base_width FLOAT, error FLOAT, base_error FLOAT, spearman FLOAT, at TIMESTAMP
)".to_string(), "How well the fires like each one foretold its size: each fire watched from its start, matched only against fires out before that moment"),
        ("ea_feed", "CREATE TABLE IF NOT EXISTS ea_feed (
  id KEYWORD PRIMARY KEY, kind KEYWORD, at TIMESTAMP, rows INT, note KEYWORD OPTIONS(indexed = false)
)".to_string(), "Where each feed stands: the last scan read, and when"),
    ]
}
