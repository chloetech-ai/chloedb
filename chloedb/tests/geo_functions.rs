//! Trigonometry, great-circle distance, H3 cells and shapes kept as text
//! (`chloedb::geo`), with the answers of the systems people compare them
//! with:
//!
//! - `sin` and the other trigonometric functions, `radians`, `degrees` and
//!   `pi` as PostgreSQL 16 answers (except `asin` and `acos` beyond -1 to
//!   1, NULL here and an error there);
//! - `haversine(lat1, lon1, lat2, lon2)` in kilometres, as Snowflake's;
//! - the `h3_*` functions as H3's C library (h3 4.1, through its Python
//!   binding) answers, with cells as INTs or their hex text as Snowflake
//!   takes them;
//! - the `st_*` functions over WKT or GeoJSON text, as PostGIS answers for
//!   `geography` distances and areas and for `geometry` topology.
//!
//! Over one table, both engines, rankings (nearest first) and groups (a
//! count per cell), with the errors that say what was wrong. Over shards
//! in chloedb-server's `tests/cluster_writes.rs`.

use chloedb::db::{Db, SqlResult};
use chloedb::query::exec::{Engine, ExecOptions};
use chloedb::query::ir::{self, Plan};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;

/// Table `places` (id KEYWORD key, name TEXT, state KEYWORD, lat FLOAT,
/// lon FLOAT) with seven rows over three segments, one without a
/// position, and `counties` (name KEYWORD key, state KEYWORD, geom TEXT)
/// with San Francisco as WKT and Alameda as GeoJSON, both rough boxes.
fn db(dir: &std::path::Path) -> Db {
    let t = Schema::new("places", vec![Field::new("id", FieldType::Keyword), Field::new("name", FieldType::Text { positions: true }), Field::new("state", FieldType::Keyword), Field::new("lat", FieldType::Float), Field::new("lon", FieldType::Float)], Some("id")).unwrap();
    let db = Db::create(&dir.join("geo.chloedb"), t, DbOptions { sync: false, segment_max_docs: 3, ..DbOptions::default() }).unwrap();
    for s in [
        "CREATE TABLE counties (name KEYWORD PRIMARY KEY, state KEYWORD, geom TEXT)",
        "INSERT INTO places (id, name, state, lat, lon) VALUES ('sf', 'San Francisco port', 'CA', 37.7749, -122.4194), ('oakland', 'Oakland port', 'CA', 37.8044, -122.2712), ('sanjose', 'San Jose', 'CA', 37.3382, -121.8863)",
        "INSERT INTO places (id, name, state, lat, lon) VALUES ('la', 'Los Angeles port', 'CA', 34.0522, -118.2437), ('santiago', 'Santiago', 'RM', -33.4489, -70.6693), ('valparaiso', 'Valparaíso port', 'V', -33.0472, -71.6127)",
        "INSERT INTO places (id, name) VALUES ('nowhere', 'a port nowhere')",
        "INSERT INTO counties (name, state, geom) VALUES ('sf', 'CA', 'POLYGON((-122.52 37.70, -122.35 37.70, -122.35 37.83, -122.52 37.83, -122.52 37.70))')",
        r#"INSERT INTO counties (name, state, geom) VALUES ('alameda', 'CA', '{"type":"Feature","properties":{"name":"Alameda"},"geometry":{"type":"Polygon","coordinates":[[[-122.35,37.45],[-121.47,37.45],[-121.47,37.91],[-122.35,37.91],[-122.35,37.45]]]}}')"#,
    ] {
        sql(&db, s);
    }
    db
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(n) => vec![vec![Value::Int(n as i64)]],
    }
}

/// The rows of `q` on the operators and on DataFusion (which runs on the
/// operators what it does not lower), which must be the same.
fn on_both(db: &Db, q: &str) -> Vec<Vec<Value>> {
    let mut seen: Option<Vec<Vec<Value>>> = None;
    for engine in [Engine::Operators, Engine::DataFusion] {
        let opts = ExecOptions { engine, ..db.exec.clone() };
        let rows = db.query_with(q, &Params::new(), &opts).unwrap_or_else(|e| panic!("{q} ({engine:?}): {e}")).rows;
        if let Some(first) = &seen {
            assert_eq!(&rows, first, "{q}: {engine:?} against the operators");
        }
        seen = Some(rows);
    }
    seen.expect("rows")
}

