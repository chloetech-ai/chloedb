//! Geography: points, lines and polygons on the Earth, kept as text.
//!
//! ChloeDB has no geometry type: a shape is a TEXT value, in WKT
//! (`POINT(-122.42 37.77)`, `POLYGON((...))`, as PostGIS writes it) or in
//! GeoJSON (`{"type": "Polygon", ...}`, a Feature or a FeatureCollection
//! too), its coordinates longitude then latitude, in degrees (WGS 84, SRID
//! 4326). [`parse`] reads either, and [`Geom::wkt`] and [`Geom::geojson`]
//! write one. The SQL functions over them are in [`sql`].
//!
//! * **Edges are straight in longitude and latitude**, as GeoJSON's RFC
//!   7946 draws them and PostGIS's `geometry` reads them: what is inside a
//!   polygon, and whether two shapes meet ([`Geom::contains`],
//!   [`Geom::intersects`]), is decided on that flat map. A shape across the
//!   antimeridian is written as two, split at longitude 180, as RFC 7946
//!   asks.
//! * **Lengths and areas are on a sphere** of the Earth's mean radius
//!   ([`EARTH_RADIUS_M`]): [`Geom::distance`] in meters, by the haversine
//!   between points and to the nearest point of an edge; [`Geom::area`] in
//!   square meters, by the spherical excess of each ring between its
//!   vertices (the formula of Chamberlain and Duquette, 2007, which
//!   Turf.js uses). Within a country these agree with an ellipsoid's to
//!   about 0.5%.
//!
//! A large shape written out in a query (a fire's perimeter of thousands
//! of vertices) is parsed once per thread and kept while queries use it
//! ([`prepared`]), so that a condition over a million rows does not parse
//! it a million times.

pub mod sql;

use std::cell::RefCell;
use std::fmt::Write as _;
use std::sync::Arc;

/// The Earth's mean radius in meters (IUGG), which distances and areas on
/// the sphere take.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// The radius `haversine()` takes, in kilometers, as Snowflake's
/// `HAVERSINE` does: its answers are Snowflake's to the last digit.
pub const HAVERSINE_RADIUS_KM: f64 = 6371.0;

/// The great-circle distance between two points given in degrees, on a
/// sphere of radius `r` (in the unit of the answer).
pub fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64, r: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().min(1.0).asin()
}

/// A position: longitude `x` and latitude `y`, in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

/// A ring of a polygon: closed, its last position its first.
pub type Ring = Vec<Pt>;

/// A shape, as WKT and GeoJSON have them.
#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    Point(Pt),
    LineString(Vec<Pt>),
    /// Its exterior ring, then its holes.
    Polygon(Vec<Ring>),
    MultiPoint(Vec<Pt>),
    MultiLineString(Vec<Vec<Pt>>),
    MultiPolygon(Vec<Vec<Ring>>),
    GeometryCollection(Vec<Geom>),
    /// A shape of no positions, of the kind named (`POINT EMPTY`).
    Empty(Kind),
}

/// The kinds of shapes, as WKT names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
    GeometryCollection,
}

impl Kind {
    const ALL: [Kind; 7] = [Kind::Point, Kind::LineString, Kind::Polygon, Kind::MultiPoint, Kind::MultiLineString, Kind::MultiPolygon, Kind::GeometryCollection];

    pub fn wkt(self) -> &'static str {
        match self {
            Kind::Point => "POINT",
            Kind::LineString => "LINESTRING",
            Kind::Polygon => "POLYGON",
            Kind::MultiPoint => "MULTIPOINT",
            Kind::MultiLineString => "MULTILINESTRING",
            Kind::MultiPolygon => "MULTIPOLYGON",
            Kind::GeometryCollection => "GEOMETRYCOLLECTION",
        }
    }

    fn geojson(self) -> &'static str {
        match self {
            Kind::Point => "Point",
            Kind::LineString => "LineString",
            Kind::Polygon => "Polygon",
            Kind::MultiPoint => "MultiPoint",
            Kind::MultiLineString => "MultiLineString",
            Kind::MultiPolygon => "MultiPolygon",
            Kind::GeometryCollection => "GeometryCollection",
        }
    }
}

/// Why a text is not a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct BadShape(pub String);

impl std::fmt::Display for BadShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T, BadShape> {
    Err(BadShape(m.into()))
}

/// The shape `text` writes, in WKT (with an optional `SRID=4326;`, and Z
/// and M values, which are dropped) or GeoJSON (a geometry, a Feature or a
/// FeatureCollection, whose features' shapes make a collection).
pub fn parse(text: &str) -> Result<Geom, BadShape> {
    let t = text.trim_start();
    let g = if t.starts_with('{') { geojson(t)? } else { Wkt { s: t.as_bytes(), at: 0 }.read()? };
    g.check()?;
    Ok(g)
}

/// A shape ready for tests: parsed, with its bounding box.
#[derive(Debug)]
pub struct Prepared {
    pub geom: Geom,
    pub bbox: BBox,
}

impl Prepared {
    pub fn new(geom: Geom) -> Prepared {
        let bbox = geom.bbox();
        Prepared { geom, bbox }
    }
}

/// Texts at least this long are kept parsed ([`prepared`]).
const KEEP_FROM: usize = 512;
/// The most texts kept, and the most bytes of them, per thread.
const KEEP_MOST: usize = 64;
const KEEP_BYTES: usize = 64 << 20;

thread_local! {
    /// The long texts this thread parsed last, most recent last, with their
    /// shapes. An entry is used only when its text is the one asked for,
    /// byte for byte.
    static KEPT: RefCell<Vec<(Box<str>, Arc<Prepared>)>> = const { RefCell::new(Vec::new()) };
}

