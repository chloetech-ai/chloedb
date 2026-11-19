//! The shape and H3 functions of SQL over values ([`Func::is_shape_or_cell`]):
//! shapes are WKT or GeoJSON text ([`super::parse`]), and H3 cells INTs or
//! their hex text (`'8928308280fffff'`), as Snowflake's `H3_*` functions
//! take them, computed by the h3o crate (H3 v4).
//!
//! NULL in is NULL out. A text that is not a shape, a cell that is not one
//! and a resolution beyond 0 to 15 are errors that say so.

use h3o::{CellIndex, LatLng, Resolution};

use super::{Geom, Kind, Pt};
use crate::error::{Error, Result};
use crate::query::ir::Func;
use crate::value::Value;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

fn sv(v: &Value) -> String {
    crate::query::hint::sql_value(v)
}

/// `func` of `args`, evaluated.
pub(crate) fn call(func: Func, args: &[&Value]) -> Result<Value> {
    if args.iter().any(|a| matches!(a, Value::Null)) {
        return Ok(Value::Null);
    }
    let arg = |i: usize| *args.get(i).unwrap_or(&&Value::Null);
    Ok(match func {
        Func::StPoint => {
            let g = Geom::Point(Pt { x: num(func, "longitude", arg(0))?, y: num(func, "latitude", arg(1))? });
            g.check().map_err(|e| err(format!("st_point: {e}")))?;
            Value::Str(g.wkt())
        }
        Func::StX | Func::StY => match &shape(func, arg(0))?.geom {
            Geom::Point(p) => Value::Float(if func == Func::StX { p.x } else { p.y }),
            Geom::Empty(Kind::Point) => Value::Null,
            g => return Err(err(format!("{} takes a POINT, not a {}", func.name(), g.kind().wkt()))),
        },
        Func::StContains | Func::StIntersects => {
            let (a, b) = (shape(func, arg(0))?, shape(func, arg(1))?);
            let meets = a.bbox.x0 <= b.bbox.x1 && b.bbox.x0 <= a.bbox.x1 && a.bbox.y0 <= b.bbox.y1 && b.bbox.y0 <= a.bbox.y1;
            Value::Bool(meets && if func == Func::StContains { a.geom.contains(&b.geom) } else { a.geom.intersects(&b.geom) })
        }
        Func::StDistance => {
            let d = shape(func, arg(0))?.geom.distance(&shape(func, arg(1))?.geom);
            if d.is_finite() {
                Value::Float(d)
            } else {
                Value::Null
            }
        }
        Func::StArea => Value::Float(shape(func, arg(0))?.geom.area()),
        Func::StAsText => Value::Str(shape(func, arg(0))?.geom.wkt()),
        Func::StAsGeoJson => Value::Str(shape(func, arg(0))?.geom.geojson()),
        Func::H3LatLngToCell | Func::H3LatLngToCellString => {
            let (lat, lng) = (num(func, "latitude", arg(0))?, num(func, "longitude", arg(1))?);
            if !(-90.0..=90.0).contains(&lat) {
                return Err(err(format!("{}: latitude {lat} is outside -90 to 90 (its arguments are latitude, then longitude)", func.name())));
            }
            let ll = LatLng::new(lat, lng).map_err(|_| err(format!("{}: ({lat}, {lng}) is not a latitude and longitude", func.name())))?;
            let cell = ll.to_cell(resolution(func, arg(2))?);
            match func {
                Func::H3LatLngToCell => Value::Int(u64::from(cell) as i64),
                _ => Value::Str(cell.to_string()),
            }
        }
        Func::H3CellToParent => {
            let c = cell(func, arg(0))?;
            let r = resolution(func, arg(1))?;
            let parent = c.parent(r).ok_or_else(|| err(format!("h3_cell_to_parent: resolution {} is finer than the cell's, {}", u8::from(r), u8::from(c.resolution()))))?;
            same_kind(arg(0), parent)
        }
        Func::H3GetResolution => Value::Int(i64::from(u8::from(cell(func, arg(0))?.resolution()))),
        Func::H3GridDistance => {
            let (a, b) = (cell(func, arg(0))?, cell(func, arg(1))?);
            if a.resolution() != b.resolution() {
                return Err(err(format!("h3_grid_distance of cells of resolutions {} and {}: they are of one resolution (h3_cell_to_parent makes a cell coarser)", u8::from(a.resolution()), u8::from(b.resolution()))));
            }
            a.grid_distance(b).map_or(Value::Null, |d| Value::Int(i64::from(d)))
        }
        Func::H3CellToPoint => {
            let ll = LatLng::from(cell(func, arg(0))?);
            Value::Str(Geom::Point(Pt { x: ll.lng(), y: ll.lat() }).wkt())
        }
        Func::H3CellToBoundary => {
            let ring: Vec<Pt> = cell(func, arg(0))?.boundary().iter().map(|ll| Pt { x: ll.lng(), y: ll.lat() }).collect();
            Value::Str(Geom::Polygon(vec![super::closed(ring)]).wkt())
        }
        Func::H3IntToString => match arg(0) {
            Value::Int(_) => Value::Str(cell(func, arg(0))?.to_string()),
            v => return Err(err(format!("h3_int_to_string takes a cell's INT, not {}", sv(v)))),
        },
        Func::H3StringToInt => match arg(0) {
            Value::Str(_) => Value::Int(u64::from(cell(func, arg(0))?) as i64),
            v => return Err(err(format!("h3_string_to_int takes a cell's hex text, not {}", sv(v)))),
        },
        Func::H3IsValidCell => Value::Bool(cell(func, arg(0)).is_ok()),
        f => return Err(err(format!("{} is not a function of shapes or H3 cells", f.name()))),
    })
}