/// The one row of `q`.
fn row(db: &Db, q: &str) -> Vec<Value> {
    let mut rows = on_both(db, q);
    assert_eq!(rows.len(), 1, "{q}: {rows:?}");
    rows.remove(0)
}

/// The first column of each row of `q`, as text, in its order.
fn ids(db: &Db, q: &str) -> Vec<String> {
    on_both(db, q).into_iter().map(|r| r[0].as_str().unwrap_or("NULL").to_string()).collect()
}

fn err_of(db: &Db, q: &str) -> String {
    match db.sql(q, &Params::new()) {
        Ok(r) => panic!("{q}: expected an error, got {:?}", match r {
            SqlResult::Rows(r) => r.rows,
            SqlResult::Written(n) => vec![vec![Value::Int(n as i64)]],
        }),
        Err(e) => e.to_string(),
    }
}

fn f(v: &Value) -> f64 {
    match v {
        Value::Float(x) => *x,
        Value::Int(i) => *i as f64,
        v => panic!("not a number: {v:?}"),
    }
}

/// `got` within `tol` of `want`.
fn near(got: &Value, want: f64, tol: f64, what: &str) {
    let g = f(got);
    assert!((g - want).abs() <= tol, "{what}: {g} against {want}");
}

fn s(x: &str) -> Value {
    Value::Str(x.into())
}

const N: Value = Value::Null;

/// The trigonometric functions answer as PostgreSQL 16's double precision
/// ones do (`SELECT sin(0), ...` there), in degrees and radians.
#[test]
fn trigonometry_answers_as_postgres_does() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let r = row(&db, "SELECT sin(0), cos(0), tan(1), cot(1), asin(1), acos(1), atan(1), atan2(1, 1), atan2(-1, -1), radians(180), degrees(pi()), pi(), degrees(1), radians(1)");
    let want = [
        0.0,
        1.0,
        1.5574077246549023,
        0.6420926159343306,
        std::f64::consts::FRAC_PI_2,
        0.0,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::FRAC_PI_4,
        -2.356194490192345,
        std::f64::consts::PI,
        180.0,
        std::f64::consts::PI,
        57.29577951308232,
        0.017453292519943295,
    ];
    for (i, (g, w)) in r.iter().zip(want).enumerate() {
        near(g, w, 1e-15, &format!("column {i}"));
    }
    // sin(radians(30)) is a half, as near as a double comes.
    near(&row(&db, "SELECT sin(radians(30))")[0], 0.5, 1e-15, "sin(radians(30))");
    // NULL in, NULL out; beyond -1 to 1 asin and acos have no answer.
    assert_eq!(row(&db, "SELECT sin(NULL), atan2(NULL, 1), asin(2), acos(-1.5), radians(NULL)"), vec![N, N, N, N, N]);
    // Of a row's columns, in WHERE and in ORDER BY.
    assert_eq!(ids(&db, "SELECT id FROM places WHERE cos(radians(lat)) < 0.8 ORDER BY id"), ["oakland", "sanjose", "sf"]);
    assert_eq!(ids(&db, "SELECT id FROM places WHERE lat IS NOT NULL ORDER BY sin(radians(lat)) LIMIT 2"), ["santiago", "valparaiso"]);
    let e = err_of(&db, "SELECT sin('x')");
    assert!(e.contains("sin takes numbers"), "{e}");
    let e = err_of(&db, "SELECT atan2(1)");
    assert!(e.contains("atan2"), "{e}");
}