/// The shape of `text`, parsed, from this thread's cache when it is long
/// and was parsed before.
pub fn prepared(text: &str) -> Result<Arc<Prepared>, BadShape> {
    if text.len() < KEEP_FROM {
        return Ok(Arc::new(Prepared::new(parse(text)?)));
    }
    let hit = KEPT.with(|k| {
        let mut k = k.borrow_mut();
        let i = k.iter().rposition(|(t, _)| t.len() == text.len() && **t == *text)?;
        // Most recent last: a shape used over many rows stays.
        let e = k.remove(i);
        let p = e.1.clone();
        k.push(e);
        Some(p)
    });
    if let Some(p) = hit {
        return Ok(p);
    }
    let p = Arc::new(Prepared::new(parse(text)?));
    KEPT.with(|k| {
        let mut k = k.borrow_mut();
        k.push((text.into(), p.clone()));
        let mut bytes: usize = k.iter().map(|(t, _)| t.len()).sum();
        while k.len() > KEEP_MOST || (bytes > KEEP_BYTES && k.len() > 1) {
            bytes -= k[0].0.len();
            k.remove(0);
        }
    });
    Ok(p)
}

/// A bounding box in degrees; empty when `x0 > x1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl BBox {
    const EMPTY: BBox = BBox { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };

    fn add(&mut self, p: Pt) {
        self.x0 = self.x0.min(p.x);
        self.y0 = self.y0.min(p.y);
        self.x1 = self.x1.max(p.x);
        self.y1 = self.y1.max(p.y);
    }

    fn meets(&self, o: &BBox) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    fn holds(&self, p: Pt) -> bool {
        self.x0 <= p.x && p.x <= self.x1 && self.y0 <= p.y && p.y <= self.y1
    }

    fn of_segment(a: Pt, b: Pt) -> BBox {
        BBox { x0: a.x.min(b.x), y0: a.y.min(b.y), x1: a.x.max(b.x), y1: a.y.max(b.y) }
    }
}

/// A shape's parts, flat: its points, its lines and its polygons.
#[derive(Default)]
struct Parts<'a> {
    points: Vec<Pt>,
    lines: Vec<&'a [Pt]>,
    polygons: Vec<&'a [Ring]>,
}

impl<'a> Parts<'a> {
    fn of(g: &'a Geom) -> Parts<'a> {
        let mut p = Parts::default();
        p.add(g);
        p
    }

    fn add(&mut self, g: &'a Geom) {
        match g {
            Geom::Point(p) => self.points.push(*p),
            Geom::MultiPoint(ps) => self.points.extend(ps),
            Geom::LineString(l) => self.lines.push(l),
            Geom::MultiLineString(ls) => self.lines.extend(ls.iter().map(Vec::as_slice)),
            Geom::Polygon(rs) => self.polygons.push(rs),
            Geom::MultiPolygon(ps) => self.polygons.extend(ps.iter().map(Vec::as_slice)),
            Geom::GeometryCollection(gs) => gs.iter().for_each(|g| self.add(g)),
            Geom::Empty(_) => {}
        }
    }

    fn is_empty(&self) -> bool {
        self.points.is_empty() && self.lines.is_empty() && self.polygons.is_empty()
    }

    /// Every vertex: the points, the lines' and the rings'.
    fn vertices<'s>(&'s self) -> impl Iterator<Item = Pt> + 's
    where
        'a: 's,
    {
        self.points.iter().copied().chain(self.lines.iter().flat_map(|l| l.iter().copied())).chain(self.polygons.iter().flat_map(|rs| rs.iter().flat_map(|r| r.iter().copied())))
    }

    /// Every edge: the lines' and the rings'.
    fn segments<'s>(&'s self) -> impl Iterator<Item = (Pt, Pt)> + 's
    where
        'a: 's,
    {
        let pairs = |l: &'s [Pt]| l.windows(2).map(|w| (w[0], w[1]));
        self.lines.iter().flat_map(move |l| pairs(l)).chain(self.polygons.iter().flat_map(move |rs| rs.iter().flat_map(move |r| pairs(r))))
    }
}

/// Where a position is with respect to a polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Inside,
    Boundary,
    Outside,
}

/// `(b - a) x (c - a)`: above 0 when `c` is left of `a -> b`.
fn cross(a: Pt, b: Pt, c: Pt) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether `p` is on the segment `a -> b`.
fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    cross(a, b, p) == 0.0 && BBox::of_segment(a, b).holds(p)
}

/// Where `p` is in the polygon of rings `rs` (exterior first): the rings
/// it is inside of, counted, decide (even-odd), and being on an edge is
/// the boundary.
fn locate(p: Pt, rs: &[Ring]) -> Place {
    let mut inside = false;
    for r in rs {
        for w in r.windows(2) {
            let (a, b) = (w[0], w[1]);
            if on_segment(p, a, b) {
                return Place::Boundary;
            }
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
    }
    if inside {
        Place::Inside
    } else {
        Place::Outside
    }
}

/// Where `p` is in the polygons `ps` together: inside one, on one's
/// boundary, or outside them all.
fn locate_any(p: Pt, ps: &[&[Ring]]) -> Place {
    let mut on = false;
    for rs in ps {
        match locate(p, rs) {
            Place::Inside => return Place::Inside,
            Place::Boundary => on = true,
            Place::Outside => {}
        }
    }
    if on {
        Place::Boundary
    } else {
        Place::Outside
    }
}

/// Whether segments `a -> b` and `c -> d` share a position.
fn segments_meet(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && on_segment(a, c, d)) || (d2 == 0.0 && on_segment(b, c, d)) || (d3 == 0.0 && on_segment(c, a, b)) || (d4 == 0.0 && on_segment(d, a, b))
}

