//! The places in memory: where each is and what lies on it, read from
//! `ea_places` when the app starts (about 100,000 rows, a few megabytes),
//! so that a fire pixel finds the place it fell on, and a fire the people
//! around it, without a query each.

use std::collections::HashMap;

use serde_json::json;

use crate::strata::{RowExt, Strata};

#[derive(Clone, Debug, Default)]
pub struct Place {
    pub id: String,
    pub region: String,
    pub lat: f64,
    pub lon: f64,
    pub people: f64,
    pub trees: f64,
    pub shrubs: f64,
    pub grass: f64,
    pub built: f64,
    pub slope: f64,
    pub elev: f64,
    pub climate: String,
    pub town: String,
    pub adm1: String,
    pub adm2: String,
    pub fire_years: Vec<i64>,
    pub crops: f64,
    pub bare: f64,
    pub water: f64,
    pub snow: f64,
    pub mine: f64,
    pub kind: String,
    pub crop: String,
}

/// Places in cells of a tenth of a degree.
#[derive(Default)]
pub struct Places {
    pub all: Vec<Place>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const CELL: f64 = 0.1;

fn key(lat: f64, lon: f64) -> (i32, i32) {
    ((lat / CELL).floor() as i32, (lon / CELL).floor() as i32)
}

/// Overlap of Major TOM's fragments: 384 pixels a side, one every 342, so
/// a square counts about 1.26 times when places are summed.
pub const OVERLAP: f64 = (384.0 * 384.0) / (342.0 * 342.0);

impl Places {
    pub fn new(all: Vec<Place>) -> Places {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, p) in all.iter().enumerate() {
            grid.entry(key(p.lat, p.lon)).or_default().push(i as u32);
        }
        Places { all, grid }
    }

    pub fn len(&self) -> usize {
        self.all.len()
    }

    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    fn around(&self, lat: f64, lon: f64, km: f64) -> impl Iterator<Item = &Place> + '_ {
        let dlat = (km / 111.32 / CELL).ceil() as i32 + 1;
        let dlon = (km / (111.32 * lat.to_radians().cos().max(0.2)) / CELL).ceil() as i32 + 1;
        let (r, c) = key(lat, lon);
        (-dlat..=dlat)
            .flat_map(move |dr| (-dlon..=dlon).map(move |dc| (r + dr, c + dc)))
            .filter_map(|k| self.grid.get(&k))
            .flatten()
            .map(|&i| &self.all[i as usize])
    }

    /// The place nearest to a point, within `km`.
    pub fn nearest(&self, lat: f64, lon: f64, km: f64) -> Option<&Place> {
        self.around(lat, lon, km)
            .map(|p| (crate::fires::km(lat, lon, p.lat, p.lon), p))
            .filter(|(d, _)| *d <= km)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|x| x.1)
    }

    /// Places for a search box: "34.05, -118.2" is the place under that
    /// point; words are towns and counties, those starting with them
    /// first, then those containing them, the biggest first. One place a
    /// town (the most people).
    pub fn find(&self, q: &str, limit: usize) -> Vec<(String, &Place)> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let nums: Vec<f64> = q
            .split([',', ' ', ';'])
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse().ok())
            .collect();
        if nums.len() == 2
            && q.chars()
                .all(|c| c.is_ascii_digit() || " ,;.-+".contains(c))
        {
            return self
                .nearest(nums[0], nums[1], 5.0)
                .map(|p| (format!("{:.3}, {:.3}", nums[0], nums[1]), p))
                .into_iter()
                .collect();
        }
        let mut best: HashMap<(String, String), (u8, &Place)> = HashMap::new();
        for p in &self.all {
            let (town, county) = (p.town.to_lowercase(), p.adm2.to_lowercase());
            let rank = if !town.is_empty() && town.starts_with(&q) {
                0
            } else if !county.is_empty() && county.starts_with(&q) {
                1
            } else if !town.is_empty() && town.contains(&q) {
                2
            } else {
                continue;
            };
            let name = if rank == 1 {
                p.adm2.clone()
            } else {
                p.town.clone()
            };
            let e = best.entry((name, p.adm1.clone())).or_insert((rank, p));
            if p.people > e.1.people {
                *e = (rank, p);
            }
        }
        let mut v: Vec<_> = best
            .into_iter()
            .map(|((name, adm1), (rank, p))| (rank, name, adm1, p))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0).then(b.3.people.total_cmp(&a.3.people)));
        v.into_iter()
            .take(limit)
            .map(|(_, name, adm1, p)| {
                (
                    if adm1.is_empty() {
                        name
                    } else {
                        format!("{name}, {adm1}")
                    },
                    p,
                )
            })
            .collect()
    }

    /// People living within `km` of a point: the places whose centres are
    /// within it, their overlap taken out.
    pub fn people_within(&self, lat: f64, lon: f64, km: f64) -> f64 {
        self.around(lat, lon, km)
            .filter(|p| crate::fires::km(lat, lon, p.lat, p.lon) <= km)
            .map(|p| p.people)
            .sum::<f64>()
            / OVERLAP
    }

    /// Reads every place (not their vectors), 10,000 rows a query.
    pub async fn read(db: &Strata) -> crate::strata::Result<Places> {
        let all = db
            .scroll_map(
                "SELECT id, region, lat, lon, people, trees, shrubs, grass, built, slope, elev, climate, town, adm1, adm2, fire_years, crops, bare, water, snow, mine, kind, crop FROM ea_places",
                json!({}),
                10000,
                |r| {
                    Some(Place {
                id: r.s("id"),
                region: r.s("region"),
                lat: r.f("lat"),
                lon: r.f("lon"),
                people: r.f("people"),
                trees: r.f("trees"),
                shrubs: r.f("shrubs"),
                grass: r.f("grass"),
                built: r.f("built"),
                slope: r.f("slope"),
                elev: r.f("elev"),
                climate: r.s("climate"),
                town: r.s("town"),
                adm1: r.s("adm1"),
                adm2: r.s("adm2"),
                fire_years: r.list("fire_years").iter().filter_map(|y| y.parse().ok()).collect(),
                crops: r.f("crops"),
                bare: r.f("bare"),
                water: r.f("water"),
                snow: r.f("snow"),
                mine: r.f("mine"),
                kind: r.s("kind"),
                crop: r.s("crop"),
                    })
                },
            )
            .await?;
        Ok(Places::new(all))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_place_and_the_people_around() {
        let p = Places::new(vec![
            Place {
                id: "a".into(),
                lat: 34.0,
                lon: -118.0,
                people: 1260.0,
                ..Default::default()
            },
            Place {
                id: "b".into(),
                lat: 34.03,
                lon: -118.0,
                people: 2520.0,
                ..Default::default()
            },
            Place {
                id: "c".into(),
                lat: 35.0,
                lon: -118.0,
                people: 9999.0,
                ..Default::default()
            },
        ]);
        assert_eq!(
            p.nearest(34.01, -118.0, 3.0).map(|x| x.id.as_str()),
            Some("a")
        );
        assert!(p.nearest(34.5, -118.0, 3.0).is_none());
        assert_eq!(p.find("34.01, -118.0", 5).len(), 1);
        // a and b are within 5 km; c is 110 km away.
        assert!((p.people_within(34.0, -118.0, 5.0) - 3780.0 / OVERLAP).abs() < 1e-6);
    }
}