/// `haversine` is Snowflake's: kilometres on a sphere of radius 6,371 km,
/// latitude before longitude. Ordered by it, the nearest rows come first;
/// a row without a position is left out of a comparison and comes last.
#[test]
fn haversine_answers_as_snowflake_does() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // Snowflake's documentation: HAVERSINE(40.7127, -74.0059, 34.0500, -118.2500) = 3936.385096389.
    near(&row(&db, "SELECT haversine(40.7127, -74.0059, 34.0500, -118.2500)")[0], 3936.385096389, 1e-6, "New York to Los Angeles");
    assert_eq!(row(&db, "SELECT haversine(1, 2, 1, 2), haversine(NULL, 2, 1, 2)"), vec![Value::Float(0.0), N]);
    let sf = "haversine(lat, lon, 37.7749, -122.4194)";
    let rows = on_both(&db, &format!("SELECT id, {sf} AS km FROM places ORDER BY km LIMIT 3"));
    assert_eq!(rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![s("sf"), s("oakland"), s("sanjose")]);
    near(&rows[1][1], 13.42962533402683, 1e-9, "San Francisco to Oakland");
    near(&rows[2][1], 67.57399276151551, 1e-9, "San Francisco to San Jose");
    for q in [format!("SELECT id FROM places ORDER BY {sf} LIMIT 3"), format!("SELECT id FROM places ORDER BY {sf} ASC LIMIT 3")] {
        assert_eq!(ids(&db, &q), ["sf", "oakland", "sanjose"], "{q}");
    }
    assert_eq!(ids(&db, &format!("SELECT id FROM places ORDER BY {sf} DESC LIMIT 3")), ["santiago", "valparaiso", "la"]);
    assert_eq!(ids(&db, &format!("SELECT id FROM places WHERE {sf} < 20 ORDER BY id")), ["oakland", "sf"]);
    assert_eq!(ids(&db, &format!("SELECT id FROM places WHERE {sf} >= 20 ORDER BY id")), ["la", "sanjose", "santiago", "valparaiso"]);
    // The same distance from its parts, as a query writes it without haversine.
    let by_hand = "2 * 6371 * asin(sqrt(pow(sin(radians(lat - 37.7749) / 2), 2) + cos(radians(lat)) * cos(radians(37.7749)) * pow(sin(radians(lon + 122.4194) / 2), 2)))";
    let rows = on_both(&db, &format!("SELECT id, {by_hand}, {sf} FROM places WHERE lat IS NOT NULL ORDER BY id"));
    for r in &rows {
        near(&r[1], f(&r[2]), 1e-9, r[0].as_str().unwrap());
    }
}

