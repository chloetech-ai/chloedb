//! Tables shaped like NYC's green taxi trips and their zones, made up from
//! a seed, and the SQL a data engineer asked of the real ones (the persona
//! study's battery): what `chloedb-sql-bench` times and the speed gate
//! compares, and what `tests/sql_battery.rs` checks the engines agree on.
//!
//! `trips` has the columns of the TLC's files that the queries read, with
//! their kinds of values: two vendors, pickup times over four months of
//! 2024 (and a few years before), trips of minutes to an hour, locations
//! skewed towards a few zones, and NULLs where the real files have them
//! (passenger counts, payment types, trip types, the store-and-forward
//! flag). `zones` is 265 locations in seven boroughs, one with none.

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::value::Value;

/// The rows of `trips` in the real data the queries were written on.
pub const ROWS: usize = 224_000;

/// The queries, by the battery's names. Each one runs on both engines.
pub const QUERIES: &[(&str, &str)] = &[
    ("J8", "SELECT pz.Borough AS pu, dz.Borough AS dropoff, COUNT(*) AS n FROM trips t JOIN zones pz ON t.PULocationID = pz.LocationID JOIN zones dz ON t.DOLocationID = dz.LocationID GROUP BY pz.Borough, dz.Borough ORDER BY n DESC, pu, dropoff LIMIT 15"),
    ("J9", "SELECT z.LocationID FROM zones z LEFT JOIN trips t ON t.PULocationID = z.LocationID WHERE t.VendorID IS NULL ORDER BY z.LocationID"),
    ("F6", "WITH d AS (SELECT date_trunc('day', lpep_pickup_datetime) AS day, COUNT(*) AS n FROM trips WHERE lpep_pickup_datetime >= '2024-01-01' AND lpep_pickup_datetime < '2024-05-01' GROUP BY day) SELECT day, n, ROUND(AVG(n) OVER (ORDER BY day ROWS BETWEEN 6 PRECEDING AND CURRENT ROW), 3) AS ma7 FROM d ORDER BY day"),
    ("F7", "SELECT PULocationID, COUNT(*) AS n, DENSE_RANK() OVER (ORDER BY COUNT(*) DESC) AS dr, ROUND(PERCENT_RANK() OVER (ORDER BY COUNT(*) DESC), 6) AS pr, ROUND(CUME_DIST() OVER (ORDER BY COUNT(*) DESC), 6) AS cd FROM trips GROUP BY PULocationID ORDER BY n DESC, PULocationID LIMIT 20"),
    ("F8", "SELECT lpep_pickup_datetime, fare_amount, ROUND(SUM(fare_amount) OVER (ORDER BY lpep_pickup_datetime, fare_amount ROWS UNBOUNDED PRECEDING), 2) AS run FROM trips WHERE PULocationID = 74 ORDER BY lpep_pickup_datetime, fare_amount LIMIT 25 OFFSET 2000"),
    ("G4", "SELECT VendorID, payment_type, COUNT(*) AS n, ROUND(SUM(total_amount), 2) AS s, ROUND(AVG(trip_distance), 4) AS d, MIN(fare_amount) AS lo, MAX(fare_amount) AS hi FROM trips GROUP BY VendorID, payment_type HAVING COUNT(*) > 100 ORDER BY VendorID, payment_type NULLS LAST"),
    ("G5", "SELECT VendorID, ROUND(STDDEV(fare_amount), 6) AS sd, ROUND(VAR_POP(tip_amount), 6) AS vp, ROUND(CORR(tip_amount, fare_amount), 6) AS r FROM trips GROUP BY VendorID ORDER BY VendorID"),
    ("G6", "SELECT payment_type, MEDIAN(trip_distance) AS med, PERCENTILE_CONT(0.9) WITHIN GROUP (ORDER BY trip_distance) AS p90, PERCENTILE_DISC(0.9) WITHIN GROUP (ORDER BY fare_amount) AS p90d FROM trips GROUP BY payment_type ORDER BY payment_type NULLS LAST"),
    ("G7", "SELECT payment_type, COUNT(*) AS n, COUNT(*) FILTER (WHERE tip_amount > 0) AS tipped FROM trips GROUP BY payment_type ORDER BY payment_type NULLS LAST"),
    ("D3", "SELECT DISTINCT store_and_fwd_flag, trip_type FROM trips"),
    ("T4", "SELECT EXTRACT(hour FROM lpep_pickup_datetime) AS h, COUNT(*) AS n FROM trips GROUP BY h ORDER BY h"),
    ("T5", "SELECT VendorID, ROUND(AVG(EXTRACT(epoch FROM lpep_dropoff_datetime) - EXTRACT(epoch FROM lpep_pickup_datetime)), 3) AS secs FROM trips GROUP BY VendorID ORDER BY VendorID"),
    ("T6", "SELECT COUNT(*) FROM trips WHERE lpep_pickup_datetime >= '2024-02-01' AND lpep_pickup_datetime < '2024-03-01'"),
    ("T7", "SELECT date_bin('15 minutes', lpep_pickup_datetime) AS b, COUNT(*) AS n FROM trips WHERE lpep_pickup_datetime >= '2024-03-05' AND lpep_pickup_datetime < '2024-03-06' GROUP BY b ORDER BY b LIMIT 10"),
    ("T8", "SELECT date_trunc('week', lpep_pickup_datetime) AS w, COUNT(*) AS n FROM trips WHERE lpep_pickup_datetime >= '2024-01-01' GROUP BY w ORDER BY w LIMIT 6"),
    ("T9", "SELECT MIN(lpep_pickup_datetime), MAX(lpep_pickup_datetime), COUNT(*) FILTER (WHERE lpep_pickup_datetime < '2024-01-01') AS before FROM trips"),
    ("N4", "SELECT COUNT(*) AS n, COUNT(passenger_count) AS with_pc, SUM(passenger_count) AS pax, ROUND(AVG(passenger_count), 6) AS avg_pc, COUNT(*) FILTER (WHERE passenger_count IS NULL) AS no_pc FROM trips"),
    ("N5", "SELECT CASE coalesce(payment_type, 0) WHEN 1 THEN 'card' WHEN 2 THEN 'cash' WHEN 0 THEN 'unknown' ELSE 'other' END AS pay, COUNT(*) AS n FROM trips GROUP BY pay ORDER BY pay"),
    ("Q5", "SELECT COUNT(*) FROM trips WHERE PULocationID IN (SELECT LocationID FROM zones WHERE Borough = 'Manhattan')"),
    ("O2", "SELECT total_amount FROM trips ORDER BY total_amount DESC LIMIT 5 OFFSET 5"),
    ("O3", "SELECT PULocationID, DOLocationID, trip_distance FROM trips WHERE PULocationID = 1 ORDER BY trip_distance * 2 DESC, DOLocationID LIMIT 5"),
    ("O4", "SELECT VendorID, fare_amount FROM trips WHERE PULocationID = 3"),
    ("A3", "SELECT SUM(PULocationID) AS s, SUM(PULocationID * DOLocationID) AS p, AVG(DOLocationID) AS a FROM trips"),
    ("P1", "SELECT COUNT(*), SUM(fare_amount), AVG(trip_distance) FROM trips"),
    ("P2", "SELECT payment_type, COUNT(*), AVG(tip_amount) FROM trips GROUP BY payment_type"),
    ("P3", "SELECT date_trunc('day', lpep_pickup_datetime) AS d, COUNT(*) FROM trips GROUP BY d"),
    ("P4", "SELECT z.Borough, COUNT(*), AVG(t.fare_amount) FROM trips t JOIN zones z ON t.PULocationID = z.LocationID GROUP BY z.Borough"),
    ("P5", "SELECT pz.Borough, dz.Borough, COUNT(*) FROM trips t JOIN zones pz ON t.PULocationID = pz.LocationID JOIN zones dz ON t.DOLocationID = dz.LocationID GROUP BY pz.Borough, dz.Borough"),
    ("P6", "WITH c AS (SELECT PULocationID, COUNT(*) AS n FROM trips GROUP BY PULocationID) SELECT PULocationID, n, RANK() OVER (ORDER BY n DESC) FROM c"),
    ("P7", "SELECT CASE WHEN trip_distance < 1 THEN 'a' WHEN trip_distance < 3 THEN 'b' ELSE 'c' END AS k, COUNT(*) FROM trips GROUP BY k"),
    ("P8", "SELECT fare_amount FROM trips ORDER BY fare_amount DESC LIMIT 10"),
];

