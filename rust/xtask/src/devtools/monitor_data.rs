//! Data for the Monitor app, generated once on the development machine.
//!
//!   cargo xtask gen-monitor-data NE_50M_ADMIN_0_COUNTRIES.geojson DBIP-COUNTRY-LITE-YYYY-MM.csv
//!
//! Generates (no network):
//!   assets/monitor/land.bin             country outlines (Natural Earth 1:50m, public domain),
//!                                       simplified and in hundredths of a degree; axia-server draws
//!                                       them in Rust (apps/monitor/charts.rs)
//!   assets/monitor/geoip-country.mmdb   IP → country (DB-IP Lite Country, CC BY 4.0), in a minimal
//!                                       MaxMind DB format (only {"country": {"iso_code"}})
//!   apps/monitor/countries.rs           [longitude, latitude] of each country's label
//!   assets/locales/{es,en,pt}/countries.json   country names (Natural Earth's NAME_ES/EN/PT)
//!
//! Downloads (by hand, once):
//!   https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson
//!   https://download.db-ip.com/free/dbip-country-lite-YYYY-MM.csv.gz   (decompressed)
//!
//! land.bin format (little-endian): "LORDMAP1", u32 countries; per country: 2 ISO bytes (or "--"),
//! i16 bbox[4] (lon0 lat0 lon1 lat1), u16 rings; per ring: u32 points and i16 (lon, lat) × points, in
//! hundredths of a degree. Inner rings (holes) are stored as regular rings: filled even-odd.

