//! Light: where the lamps are, and light spill baked into vertices.
//!
//! Every light floods outward only through open space (corridor to corridor,
//! landing to stair, lane to doorway, across bridges and passages), so it never
//! leaks through a wall. Each vertex then sums the lights that reach its space.

use crate::citymesh::{hash, srgb};
use crate::world::{Aabb, S};
use glam::Vec3;
use kwc_engine::mesh::MeshData;
use kwc_sim::*;
use std::collections::{HashMap, VecDeque};

const C: f32 = CELL_M;
/// Level used for spaces open top to bottom: lanes, stairwells, roofs.
pub const OPEN: u8 = 255;
pub type Space = (Cell, u8);

pub struct PointLight {
    pub pos: Vec3,
    pub col: Vec3,
    pub range: f32,
}

/// A visible lamp or sign: geometry plus the light it gives.
pub struct Fixture {
    pub aabb: Aabb,
    pub col: [f32; 3],
    /// Emission; negative = a painted sign (keeps its colour by day).
    pub emit: f32,
}

fn lin(c: [f32; 3]) -> Vec3 {
    Vec3::from(c)
}

/// Is there a fluorescent tube on this storey of a corridor or landing, and is it working?
pub fn tube(c: Cell, f: i32, landing: bool) -> Option<bool> {
    (landing || hash(c.0 as u32, c.1 as u32, f as u32) < 0.3).then(|| hash(c.0 as u32 + 7, c.1 as u32, f as u32) < 0.75)
}

pub fn tube_box(c: Cell, f: i32) -> Aabb {
    let (x0, z0) = (c.0 as f32 * C, c.1 as f32 * C);
    let y = f as f32 * S + S - crate::world::SLAB;
    Aabb::new(Vec3::new(x0 + 0.3, y - 0.06, z0 + 0.7), Vec3::new(x0 + C - 0.3, y - 0.02, z0 + 0.8))
}

pub const TUBE_COL: [f32; 3] = [0.62, 1.0, 0.74];
pub const BULB_COL: [f32; 3] = [1.0, 0.55, 0.25];

/// The bulb over a stair's turning landing on floor `f`, if it works.
pub fn stair_bulb(plot: &Plot, f: i32) -> Option<Aabb> {
    let w = crate::world::Well::of(plot);
    let p = w.point(2.9, 1.5);
    let y = f as f32 * S + S / 2.0 + 2.1;
    (hash(plot.id, f as u32, 77) < 0.8).then(|| Aabb::new(Vec3::new(p.x - 0.08, y, p.z - 0.08), Vec3::new(p.x + 0.08, y + 0.14, p.z + 0.08)))
}

const SIGN_COLS: &[[u8; 3]] = &[[255, 60, 70], [255, 90, 180], [80, 255, 140], [70, 220, 255], [255, 190, 60], [240, 240, 255]];

/// Each main lane and its side alleys share a colour: their lamps and most of
/// their signs, so "the red alleys" come to mean somewhere.
const FAMILY: &[[u8; 3]] = &[
    [255, 70, 60],   // red
    [255, 180, 60],  // amber
    [90, 255, 150],  // green
    [80, 210, 255],  // cyan
    [255, 100, 200], // pink
    [190, 130, 255], // violet
    [255, 240, 200], // white
    [255, 140, 40],  // orange
    [60, 255, 230],  // teal
];

const FRAME: [f32; 3] = [0.018, 0.016, 0.014];