/// Whether segments `a -> b` and `c -> d` cross at a position inside both,
/// each going from one side of the other to its other side.
fn segments_cross(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn mid(a: Pt, b: Pt) -> Pt {
    Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 }
}

impl Geom {
    pub fn kind(&self) -> Kind {
        match self {
            Geom::Point(_) => Kind::Point,
            Geom::LineString(_) => Kind::LineString,
            Geom::Polygon(_) => Kind::Polygon,
            Geom::MultiPoint(_) => Kind::MultiPoint,
            Geom::MultiLineString(_) => Kind::MultiLineString,
            Geom::MultiPolygon(_) => Kind::MultiPolygon,
            Geom::GeometryCollection(_) => Kind::GeometryCollection,
            Geom::Empty(k) => *k,
        }
    }

    pub fn is_empty(&self) -> bool {
        Parts::of(self).is_empty()
    }

    pub fn bbox(&self) -> BBox {
        let mut b = BBox::EMPTY;
        Parts::of(self).vertices().for_each(|p| b.add(p));
        b
    }

    /// Positions in range: longitude from -180 to 180, latitude from -90 to
    /// 90, and lines and rings with enough of them.
    pub(crate) fn check(&self) -> Result<(), BadShape> {
        let parts = Parts::of(self);
        for p in parts.vertices() {
            if !(p.x.is_finite() && p.y.is_finite()) {
                return bad(format!("a position of a shape is a finite longitude and latitude, not ({} {})", p.x, p.y));
            }
            if !(-90.0..=90.0).contains(&p.y) {
                return bad(format!("latitude {} is outside -90 to 90: a shape's positions are longitude then latitude, in degrees", p.y));
            }
            if !(-180.0..=180.0).contains(&p.x) {
                return bad(format!("longitude {} is outside -180 to 180: a shape's positions are longitude then latitude, in degrees", p.x));
            }
        }
        if parts.lines.iter().any(|l| l.len() < 2) {
            return bad("a line has at least two positions");
        }
        if parts.polygons.iter().flat_map(|rs| rs.iter()).any(|r| r.len() < 4) {
            return bad("a polygon's ring has at least three positions, and ends where it starts");
        }
        Ok(())
    }

    /// Whether every position of `o` is in this shape and some position of
    /// `o`'s inside is in this one's inside, as `ST_Contains` decides: a
    /// point on a polygon's boundary is not in it. Exact for points; a line
    /// or polygon is decided by its vertices, its edges' crossings with
    /// this shape's boundary and their middles.
    pub fn contains(&self, o: &Geom) -> bool {
        let (a, b) = (Parts::of(self), Parts::of(o));
        if a.is_empty() || b.is_empty() {
            return false;
        }
        let (ba, bb) = (self.bbox(), o.bbox());
        if !(ba.x0 <= bb.x0 && bb.x1 <= ba.x1 && ba.y0 <= bb.y0 && bb.y1 <= ba.y1) {
            return false;
        }
        if a.polygons.is_empty() {
            return contains_without_area(&a, &b);
        }
        let place = |p: Pt| locate_any(p, &a.polygons);
        // Every vertex of `o` in, or on the boundary.
        let mut inner = false;
        for p in b.vertices() {
            match place(p) {
                Place::Outside => return false,
                Place::Inside => inner = true,
                Place::Boundary => {}
            }
        }
        // No edge of `o` crosses out, and none runs outside between two
        // positions on the boundary.
        for (p, q) in b.segments() {
            let sb = BBox::of_segment(p, q);
            for (c, d) in a.segments() {
                if sb.meets(&BBox::of_segment(c, d)) && segments_cross(p, q, c, d) {
                    return false;
                }
            }
            match place(mid(p, q)) {
                Place::Outside => return false,
                Place::Inside => inner = true,
                Place::Boundary => {}
            }
        }
        // No part of this shape's boundary inside `o`: a hole of this one
        // in `o`, or `o` around this one.
        if !b.polygons.is_empty() && a.vertices().any(|p| locate_any(p, &b.polygons) == Place::Inside) {
            return false;
        }
        if !inner {
            // Every position on the boundary (`o` is this shape): the middle
            // of `o`'s vertices.
            let vs: Vec<Pt> = b.vertices().collect();
            let n = vs.len() as f64;
            let c = Pt { x: vs.iter().map(|p| p.x).sum::<f64>() / n, y: vs.iter().map(|p| p.y).sum::<f64>() / n };
            inner = !b.polygons.is_empty() && place(c) == Place::Inside && locate_any(c, &b.polygons) == Place::Inside;
        }
        inner
    }

    /// Whether the two shapes share a position (`ST_Intersects`): one
    /// touching the other's boundary counts.
    pub fn intersects(&self, o: &Geom) -> bool {
        let (a, b) = (Parts::of(self), Parts::of(o));
        if a.is_empty() || b.is_empty() || !self.bbox().meets(&o.bbox()) {
            return false;
        }
        intersects_parts(&a, &b, &o.bbox())
    }

    /// The least distance in meters, on the sphere, between a position of
    /// this shape and one of `o`'s: 0 where they meet. Between two points
    /// the haversine; to an edge, the haversine to the edge's nearest
    /// position on a flat map of the neighbourhood (good for edges up to a
    /// few hundred kilometers). NaN for an empty shape.
    pub fn distance(&self, o: &Geom) -> f64 {
        let (a, b) = (Parts::of(self), Parts::of(o));
        if a.is_empty() || b.is_empty() {
            return f64::NAN;
        }
        if self.bbox().meets(&o.bbox()) && intersects_parts(&a, &b, &o.bbox()) {
            return 0.0;
        }
        let mut best = f64::INFINITY;
        let points = |p: &Parts| p.points.clone();
        let (ap, bp) = (points(&a), points(&b));
        let (asg, bsg): (Vec<(Pt, Pt)>, Vec<(Pt, Pt)>) = (a.segments().collect(), b.segments().collect());
        for p in &ap {
            for q in &bp {
                best = best.min(meters(*p, *q));
            }
            for &(c, d) in &bsg {
                best = best.min(to_segment(*p, c, d));
            }
        }
        for q in &bp {
            for &(c, d) in &asg {
                best = best.min(to_segment(*q, c, d));
            }
        }
        // Two edges that do not meet are nearest at an end of one of them.
        for &(p, q) in &asg {
            for &(c, d) in &bsg {
                best = best.min(to_segment(p, c, d)).min(to_segment(q, c, d)).min(to_segment(c, p, q)).min(to_segment(d, p, q));
            }
        }
        best
    }