use super::ojson::{self, Json};
use crate::{at, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::process::ExitCode;

const MAX4: u32 = 24; // precision of the countries file for IPv4 (see build_mmdb)
const MAX6: u32 = 32;
const SIMPLIFY: f64 = 0.02; // Douglas-Peucker tolerance, in degrees (~2 km)

fn number(value: Option<&Json>) -> Option<f64> {
    match value {
        Some(Json::Number(text)) => text.parse().ok(),
        _ => None,
    }
}

fn iso_of(props: &Json) -> Option<String> {
    for key in ["ISO_A2_EH", "ISO_A2", "WB_A2"] {
        if let Some(v) = props.get(key).and_then(Json::as_str) {
            if v.chars().count() == 2 && v.chars().all(char::is_alphabetic) && v.to_uppercase() == v {
                return Some(v.to_string());
            }
        }
    }
    None
}

type Ring = Vec<(f64, f64)>;

/// The polygons of a geometry, each a list of rings.
fn polygons(geometry: &Json) -> Vec<Vec<Ring>> {
    let ring = |r: &Json| -> Ring { r.items().iter().map(|p| (number(p.items().first()).unwrap_or(0.0), number(p.items().get(1)).unwrap_or(0.0))).collect() };
    let polygon = |p: &Json| -> Vec<Ring> { p.items().iter().map(ring).collect() };
    let coordinates = geometry.get("coordinates");
    match geometry.get("type").and_then(Json::as_str) {
        Some("Polygon") => coordinates.map(|c| vec![polygon(c)]).unwrap_or_default(),
        Some("MultiPolygon") => coordinates.map(|c| c.items().iter().map(polygon).collect()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Iterative Douglas-Peucker.
pub fn simplify(pts: &[(f64, f64)], eps: f64) -> Vec<(f64, f64)> {
    if pts.len() < 4 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        let ((x0, y0), (x1, y1)) = (pts[i], pts[j]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let n = (dx * dx + dy * dy).powf(0.5);
        let (mut best, mut at) = (0.0f64, 0usize);
        for (k, &(x, y)) in pts.iter().enumerate().take(j).skip(i + 1) {
            let d = if n != 0.0 { (dy * (x - x0) - dx * (y - y0)).abs() / n } else { ((x - x0).powi(2) + (y - y0).powi(2)).powf(0.5) };
            if d > best {
                (best, at) = (d, k);
            }
        }
        if best > eps && at > 0 {
            keep[at] = true;
            stack.push((i, at));
            stack.push((at, j));
        }
    }
    pts.iter().zip(&keep).filter(|(_, &k)| k).map(|(p, _)| *p).collect()
}

/// Python's `round(x)`: halves go to the even neighbour.
fn round_half_even(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// The bytes of land.bin and its (countries, points) counts.
pub fn build_land(geo: &Json) -> (Vec<u8>, usize, usize) {
    let (mut blob, mut count, mut npts) = (Vec::new(), 0usize, 0usize);
    for feat in geo.get("features").map_or(&[][..], Json::items) {
        let props = feat.get("properties").cloned().unwrap_or(Json::Null);
        let cc = iso_of(&props).unwrap_or_else(|| "--".into());
        if cc == "AQ" {
            continue; // Antarctica is left off the map (cut at 56° S)
        }
        let mut rings: Vec<Vec<(i64, i64)>> = Vec::new();
        for poly in polygons(feat.get("geometry").unwrap_or(&Json::Null)) {
            for ring in poly {
                let mut q: Vec<(i64, i64)> = Vec::new();
                for (lon, lat) in simplify(&ring, SIMPLIFY) {
                    let pt = (round_half_even(lon * 100.0), round_half_even(lat * 100.0));
                    if q.last() != Some(&pt) {
                        q.push(pt);
                    }
                }
                if q.len() >= 4 {
                    rings.push(q);
                }
            }
        }
        if rings.is_empty() {
            continue;
        }
        let xs: Vec<i64> = rings.iter().flatten().map(|p| p.0).collect();
        let ys: Vec<i64> = rings.iter().flatten().map(|p| p.1).collect();
        blob.extend_from_slice(&cc.as_bytes()[..2.min(cc.len())]);
        for v in [xs.iter().min(), ys.iter().min(), xs.iter().max(), ys.iter().max()] {
            blob.extend_from_slice(&(*v.unwrap() as i16).to_le_bytes());
        }
        blob.extend_from_slice(&(rings.len() as u16).to_le_bytes());
        for r in &rings {
            blob.extend_from_slice(&(r.len() as u32).to_le_bytes());
            for &(x, y) in r {
                blob.extend_from_slice(&(x as i16).to_le_bytes());
                blob.extend_from_slice(&(y as i16).to_le_bytes());
            }
            npts += r.len();
        }
        count += 1;
    }
    let mut out = b"LORDMAP1".to_vec();
    out.extend_from_slice(&(count as u32).to_le_bytes());
    out.extend_from_slice(&blob);
    (out, count, npts)
}

// ---- IP → country (MaxMind DB) ----

fn mm_str(s: &str) -> Vec<u8> {
    assert!(s.len() < 29);
    let mut out = vec![0x40 | s.len() as u8];
    out.extend_from_slice(s.as_bytes());
    out
}

fn mm_map(n: u8) -> Vec<u8> {
    vec![0xE0 | n]
}

fn mm_uint(v: u64, typ: u8) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let b = &bytes[(v.leading_zeros() / 8) as usize..];
    let mut out = if typ <= 7 { vec![(typ << 5) | b.len() as u8] } else { vec![b.len() as u8, typ - 7] };
    out.extend_from_slice(b);
    out
}

/// The 128-bit tree: `l`/`r` hold a node index (> 0), 0 (empty) or -(country + 1) (a leaf).
struct Tree {
    l: Vec<i64>,
    r: Vec<i64>,
}

impl Tree {
    fn insert(&mut self, net: u128, plen: u32, val: i64) -> Result<()> {
        let mut node = 0usize;
        for d in 0..plen {
            let bit = (net >> (127 - d)) & 1;
            let next = if bit == 1 { self.r[node] } else { self.l[node] };
            if d == plen - 1 {
                if next != 0 {
                    return Err("overlapping ranges".into());
                }
                if bit == 1 { self.r[node] = val } else { self.l[node] = val }
                return Ok(());
            }
            if next < 0 {
                return Err("overlapping ranges".into());
            }
            let next = if next == 0 {
                self.l.push(0);
                self.r.push(0);
                let n = self.l.len() as i64 - 1;
                if bit == 1 { self.r[node] = n } else { self.l[node] = n }
                n
            } else {
                next
            };
            node = next as usize;
        }
        Ok(())
    }
}

/// Parses one CSV row of plain fields (DB-IP has no quoting).
fn csv_row(line: &str) -> Vec<&str> {
    line.trim_end_matches(['\r', '\n']).split(',').map(|f| f.trim_matches('"')).collect()
}

fn ip_value(text: &str) -> Option<(u128, bool)> {
    match text.parse::<IpAddr>().ok()? {
        IpAddr::V4(a) => Some((u128::from(u32::from(a)), true)),
        IpAddr::V6(a) => Some((u128::from(a), false)),
    }
}

/// The mmdb bytes (given a build time) and the (nodes, ranges) counts.
pub fn build_mmdb(csv: &str, epoch: u64) -> Result<(Vec<u8>, usize, usize)> {
    let mut codes: Vec<String> = Vec::new();
    let mut ranges: Vec<(u128, u128, String)> = Vec::new();
    for line in csv.lines().filter(|l| !l.is_empty()) {
        let row = csv_row(line);
        let [a, b, cc] = row[..] else { continue };
        if cc == "ZZ" || cc.len() != 2 {
            continue;
        }
        let (Some((lo, v4)), Some((hi, _))) = (ip_value(a), ip_value(b)) else { return Err(format!("bad address in {line}")) };
        if !v4 && lo >> 32 == 0 {
            continue; // do not overwrite the IPv4 subtree
        }
        ranges.push((lo, hi, cc.to_string()));
        if !codes.iter().any(|c| c == cc) {
            codes.push(cc.to_string());
        }
    }
    ranges.sort();
    let mut merged: Vec<(u128, u128, String)> = Vec::new();
    for (lo, hi, cc) in ranges {
        match merged.last_mut() {
            Some(last) if last.2 == cc && last.1.checked_add(1) == Some(lo) => last.1 = hi,
            _ => merged.push((lo, hi, cc)),
        }
    }
    let code_index: HashMap<&str, usize> = codes.iter().enumerate().map(|(i, c)| (c.as_str(), i)).collect();
    let mut tree = Tree { l: vec![0], r: vec![0] };
    // To keep the file small, prefixes longer than /24 (IPv4) or /MAX6 (IPv6) are not stored: the
    // whole block goes to the country with the most addresses inside it.
    let mut blocks: Vec<((u128, u32), Vec<(i64, u128)>)> = Vec::new();
    for (mut lo, hi, cc) in merged.iter().cloned() {
        let val = -(code_index[cc.as_str()] as i64 + 1);
        loop {
            let mut size = if lo != 0 { lo.trailing_zeros() } else { 128 };
            let span = |s: u32| if s >= 128 { u128::MAX } else { (1u128 << s) - 1 };
            while size > 0 && lo.saturating_add(span(size)) > hi {
                size -= 1;
            }
            let plen = 128 - size;
            let lim = if lo >> 32 == 0 { 96 + MAX4 } else { MAX6 };
            if plen <= lim {
                tree.insert(lo, plen, val)?;
            } else {
                let key = (lo >> (128 - lim), lim);
                let slot = match blocks.iter().position(|(k, _)| *k == key) {
                    Some(i) => i,
                    None => {
                        blocks.push((key, Vec::new()));
                        blocks.len() - 1
                    }
                };
                let counts = &mut blocks[slot].1;
                match counts.iter_mut().find(|(v, _)| *v == val) {
                    Some(c) => c.1 += 1u128 << size,
                    None => counts.push((val, 1u128 << size)),
                }
            }
            match lo.checked_add(span(size)).and_then(|e| e.checked_add(1)) {
                Some(next) if next <= hi => lo = next,
                _ => break,
            }
        }
    }
    for ((net, lim), counts) in &blocks {
        let (mut best, mut best_count) = (counts[0].0, counts[0].1);
        for &(v, c) in &counts[1..] {
            if c > best_count {
                (best, best_count) = (v, c);
            }
        }
        tree.insert(net << (128 - lim), *lim, best)?;
    }
    // nodes that ended up with two equal leaves are folded (bottom-up)
    let mut changed = true;
    while changed {
        changed = false;
        for i in (0..tree.l.len()).rev() {
            for side in 0..2 {
                let c = if side == 0 { tree.l[i] } else { tree.r[i] };
                if c > 0 {
                    let c = c as usize;
                    if tree.l[c] < 0 && tree.l[c] == tree.r[c] {
                        let leaf = tree.l[c];
                        if side == 0 { tree.l[i] = leaf } else { tree.r[i] = leaf }
                        tree.l[c] = 0;
                        tree.r[c] = 0;
                        changed = true;
                    }
                }
            }
        }
    }
    // renumber the reachable nodes in BFS order
    let mut order = vec![0usize];
    let mut index: HashMap<usize, usize> = HashMap::from([(0, 0)]);
    let mut k = 0;
    while k < order.len() {
        let n = order[k];
        k += 1;
        for c in [tree.l[n], tree.r[n]] {
            if c > 0 && !index.contains_key(&(c as usize)) {
                index.insert(c as usize, order.len());
                order.push(c as usize);
            }
        }
    }
    let node_count = order.len();
    let (mut data, mut offsets) = (Vec::new(), Vec::new());
    for cc in &codes {
        offsets.push(data.len());
        data.extend(mm_map(1));
        data.extend(mm_str("country"));
        data.extend(mm_map(1));
        data.extend(mm_str("iso_code"));
        data.extend(mm_str(cc));
    }
    let record = |c: i64| -> usize {
        match c {
            c if c > 0 => index[&(c as usize)],
            0 => node_count,
            c => node_count + 16 + offsets[(-c - 1) as usize],
        }
    };
    let mut db = Vec::new();
    for &n in &order {
        let (l, r) = (record(tree.l[n]), record(tree.r[n]));
        if l >= 1 << 24 || r >= 1 << 24 {
            return Err("record_size 28 is needed".into());
        }
        db.extend_from_slice(&(l as u32).to_be_bytes()[1..]);
        db.extend_from_slice(&(r as u32).to_be_bytes()[1..]);
    }
    let mut meta = mm_map(8);
    for (key, value) in [
        ("node_count", mm_uint(node_count as u64, 6)),
        ("record_size", mm_uint(24, 5)),
        ("ip_version", mm_uint(6, 5)),
        ("database_type", mm_str("DBIP-Country-Lite")),
        ("binary_format_major_version", mm_uint(2, 5)),
        ("binary_format_minor_version", mm_uint(0, 5)),
        ("build_epoch", mm_uint(epoch, 9)),
        ("languages", vec![0x00, 0x04]), // empty array
    ] {
        meta.extend(mm_str(key));
        meta.extend(value);
    }
    db.extend_from_slice(&[0u8; 16]);
    db.extend(data);
    db.extend_from_slice(b"\xab\xcd\xefMaxMind.com");
    db.extend(meta);
    Ok((db, node_count, merged.len()))
}

// ---- countries ----

/// Label positions and names per language, keyed by ISO code.
pub struct Countries {
    pub positions: BTreeMap<String, (f64, f64)>,
    pub names: [(&'static str, BTreeMap<String, String>); 3],
}

pub fn countries(geo: &Json) -> Countries {
    let mut out = Countries { positions: BTreeMap::new(), names: [("es", BTreeMap::new()), ("en", BTreeMap::new()), ("pt", BTreeMap::new())] };
    for feat in geo.get("features").map_or(&[][..], Json::items) {
        let Some(p) = feat.get("properties") else { continue };
        let Some(cc) = iso_of(p) else { continue };
        if out.positions.contains_key(&cc) {
            continue;
        }
        let (Some(lon), Some(lat)) = (number(p.get("LABEL_X")), number(p.get("LABEL_Y"))) else { continue };
        let two = |x: f64| format!("{x:.2}").parse::<f64>().unwrap_or(x);
        out.positions.insert(cc.clone(), (two(lon), two(lat)));
        for (lang, names) in out.names.iter_mut() {
            let key = format!("NAME_{}", lang.to_uppercase());
            let pick = [key.as_str(), "NAME_EN", "NAME"].iter().find_map(|k| p.get(k).and_then(Json::as_str).filter(|s| !s.is_empty()));
            names.insert(cc.clone(), pick.map_or(cc.clone(), String::from));
        }
    }
    out
}

/// Python's `repr(float)`.
fn py_float(x: f64) -> String {
    let text = format!("{x}");
    if text.contains('.') || text.contains("inf") || text.contains("NaN") { text } else { format!("{text}.0") }
}

pub fn positions_source(c: &Countries) -> String {
    let body: Vec<String> = c.positions.iter().map(|(cc, (lon, lat))| format!("\"{cc}\":[{},{}]", py_float(*lon), py_float(*lat))).collect();
    format!("//! Country label positions generated from Natural Earth (public domain).\npub const POSITIONS: &str = r#\"{{{}}}\"#;\n", body.join(","))
}

pub fn names_json(names: &BTreeMap<String, String>) -> String {
    let fields = names.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect();
    format!("{}\n", ojson::pretty(&Json::Object(fields)))
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let [geojson, csv] = args else {
        eprintln!("usage: cargo xtask gen-monitor-data <ne_50m_admin_0_countries.geojson> <dbip-country-lite-YYYY-MM.csv>");
        return Ok(ExitCode::from(1));
    };
    let monitor = repo.join("assets/monitor");
    at(&monitor, fs::create_dir_all(&monitor))?;
    let geo = ojson::parse(&at(Path::new(geojson), fs::read_to_string(geojson))?).map_err(|e| format!("{geojson}: {e}"))?;
    let (land, nc, npts) = build_land(&geo);
    at(&monitor, fs::write(monitor.join("land.bin"), land))?;
    let c = countries(&geo);
    let source = repo.join("apps/monitor/countries.rs");
    at(&source, fs::write(&source, positions_source(&c)))?;
    for (lang, names) in &c.names {
        let file = repo.join(format!("assets/locales/{lang}/countries.json"));
        at(&file, fs::write(&file, names_json(names)))?;
    }
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (db, nodes, nranges) = build_mmdb(&at(Path::new(csv), fs::read_to_string(csv))?, epoch)?;
    at(&monitor, fs::write(monitor.join("geoip-country.mmdb"), db))?;
    println!("outlines: {nc} countries, {npts} points; {} labels; mmdb: {nranges} ranges, {nodes} nodes", c.positions.len());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader of the minimal MaxMind DB this tool writes: the country of an address.
    fn lookup(db: &[u8], ip: &str) -> Option<String> {
        let marker = b"\xab\xcd\xefMaxMind.com";
        let meta_at = db.windows(marker.len()).rposition(|w| w == marker)? + marker.len();
        let meta = &db[meta_at..];
        // node_count is the first metadata field: key, then a uint16/uint32 value
        let key = mm_str("node_count");
        assert_eq!(&meta[1..1 + key.len()], key.as_slice());
        let value = &meta[1 + key.len()..];
        let nodes = value[1..1 + (value[0] & 7) as usize].iter().fold(0usize, |n, &b| n << 8 | b as usize);
        let (addr, _) = ip_value(ip)?;
        let mut node = 0usize;
        for i in (0..128).rev() {
            let at = node * 6 + usize::from((addr >> i) & 1 == 1) * 3;
            let next = db[at..at + 3].iter().fold(0usize, |n, &b| n << 8 | b as usize);
            if next == nodes {
                return None;
            }
            if next > nodes {
                // {"country": {"iso_code": "XX"}}: 1 + 8 + 1 + 9 bytes, then the two-letter string
                let record = &db[nodes * 6 + 16 + (next - nodes - 16)..];
                assert_eq!(record[19], 0x42);
                return Some(String::from_utf8_lossy(&record[20..22]).into_owned());
            }
            node = next;
        }
        None
    }

    #[test]
    fn countries_lookups_follow_the_ranges_and_miss_outside_them() {
        let csv = "1.0.0.0,1.0.0.255,AU\n1.0.1.0,1.0.3.255,CN\n8.8.8.0,8.8.8.255,US\n2001:200::,2001:200:ffff:ffff:ffff:ffff:ffff:ffff,JP\n::1,::ff,XX\n9.9.9.0,9.9.9.255,ZZ\n";
        let (db, nodes, ranges) = build_mmdb(csv, 1_700_000_000).unwrap();
        assert_eq!(ranges, 4);
        assert!(nodes > 1);
        assert_eq!(lookup(&db, "1.0.0.77").as_deref(), Some("AU"));
        assert_eq!(lookup(&db, "1.0.2.9").as_deref(), Some("CN"));
        assert_eq!(lookup(&db, "8.8.8.8").as_deref(), Some("US"));
        assert_eq!(lookup(&db, "2001:200:1234::1").as_deref(), Some("JP"));
        assert_eq!(lookup(&db, "9.9.9.9"), None);
        assert_eq!(lookup(&db, "1.0.4.1"), None);
        assert!(db.windows(8).any(|w| w == b"MaxMind."));
    }

    #[test]
    fn overlapping_ranges_are_refused() {
        assert!(build_mmdb("1.0.0.0,1.0.0.255,AU\n1.0.0.128,1.0.1.0,CN\n", 0).is_err());
    }

    #[test]
    fn the_encoders_follow_the_maxmind_format() {
        assert_eq!(mm_str("ab"), [0x42, b'a', b'b']);
        assert_eq!(mm_map(8), [0xE8]);
        assert_eq!(mm_uint(0, 5), [0xA0]);
        assert_eq!(mm_uint(24, 5), [0xA1, 24]);
        assert_eq!(mm_uint(0x1234, 6), [0xC2, 0x12, 0x34]);
        assert_eq!(mm_uint(1_700_000_000, 9), [4, 2, 0x65, 0x53, 0xF1, 0x00]);
    }

    #[test]
    fn simplify_keeps_the_corners_and_rounding_goes_to_even() {
        let line = [(0.0, 0.0), (1.0, 0.001), (2.0, 0.0), (2.0, 1.0), (2.0, 2.0)];
        assert_eq!(simplify(&line, 0.02), [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0)]);
        assert_eq!(simplify(&line[..3], 0.02).len(), 3);
        assert_eq!((round_half_even(0.5), round_half_even(1.5), round_half_even(2.5), round_half_even(-0.5)), (0, 2, 2, 0));
    }

    #[test]
    fn land_and_country_files_from_a_small_geojson() {
        let geo = ojson::parse(
            r#"{"features":[
              {"properties":{"ISO_A2_EH":"BR","LABEL_X":-53.456,"LABEL_Y":-10.0,"NAME_ES":"Brasil","NAME_EN":"Brazil","NAME_PT":"Brasil"},
               "geometry":{"type":"Polygon","coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]]]}},
              {"properties":{"ISO_A2_EH":"AQ","LABEL_X":1,"LABEL_Y":-80},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]}},
              {"properties":{"ISO_A2_EH":"-99","ISO_A2":"FR","LABEL_X":2.5,"LABEL_Y":46.5,"NAME_EN":"France","NAME":"Fr"},"geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[2,0],[2,2],[0,2],[0,0]]]]}}
            ]}"#,
        )
        .unwrap();
        let (land, count, points) = build_land(&geo);
        assert_eq!((count, points), (2, 10));
        assert_eq!(&land[..8], b"LORDMAP1");
        assert_eq!(u32::from_le_bytes(land[8..12].try_into().unwrap()), 2);
        assert_eq!(&land[12..14], b"BR");
        assert_eq!(i16::from_le_bytes(land[18..20].try_into().unwrap()), 1000);
        let c = countries(&geo);
        assert_eq!(
            positions_source(&c),
            "//! Country label positions generated from Natural Earth (public domain).\npub const POSITIONS: &str = r#\"{\"AQ\":[1.0,-80.0],\"BR\":[-53.46,-10.0],\"FR\":[2.5,46.5]}\"#;\n"
        );
        assert_eq!(c.names[1].1["FR"], "France");
        assert_eq!(c.names[0].1["FR"], "France");
        assert_eq!(c.names[0].1["BR"], "Brasil");
        assert_eq!(names_json(&c.names[1].1), "{\n  \"AQ\": \"AQ\",\n  \"BR\": \"Brazil\",\n  \"FR\": \"France\"\n}\n");
        assert_eq!(py_float(3.0), "3.0");
    }
}