/// A vertical sign sticking out from the wall over the lane: a dark board with a
/// column of glowing characters, like the shop signs of the period.
fn vertical_sign(out: &mut Vec<Fixture>, c: Cell, d: Dir, y: f32, col: [f32; 3], lit: bool, glyphs: i32) {
    let (a0, a1) = (0.72, 0.78); // thin board, perpendicular to the wall
    let depth = 0.75;
    let gh = 0.34;
    let h = glyphs as f32 * (gh + 0.08) + 0.12;
    // Bracket and board.
    out.push(Fixture { aabb: on_wall(c, d, a0 + 0.01, a1 - 0.01, y + h - 0.05, y + h, depth + 0.1), col: FRAME, emit: 0.0 });
    out.push(Fixture { aabb: on_wall(c, d, a0, a1, y, y + h, depth).shrink_from_wall(d, 0.1), col: FRAME, emit: 0.0 });
    // Characters, each a little proud of the board on both faces.
    for k in 0..glyphs {
        let gy = y + 0.1 + k as f32 * (gh + 0.08);
        let b = on_wall(c, d, a0 - 0.012, a1 + 0.012, gy, gy + gh, depth - 0.1).shrink_from_wall(d, 0.2);
        out.push(Fixture { aabb: b, col, emit: if lit { -0.9 } else { -0.04 } });
    }
}

/// A signboard flat on the outer wall: dark board, a row of glowing characters.
fn board_sign(out: &mut Vec<Fixture>, c: Cell, d: Dir, y: f32, col: [f32; 3], lit: bool, glyphs: i32) {
    out.push(Fixture { aabb: on_wall(c, d, 0.05, 1.45, y, y + 0.8, 0.06), col: FRAME, emit: 0.0 });
    let gw = 1.3 / glyphs as f32;
    for k in 0..glyphs {
        let a = 0.1 + k as f32 * gw;
        out.push(Fixture { aabb: on_wall(c, d, a + 0.04, a + gw - 0.04, y + 0.12, y + 0.68, 0.075), col, emit: if lit { -0.8 } else { -0.04 } });
    }
}

trait FromWall {
    fn shrink_from_wall(self, d: Dir, amount: f32) -> Self;
}

impl FromWall for Aabb {
    /// Pull the wall-side face of a sticking-out box away from the wall.
    fn shrink_from_wall(mut self, d: Dir, amount: f32) -> Aabb {
        match d {
            Dir::N => self.max.z -= amount,
            Dir::S => self.min.z += amount,
            Dir::E => self.min.x += amount,
            Dir::W => self.max.x -= amount,
        }
        self
    }
}

/// Which main lane a lane cell belongs to (index into `city.lanes`).
pub fn family(city: &City, dir: &address::Directory, c: Cell) -> Option<usize> {
    let name = dir.lane_at(city, c)?;
    city.lanes.iter().position(|l| name.starts_with(l.name.as_str()))
}

pub fn family_col(k: usize) -> [f32; 3] {
    let c = FAMILY[k % FAMILY.len()];
    srgb(c[0], c[1], c[2])
}

/// Street plaques: where one lane meets another, a blue enamel plaque on the
/// wall of each. Returns (plaque, lane index in the directory, facing out).
pub fn plaques(city: &City, dir: &address::Directory, year: u16) -> Vec<(Aabb, u16, Vec3)> {
    let mut out = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let Some(me) = dir.lane_of[city.idx(c)] else { continue };
            let junction = city.neighbours(c).any(|(_, n)| dir.lane_of[city.idx(n)].is_some_and(|o| o != me));
            if !junction {
                continue;
            }
            // On the first wall of this cell.
            if let Some((d, _)) = city.neighbours(c).find(|(_, n)| city.ground_at(*n) == Ground::Plot && city.height_at(*n, year) > 0) {
                let (dx, dz) = d.delta();
                let out_dir = -Vec3::new(dx as f32, 0.0, dz as f32);
                // Flat against the neighbour's wall, just proud of it.
                out.push((plaque_box(c, d), me, out_dir));
            }
        }
    }
    out
}

fn face_of(c: Cell, d: Dir) -> Vec3 {
    let (dx, dz) = d.delta();
    Vec3::new((c.0 as f32 + 0.5 + dx as f32 * 0.5) * C, 0.0, (c.1 as f32 + 0.5 + dz as f32 * 0.5) * C)
}