    /// The area in square meters on the sphere: each polygon's exterior
    /// less its holes; 0 for points and lines.
    pub fn area(&self) -> f64 {
        Parts::of(self).polygons.iter().map(|rs| rs.iter().enumerate().map(|(i, r)| if i == 0 { ring_area(r).abs() } else { -ring_area(r).abs() }).sum::<f64>().max(0.0)).sum()
    }

    /// The shape in WKT, as PostGIS's `ST_AsText` writes it.
    pub fn wkt(&self) -> String {
        let mut s = String::new();
        self.write_wkt(&mut s);
        s
    }

    fn write_wkt(&self, s: &mut String) {
        fn pt(s: &mut String, p: &Pt) {
            let _ = write!(s, "{} {}", p.x, p.y);
        }
        fn list(s: &mut String, ps: &[Pt]) {
            s.push('(');
            for (i, p) in ps.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                pt(s, p);
            }
            s.push(')');
        }
        fn rings(s: &mut String, rs: &[Ring]) {
            s.push('(');
            for (i, r) in rs.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                list(s, r);
            }
            s.push(')');
        }
        s.push_str(self.kind().wkt());
        match self {
            Geom::Empty(_) => s.push_str(" EMPTY"),
            Geom::Point(p) => list(s, std::slice::from_ref(p)),
            Geom::LineString(l) => list(s, l),
            Geom::Polygon(rs) => rings(s, rs),
            Geom::MultiPoint(ps) => {
                s.push('(');
                for (i, p) in ps.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    list(s, std::slice::from_ref(p));
                }
                s.push(')');
            }
            Geom::MultiLineString(ls) => rings(s, ls),
            Geom::MultiPolygon(ps) => {
                s.push('(');
                for (i, rs) in ps.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    rings(s, rs);
                }
                s.push(')');
            }
            Geom::GeometryCollection(gs) if gs.is_empty() => s.push_str(" EMPTY"),
            Geom::GeometryCollection(gs) => {
                s.push('(');
                for (i, g) in gs.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    g.write_wkt(s);
                }
                s.push(')');
            }
        }
    }

    /// The shape as a GeoJSON geometry, `{"type":"Point","coordinates":[-122.42,37.77]}`.
    pub fn geojson(&self) -> String {
        let mut s = String::new();
        self.write_geojson(&mut s);
        s
    }

    fn write_geojson(&self, s: &mut String) {
        fn pt(s: &mut String, p: &Pt) {
            let _ = write!(s, "[{},{}]", p.x, p.y);
        }
        fn list(s: &mut String, ps: &[Pt]) {
            s.push('[');
            for (i, p) in ps.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                pt(s, p);
            }
            s.push(']');
        }
        fn rings(s: &mut String, rs: &[Ring]) {
            s.push('[');
            for (i, r) in rs.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                list(s, r);
            }
            s.push(']');
        }
        let _ = write!(s, "{{\"type\":\"{}\",", self.kind().geojson());
        match self {
            Geom::GeometryCollection(gs) => {
                s.push_str("\"geometries\":[");
                for (i, g) in gs.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    g.write_geojson(s);
                }
                s.push(']');
            }
            Geom::Empty(Kind::GeometryCollection) => s.push_str("\"geometries\":[]"),
            g => {
                s.push_str("\"coordinates\":");
                match g {
                    Geom::Point(p) => pt(s, p),
                    Geom::LineString(l) | Geom::MultiPoint(l) => list(s, l),
                    Geom::Polygon(rs) | Geom::MultiLineString(rs) => rings(s, rs),
                    Geom::MultiPolygon(ps) => {
                        s.push('[');
                        for (i, rs) in ps.iter().enumerate() {
                            if i > 0 {
                                s.push(',');
                            }
                            rings(s, rs);
                        }
                        s.push(']');
                    }
                    _ => s.push_str("[]"),
                }
            }
        }
        s.push('}');
    }
}

/// [`Geom::contains`] for a shape `a` of points and lines alone.
fn contains_without_area(a: &Parts, b: &Parts) -> bool {
    if !b.polygons.is_empty() {
        return false;
    }
    let on = |p: Pt| a.points.contains(&p) || a.lines.iter().any(|l| l.windows(2).any(|w| on_segment(p, w[0], w[1])));
    // A line's interior is all of it but the ends of a line that does not
    // close.
    let end = |p: Pt| a.points.is_empty() && a.lines.iter().all(|l| l.first() != l.last() && (l.first() == Some(&p) || l.last() == Some(&p)));
    let mut inner = false;
    for p in b.vertices() {
        if !on(p) {
            return false;
        }
        inner |= !end(p);
    }
    for (p, q) in b.segments() {
        if !on(mid(p, q)) {
            return false;
        }
        inner = true;
    }
    inner
}