/// The `h3_*` functions answer as H3's C library: the cell of a point at
/// a resolution, its parent, resolution, center and outline, and the
/// grid distance between cells, as INTs or as hex text.
#[test]
fn h3_answers_as_the_c_library_does() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // h3.latlng_to_cell(37.769377, -122.388903, 9) and (52.516262, 13.377704, 8).
    assert_eq!(
        row(&db, "SELECT h3_latlng_to_cell_string(37.769377, -122.388903, 9), h3_latlng_to_cell(52.516262, 13.377704, 8), h3_latlng_to_cell_string(52.516262, 13.377704, 8), h3_latlng_to_cell_string(0, 0, 0)"),
        vec![s("89283082e73ffff"), Value::Int(613036919424548863), s("881f1d4887fffff"), s("8075fffffffffff")]
    );
    // A parent is of the kind its cell is: an INT for an INT, text for text.
    assert_eq!(
        row(&db, "SELECT h3_cell_to_parent('8928308280fffff', 5), h3_cell_to_parent(613036919424548863, 5), h3_get_resolution('8928308280fffff'), h3_get_resolution(613036919424548863), h3_grid_distance('8928308280fffff', '8928308280bffff')"),
        vec![s("85283083fffffff"), Value::Int(599526121473572863), Value::Int(9), Value::Int(8), Value::Int(1)]
    );
    assert_eq!(
        row(&db, "SELECT h3_int_to_string(613036919424548863), h3_string_to_int('881f1d4887fffff'), h3_is_valid_cell('zz'), h3_is_valid_cell(613036919424548863), h3_is_valid_cell('881f1d4887fffff'), h3_is_valid_cell(5), h3_cell_to_parent(NULL, 3)"),
        vec![s("881f1d4887fffff"), Value::Int(613036919424548863), Value::Bool(false), Value::Bool(true), Value::Bool(true), Value::Bool(false), N]
    );
    // h3.cell_to_latlng('8928308280fffff') = (37.77670234943567, -122.41845932318311).
    let center = row(&db, "SELECT st_x(h3_cell_to_point('8928308280fffff')), st_y(h3_cell_to_point('8928308280fffff'))");
    near(&center[0], -122.41845932318311, 1e-12, "longitude");
    near(&center[1], 37.77670234943567, 1e-12, "latitude");
    // Its outline is a closed polygon of its six corners, longitude first,
    // and contains its center.
    let outline = row(&db, "SELECT h3_cell_to_boundary('8928308280fffff')")[0].as_str().unwrap().to_string();
    assert!(outline.starts_with("POLYGON((-122.4171997184165"), "{outline}");
    assert_eq!(outline.matches(',').count(), 6, "{outline}");
    assert_eq!(row(&db, "SELECT st_contains(h3_cell_to_boundary('8928308280fffff'), h3_cell_to_point('8928308280fffff'))"), vec![Value::Bool(true)]);
    // h3.grid_distance between San Francisco's and Oakland's cells at 7.
    assert_eq!(row(&db, "SELECT h3_grid_distance(h3_latlng_to_cell(37.7749, -122.4194, 7), h3_latlng_to_cell(37.8044, -122.2712, 7))"), vec![Value::Int(6)]);

    // A count per cell, as a map draws it: San Francisco and Oakland share
    // a cell at resolution 5 (85283083fffffff), and the row without a
    // position is in the NULL group.
    let rows = on_both(&db, "SELECT h3_latlng_to_cell(lat, lon, 5) AS c, COUNT(*) FROM places GROUP BY c ORDER BY c");
    let want: Vec<Vec<Value>> = [(599685771850416127, 2), (599686042433355775, 1), (599711151885910015, 1), (602123631794847743, 1), (602123720915419135, 1)]
        .into_iter()
        .map(|(c, n)| vec![Value::Int(c), Value::Int(n)])
        .chain([vec![N, Value::Int(1)]])
        .collect();
    assert_eq!(rows, want);
    let rows = on_both(&db, "SELECT h3_latlng_to_cell_string(lat, lon, 5) AS c, COUNT(*) AS n FROM places WHERE lat IS NOT NULL GROUP BY c ORDER BY n DESC, c LIMIT 1");
    assert_eq!(rows, vec![vec![s("85283083fffffff"), Value::Int(2)]]);
    let rows = on_both(&db, "SELECT h3_cell_to_boundary(h3_latlng_to_cell(lat, lon, 2)) AS cell, COUNT(*) FROM places WHERE lat < 0 GROUP BY cell");
    assert_eq!(rows.len(), 1, "Santiago and Valparaíso share a cell at resolution 2 (82b2c7fffffffff): {rows:?}");
    assert!(rows[0][0].as_str().unwrap().starts_with("POLYGON(("), "{rows:?}");

    for (q, says) in [
        ("SELECT h3_latlng_to_cell(1, 2, 16)", "from 0 (cells of about 4.4 million km²) to 15"),
        ("SELECT h3_latlng_to_cell(1, 2, 2.5)", "whole number"),
        ("SELECT h3_latlng_to_cell(-122.4, 37.7, 5)", "latitude -122.4 is outside -90 to 90"),
        ("SELECT h3_latlng_to_cell('a', 2, 5)", "latitude is a number"),
        ("SELECT h3_cell_to_parent('85283083fffffff', 9)", "finer than the cell's, 5"),
        ("SELECT h3_grid_distance('8928308280fffff', '85283083fffffff')", "resolutions 9 and 5"),
        ("SELECT h3_get_resolution('nope')", "'nope' is not an H3 cell"),
        ("SELECT h3_int_to_string('881f1d4887fffff')", "takes a cell's INT"),
        ("SELECT h3_string_to_int(613036919424548863)", "takes a cell's hex text"),
    ] {
        let e = err_of(&db, q);
        assert!(e.contains(says), "{q}: {e}");
    }
}

