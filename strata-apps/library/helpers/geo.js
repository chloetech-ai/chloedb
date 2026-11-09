// title: Geo
// description: Places on the map as plain numbers: the distance between two points, the nearest places to one, the box and centre around many, and the map-tile arithmetic a map needs to place pins.
// api: distance(a, b, unit = 'km' | 'mi' | 'm') -> number (a and b are {lat, lng});
//   nearest(point, items, {limit = 5, within, unit = 'km', at = (item) => item}) -> [{item, distance}] closest first;
//   bounds(points) -> {south, west, north, east} or null; center(points) -> {lat, lng} or null;
//   format(km, unit = 'km') -> '850 m', '12.4 km', '3.1 mi'; parse('52.52, 13.405') -> {lat, lng} or null;
//   tile maths (Web Mercator, 256 px tiles): project({lat, lng}, zoom) -> {x, y} in pixels of the whole world;
//   unproject({x, y}, zoom) -> {lat, lng}; fit(bounds, width, height, {maxZoom = 17, padding = 40}) -> {center, zoom}
function helper() {
  const R = 6371.0088;
  const rad = (d) => (d * Math.PI) / 180;
  const deg = (r) => (r * 180) / Math.PI;
  const ok = (p) => p && Number.isFinite(Number(p.lat)) && Number.isFinite(Number(p.lng));
  const per = { km: 1, m: 1000, mi: 0.621371 };
  function distance(a, b, unit = 'km') {
    if (!ok(a) || !ok(b)) return null;
    const dLat = rad(b.lat - a.lat);
    const dLng = rad(b.lng - a.lng);
    const s = Math.sin(dLat / 2) ** 2 + Math.cos(rad(a.lat)) * Math.cos(rad(b.lat)) * Math.sin(dLng / 2) ** 2;
    return 2 * R * Math.asin(Math.min(1, Math.sqrt(s))) * (per[unit] || 1);
  }
  function nearest(point, items, { limit = 5, within = null, unit = 'km', at = (x) => x } = {}) {
    return (items || []).map((item) => ({ item, distance: distance(point, at(item), unit) }))
      .filter((r) => r.distance !== null && (within === null || r.distance <= within))
      .sort((a, b) => a.distance - b.distance).slice(0, limit);
  }
  function bounds(points) {
    const ps = (points || []).filter(ok);
    if (!ps.length) return null;
    return { south: Math.min(...ps.map((p) => +p.lat)), north: Math.max(...ps.map((p) => +p.lat)), west: Math.min(...ps.map((p) => +p.lng)), east: Math.max(...ps.map((p) => +p.lng)) };
  }
  function center(points) {
    const b = bounds(points);
    return b ? { lat: (b.south + b.north) / 2, lng: (b.west + b.east) / 2 } : null;
  }
  function format(km, unit = 'km') {
    if (km === null || km === undefined || !Number.isFinite(Number(km))) return '';
    if (unit === 'mi') { const mi = km * per.mi; return `${mi < 10 ? mi.toFixed(1) : Math.round(mi)} mi`; }
    if (km < 1) return `${Math.round(km * 1000)} m`;
    return `${km < 10 ? km.toFixed(1) : Math.round(km)} km`;
  }
  function parse(text) {
    const m = String(text || '').match(/(-?\d+(?:\.\d+)?)\s*[,; ]\s*(-?\d+(?:\.\d+)?)/);
    if (!m) return null;
    const lat = Number(m[1]); const lng = Number(m[2]);
    return Math.abs(lat) <= 90 && Math.abs(lng) <= 180 ? { lat, lng } : null;
  }
  const SIZE = 256;
  function project(p, zoom) {
    const scale = SIZE * 2 ** zoom;
    const lat = Math.max(-85.05112878, Math.min(85.05112878, Number(p.lat)));
    const s = Math.sin(rad(lat));
    return { x: ((Number(p.lng) + 180) / 360) * scale, y: (0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI)) * scale };
  }
  function unproject(pt, zoom) {
    const scale = SIZE * 2 ** zoom;
    const lng = (pt.x / scale) * 360 - 180;
    const n = Math.PI - (2 * Math.PI * pt.y) / scale;
    return { lat: deg(Math.atan(Math.sinh(n))), lng };
  }
  function fit(b, width, height, { maxZoom = 17, padding = 40 } = {}) {
    if (!b) return { center: { lat: 20, lng: 0 }, zoom: 1 };
    const c = { lat: (b.south + b.north) / 2, lng: (b.west + b.east) / 2 };
    for (let z = maxZoom; z > 0; z--) {
      const a = project({ lat: b.north, lng: b.west }, z);
      const d = project({ lat: b.south, lng: b.east }, z);
      if (Math.abs(d.x - a.x) <= Math.max(1, width - 2 * padding) && Math.abs(d.y - a.y) <= Math.max(1, height - 2 * padding)) return { center: c, zoom: z };
    }
    return { center: c, zoom: 1 };
  }
  return { distance, nearest, bounds, center, format, parse, project, unproject, fit };
}
