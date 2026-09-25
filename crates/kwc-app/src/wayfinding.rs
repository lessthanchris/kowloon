//! Wayfinding: the city helping you learn it. Signposts at lane junctions
//! and gate openings point the way (and the distance, walking) to the big
//! landmarks: the South Gate, the Yamen, the Big Well, the temples. They
//! stand in every era, so the 1987 maze keeps the bearings you learned in
//! 1950, and they're part of the city, not the HUD, so they count in Memory
//! races. Stair landings get their floor painted on the wall.

use crate::world::{Aabb, S};
use glam::Vec3;
use kwc_sim::*;
use std::collections::VecDeque;

const C: f32 = CELL_M;

/// A signpost: its board (on a wall, proud of it), where its text goes,
/// which way the text reads, and the lines.
pub struct Signpost {
    pub board: Aabb,
    pub centre: Vec3,
    pub right: Vec3,
    pub lines: Vec<String>,
    /// Gate boards are bigger.
    pub gate: bool,
}

/// A stop on the guided tour: what it is, a word about it, where to stand,
/// and which way to look.
pub struct Stop {
    pub name: String,
    pub about: String,
    pub stand: Vec3,
    pub facing: Vec3,
}

fn about(name: &str) -> String {
    let s = if name.contains("South Gate") {
        "The South Gate. The old fort's walls came down during the war, but this stayed the way in, off Tung Tau Tsuen Road."
    } else if name.contains("Yamen") {
        "The Yamen: the old magistrate's offices from the Qing garrison, and the one place that was never built over. Over the years it was an almshouse, a school and a clinic."
    } else if name.contains("Well") {
        "The Big Well. Before water was piped in there were only a handful of standpipes for the whole city, and people queued with buckets."
    } else if name.contains("Tin Hau") {
        "Tin Hau Temple, for the goddess of the sea, patron of the fishing families who brought her here."
    } else if name.contains("Fuk Tak") {
        "Fuk Tak Temple, for the earth god who watches over a neighbourhood."
    } else {
        "A landmark worth knowing."
    };
    s.to_string()
}

/// The guided tour: every landmark, nearest first from `start`, each with
/// somewhere to stand in the lane beside it.
pub fn tour(city: &City, year: u16, start: Vec3) -> Vec<Stop> {
    let mut stops: Vec<Stop> = landmarks(city)
        .into_iter()
        .filter_map(|(name, cells)| {
            let at = |c: Cell| Vec3::new((c.0 as f32 + 0.5) * C, 0.0, (c.1 as f32 + 0.5) * C);
            // Stand in the open cell closest to the landmark's middle, facing it.
            let mid = anchor(city, &name).unwrap_or_else(|| cells.iter().map(|&c| at(c)).sum::<Vec3>() / cells.len() as f32);
            let c = *cells.iter().filter(|&&c| open(city, year, c)).min_by(|a, b| at(**a).distance(mid).total_cmp(&at(**b).distance(mid)))?;
            let facing = Vec3::new(mid.x - at(c).x, 0.0, mid.z - at(c).z).normalize_or(Vec3::X);
            Some(Stop { about: about(&name), name, stand: at(c), facing })
        })
        .collect();
    // Nearest first, then nearest to the last one.
    let mut out = vec![];
    let mut here = start;
    while !stops.is_empty() {
        let k = (0..stops.len()).min_by(|&a, &b| stops[a].stand.distance(here).total_cmp(&stops[b].stand.distance(here))).unwrap();
        let s = stops.swap_remove(k);
        here = s.stand;
        out.push(s);
    }
    out
}