/// [`Geom::intersects`] of two shapes' parts; `bb` is `b`'s box.
fn intersects_parts(a: &Parts, b: &Parts, bb: &BBox) -> bool {
    // A vertex of one inside or on the other: points, and containment
    // without crossing edges.
    if !a.polygons.is_empty() && b.vertices().any(|p| locate_any(p, &a.polygons) != Place::Outside) {
        return true;
    }
    if !b.polygons.is_empty() && a.vertices().any(|p| locate_any(p, &b.polygons) != Place::Outside) {
        return true;
    }
    let on_lines = |p: Pt, s: &Parts| s.points.contains(&p) || s.segments().any(|(c, d)| on_segment(p, c, d));
    if a.points.iter().any(|&p| on_lines(p, b)) || b.points.iter().any(|&p| on_lines(p, a)) {
        return true;
    }
    let bs: Vec<(Pt, Pt, BBox)> = b.segments().map(|(c, d)| (c, d, BBox::of_segment(c, d))).collect();
    for (p, q) in a.segments() {
        let sb = BBox::of_segment(p, q);
        if !sb.meets(bb) {
            continue;
        }
        if bs.iter().any(|(c, d, cb)| sb.meets(cb) && segments_meet(p, q, *c, *d)) {
            return true;
        }
    }
    false
}

/// The haversine between two positions, in meters.
fn meters(p: Pt, q: Pt) -> f64 {
    haversine(p.y, p.x, q.y, q.x, EARTH_RADIUS_M)
}

/// The meters from `p` to the nearest position of the edge `a -> b`, found
/// on a flat map around `p` (longitudes scaled by the cosine of its
/// latitude).
fn to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let k = p.y.to_radians().cos();
    let wrap = |dx: f64| if dx > 180.0 { dx - 360.0 } else if dx < -180.0 { dx + 360.0 } else { dx };
    let (ax, ay) = (wrap(a.x - p.x) * k, a.y - p.y);
    let (bx, by) = (wrap(b.x - p.x) * k, b.y - p.y);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0) };
    let near = Pt { x: a.x + t * wrap(b.x - a.x), y: a.y + t * (b.y - a.y) };
    meters(p, near)
}

/// A closed ring's signed area on the sphere, in square meters
/// (Chamberlain and Duquette, "Some algorithms for polygons on a sphere",
/// 2007): positive counterclockwise.
fn ring_area(r: &[Pt]) -> f64 {
    let n = r.len().saturating_sub(1);
    if n < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 0..n {
        let (lo, m, hi) = (r[i], r[(i + 1) % n], r[(i + 2) % n]);
        total += (hi.x - lo.x).to_radians() * m.y.to_radians().sin();
    }
    total * EARTH_RADIUS_M * EARTH_RADIUS_M / 2.0
}

/// A WKT reader.
struct Wkt<'a> {
    s: &'a [u8],
    at: usize,
}

