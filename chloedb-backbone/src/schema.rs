//! The app's tables, all named `bb_*`. Three kinds, as NetOps' are:
//!
//! - **records** that change slowly: networks (`bb_as`), the relationships
//!   between them (two edge tables, so `REACHABLE`, `PATH` and `ppr` walk
//!   them), exchanges, buildings, who is at which, ROAs, RIPE Atlas probes;
//! - **streams**, kept a while and dropped by `retain`: every BGP update a
//!   collector heard, each checked against RPKI and against yesterday's
//!   origins as it arrived; Atlas pings and traceroutes; IODA's outages;
//! - **what the app decides**: alert rules (each a SQL statement), the
//!   alerts they raised, and incidents, which group alerts that share an
//!   owner, an announcer or a carrier.

/// (table, CREATE TABLE statement, what it holds).
pub fn tables() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("bb_as", "CREATE TABLE IF NOT EXISTS bb_as (
  asn INT PRIMARY KEY, name TEXT OPTIONS(analyzer = 'simple'), short KEYWORD, handle KEYWORD, country KEYWORD,
  kind KEYWORD, scope KEYWORD, traffic KEYWORD, website KEYWORD OPTIONS(indexed = false), pdb INT,
  prefixes4 INT, prefixes6 INT, ixes INT, facs INT, providers INT, customers INT, peers INT, cone INT,
  roas INT, probes INT, updated TIMESTAMP, created TIMESTAMP
)", "Networks (autonomous systems): the RIRs' names and countries, PeeringDB's records, CAIDA's counts of providers, customers and peers"),
        ("bb_p2c", "CREATE TABLE IF NOT EXISTS bb_p2c (
  id KEYWORD PRIMARY KEY, src INT NOT NULL REFERENCES bb_as, dst INT NOT NULL REFERENCES bb_as
) OPTIONS(edges = 'src, dst')", "Provider to customer, from CAIDA's AS relationships: an edge table, so walks follow who buys transit from whom"),
        ("bb_p2p", "CREATE TABLE IF NOT EXISTS bb_p2p (
  id KEYWORD PRIMARY KEY, src INT NOT NULL REFERENCES bb_as, dst INT NOT NULL REFERENCES bb_as
) OPTIONS(edges = 'src, dst')", "Settlement-free peers, from CAIDA's AS relationships"),
        ("bb_ix", "CREATE TABLE IF NOT EXISTS bb_ix (
  id INT PRIMARY KEY, name TEXT OPTIONS(analyzer = 'simple'), short KEYWORD, city KEYWORD, country KEYWORD,
  continent KEYWORD, members INT, lat FLOAT, lon FLOAT, website KEYWORD OPTIONS(indexed = false),
  updated TIMESTAMP, created TIMESTAMP
)", "Internet exchanges (PeeringDB), placed at their buildings"),
        ("bb_fac", "CREATE TABLE IF NOT EXISTS bb_fac (
  id INT PRIMARY KEY, name TEXT OPTIONS(analyzer = 'simple'), short KEYWORD, city KEYWORD, country KEYWORD,
  lat FLOAT, lon FLOAT, nets INT, ixes INT, updated TIMESTAMP
)", "Buildings where networks meet (PeeringDB facilities)"),
        ("bb_member", "CREATE TABLE IF NOT EXISTS bb_member (
  id INT PRIMARY KEY, ix INT, asn INT, speed INT, ip4 KEYWORD, ip6 KEYWORD, rs BOOL,
  updated TIMESTAMP, created TIMESTAMP
)", "Who is on which exchange, at what speed (PeeringDB netixlan)"),
        ("bb_netfac", "CREATE TABLE IF NOT EXISTS bb_netfac (
  id INT PRIMARY KEY, fac INT, asn INT, updated TIMESTAMP, created TIMESTAMP
)", "Who is in which building (PeeringDB netfac)"),
        ("bb_roa", "CREATE TABLE IF NOT EXISTS bb_roa (
  id KEYWORD PRIMARY KEY, prefix KEYWORD, asn INT, maxlen INT, af INT, plen INT, lo INT, hi INT,
  ta KEYWORD, expires TIMESTAMP
)", "RPKI's validated ROAs (rpki-client): who may announce which prefixes, how long"),
        ("bb_probe", "CREATE TABLE IF NOT EXISTS bb_probe (
  id INT PRIMARY KEY, asn INT, country KEYWORD, lat FLOAT, lon FLOAT, prefix KEYWORD, anchor BOOL,
  status KEYWORD, since TIMESTAMP
)", "RIPE Atlas probes that are connected: where they are, in which network"),
        ("bb_ping", "CREATE TABLE IF NOT EXISTS bb_ping (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, msm INT, target KEYWORD, probe INT, asn INT, country KEYWORD,
  rtt FLOAT, avg FLOAT, loss FLOAT
) OPTIONS(retain = '30 days', retain_by = 'ts')", "RIPE Atlas pings from every probe to a root server, a row per probe and round"),
        ("bb_trace", "CREATE TABLE IF NOT EXISTS bb_trace (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, msm INT, target KEYWORD, probe INT, asn INT, country KEYWORD,
  hops INT, rtt FLOAT, jump_hop INT, jump_ms FLOAT, jump_from KEYWORD, jump_to KEYWORD, jump_as INT, before_as INT,
  ips KEYWORD[], path_as INT[], route KEYWORD OPTIONS(indexed = false)
) OPTIONS(retain = '30 days', retain_by = 'ts')", "RIPE Atlas traceroutes to a root server: each hop, and where the time jumps"),
        ("bb_outage", "CREATE TABLE IF NOT EXISTS bb_outage (
  id KEYWORD PRIMARY KEY, kind KEYWORD, entity KEYWORD, asn INT, country KEYWORD, name TEXT OPTIONS(analyzer = 'simple'),
  source KEYWORD, start TIMESTAMP, until TIMESTAMP, score FLOAT, open BOOL
)", "IODA's outages: a country, region or network whose addresses stopped answering or lost their routes"),
        ("bb_updates", "CREATE TABLE IF NOT EXISTS bb_updates (
  id INT PRIMARY KEY, ts TIMESTAMP, collector KEYWORD, peer KEYWORD, peer_as INT, kind KEYWORD,
  prefix KEYWORD, af INT, plen INT, lo INT, hi INT, origin INT, path INT[], route KEYWORD OPTIONS(indexed = false),
  hops INT, rpki KEYWORD, seen KEYWORD, owner INT, country KEYWORD, comms KEYWORD[]
) OPTIONS(retain = '30 days', retain_by = 'ts', cluster_by = 'prefix', compression = 'zstd')", "Every BGP update a RIS collector heard: a row per prefix announced or withdrawn, checked against RPKI and yesterday's origins as it arrived"),
        ("bb_rules", "CREATE TABLE IF NOT EXISTS bb_rules (
  id KEYWORD PRIMARY KEY, title KEYWORD, kind KEYWORD, severity KEYWORD, every_s INT, sql KEYWORD OPTIONS(indexed = false),
  about KEYWORD OPTIONS(indexed = false), enabled BOOL, last_run TIMESTAMP, last_ms FLOAT, last_rows INT
)", "Alert rules: each a SQL statement run every minute, whose rows are the alerts"),
        ("bb_alerts", "CREATE TABLE IF NOT EXISTS bb_alerts (
  id KEYWORD PRIMARY KEY, rule KEYWORD, kind KEYWORD, severity KEYWORD, subject KEYWORD, prefix KEYWORD,
  origin INT, owner INT, asn INT, country KEYWORD, peers INT, updates INT, value FLOAT,
  title KEYWORD OPTIONS(indexed = false), detail KEYWORD OPTIONS(indexed = false),
  first_seen TIMESTAMP, last_seen TIMESTAMP, state KEYWORD, incident KEYWORD, carriers INT[]
)", "What the rules found: one row per thing wrong, open until a rule stops finding it"),
        ("bb_incidents", "CREATE TABLE IF NOT EXISTS bb_incidents (
  id KEYWORD PRIMARY KEY, kind KEYWORD, severity KEYWORD, title KEYWORD OPTIONS(indexed = false),
  summary KEYWORD OPTIONS(indexed = false), shared KEYWORD, alerts INT, asns INT[], countries KEYWORD[],
  prefixes KEYWORD[], first_seen TIMESTAMP, last_seen TIMESTAMP, state KEYWORD
)", "Alerts grouped by what they share: an owner, an announcer, a country"),
        ("bb_feed", "CREATE TABLE IF NOT EXISTS bb_feed (
  id KEYWORD PRIMARY KEY, kind KEYWORD, at TIMESTAMP, rows INT, note KEYWORD OPTIONS(indexed = false)
)", "Where each source stands: the last file or message read, and when"),
    ]
}

/// The rollup behind the charts of everything heard: updates a minute per
/// collector and kind.
pub const ROLLUPS: &[&str] = &[
    "CREATE ROLLUP IF NOT EXISTS bb_updates_1m ON bb_updates OPTIONS(lateness = '10 minutes', retain = '400 days') AS SELECT date_trunc('minute', ts), collector, kind, rpki, COUNT(*)",
];