/// Where a landmark itself is (not the lane beside it).
fn anchor(city: &City, name: &str) -> Option<Vec3> {
    let at = |c: Cell| Vec3::new((c.0 as f32 + 0.5) * C, 0.0, (c.1 as f32 + 0.5) * C);
    if name == "South Gate" {
        return Some(at(city.south_gate));
    }
    if name == "Yamen" {
        let cells: Vec<Cell> = (0..city.d as u16).flat_map(|j| (0..city.w as u16).map(move |i| (i, j))).filter(|&c| city.ground_at(c) == Ground::Yamen).collect();
        return (!cells.is_empty()).then(|| cells.iter().map(|&c| at(c)).sum::<Vec3>() / cells.len() as f32);
    }
    city.features.iter().find(|f| f.name.as_deref() == Some(name)).map(|f| at(f.cell))
}

/// Places you'd give directions by, and the open ground right at them.
fn landmarks(city: &City) -> Vec<(String, Vec<Cell>)> {
    let mut out = vec![];
    let open_near = |cells: &[Cell]| -> Vec<Cell> {
        let mut v: Vec<Cell> = cells.iter().flat_map(|&c| std::iter::once(c).chain(city.neighbours(c).map(|(_, n)| n))).filter(|&n| city.ground_at(n) == Ground::Alley).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    out.push(("South Gate".to_string(), open_near(&[city.south_gate])));
    let yamen: Vec<Cell> = (0..city.d as u16).flat_map(|j| (0..city.w as u16).map(move |i| (i, j))).filter(|&c| city.ground_at(c) == Ground::Yamen).collect();
    out.push(("Yamen".to_string(), open_near(&yamen)));
    for f in &city.features {
        let name = match f.kind {
            FeatureKind::NaturalWell | FeatureKind::Temple => f.name.clone().unwrap_or_else(|| "Temple".into()),
            _ => continue,
        };
        out.push((name, open_near(&[f.cell])));
    }
    out.retain(|(_, cells)| !cells.is_empty());
    out
}

/// Open ground at street level in `year`: lanes, wells, plots not yet built on.
fn open(city: &City, year: u16, c: Cell) -> bool {
    match city.ground_at(c) {
        Ground::Alley | Ground::Well => true,
        Ground::Plot => city.height_at(c, year) == 0,
        _ => false,
    }
}

/// Steps from every open cell to the nearest of `from`.
fn distances(city: &City, year: u16, from: &[Cell]) -> Vec<u32> {
    let mut d = vec![u32::MAX; city.w * city.d];
    let mut q = VecDeque::new();
    for &c in from {
        d[city.idx(c)] = 0;
        q.push_back(c);
    }
    while let Some(c) = q.pop_front() {
        let k = d[city.idx(c)];
        for (_, n) in city.neighbours(c) {
            if open(city, year, n) && d[city.idx(n)] == u32::MAX {
                d[city.idx(n)] = k + 1;
                q.push_back(n);
            }
        }
    }
    d
}

pub fn signposts(city: &City, year: u16) -> Vec<Signpost> {
    let marks: Vec<(String, Vec<u32>)> = landmarks(city).into_iter().map(|(name, cells)| (name, distances(city, year, &cells))).collect();
    let built = |c: Cell| city.ground_at(c) == Ground::Plot && city.height_at(c, year) > 0;
    // Candidates: gate openings first, then junctions (three or more ways on).
    let mut cands: Vec<(Cell, bool)> = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) != Ground::Alley {
                continue;
            }
            let gate = Dir::ALL.iter().any(|&d| city.step(c, d).is_none_or(|n| city.ground_at(n) == Ground::Outside));
            let ways = city.neighbours(c).filter(|&(_, n)| open(city, year, n)).count();
            if gate || ways >= 3 {
                cands.push((c, gate));
            }
        }
    }
    cands.sort_by_key(|&(_, gate)| !gate);
    let mut placed: Vec<Cell> = vec![];
    let mut out = vec![];
    for (c, gate) in cands {
        // Not too many: one every few cells.
        let spacing = if gate { 3 } else { 5 };
        if placed.iter().any(|p| (p.0 as i32 - c.0 as i32).abs() + (p.1 as i32 - c.1 as i32).abs() < spacing) {
            continue;
        }
        // On a wall of a standing building (the last one round, to keep
        // clear of the street plaque, which takes the first).
        let Some(wall) = Dir::ALL.into_iter().rev().find(|&d| city.step(c, d).is_some_and(built)) else { continue };
        let (dx, dz) = wall.delta();
        let dvec = Vec3::new(dx as f32, 0.0, dz as f32);
        let normal = -dvec;
        let right = dvec.cross(Vec3::Y);
        let k = city.idx(c);
        let mut ways: Vec<(u32, String)> = vec![];
        for (name, dist) in &marks {
            let here = dist[k];
            if here == 0 || here == u32::MAX || here > 160 {
                continue;
            }
            // The first step towards it.
            let Some((_, n)) = city.neighbours(c).filter(|&(_, n)| dist[city.idx(n)] < here).min_by_key(|&(_, n)| dist[city.idx(n)]) else { continue };
            let v = Vec3::new(n.0 as f32 - c.0 as f32, 0.0, n.1 as f32 - c.1 as f32);
            let metres = ((here as f32 * C / 10.0).round() * 10.0).max(10.0);
            let line = if v.dot(right) > 0.5 {
                format!("{name} » {metres:.0} m")
            } else if v.dot(right) < -0.5 {
                format!("« {name} {metres:.0} m")
            } else {
                format!("{name} (behind) {metres:.0} m")
            };
            ways.push((here, line));
        }
        if ways.is_empty() {
            continue;
        }
        ways.sort();
        // A one-storey wall only has room for two lines.
        let low = city.step(c, wall).is_some_and(|n| city.height_at(n, year) <= 1);
        let n_lines = if low { 2 } else { 3 };
        let mut lines: Vec<String> = ways.into_iter().take(n_lines).map(|(_, l)| l).collect();
        if gate && !low {
            lines.insert(0, "KOWLOON WALLED CITY".to_string());
            lines.truncate(3);
        }
        let line_h = 0.11;
        let (w, h) = (1.3, lines.len() as f32 * line_h * 1.25 + 0.08);
        let face = Vec3::new((c.0 as f32 + 0.5) * C, 0.0, (c.1 as f32 + 0.5) * C) + dvec * (C * 0.5);
        let y0 = 2.45;
        let a = face + Vec3::Y * y0 - right * (w / 2.0);
        let b = face + Vec3::Y * (y0 + h) + right * (w / 2.0) + normal * 0.03;
        let board = Aabb::new(a.min(b), a.max(b));
        out.push(Signpost { board, centre: face + normal * 0.035 + Vec3::Y * (y0 + h / 2.0), right, lines, gate });
        placed.push(c);
    }
    out
}