/// splitmix64: the same rows from the same seed on every machine.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// In `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// 2024-01-01T00:00:00Z in microseconds.
const JAN_2024: i64 = 1_704_067_200_000_000;
const MINUTE: i64 = 60_000_000;
const DAY: i64 = 1_440 * MINUTE;

const BOROUGHS: [&str; 7] = ["Manhattan", "Brooklyn", "Queens", "Bronx", "Staten Island", "EWR", "Unknown"];

/// A location, skewed: a few zones start or end most trips, as in East
/// Harlem and Elmhurst.
fn location(rng: &mut Rng) -> i64 {
    match rng.below(10) {
        0..=2 => [74, 75, 41, 42, 166][rng.below(5) as usize],
        3..=5 => 1 + rng.below(60) as i64,
        _ => 1 + rng.below(265) as i64,
    }
}

/// Money with cents.
fn cents(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// One trip, in the column order of [`create`]'s `trips`.
pub fn trip(i: usize, rng: &mut Rng) -> Vec<Value> {
    let vendor = if rng.below(100) < 15 { 1 } else { 2 };
    let pickup = match rng.below(2000) {
        0 => JAN_2024 - rng.below(30) as i64 * DAY - rng.below(1_440) as i64 * MINUTE,
        1 => JAN_2024 - (5 + rng.below(12) as i64) * 365 * DAY,
        _ => JAN_2024 + (rng.unit() * 121.0 * DAY as f64) as i64,
    };
    let minutes = 2.0 + (-(1.0 - rng.unit()).ln() * 12.0).min(180.0);
    let dropoff = pickup + (minutes * MINUTE as f64) as i64 + rng.below(60) as i64 * 1_000_000;
    let flag = match rng.below(1000) {
        0..=1 => Value::Null,
        2..=4 => Value::Str("Y".into()),
        _ => Value::Str("N".into()),
    };
    let passengers = match rng.below(100) {
        0..=4 => Value::Null,
        5 => Value::Int(0),
        6..=84 => Value::Int(1),
        85..=94 => Value::Int(2),
        _ => Value::Int(3 + rng.below(4) as i64),
    };
    let distance = cents((minutes / 4.0) * (0.4 + rng.unit()));
    let mut fare = cents(3.0 + 2.5 * distance + rng.unit() * 2.0);
    if rng.below(200) == 0 {
        fare = -fare;
    }
    let payment = match rng.below(100) {
        0..=4 => Value::Null,
        5..=64 => Value::Int(1),
        65..=94 => Value::Int(2),
        p => Value::Int(3 + (p % 3) as i64),
    };
    let tip = match (&payment, rng.below(10)) {
        (Value::Int(1), 0..=7) => cents(fare.abs() * (0.1 + rng.unit() * 0.2)),
        _ => 0.0,
    };
    let trip_type = match rng.below(100) {
        0 => Value::Null,
        1..=4 => Value::Int(2),
        _ => Value::Int(1),
    };
    vec![
        Value::Int(i as i64),
        Value::Int(vendor),
        Value::Timestamp(pickup),
        Value::Timestamp(dropoff),
        flag,
        Value::Int(location(rng)),
        Value::Int(location(rng)),
        passengers,
        Value::Float(distance),
        Value::Float(fare),
        Value::Float(tip),
        Value::Float(cents(fare + tip + 1.5)),
        payment,
        trip_type,
    ]
}

/// The schema `zones`, which a database is created with ([`Db::create`]);
/// [`create`] adds `trips`.
pub fn zones_schema() -> Schema {
    Schema::new(
        "zones",
        vec![Field::new("LocationID", FieldType::Int), Field::new("Borough", FieldType::Keyword), Field::new("Zone", FieldType::Keyword)],
        Some("LocationID"),
    )
    .expect("the zones' schema")
}

/// Loads `zones` and `rows` trips from `seed` into `db`, created with
/// [`zones_schema`].
pub fn create(db: &Db, rows: usize, seed: u64) {
    let mut rng = Rng::new(seed);
    db.load_into(
        "zones",
        (1..=265i64).map(|id| {
            let borough = match id {
                264 => Value::Str("Unknown".into()),
                265 => Value::Null,
                1 => Value::Str("EWR".into()),
                _ => Value::Str(BOROUGHS[(id as usize * 7 + id as usize / 13) % 5].into()),
            };
            vec![Value::Int(id), borough, Value::Str(format!("zone {id}"))]
        }),
    )
    .expect("load zones");
    db.sql(
        "CREATE TABLE trips (id INT PRIMARY KEY, VendorID INT, lpep_pickup_datetime TIMESTAMP, lpep_dropoff_datetime TIMESTAMP, \
         store_and_fwd_flag KEYWORD, PULocationID INT, DOLocationID INT, passenger_count INT, trip_distance FLOAT, \
         fare_amount FLOAT, tip_amount FLOAT, total_amount FLOAT, payment_type INT, trip_type INT)",
        &Params::new(),
    )
    .expect("table trips");
    db.load_into("trips", (0..rows).map(|i| trip(i, &mut rng))).expect("load trips");
}