/// The `st_*` functions read WKT and GeoJSON (a geometry, a Feature or a
/// FeatureCollection), longitude before latitude. Containment and overlap
/// are of the shapes as drawn on a flat map of longitude and latitude (as
/// PostGIS's `geometry` and GeoJSON have them), distances and areas on
/// the sphere in metres and square metres (as its `geography`).
#[test]
fn shapes_as_wkt_or_geojson_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    assert_eq!(
        row(&db, r#"SELECT st_point(-122.42, 37.77), st_x(st_point(-122.42, 37.77)), st_y('POINT(1 2)'), st_astext('{"type":"Point","coordinates":[1,2]}'), st_asgeojson('POINT(1 2)'), st_asgeojson('SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))')"#),
        vec![s("POINT(-122.42 37.77)"), Value::Float(-122.42), Value::Float(2.0), s("POINT(1 2)"), s(r#"{"type":"Point","coordinates":[1,2]}"#), s(r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#)]
    );
    let square = "'POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))'";
    let holed = "'POLYGON((0 0, 4 0, 4 4, 0 4, 0 0), (1 1, 3 1, 3 3, 1 3, 1 1))'";
    assert_eq!(
        row(&db, &format!("SELECT st_contains({square}, 'POINT(1 1)'), st_within('POINT(1 1)', {square}), st_contains({square}, 'POINT(4 4)'), st_intersects({square}, 'POINT(4 4)'), st_contains({square}, 'POINT(5 1)'), st_contains({holed}, 'POINT(2 2)'), st_contains({holed}, 'POINT(0.5 2)')")),
        vec![Value::Bool(true), Value::Bool(true), Value::Bool(false), Value::Bool(true), Value::Bool(false), Value::Bool(false), Value::Bool(true)]
    );
    // Shapes that overlap, touch, cross or are apart.
    assert_eq!(
        row(&db, &format!("SELECT st_intersects({square}, 'POLYGON((3 3, 6 3, 6 6, 3 6, 3 3))'), st_intersects({square}, 'LINESTRING(-1 2, 5 2)'), st_intersects({square}, 'POLYGON((5 5, 6 5, 6 6, 5 5))'), st_contains({square}, 'LINESTRING(1 1, 2 2)'), st_contains({square}, 'POLYGON((1 1, 2 1, 2 2, 1 1))'), st_intersects({holed}, 'POINT(2 2)')")),
        vec![Value::Bool(true), Value::Bool(true), Value::Bool(false), Value::Bool(true), Value::Bool(true), Value::Bool(false)]
    );
    // A degree of latitude is 111,195.08 m on the sphere of the Earth's
    // mean radius, 6,371,008.8 m (BigQuery's and Turf's; PostGIS's
    // geography with use_spheroid false takes 6,371,008.77 m, 111,195.0797);
    // a square degree at the equator is 12,363.7 km².
    let r = row(&db, "SELECT st_distance('POINT(0 0)', 'POINT(0 1)'), st_dwithin('POINT(0 0)', 'POINT(0 1)', 200000), st_dwithin('POINT(0 0)', 'POINT(0 1)', 100000), st_area('POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))'), st_area('POINT(1 1)'), st_distance('POINT(2 2)', 'POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))')");
    near(&r[0], 111195.08, 0.01, "a degree of latitude");
    assert_eq!(&r[1..3], &[Value::Bool(true), Value::Bool(false)]);
    near(&r[3], 12_363_718_145.18, 1.0, "a square degree");
    assert_eq!(&r[4..], &[Value::Float(0.0), Value::Float(0.0)]);
    near(&row(&db, "SELECT st_distance('POINT(-122.4194 37.7749)', st_point(-122.2712, 37.8044))")[0], 13_429.6253, 0.5, "San Francisco to Oakland in metres");

    // Over the rows of a table: the places in a county drawn as WKT, the
    // others, those within 20 km, and the nearest to Santiago.
    let sf_box = "'POLYGON((-122.52 37.70, -122.35 37.70, -122.35 37.83, -122.52 37.83, -122.52 37.70))'";
    assert_eq!(ids(&db, &format!("SELECT id FROM places WHERE st_contains({sf_box}, st_point(lon, lat))")), ["sf"]);
    assert_eq!(ids(&db, &format!("SELECT id FROM places WHERE NOT st_contains({sf_box}, st_point(lon, lat)) ORDER BY id")), ["la", "oakland", "sanjose", "santiago", "valparaiso"]);
    assert_eq!(ids(&db, "SELECT id FROM places WHERE st_dwithin(st_point(lon, lat), 'POINT(-122.4194 37.7749)', 20000) ORDER BY id"), ["oakland", "sf"]);
    assert_eq!(ids(&db, "SELECT id FROM places ORDER BY st_distance(st_point(lon, lat), 'POINT(-70.6 -33.4)') LIMIT 2"), ["santiago", "valparaiso"]);
    // Shapes kept in a column, as WKT in one row and a GeoJSON Feature in
    // the other.
    let rows = on_both(&db, "SELECT name, st_area(geom) / 1e6, st_contains(geom, 'POINT(-122.27 37.80)'), st_astext(geom) FROM counties ORDER BY name");
    assert_eq!(rows[0][0], s("alameda"));
    near(&rows[0][1], 3961.2003, 0.001, "Alameda's box in km²");
    near(&rows[1][1], 216.0137, 0.001, "San Francisco's box in km²");
    assert_eq!((rows[0][2].clone(), rows[1][2].clone()), (Value::Bool(true), Value::Bool(false)));
    assert_eq!(rows[0][3], s("POLYGON((-122.35 37.45,-121.47 37.45,-121.47 37.91,-122.35 37.91,-122.35 37.45))"));
    assert_eq!(ids(&db, "SELECT name FROM counties WHERE st_intersects(geom, 'LINESTRING(-122.5 37.8, -122.2 37.8)') ORDER BY name"), ["alameda", "sf"]);

    for (q, says) in [
        ("SELECT st_contains('POLYGON((0 0', 'POINT(1 1)')", "expected ')' in WKT at its end"),
        ("SELECT st_contains('hello', 'POINT(1 1)')", "not a kind of shape"),
        ("SELECT st_area('')", "an empty text is not a shape"),
        ("SELECT st_x('POLYGON((0 0, 1 0, 1 1, 0 0))')", "st_x takes a POINT, not a POLYGON"),
        ("SELECT st_point(37.77, -122.42)", "st_point"),
        ("SELECT st_area(5)", "takes shapes as WKT or GeoJSON text"),
        ("SELECT st_contain('a', 'b')", "did you mean st_contains?"),
    ] {
        let e = err_of(&db, q);
        assert!(e.contains(says), "{q}: {e}");
    }
}

/// Over joined and derived rows, and in a ranking with a score: places
/// joined to the counties of their state that hold them (a join needs an
/// equality, so the shapes are found by a coarse key and tested in the
/// WHERE), and a search ranked by its relevance and nearness.
#[test]
fn over_joined_rows_and_in_rankings() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let rows = on_both(&db, "SELECT p.id, c.name FROM places p JOIN counties c ON p.state = c.state WHERE st_contains(c.geom, st_point(p.lon, p.lat)) ORDER BY p.id");
    assert_eq!(rows, vec![vec![s("oakland"), s("alameda")], vec![s("sf"), s("sf")]]);
    let rows = on_both(&db, "SELECT c.name, COUNT(*) FROM places p JOIN counties c ON p.state = c.state WHERE st_dwithin(st_point(p.lon, p.lat), c.geom, 50000) GROUP BY c.name ORDER BY c.name");
    assert_eq!(rows, vec![vec![s("alameda"), Value::Int(3)], vec![s("sf"), Value::Int(2)]], "San Jose is 57 km from San Francisco's box");
    let rows = on_both(&db, "SELECT d.id, d.cell FROM (SELECT id, h3_latlng_to_cell_string(lat, lon, 5) AS cell FROM places WHERE lat > 37) AS d WHERE d.cell = '85283083fffffff' ORDER BY d.id");
    assert_eq!(rows, vec![vec![s("oakland"), s("85283083fffffff")], vec![s("sf"), s("85283083fffffff")]]);
    // The ports, nearest San Francisco first among those that match.
    let q = "SELECT id FROM places WHERE MATCH(name, 'port') ORDER BY bm25(name, 'port') / (1 + haversine(lat, lon, 37.7749, -122.4194)) DESC LIMIT 3";
    assert_eq!(ids(&db, q), ["sf", "oakland", "la"]);
    let q = "SELECT id FROM places WHERE MATCH(name, 'port') ORDER BY bm25(name, 'port') * (1 + sin(radians(lat))) DESC LIMIT 2";
    assert_eq!(ids(&db, q), ["oakland", "sf"]);
}

/// A plan with these functions names its feature `geo`, so that a ChloeDB
/// from before them refuses it by name, and goes through JSON whole;
/// `radians`, `degrees` and `pi` are arithmetic every version reads.
#[test]
fn geo_functions_name_their_plan_feature() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    for q in [
        "SELECT sin(lat) FROM places",
        "SELECT cot(lat) FROM places",
        "SELECT id FROM places ORDER BY haversine(lat, lon, 0, 0) LIMIT 2",
        "SELECT h3_latlng_to_cell(lat, lon, 5) AS c, COUNT(*) FROM places GROUP BY c",
        "SELECT id FROM places WHERE st_dwithin(st_point(lon, lat), 'POINT(0 0)', 1000)",
        "SELECT name FROM counties WHERE st_within('POINT(-122.4 37.7)', geom)",
    ] {
        let plan = db.explain(q, &Params::new()).unwrap();
        assert!(plan.features.iter().any(|f| f == ir::GEO), "{q}: {:?}", plan.features);
        assert_eq!(Plan::from_json(&plan.to_json().unwrap()).unwrap(), plan, "{q}");
        let features: Vec<&str> = plan.features.iter().map(String::as_str).collect();
        let older: Vec<&str> = ir::FEATURES.iter().copied().filter(|f| *f != ir::GEO).collect();
        let e = Plan::readable_by(plan.version as u64, &features, &older).err().expect("refused").to_string();
        assert!(e.contains("\"geo\""), "{q}: {e}");
    }
    let plan = db.explain("SELECT radians(lat), degrees(lon), pi() FROM places", &Params::new()).unwrap();
    assert!(!plan.features.iter().any(|f| f == ir::GEO), "{:?}", plan.features);
    let SqlResult::Rows(r) = db.sql("EXPLAIN SELECT id FROM places WHERE st_contains('POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))', st_point(lon, lat))", &Params::new()).unwrap() else { panic!("rows") };
    let text = r.rows.into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Filter: st_contains('POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))', st_point(lon, lat))"), "{text}");
}