/// Floor numbers on each stair landing: (text centre, reading direction, text).
pub fn floor_marks(city: &City, year: u16) -> Vec<(Vec3, Vec3, String)> {
    let mut out = vec![];
    for plot in city.plots.iter().filter(|p| p.height_at(year) > 0) {
        let land = plot.core[0];
        let pid = plot.id;
        for f in 0..plot.height_at(year) {
            // A plain wall of the landing: not the stair, not a corridor, not the street door.
            let wall = Dir::ALL.into_iter().find(|&d| {
                city.step(land, d).is_some_and(|n| !plot.core.contains(&n) && !city.circ_at(n, pid, f) && (f > 0 || city.ground_at(n) != Ground::Alley) && city.ground_at(n) == Ground::Plot)
            });
            let Some(wall) = wall else { continue };
            let (dx, dz) = wall.delta();
            let dvec = Vec3::new(dx as f32, 0.0, dz as f32);
            let face = Vec3::new((land.0 as f32 + 0.5) * C, 0.0, (land.1 as f32 + 0.5) * C) + dvec * (C * 0.5 - crate::world::WALL - 0.012);
            let text = if f == 0 { "G/F".to_string() } else { format!("{f}/F") };
            out.push((face + Vec3::Y * (f as f32 * S + 1.75), dvec.cross(Vec3::Y), text));
        }
    }
    out
}