/// A plaque on the wall at face `d` of lane cell `c` (sticking 3 cm into the lane).
fn plaque_box(c: Cell, d: Dir) -> Aabb {
    let f = face_of(c, d);
    let (dx, dz) = d.delta();
    let (y0, y1) = (2.05, 2.4);
    if dx != 0 {
        let x = f.x - dx as f32 * 0.03;
        Aabb::new(Vec3::new(x.min(f.x), y0, f.z - 0.35), Vec3::new(x.max(f.x), y1, f.z + 0.35))
    } else {
        let z = f.z - dz as f32 * 0.03;
        Aabb::new(Vec3::new(f.x - 0.35, y0, z.min(f.z)), Vec3::new(f.x + 0.35, y1, z.max(f.z)))
    }
}

/// Visible landmarks: the South Gate arch, standpipes with their bucket queues,
/// the Big Well. (Temples are dressed with their doors.)
pub fn landmarks(city: &City) -> Vec<Fixture> {
    let mut out = vec![];
    let stone = srgb(150, 146, 136);
    for f in &city.features {
        let (x0, z0) = (f.cell.0 as f32 * C, f.cell.1 as f32 * C);
        let (cx, cz) = (x0 + C / 2.0, z0 + C / 2.0);
        match f.kind {
            FeatureKind::SouthGate => {
                for (px, pz) in [(x0, z0), (x0 + C - 0.3, z0), (x0, z0 + C - 0.3), (x0 + C - 0.3, z0 + C - 0.3)] {
                    out.push(Fixture { aabb: Aabb::new(Vec3::new(px, 0.0, pz), Vec3::new(px + 0.3, 3.3, pz + 0.3)), col: stone, emit: 0.0 });
                }
                out.push(Fixture { aabb: Aabb::new(Vec3::new(x0 - 0.2, 3.3, z0 - 0.2), Vec3::new(x0 + C + 0.2, 3.8, z0 + C + 0.2)), col: stone, emit: 0.0 });
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - 0.5, 3.35, z0 - 0.25), Vec3::new(cx + 0.5, 3.75, z0 + C + 0.25)), col: srgb(120, 30, 25), emit: -0.1 });
            }
            FeatureKind::WaterStandpipe => {
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - 0.05, 0.0, cz - 0.05), Vec3::new(cx + 0.05, 1.1, cz + 0.05)), col: srgb(70, 90, 80), emit: 0.0 });
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - 0.05, 0.95, cz - 0.05), Vec3::new(cx + 0.3, 1.02, cz + 0.05)), col: srgb(70, 90, 80), emit: 0.0 });
                // The queue of buckets and cans.
                let cols = [srgb(190, 40, 30), srgb(40, 80, 170), srgb(160, 160, 150), srgb(200, 170, 40)];
                for k in 0..6 {
                    let t = k as f32 * 0.32;
                    let (bx, bz) = (x0 + 0.15 + (t % (C - 0.3)), z0 + 0.1 + (k % 2) as f32 * (C - 0.5));
                    out.push(Fixture { aabb: Aabb::new(Vec3::new(bx, 0.0, bz), Vec3::new(bx + 0.28, 0.32, bz + 0.28)), col: cols[k % 4], emit: 0.0 });
                }
            }
            FeatureKind::NaturalWell => {
                let (r, h) = (0.6, 0.7);
                for (a0, a1, b0, b1) in [(-r, r, -r, -r + 0.2), (-r, r, r - 0.2, r), (-r, -r + 0.2, -r, r), (r - 0.2, r, -r, r)] {
                    out.push(Fixture { aabb: Aabb::new(Vec3::new(cx + a0, 0.0, cz + b0), Vec3::new(cx + a1, h, cz + b1)), col: stone, emit: 0.0 });
                }
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - r + 0.2, 0.3, cz - r + 0.2), Vec3::new(cx + r - 0.2, 0.32, cz + r - 0.2)), col: srgb(20, 40, 50), emit: 0.0 });
                // Rope and pulley frame.
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - r, h, cz - 0.04), Vec3::new(cx - r + 0.08, 2.0, cz + 0.04)), col: srgb(110, 80, 50), emit: 0.0 });
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx + r - 0.08, h, cz - 0.04), Vec3::new(cx + r, 2.0, cz + 0.04)), col: srgb(110, 80, 50), emit: 0.0 });
                out.push(Fixture { aabb: Aabb::new(Vec3::new(cx - r, 1.95, cz - 0.04), Vec3::new(cx + r, 2.03, cz + 0.04)), col: srgb(110, 80, 50), emit: 0.0 });
            }
            _ => {}
        }
    }
    out
}