/// DataFusion lowers the functions itself, rather than leaving the plan
/// to the operators.
#[cfg(feature = "datafusion")]
#[test]
fn datafusion_lowers_them() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let opts = ExecOptions { engine: Engine::DataFusionOnly, ..db.exec.clone() };
    for (q, want) in [
        ("SELECT id, h3_latlng_to_cell_string(lat, lon, 5) FROM places WHERE haversine(lat, lon, 37.7749, -122.4194) < 20 ORDER BY id", vec![vec![s("oakland"), s("85283083fffffff")], vec![s("sf"), s("85283083fffffff")]]),
        ("SELECT id FROM places WHERE st_contains('POLYGON((-122.52 37.70, -122.35 37.70, -122.35 37.83, -122.52 37.83, -122.52 37.70))', st_point(lon, lat))", vec![vec![s("sf")]]),
        ("SELECT h3_cell_to_parent(h3_latlng_to_cell(lat, lon, 5), 2) AS c, COUNT(*) FROM places WHERE lat < 0 GROUP BY c", vec![vec![Value::Int(588613104569417727), Value::Int(2)]]),
        ("SELECT name, st_contains(geom, 'POINT(-122.27 37.80)') FROM counties ORDER BY name", vec![vec![s("alameda"), Value::Bool(true)], vec![s("sf"), Value::Bool(false)]]),
    ] {
        let rows = db.query_with(q, &Params::new(), &opts).unwrap_or_else(|e| panic!("{q}: {e}")).rows;
        assert_eq!(rows, want, "{q}");
    }
}
