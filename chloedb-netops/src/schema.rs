//! The app's tables in ChloeDB, all named `netops_*` (one scoped key covers
//! them). Three kinds:
//!
//! - **records** that change slowly and are worth joining: sites, devices,
//!   links (an edge table, so `REACHABLE`, `PATH` and `ppr` walk it),
//!   circuits, services, customers, changes, config versions, tickets,
//!   runbooks, advisories;
//! - **telemetry**, written every minute and dropped by `retain`: rollups of
//!   link, device and service metrics, flow summaries and syslog (with a
//!   substring index, for the needle searches an operator makes);
//! - **what the app decides**: alert rules (each a SQL query), alerts,
//!   incidents and their timelines, agent runs with every step and query,
//!   the receipts of what an agent was shown, proposed actions, judgments.

pub const MODEL: &str = "potion-retrieval-32M";

/// (table, CREATE TABLE statement, what it holds).
pub fn tables() -> Vec<(&'static str, String, &'static str)> {
    let emb = |from: &str| format!("emb VECTOR OPTIONS(embed = '{from}', model = '{MODEL}')");
    vec![
        ("netops_sites", "CREATE TABLE IF NOT EXISTS netops_sites (
  id KEYWORD PRIMARY KEY, name KEYWORD, city KEYWORD, country KEYWORD, region KEYWORD, kind KEYWORD,
  lat FLOAT, lon FLOAT, devices INT
)".into(), "Sites: points of presence and data centres"),
        ("netops_devices", "CREATE TABLE IF NOT EXISTS netops_devices (
  id KEYWORD PRIMARY KEY, num INT, site KEYWORD, role KEYWORD, vendor KEYWORD, model KEYWORD,
  os KEYWORD, os_version KEYWORD, mgmt_ip KEYWORD OPTIONS(substring = true), loopback KEYWORD, asn INT,
  status KEYWORD, serial KEYWORD OPTIONS(substring = true), tags KEYWORD[], installed TIMESTAMP
)".into(), "Devices, as the inventory records them"),
        ("netops_links", "CREATE TABLE IF NOT EXISTS netops_links (
  id KEYWORD PRIMARY KEY,
  src KEYWORD NOT NULL REFERENCES netops_devices, dst KEYWORD NOT NULL REFERENCES netops_devices,
  num INT, kind KEYWORD, a_if KEYWORD, z_if KEYWORD, capacity_mbps INT, circuit KEYWORD, provider KEYWORD,
  plane KEYWORD, status KEYWORD, latency_ms FLOAT, a_site KEYWORD, z_site KEYWORD
) OPTIONS(edges = 'src, dst')".into(), "The inventory's links: an edge table, so graph queries walk the topology"),
        ("netops_seen_links", "CREATE TABLE IF NOT EXISTS netops_seen_links (
  id KEYWORD PRIMARY KEY,
  src KEYWORD NOT NULL REFERENCES netops_devices, dst KEYWORD NOT NULL REFERENCES netops_devices,
  a_if KEYWORD, z_if KEYWORD, evidence INT, protocols KEYWORD[], first_seen TIMESTAMP, last_seen TIMESTAMP,
  confidence FLOAT, state KEYWORD
) OPTIONS(edges = 'src, dst')".into(), "Links read from the logs (LLDP neighbours, routing adjacencies), compared with the inventory"),
        ("netops_circuits", "CREATE TABLE IF NOT EXISTS netops_circuits (
  id KEYWORD PRIMARY KEY, provider KEYWORD, link KEYWORD, a_site KEYWORD, z_site KEYWORD, capacity_mbps INT,
  monthly_cost FLOAT, sla KEYWORD, status KEYWORD, contract_months_left INT
)".into(), "Leased circuits and who provides them"),
        ("netops_bgp", "CREATE TABLE IF NOT EXISTS netops_bgp (
  id KEYWORD PRIMARY KEY, device KEYWORD, peer_ip KEYWORD, peer_asn INT, peer_name KEYWORD, kind KEYWORD,
  state KEYWORD, prefixes_rx INT, prefixes_tx INT, since TIMESTAMP
)".into(), "BGP sessions with transit providers and peers"),
        ("netops_services", "CREATE TABLE IF NOT EXISTS netops_services (
  id KEYWORD PRIMARY KEY, name KEYWORD, tier INT, owner KEYWORD, kind KEYWORD, description TEXT,
  slo_target FLOAT, latency_slo_ms FLOAT, revenue_per_min FLOAT, customers INT,
  devices KEYWORD[], device_nums INT[], sites KEYWORD[]
)".into(), "Services sold, their objectives and the devices they ride on"),
        ("netops_customers", "CREATE TABLE IF NOT EXISTS netops_customers (
  id KEYWORD PRIMARY KEY, num INT, name TEXT, tier KEYWORD, site KEYWORD, device KEYWORD, device_num INT,
  services KEYWORD[], mrr FLOAT, contact KEYWORD, prefix KEYWORD OPTIONS(substring = true), port KEYWORD, vlan INT,
  circuit KEYWORD OPTIONS(substring = true)
)".into(), "Customers, where they attach and what they are known by (prefix, port, circuit)"),
        ("netops_link_metrics", "CREATE TABLE IF NOT EXISTS netops_link_metrics (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, link KEYWORD, util FLOAT, in_bps FLOAT, out_bps FLOAT,
  errors INT, discards INT, loss FLOAT, latency_ms FLOAT, rx_dbm FLOAT, up INT
) OPTIONS(retain = '3 days', retain_by = 'ts')".into(), "A point a minute per link: the rollup an incident needs, not a metrics store"),
        ("netops_device_metrics", "CREATE TABLE IF NOT EXISTS netops_device_metrics (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, device KEYWORD, cpu FLOAT, mem FLOAT, temp_c FLOAT,
  bgp_updates INT, bgp_up INT, sessions INT, up INT
) OPTIONS(retain = '3 days', retain_by = 'ts')".into(), "A point a minute per device"),
        ("netops_service_metrics", "CREATE TABLE IF NOT EXISTS netops_service_metrics (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, service KEYWORD, requests INT, errors INT,
  latency_p50 FLOAT, latency_p95 FLOAT
) OPTIONS(retain = '7 days', retain_by = 'ts')".into(), "A point a minute per service: requests, errors, latency"),
        ("netops_flows", "CREATE TABLE IF NOT EXISTS netops_flows (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, exporter KEYWORD, src_site KEYWORD, dst_site KEYWORD,
  src_asn INT, src_name KEYWORD, dst_net KEYWORD, app KEYWORD, proto KEYWORD, dst_port INT,
  bytes INT, packets INT, flows INT
) OPTIONS(retain = '3 days', retain_by = 'ts')".into(), "Flow summaries a minute: who talks to whom"),
        ("netops_syslog", "CREATE TABLE IF NOT EXISTS netops_syslog (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, device KEYWORD, site KEYWORD, severity KEYWORD, sev INT,
  program KEYWORD, template KEYWORD, message TEXT OPTIONS(substring = true)
) OPTIONS(retain = '7 days', retain_by = 'ts', compression = 'zstd')".into(), "Device logs, searched by words (BM25) and by any substring (an FM-index)"),
        ("netops_alert_rules", "CREATE TABLE IF NOT EXISTS netops_alert_rules (
  id KEYWORD PRIMARY KEY, name KEYWORD, description TEXT, sql_text TEXT OPTIONS(indexed = false),
  cmp KEYWORD, threshold FLOAT, unit KEYWORD, severity KEYWORD, entity_kind KEYWORD, window_s INT, clear_after INT,
  enabled BOOL, runbook KEYWORD, last_eval TIMESTAMP, last_ms FLOAT, last_rows INT, firing INT, builtin BOOL
)".into(), "Alert rules: each one is a SQL query over these tables, run every evaluation"),
        ("netops_alerts", "CREATE TABLE IF NOT EXISTS netops_alerts (
  id KEYWORD PRIMARY KEY, rule KEYWORD, rule_name KEYWORD, severity KEYWORD, status KEYWORD,
  entity_kind KEYWORD, entity KEYWORD, site KEYWORD, devices KEYWORD[], summary TEXT,
  reading FLOAT, threshold FLOAT, unit KEYWORD, started TIMESTAMP, ended TIMESTAMP, last_seen TIMESTAMP,
  incident KEYWORD, acked_by KEYWORD, acked_at TIMESTAMP, assignee KEYWORD, silenced KEYWORD
)".into(), "Alerts the rules raised, with who acknowledged each and what silenced it"),
        ("netops_incidents", format!("CREATE TABLE IF NOT EXISTS netops_incidents (
  id KEYWORD PRIMARY KEY, title TEXT, severity KEYWORD, status KEYWORD,
  started TIMESTAMP, detected TIMESTAMP, mitigated TIMESTAMP, resolved TIMESTAMP,
  commander KEYWORD, summary TEXT, root_kind KEYWORD, root_entity KEYWORD, root_cause TEXT, confidence FLOAT,
  category KEYWORD, devices KEYWORD[], device_nums INT[], sites KEYWORD[], services KEYWORD[], customers INT,
  revenue_at_risk FLOAT, alert_count INT, change_id KEYWORD, runbook KEYWORD, postmortem TEXT,
  origin KEYWORD, fault_id KEYWORD, mttr_min FLOAT, ttd_s FLOAT,
  {}
)", emb("title, summary, root_cause")), "Incidents: alerts correlated over the topology, with the cause the agent named"),
        ("netops_timeline", "CREATE TABLE IF NOT EXISTS netops_timeline (
  id KEYWORD PRIMARY KEY, incident KEYWORD, ts TIMESTAMP, kind KEYWORD, actor KEYWORD, note TEXT, ref_id KEYWORD
)".into(), "What happened in each incident, in order"),
        ("netops_changes", "CREATE TABLE IF NOT EXISTS netops_changes (
  id KEYWORD PRIMARY KEY, title TEXT, description TEXT, kind KEYWORD, risk KEYWORD, status KEYWORD,
  window_start TIMESTAMP, window_end TIMESTAMP, executed TIMESTAMP, owner KEYWORD, approver KEYWORD,
  devices KEYWORD[], device_nums INT[], sites KEYWORD[], services KEYWORD[], ticket KEYWORD,
  rollback_plan TEXT, diff TEXT OPTIONS(indexed = false), outcome KEYWORD,
  started TIMESTAMP, started_by KEYWORD, verified TIMESTAMP
)".into(), "Changes, planned and done, with the devices each touches (INT[] so scopes overlap with &&)"),
        ("netops_configs", "CREATE TABLE IF NOT EXISTS netops_configs (
  id KEYWORD PRIMARY KEY, device KEYWORD, version INT, ts TIMESTAMP, author KEYWORD, change_id KEYWORD,
  lines INT, body TEXT OPTIONS(substring = true)
)".into(), "Every version of each device's configuration, for blame and search"),
        ("netops_silences", "CREATE TABLE IF NOT EXISTS netops_silences (
  id KEYWORD PRIMARY KEY, kind KEYWORD, matcher KEYWORD, reason TEXT, created_by KEYWORD, created TIMESTAMP,
  starts TIMESTAMP, ends TIMESTAMP, source KEYWORD, status KEYWORD
)".into(), "Silences: what should not page for a while, and why (a change window, a carrier's maintenance, a known fault)"),
        ("netops_maintenance", "CREATE TABLE IF NOT EXISTS netops_maintenance (
  id KEYWORD PRIMARY KEY, provider KEYWORD, reference KEYWORD, circuit KEYWORD, link KEYWORD, a_site KEYWORD, z_site KEYWORD,
  window_start TIMESTAMP, window_end TIMESTAMP, impact KEYWORD, expected TEXT, status KEYWORD, received TIMESTAMP,
  subject TEXT, raw TEXT, acked_by KEYWORD, change_id KEYWORD
)".into(), "Carriers' maintenance notices, read out of their emails and matched to the circuits they name"),
        ("netops_panels", "CREATE TABLE IF NOT EXISTS netops_panels (
  id KEYWORD PRIMARY KEY, incident KEYWORD, category KEYWORD, tkey KEYWORD, pos INT, title TEXT, kind KEYWORD, unit KEYWORD,
  sql_text TEXT OPTIONS(indexed = false), window_min INT, source KEYWORD, asked KEYWORD, added_by KEYWORD, added TIMESTAMP, removed BOOL
)".into(), "Each incident's board: one read-only query a panel, from its cause's template or added by the people in the room"),
        ("netops_handovers", "CREATE TABLE IF NOT EXISTS netops_handovers (
  id KEYWORD PRIMARY KEY, ts TIMESTAMP, kind KEYWORD, author KEYWORD, body TEXT, done BOOL
)".into(), "Shift handovers: the briefings kept, and the notes people leave for the next shift"),
        ("netops_tickets", format!("CREATE TABLE IF NOT EXISTS netops_tickets (
  id KEYWORD PRIMARY KEY, title TEXT, body TEXT, status KEYWORD, priority KEYWORD, assignee KEYWORD,
  queue KEYWORD, incident KEYWORD, device KEYWORD, customer KEYWORD, opened TIMESTAMP, updated TIMESTAMP,
  resolution TEXT, runbook KEYWORD,
  {}
)", emb("title, body, resolution")), "Tickets, open and closed"),
        ("netops_runbooks", format!("CREATE TABLE IF NOT EXISTS netops_runbooks (
  id KEYWORD PRIMARY KEY, title TEXT, body TEXT, tags KEYWORD[], category KEYWORD, uses INT, wins INT,
  updated TIMESTAMP, owner KEYWORD,
  {}
)", emb("title, body")), "Runbooks, found by words and by meaning"),
        ("netops_judgments", "CREATE TABLE IF NOT EXISTS netops_judgments (
  id KEYWORD PRIMARY KEY, incident KEYWORD, runbook KEYWORD, grade INT, ts TIMESTAMP, judge KEYWORD, search TEXT
)".into(), "Which runbook closed which incident: grades the ranking can learn from"),
        ("netops_oncall", "CREATE TABLE IF NOT EXISTS netops_oncall (
  id KEYWORD PRIMARY KEY, team KEYWORD, person KEYWORD, role KEYWORD, starts TIMESTAMP, ends TIMESTAMP, contact KEYWORD
)".into(), "Who is on call, by team and shift"),
        ("netops_advisories", "CREATE TABLE IF NOT EXISTS netops_advisories (
  id KEYWORD PRIMARY KEY, vendor KEYWORD, os KEYWORD, affected KEYWORD[], fixed_in KEYWORD, severity KEYWORD,
  cvss FLOAT, title TEXT, summary TEXT, published TIMESTAMP
)".into(), "Vendor advisories, joined with the inventory's software versions"),
        ("netops_agent_runs", "CREATE TABLE IF NOT EXISTS netops_agent_runs (
  id KEYWORD PRIMARY KEY, agent KEYWORD, incident KEYWORD, subject KEYWORD, started TIMESTAMP, finished TIMESTAMP,
  status KEYWORD, summary TEXT, confidence FLOAT, steps INT, queries INT, rows_read INT, tokens INT, ms FLOAT,
  graph_engine KEYWORD, verdict KEYWORD
)".into(), "Each run of an agent"),
        ("netops_agent_steps", "CREATE TABLE IF NOT EXISTS netops_agent_steps (
  id KEYWORD PRIMARY KEY, run KEYWORD, n INT, ts TIMESTAMP, tool KEYWORD, title TEXT, detail TEXT,
  sql_text TEXT OPTIONS(indexed = false), row_count INT, ms FLOAT
)".into(), "Every step of every run, with the SQL it ran"),
        ("netops_receipts", "CREATE TABLE IF NOT EXISTS netops_receipts (
  id KEYWORD PRIMARY KEY, run KEYWORD, incident KEYWORD, n INT, source KEYWORD, row_id KEYWORD, title TEXT,
  score FLOAT, tokens INT, used INT, decision KEYWORD, excerpt TEXT OPTIONS(indexed = false)
)".into(), "What each agent was shown and what was left out: PACK's decision for every candidate row"),
        ("netops_actions", "CREATE TABLE IF NOT EXISTS netops_actions (
  id KEYWORD PRIMARY KEY, run KEYWORD, incident KEYWORD, kind KEYWORD, title TEXT, detail TEXT,
  command TEXT OPTIONS(indexed = false), target KEYWORD, risk KEYWORD, status KEYWORD,
  proposed TIMESTAMP, decided TIMESTAMP, decided_by KEYWORD, result TEXT
)".into(), "Actions agents proposed, and who approved them"),
        ("netops_faults", "CREATE TABLE IF NOT EXISTS netops_faults (
  id KEYWORD PRIMARY KEY, scenario KEYWORD, started TIMESTAMP, ended TIMESTAMP, phase KEYWORD,
  truth_kind KEYWORD, truth_entity KEYWORD, category KEYWORD, incident KEYWORD, origin KEYWORD,
  detail TEXT OPTIONS(indexed = false)
)".into(), "The simulator's answer key: the fault it injected (agents never read it)"),
        ("netops_sources", "CREATE TABLE IF NOT EXISTS netops_sources (
  id KEYWORD PRIMARY KEY, name KEYWORD, kind KEYWORD, table_name KEYWORD, via KEYWORD, status KEYWORD,
  rate FLOAT, lag_s FLOAT, last_seen TIMESTAMP, retention KEYWORD, note TEXT, snippet TEXT OPTIONS(indexed = false)
)".into(), "Where each table's rows come from"),
        ("netops_state", "CREATE TABLE IF NOT EXISTS netops_state (
  id KEYWORD PRIMARY KEY, val TEXT OPTIONS(indexed = false), updated TIMESTAMP
)".into(), "The app's own bookkeeping"),
    ]
}

/// The tables in the order to drop them (those that reference others first).
pub fn names() -> Vec<&'static str> {
    tables().into_iter().map(|t| t.0).collect()
}

pub const TELEMETRY: &[&str] = &["netops_link_metrics", "netops_device_metrics", "netops_service_metrics", "netops_flows", "netops_syslog"];