/// A number argument of `func` (`what` names it).
fn num(func: Func, what: &str, v: &Value) -> Result<f64> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) if f.is_finite() => Ok(*f),
        v => Err(err(format!("{}'s {what} is a number, not {}", func.name(), sv(v)))),
    }
}

/// The shape of a text argument of `func`.
fn shape(func: Func, v: &Value) -> Result<std::sync::Arc<super::Prepared>> {
    match v {
        Value::Str(s) => super::prepared(s).map_err(|e| err(format!("{}: {e}", func.name()))),
        v => Err(err(format!("{} takes shapes as WKT or GeoJSON text (st_point(lon, lat) makes a point), not {}", func.name(), sv(v)))),
    }
}

/// An H3 resolution argument, 0 to 15.
fn resolution(func: Func, v: &Value) -> Result<Resolution> {
    let r = match v {
        Value::Int(i) => *i,
        Value::Float(f) if f.fract() == 0.0 => *f as i64,
        v => return Err(err(format!("{}'s resolution is a whole number from 0 to 15, not {}", func.name(), sv(v)))),
    };
    u8::try_from(r).ok().and_then(|r| Resolution::try_from(r).ok()).ok_or_else(|| err(format!("{}'s resolution is from 0 (cells of about 4.4 million km²) to 15 (about 1 m²), not {r}", func.name())))
}

/// An H3 cell argument: its INT, or its hex text.
fn cell(func: Func, v: &Value) -> Result<CellIndex> {
    let c = match v {
        Value::Int(i) => u64::try_from(*i).ok().and_then(|i| CellIndex::try_from(i).ok()),
        Value::Str(s) => s.trim().parse::<CellIndex>().ok(),
        _ => None,
    };
    c.ok_or_else(|| err(format!("{}: {} is not an H3 cell (an INT such as 617700169958293503, or its hex text such as '8928308280fffff')", func.name(), sv(v))))
}

/// `c` as `like` is: an INT for an INT, its hex text for text.
fn same_kind(like: &Value, c: CellIndex) -> Value {
    match like {
        Value::Str(_) => Value::Str(c.to_string()),
        _ => Value::Int(u64::from(c) as i64),
    }
}