fn centre(c: Cell) -> (f32, f32) {
    ((c.0 as f32 + 0.5) * C, (c.1 as f32 + 0.5) * C)
}

/// Wall of cell `c` facing `d`, as a thin box sticking out `out` metres from it.
fn on_wall(c: Cell, d: Dir, a0: f32, a1: f32, y0: f32, y1: f32, out: f32) -> Aabb {
    let (x0, z0) = (c.0 as f32 * C, c.1 as f32 * C);
    let (x1, z1) = (x0 + C, z0 + C);
    match d {
        Dir::N => Aabb::new(Vec3::new(x0 + a0, y0, z0 - out), Vec3::new(x0 + a1, y1, z0)),
        Dir::S => Aabb::new(Vec3::new(x0 + a0, y0, z1), Vec3::new(x0 + a1, y1, z1 + out)),
        Dir::E => Aabb::new(Vec3::new(x1, y0, z0 + a0), Vec3::new(x1 + out, y1, z0 + a1)),
        Dir::W => Aabb::new(Vec3::new(x0 - out, y0, z0 + a0), Vec3::new(x0, y1, z0 + a1)),
    }
}

/// Lamps on lane walls, signs over the lanes, and signboards on the outer wall
/// (the Walled City's edge was plastered with dentists' boards).
pub fn fixtures(city: &City, year: u16) -> Vec<Fixture> {
    let dir = address::build(city);
    let mut out = landmarks(city);
    for (b, lane, _) in plaques(city, &dir, year) {
        let _ = lane;
        out.push(Fixture { aabb: b, col: srgb(30, 60, 150), emit: -0.25 });
    }
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) != Ground::Plot {
                continue;
            }
            let h = city.height_at(c, year) as i32;
            if h == 0 {
                continue;
            }
            for d in Dir::ALL {
                let Some(n) = city.step(c, d) else { continue };
                let r = |k: u32| hash(i as u32 * 4 + d as u32, j as u32, k);
                match city.ground_at(n) {
                    Ground::Alley => {
                        // A bulb or a tube bracketed to the wall above head height.
                        let fam = family(city, &dir, n);
                        if r(1) < 0.14 {
                            let bulb = r(2) < 0.5;
                            // Lamps are tinted with their lane family's colour.
                            let col = match fam {
                                Some(k) if r(7) < 0.8 => family_col(k),
                                _ if bulb => srgb(255, 190, 120),
                                _ => TUBE_COL,
                            };
                            let (a0, a1) = if bulb { (0.65, 0.85) } else { (0.2, 1.3) };
                            out.push(Fixture { aabb: on_wall(c, d, a0, a1, 2.55, 2.65, 0.15), col, emit: 2.0 });
                        }
                        // A projecting sign on a lower floor, always below this building's roof.
                        if h >= 2 && r(3) < 0.08 {
                            let f = 1 + (r(4) * (h - 1).min(2) as f32) as i32;
                            let y = f as f32 * S + 0.5;
                            let sc = SIGN_COLS[(r(5) * SIGN_COLS.len() as f32) as usize % SIGN_COLS.len()];
                            let lit = r(6) < 0.75;
                            let col = match fam {
                                Some(k) if r(8) < 0.7 => family_col(k),
                                _ => srgb(sc[0], sc[1], sc[2]),
                            };
                            vertical_sign(&mut out, c, d, y, col, lit, 2 + (r(9) * 3.0) as i32);
                        }
                    }
                    Ground::Outside => {
                        // Signboards on the outer wall, lower half of the building.
                        let floors = (h / 2).max(1);
                        for f in 1..floors {
                            if hash(i as u32 + d as u32 * 977, j as u32, f as u32 + 50) < 0.10 {
                                let sc = SIGN_COLS[(hash(i as u32, j as u32, f as u32 + 51) * SIGN_COLS.len() as f32) as usize % SIGN_COLS.len()];
                                let y = f as f32 * S + 0.3;
                                let lit = hash(i as u32, j as u32, f as u32 + 52) < 0.7;
                                let glyphs = 2 + (hash(i as u32, j as u32, f as u32 + 53) * 3.0) as i32;
                                board_sign(&mut out, c, d, y, srgb(sc[0], sc[1], sc[2]), lit, glyphs);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Which open space a point is in (None = inside something solid).
pub fn space_of(city: &City, year: u16, p: Vec3) -> Option<Space> {
    let (i, j) = ((p.x / C).floor() as i32, (p.z / C).floor() as i32);
    if !city.in_bounds(i, j) {
        return Some(((0, 0), OPEN)); // the world outside: one big open space
    }
    let c = (i as u16, j as u16);
    match city.ground_at(c) {
        Ground::Plot => {
            let h = city.height_at(c, year) as i32;
            let lv = (p.y / S).floor() as i32;
            if lv >= h || city.role_at(c) == Role::Stair {
                return Some((c, OPEN));
            }
            let pid = city.plot_of[city.idx(c)];
            (lv >= 0 && city.circ_at(c, pid, lv as u8)).then_some((c, lv as u8))
        }
        Ground::Yamen => None,
        Ground::Outside => Some(((0, 0), OPEN)),
        _ => Some((c, OPEN)),
    }
}

/// Open spaces directly connected to `s`.
fn neighbours(city: &City, year: u16, links: &HashMap<Space, Vec<Space>>, s: Space) -> Vec<Space> {
    let (c, lv) = s;
    let mut v = links.get(&s).cloned().unwrap_or_default();
    let open_ground = |n: Cell| matches!(city.ground_at(n), Ground::Alley | Ground::Well);
    if lv == OPEN {
        match city.ground_at(c) {
            Ground::Plot if city.role_at(c) == Role::Stair => {
                let plot = city.plot_at(c).unwrap();
                for f in 0..city.height_at(c, year) {
                    v.push((plot.core[0], f));
                }
                for &o in &plot.core[1..] {
                    if o != c {
                        v.push((o, OPEN));
                    }
                }
            }
            Ground::Plot => {}
            _ => {
                for (_, n) in city.neighbours(c) {
                    if open_ground(n) {
                        v.push((n, OPEN));
                    } else if city.role_at(n) == Role::Core && city.height_at(n, year) > 0 {
                        v.push((n, 0)); // the stair doorway off the lane
                    }
                }
            }
        }
        return v;
    }
    let pid = city.plot_of[city.idx(c)];
    for (_, n) in city.neighbours(c) {
        if city.circ_at(n, pid, lv) {
            v.push((n, lv));
        }
        if city.role_at(c) == Role::Core {
            if city.plot_of[city.idx(n)] == pid && city.role_at(n) == Role::Stair {
                v.push((n, OPEN));
            }
            if lv == 0 && open_ground(n) {
                v.push((n, OPEN));
            }
        }
    }
    v
}

pub fn collect(city: &City, year: u16) -> Vec<PointLight> {
    let mut lights = vec![];
    // Tubes on landings and corridors.
    for p in city.plots.iter().filter(|p| p.height_at(year) > 0) {
        let h = p.height_at(year) as i32;
        for &c in &p.cells {
            let landing = city.role_at(c) == Role::Core;
            for f in 0..h {
                if !city.circ_at(c, p.id, f as u8) {
                    continue;
                }
                if tube(c, f, landing) == Some(true) {
                    let b = tube_box(c, f);
                    lights.push(PointLight { pos: (b.min + b.max) * 0.5 - Vec3::Y * 0.1, col: lin(TUBE_COL) * 0.9, range: 3.6 });
                }
            }
        }
    }
    // Bare bulbs over the stairs' turning landings.
    for p in city.plots.iter().filter(|p| p.height_at(year) > 0) {
        for f in 0..p.height_at(year) as i32 {
            if let Some(b) = stair_bulb(p, f) {
                lights.push(PointLight { pos: (b.min + b.max) * 0.5 - Vec3::Y * 0.2, col: lin(BULB_COL) * 1.1, range: 4.5 });
            }
        }
    }
    // Open doors and shopfronts.
    for u in city.units_at(year).filter(|u| u.door_state == DoorState::Open) {
        let (cx, cz) = centre(u.door.cell);
        let (dx, dz) = u.door.facing.delta();
        let pos = Vec3::new(cx + dx as f32 * C * 0.75, u.floor as f32 * S + 1.4, cz + dz as f32 * C * 0.75);
        let shop = city.step(u.door.cell, u.door.facing).is_some_and(|n| city.ground_at(n) == Ground::Alley);
        let warm = hash(u.id, 3, 3) < 0.5;
        let col = if warm { lin(srgb(255, 200, 140)) } else { lin(srgb(210, 255, 225)) };
        lights.push(PointLight { pos, col: col * if shop { 1.2 } else { 0.6 }, range: if shop { 4.5 } else { 2.5 } });
    }
    // Lamps and lit signs.
    for fx in fixtures(city, year) {
        if fx.emit.abs() > 0.5 {
            let pos = (fx.aabb.min + fx.aabb.max) * 0.5;
            lights.push(PointLight { pos, col: lin(fx.col) * 0.9, range: 4.5 });
        }
    }
    lights
}

/// Bake light spill into `mesh` vertices.
pub fn bake(city: &City, year: u16, lights: &[PointLight], meshes: &mut [&mut MeshData]) {
    // Bridges and passages join spaces across buildings.
    let mut links: HashMap<Space, Vec<Space>> = HashMap::new();
    for br in city.bridges.iter().filter(|b| b.year <= year) {
        let mut chain: Vec<Space> = vec![(br.a, br.floor)];
        chain.extend(br.span.iter().map(|&c| (c, OPEN)));
        chain.push((br.b, br.floor));
        for w in chain.windows(2) {
            links.entry(w[0]).or_default().push(w[1]);
            links.entry(w[1]).or_default().push(w[0]);
        }
    }
    // Flood each light through open space, limited by its range.
    let mut reach: HashMap<Space, Vec<u32>> = HashMap::new();
    for (k, l) in lights.iter().enumerate() {
        let Some(s0) = space_of(city, year, l.pos) else { continue };
        let steps = (l.range / C).ceil() as u32 + 1;
        let mut seen = HashMap::from([(s0, 0u32)]);
        let mut q = VecDeque::from([s0]);
        while let Some(s) = q.pop_front() {
            reach.entry(s).or_default().push(k as u32);
            let d = seen[&s];
            if d >= steps || s.0 == (0, 0) && s.1 == OPEN {
                continue;
            }
            for n in neighbours(city, year, &links, s) {
                let (nx, nz) = centre(n.0);
                if Vec3::new(nx - l.pos.x, 0.0, nz - l.pos.z).length() > l.range + C {
                    continue;
                }
                if !seen.contains_key(&n) {
                    seen.insert(n, d + 1);
                    q.push_back(n);
                }
            }
        }
    }
    for mesh in meshes.iter_mut() {
        for v in mesh.vertices.iter_mut() {
            let p = Vec3::from(v.pos);
            let n = Vec3::from(v.normal);
            let Some(s) = space_of(city, year, p + n * 0.05) else { continue };
            let Some(ids) = reach.get(&s) else { continue };
            let mut acc = Vec3::ZERO;
            for &k in ids {
                let l = &lights[k as usize];
                let to = l.pos - p;
                let d = to.length();
                if d >= l.range {
                    continue;
                }
                let fall = (1.0 - d / l.range).powi(2);
                let wrap = (n.dot(to / d.max(1e-3)) * 0.7 + 0.3).max(0.0);
                acc += l.col * fall * wrap;
            }
            v.light = acc.into();
        }
    }
}