impl Wkt<'_> {
    fn read(mut self) -> Result<Geom, BadShape> {
        // EWKT's SRID, which must be WGS 84's.
        if self.s.len() >= 5 && self.s[..5].eq_ignore_ascii_case(b"SRID=") {
            let end = self.s.iter().position(|&c| c == b';').ok_or_else(|| BadShape("SRID=4326; comes before the shape".into()))?;
            let srid = std::str::from_utf8(&self.s[5..end]).unwrap_or("").trim();
            if srid != "4326" {
                return bad(format!("SRID {srid}: ChloeDB's shapes are longitude and latitude in degrees, WGS 84 (SRID 4326)"));
            }
            self.at = end + 1;
        }
        let g = self.geom()?;
        self.space();
        if self.at < self.s.len() {
            return bad(format!("unexpected {} after a shape in WKT", self.rest()));
        }
        Ok(g)
    }

    /// Where the reader is, for a message: the next characters, or the end.
    fn rest(&self) -> String {
        match self.at >= self.s.len() {
            true => "its end".into(),
            false => format!("{:?}", String::from_utf8_lossy(&self.s[self.at..(self.at + 20).min(self.s.len())])),
        }
    }

    fn space(&mut self) {
        while self.at < self.s.len() && self.s[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.s.get(self.at).copied()
    }

    fn byte(&mut self, b: u8) -> Result<(), BadShape> {
        match self.peek() {
            Some(c) if c == b => {
                self.at += 1;
                Ok(())
            }
            _ => bad(format!("expected {:?} in WKT at {}", b as char, self.rest())),
        }
    }

    fn word(&mut self) -> String {
        self.space();
        let start = self.at;
        while self.at < self.s.len() && self.s[self.at].is_ascii_alphabetic() {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.at]).to_ascii_uppercase()
    }

    /// `EMPTY`, or else the opening `(`, after a kind and its Z, M or ZM.
    fn open(&mut self) -> Result<bool, BadShape> {
        let save = self.at;
        match self.word().as_str() {
            "EMPTY" => return Ok(false),
            "Z" | "M" | "ZM" => return self.open(),
            "" => {}
            _ => self.at = save,
        }
        self.byte(b'(')?;
        Ok(true)
    }

    fn geom(&mut self) -> Result<Geom, BadShape> {
        let w = self.word();
        let kind = Kind::ALL.into_iter().find(|k| k.wkt() == w).ok_or_else(|| {
            BadShape(if w.is_empty() {
                format!("{} is not a shape: a shape is WKT (POINT(-122.42 37.77), POLYGON((...))) or GeoJSON ({{\"type\": \"Polygon\", ...}})", if self.at >= self.s.len() { "an empty text".to_string() } else { self.rest() })
            } else {
                format!("{w} is not a kind of shape: POINT, LINESTRING, POLYGON, MULTIPOINT, MULTILINESTRING, MULTIPOLYGON or GEOMETRYCOLLECTION")
            })
        })?;
        if !self.open()? {
            return Ok(Geom::Empty(kind));
        }
        let g = match kind {
            Kind::Point => Geom::Point(self.pt()?),
            Kind::LineString => Geom::LineString(self.pts_open()?),
            Kind::Polygon => Geom::Polygon(self.rings_open()?),
            Kind::MultiPoint => {
                let mut ps = Vec::new();
                loop {
                    // MULTIPOINT((1 2), (3 4)) and MULTIPOINT(1 2, 3 4).
                    if self.peek() == Some(b'(') {
                        self.at += 1;
                        ps.push(self.pt()?);
                        self.byte(b')')?;
                    } else {
                        ps.push(self.pt()?);
                    }
                    if !self.comma() {
                        break;
                    }
                }
                Geom::MultiPoint(ps)
            }
            Kind::MultiLineString => Geom::MultiLineString(self.list(|w| {
                w.byte(b'(')?;
                w.pts_open()
            })?),
            Kind::MultiPolygon => Geom::MultiPolygon(self.list(|w| {
                w.byte(b'(')?;
                w.rings_open()
            })?),
            Kind::GeometryCollection => Geom::GeometryCollection(self.list(|w| w.geom())?),
        };
        if kind != Kind::LineString && kind != Kind::Polygon && kind != Kind::MultiLineString && kind != Kind::MultiPolygon && kind != Kind::GeometryCollection {
            self.byte(b')')?;
        }
        Ok(g)
    }

    fn comma(&mut self) -> bool {
        if self.peek() == Some(b',') {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Items read by `f`, separated by commas, up to and with the closing
    /// `)`.
    fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T, BadShape>) -> Result<Vec<T>, BadShape> {
        let mut out = vec![f(self)?];
        while self.comma() {
            out.push(f(self)?);
        }
        self.byte(b')')?;
        Ok(out)
    }

    fn pts_open(&mut self) -> Result<Vec<Pt>, BadShape> {
        self.list(|w| w.pt())
    }

    fn rings_open(&mut self) -> Result<Vec<Ring>, BadShape> {
        self.list(|w| {
            w.byte(b'(')?;
            Ok(closed(w.pts_open()?))
        })
    }

    fn num(&mut self) -> Result<f64, BadShape> {
        self.space();
        let start = self.at;
        while self.at < self.s.len() && matches!(self.s[self.at], b'0'..=b'9' | b'.' | b'-' | b'+' | b'e' | b'E') {
            self.at += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
        t.parse::<f64>().map_err(|_| BadShape(format!("expected a number in WKT at {:?}", String::from_utf8_lossy(&self.s[start..(start + 20).min(self.s.len())]))))
    }

    /// A position: longitude and latitude, and any Z and M after them.
    fn pt(&mut self) -> Result<Pt, BadShape> {
        let (x, y) = (self.num()?, self.num()?);
        while matches!(self.peek(), Some(b'0'..=b'9' | b'-' | b'+' | b'.')) {
            self.num()?;
        }
        Ok(Pt { x, y })
    }
}

/// `r` ending where it starts.
fn closed(mut r: Vec<Pt>) -> Ring {
    if r.len() > 1 && r.first() != r.last() {
        r.push(r[0]);
    }
    r
}

/// The shape of a GeoJSON text.
fn geojson(text: &str) -> Result<Geom, BadShape> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| BadShape(format!("not GeoJSON: {e}")))?;
    geojson_of(&v)
}

fn geojson_of(v: &serde_json::Value) -> Result<Geom, BadShape> {
    use serde_json::Value as J;
    let ty = v.get("type").and_then(J::as_str).ok_or_else(|| BadShape("a GeoJSON object has a \"type\"".into()))?;
    let pt = |c: &J| -> Result<Pt, BadShape> {
        match c.as_array().map(Vec::as_slice) {
            Some([x, y, ..]) => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => Ok(Pt { x, y }),
                _ => bad(format!("a GeoJSON position is [longitude, latitude], not {c}")),
            },
            _ => bad(format!("a GeoJSON position is [longitude, latitude], not {c}")),
        }
    };
    let arr = |c: &J| -> Result<Vec<J>, BadShape> { c.as_array().cloned().ok_or_else(|| BadShape(format!("expected a list in GeoJSON, not {c}"))) };
    let pts = |c: &J| -> Result<Vec<Pt>, BadShape> { arr(c)?.iter().map(pt).collect() };
    let rings = |c: &J| -> Result<Vec<Ring>, BadShape> { arr(c)?.iter().map(|r| pts(r).map(closed)).collect() };
    let coords = || v.get("coordinates").ok_or_else(|| BadShape(format!("a GeoJSON {ty} has \"coordinates\"")));
    let empty = |k: Kind, c: &J| c.as_array().is_some_and(Vec::is_empty).then_some(Geom::Empty(k));
    Ok(match ty {
        "Point" => match empty(Kind::Point, coords()?) {
            Some(e) => e,
            None => Geom::Point(pt(coords()?)?),
        },
        "LineString" => empty(Kind::LineString, coords()?).map_or_else(|| Ok(Geom::LineString(pts(coords()?)?)), Ok)?,
        "Polygon" => empty(Kind::Polygon, coords()?).map_or_else(|| Ok(Geom::Polygon(rings(coords()?)?)), Ok)?,
        "MultiPoint" => empty(Kind::MultiPoint, coords()?).map_or_else(|| Ok(Geom::MultiPoint(pts(coords()?)?)), Ok)?,
        "MultiLineString" => empty(Kind::MultiLineString, coords()?).map_or_else(|| Ok(Geom::MultiLineString(arr(coords()?)?.iter().map(pts).collect::<Result<_, _>>()?)), Ok)?,
        "MultiPolygon" => empty(Kind::MultiPolygon, coords()?).map_or_else(|| Ok(Geom::MultiPolygon(arr(coords()?)?.iter().map(rings).collect::<Result<_, _>>()?)), Ok)?,
        "GeometryCollection" => {
            let gs = arr(v.get("geometries").ok_or_else(|| BadShape("a GeoJSON GeometryCollection has \"geometries\"".into()))?)?;
            match gs.is_empty() {
                true => Geom::Empty(Kind::GeometryCollection),
                false => Geom::GeometryCollection(gs.iter().map(geojson_of).collect::<Result<_, _>>()?),
            }
        }
        // A Feature is its geometry (none is an empty shape); a
        // FeatureCollection, its features' shapes together.
        "Feature" => match v.get("geometry") {
            None | Some(J::Null) => Geom::Empty(Kind::GeometryCollection),
            Some(g) => geojson_of(g)?,
        },
        "FeatureCollection" => {
            let fs = arr(v.get("features").ok_or_else(|| BadShape("a GeoJSON FeatureCollection has \"features\"".into()))?)?;
            let gs: Vec<Geom> = fs.iter().map(geojson_of).collect::<Result<Vec<_>, _>>()?.into_iter().filter(|g| !matches!(g, Geom::Empty(_))).collect();
            match gs.len() {
                0 => Geom::Empty(Kind::GeometryCollection),
                1 => gs.into_iter().next().expect("one shape"),
                _ => Geom::GeometryCollection(gs),
            }
        }
        t => return bad(format!("{t:?} is not a GeoJSON type: Point, LineString, Polygon, their Multi kinds, GeometryCollection, Feature or FeatureCollection")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(s: &str) -> Geom {
        parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn wkt_and_geojson_read_and_write() {
        for (text, wkt) in [
            ("POINT(-122.42 37.77)", "POINT(-122.42 37.77)"),
            ("point ( 1 2 )", "POINT(1 2)"),
            ("SRID=4326;POINT Z (1 2 3)", "POINT(1 2)"),
            ("LINESTRING(0 0, 1 1, 2 0)", "LINESTRING(0 0,1 1,2 0)"),
            ("POLYGON((0 0, 4 0, 4 4, 0 4, 0 0), (1 1, 2 1, 2 2, 1 1))", "POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 1))"),
            ("POLYGON((0 0, 4 0, 4 4, 0 4))", "POLYGON((0 0,4 0,4 4,0 4,0 0))"),
            ("MULTIPOINT(1 2, 3 4)", "MULTIPOINT((1 2),(3 4))"),
            ("MULTIPOINT((1 2), (3 4))", "MULTIPOINT((1 2),(3 4))"),
            ("MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))", "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))"),
            ("GEOMETRYCOLLECTION(POINT(1 2), LINESTRING(0 0, 1 1))", "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))"),
            ("POINT EMPTY", "POINT EMPTY"),
            ("GEOMETRYCOLLECTION EMPTY", "GEOMETRYCOLLECTION EMPTY"),
            (r#"{"type": "Point", "coordinates": [-122.42, 37.77]}"#, "POINT(-122.42 37.77)"),
            (r#"{"type": "Feature", "properties": {"name": "x"}, "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]]}}"#, "POLYGON((0 0,1 0,1 1,0 0))"),
            (
                r#"{"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [1, 2]}}, {"type": "Feature", "geometry": {"type": "Point", "coordinates": [3, 4]}}]}"#,
                "GEOMETRYCOLLECTION(POINT(1 2),POINT(3 4))",
            ),
        ] {
            assert_eq!(g(text).wkt(), wkt, "{text}");
            // GeoJSON and back.
            assert_eq!(g(&g(text).geojson()), g(wkt), "{text}");
        }
        assert_eq!(g("POINT(-122.42 37.77)").geojson(), r#"{"type":"Point","coordinates":[-122.42,37.77]}"#);
        assert_eq!(g("POLYGON((0 0, 1 0, 1 1, 0 0))").geojson(), r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#);
        assert_eq!(g("GEOMETRYCOLLECTION EMPTY").geojson(), r#"{"type":"GeometryCollection","geometries":[]}"#);
        for (text, why) in [
            ("POINT(1)", "expected a number"),
            ("POINT(1 2", "expected ')'"),
            ("CIRCLE(1 2)", "not a kind of shape"),
            ("POINT(1 95)", "latitude 95"),
            ("POINT(200 5)", "longitude 200"),
            ("SRID=3857;POINT(1 2)", "SRID 3857"),
            ("POLYGON((0 0, 1 1))", "at least three positions"),
            ("LINESTRING(0 0)", "at least two"),
            ("POINT(1 2) x", "after a shape"),
            (r#"{"type": "Circle"}"#, "not a GeoJSON type"),
            ("{bad", "not GeoJSON"),
            ("hello", "not a kind of shape"),
            ("", "is not a shape"),
        ] {
            let e = parse(text).err().unwrap_or_else(|| panic!("{text}: parsed")).0;
            assert!(e.contains(why), "{text}: {e}");
        }
    }

    #[test]
    fn points_in_polygons() {
        let sq = g("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0), (4 4, 6 4, 6 6, 4 6, 4 4))");
        assert!(sq.contains(&g("POINT(1 1)")));
        assert!(!sq.contains(&g("POINT(5 5)")), "in the hole");
        assert!(!sq.contains(&g("POINT(0 5)")), "on the boundary");
        assert!(sq.intersects(&g("POINT(0 5)")), "the boundary meets");
        assert!(!sq.contains(&g("POINT(11 5)")));
        assert!(!sq.intersects(&g("POINT(11 5)")));
        assert!(sq.contains(&g("MULTIPOINT(1 1, 2 2)")));
        assert!(!sq.contains(&g("MULTIPOINT(1 1, 20 2)")));
        // A concave polygon: a C.
        let c = g("POLYGON((0 0, 10 0, 10 2, 2 2, 2 8, 10 8, 10 10, 0 10, 0 0))");
        assert!(c.contains(&g("POINT(1 5)")));
        assert!(!c.contains(&g("POINT(5 5)")));
        assert!(!c.contains(&g("LINESTRING(1 1, 9 9)")), "leaves through the gap");
        assert!(c.contains(&g("LINESTRING(1 1, 1 9)")));
        // Lines and polygons in polygons.
        assert!(sq.contains(&g("POLYGON((1 1, 3 1, 3 3, 1 1))")));
        assert!(!sq.contains(&g("POLYGON((1 1, 5 1, 5 5, 1 1))")), "over the hole");
        assert!(!sq.contains(&g("POLYGON((3 3, 7 3, 7 7, 3 7, 3 3))")), "around the hole");
        assert!(g("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))").contains(&g("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))")), "itself");
        assert!(!g("POLYGON((1 1, 2 1, 2 2, 1 1))").contains(&sq));
        // Lines and points.
        let l = g("LINESTRING(0 0, 10 0)");
        assert!(l.contains(&g("POINT(5 0)")));
        assert!(!l.contains(&g("POINT(0 0)")), "an end is the boundary");
        assert!(l.intersects(&g("POINT(0 0)")));
        assert!(l.intersects(&g("LINESTRING(5 -1, 5 1)")));
        assert!(!l.intersects(&g("LINESTRING(5 1, 6 1)")));
        assert!(g("POINT(1 2)").contains(&g("POINT(1 2)")));
        // Polygons that overlap, touch, nest and miss.
        let a = g("POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))");
        assert!(a.intersects(&g("POLYGON((2 2, 6 2, 6 6, 2 6, 2 2))")));
        assert!(a.intersects(&g("POLYGON((4 0, 8 0, 8 4, 4 4, 4 0))")), "a shared edge");
        assert!(a.intersects(&g("POLYGON((1 1, 2 1, 2 2, 1 1))")), "inside");
        assert!(g("POLYGON((1 1, 2 1, 2 2, 1 1))").intersects(&a), "around");
        assert!(!a.intersects(&g("POLYGON((5 5, 6 5, 6 6, 5 5))")));
        assert!(!sq.intersects(&g("POLYGON((4.5 4.5, 5.5 4.5, 5.5 5.5, 4.5 4.5))")), "in the hole");
        assert!(!a.contains(&Geom::Empty(Kind::Point)) && !a.intersects(&Geom::Empty(Kind::Point)));
    }

    #[test]
    fn distances_and_areas_on_the_sphere() {
        // San Francisco to Los Angeles, 559 km.
        let (sf, la) = (g("POINT(-122.4194 37.7749)"), g("POINT(-118.2437 34.0522)"));
        let d = sf.distance(&la);
        assert!((d - 559_120.0).abs() < 500.0, "{d}");
        assert_eq!(sf.distance(&sf), 0.0);
        // One degree of a meridian, from a point to a line along the equator.
        let d = g("POINT(0 1)").distance(&g("LINESTRING(-5 0, 5 0)"));
        assert!((d - 111_195.0).abs() < 1.0, "{d}");
        // To a segment's end, past it.
        let d = g("POINT(6 0)").distance(&g("LINESTRING(-5 0, 5 0)"));
        assert!((d - 111_195.0).abs() < 1.0, "{d}");
        assert_eq!(g("POINT(1 1)").distance(&g("POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))")), 0.0);
        let d = g("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))").distance(&g("POLYGON((2 0, 3 0, 3 1, 2 1, 2 0))"));
        assert!((d - 111_195.0).abs() < 20.0, "{d}");
        // Across the antimeridian.
        let d = g("POINT(179.5 0)").distance(&g("POINT(-179.5 0)"));
        assert!((d - 111_195.0).abs() < 1.0, "{d}");
        // A degree square at the equator: R^2 * dλ * (sin φ2 - sin φ1).
        let want = EARTH_RADIUS_M * EARTH_RADIUS_M * 1f64.to_radians() * 1f64.to_radians().sin();
        let a = g("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))").area();
        assert!((a - want).abs() / want < 1e-12, "{a} {want}");
        // Clockwise is the same area; a hole takes its own off.
        assert!((g("POLYGON((0 0, 0 1, 1 1, 1 0, 0 0))").area() - want).abs() / want < 1e-12);
        let holed = g("POLYGON((0 0, 2 0, 2 2, 0 2, 0 0), (0.5 0.5, 1.5 0.5, 1.5 1.5, 0.5 1.5, 0.5 0.5))").area();
        let outer = g("POLYGON((0 0, 2 0, 2 2, 0 2, 0 0))").area();
        let hole = g("POLYGON((0.5 0.5, 1.5 0.5, 1.5 1.5, 0.5 1.5, 0.5 0.5))").area();
        assert!((holed - (outer - hole)).abs() < 1e-3);
        assert_eq!(g("LINESTRING(0 0, 1 1)").area(), 0.0);
        // Haversine as Snowflake answers it, in kilometers.
        assert_eq!(format!("{:.9}", haversine(40.7127, -74.0059, 34.05, -118.25, HAVERSINE_RADIUS_KM)), "3936.385096389");
    }

    #[test]
    fn long_texts_parse_once() {
        let ring: Vec<String> = (0..200).map(|i| format!("{} {}", (i as f64 * 0.01).sin(), (i as f64 * 0.01).cos())).collect();
        let text = format!("LINESTRING({})", ring.join(", "));
        assert!(text.len() >= KEEP_FROM);
        let a = prepared(&text).unwrap();
        let b = prepared(&text).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        // Another text of the same length is another shape.
        let other = text.replacen("0 1", "0 2", 1);
        assert_eq!(other.len(), text.len());
        assert!(!Arc::ptr_eq(&a, &prepared(&other).unwrap()));
    }
}
