//! The walkable city: one list of boxes that is both what you see inside and what
//! you collide with. Landings, corridors, walls with doorways, dog-leg stairs,
//! stair huts, bridges, parapets, ladders, lanes built over at upper floors,
//! unit doors and shopfronts.

use crate::citymesh::{hash, srgb};
use glam::Vec3;
use kwc_engine::mesh::{Faces, MeshData};
use kwc_sim::*;
use std::collections::HashMap;

pub const S: f32 = STOREY_M;
const C: f32 = CELL_M;
/// Floor slab thickness.
pub const SLAB: f32 = 0.2;
/// Partition wall thickness.
const WALL: f32 = 0.1;
/// Stair: eight risers per half-flight (17.5 cm), half a storey each flight.
const RISERS: usize = 8;
/// Stairwell plan (a 3 x 3 m well): a lip at floor level by the landing, the
/// flights' run, and the turning landing at the far end.
const NEAR: f32 = 0.1;
const RUN: f32 = 2.1;
const WELL: f32 = 2.0 * C;
const PARAPET: f32 = 1.0;
const DOOR_W: f32 = 0.95;
const DOOR_H: f32 = 2.1;
/// Lanes are built over from this height up (two storeys of headroom).
pub const OVERBUILD_FROM: f32 = 2.0 * S;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Aabb {
        Aabb { min: min.min(max), max: min.max(max) }
    }
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x && self.max.x > o.min.x && self.min.y < o.max.y && self.max.y > o.min.y && self.min.z < o.max.z && self.max.z > o.min.z
    }
}

/// A climbable volume: inside it, forward input climbs instead of walking.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Ladder {
    pub volume: Aabb,
    /// Horizontal direction you face to climb it (towards the taller wall).
    pub facing: Vec3,
    /// Roof height at the top.
    pub top: f32,
}

pub struct WalkWorld {
    pub boxes: Vec<Aabb>,
    pub ladders: Vec<Ladder>,
    buckets: HashMap<(i32, i32), Vec<u32>>,
    pub spawn: Vec3,
    pub spawn_yaw: f32,
}

impl WalkWorld {
    fn bucket_range(a: &Aabb) -> (i32, i32, i32, i32) {
        ((a.min.x / C).floor() as i32, (a.max.x / C).floor() as i32, (a.min.z / C).floor() as i32, (a.max.z / C).floor() as i32)
    }

    fn index(&mut self) {
        self.buckets.clear();
        for (n, b) in self.boxes.iter().enumerate() {
            let (i0, i1, j0, j1) = Self::bucket_range(b);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    self.buckets.entry((i, j)).or_default().push(n as u32);
                }
            }
        }
    }

    /// Boxes that might touch `q`.
    pub fn near(&self, q: &Aabb) -> impl Iterator<Item = &Aabb> + '_ {
        let (i0, i1, j0, j1) = Self::bucket_range(q);
        let mut ids: Vec<u32> = vec![];
        for j in j0..=j1 {
            for i in i0..=i1 {
                if let Some(v) = self.buckets.get(&(i, j)) {
                    ids.extend_from_slice(v);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(move |k| &self.boxes[k as usize])
    }

    pub fn blocked(&self, q: &Aabb) -> bool {
        // The whole world stands on solid ground.
        q.min.y < 0.0 || self.near(q).any(|b| b.overlaps(q))
    }

    pub fn ladder_at(&self, q: &Aabb) -> Option<&Ladder> {
        self.ladders.iter().find(|l| l.volume.overlaps(q))
    }
}

/// Collects geometry: every piece can render, collide, or both.
struct Builder {
    boxes: Vec<Aabb>,
    ladders: Vec<Ladder>,
    mesh: MeshData,
}

impl Builder {
    fn solid(&mut self, a: Aabb, col: [f32; 3], faces: Faces) {
        self.boxes.push(a);
        self.mesh.cuboid(a.min, a.max, col, 0.0, faces);
    }
    fn visual(&mut self, a: Aabb, col: [f32; 3], emit: f32, faces: Faces) {
        self.mesh.cuboid(a.min, a.max, col, emit, faces);
    }
}

fn cell_rect(c: Cell) -> (f32, f32, f32, f32) {
    let (x0, z0) = (c.0 as f32 * C, c.1 as f32 * C);
    (x0, z0, x0 + C, z0 + C)
}

/// A strip `t` thick against face `d` of cell `c`, from `a0` to `a1` along the face.
fn face_box(c: Cell, d: Dir, t: f32, a0: f32, a1: f32, y0: f32, y1: f32) -> Aabb {
    let (x0, z0, x1, z1) = cell_rect(c);
    let (lo, hi) = match d {
        Dir::N => (Vec3::new(x0 + a0, y0, z0), Vec3::new(x0 + a1, y1, z0 + t)),
        Dir::S => (Vec3::new(x0 + a0, y0, z1 - t), Vec3::new(x0 + a1, y1, z1)),
        Dir::E => (Vec3::new(x1 - t, y0, z0 + a0), Vec3::new(x1, y1, z0 + a1)),
        Dir::W => (Vec3::new(x0, y0, z0 + a0), Vec3::new(x0 + t, y1, z0 + a1)),
    };
    Aabb::new(lo, hi)
}

/// Same strip, but just outside the face (in the neighbouring cell).
fn face_box_out(c: Cell, d: Dir, t: f32, a0: f32, a1: f32, y0: f32, y1: f32) -> Aabb {
    let mut b = face_box(c, d, 0.0, a0, a1, y0, y1);
    match d {
        Dir::N => b.min.z -= t,
        Dir::S => b.max.z += t,
        Dir::E => b.max.x += t,
        Dir::W => b.min.x -= t,
    }
    b
}

/// Shift a box along `d` by `amount` metres (negative = back into the cell).
fn nudge(a: &mut Aabb, d: Dir, amount: f32) {
    let (dx, dz) = d.delta();
    let v = Vec3::new(dx as f32, 0.0, dz as f32) * amount;
    a.min += v;
    a.max += v;
}

/// Stairwell frame: `u` runs from the landing's edge into the well, `v` across
/// it from the A flight's side (0..1.5 m) to the B flight's (1.5..3 m).
pub struct Well {
    origin: Vec3,
    u: Vec3,
    v: Vec3,
}

impl Well {
    pub fn of(plot: &Plot) -> Well {
        let (na, nb) = (plot.core[1], plot.core[3]);
        let du = dir_between(plot.core[0], na).expect("landing beside the stair").delta();
        let dv = dir_between(na, nb).expect("stairwell is 2 x 2").delta();
        let u = Vec3::new(du.0 as f32, 0.0, du.1 as f32);
        let v = Vec3::new(dv.0 as f32, 0.0, dv.1 as f32);
        let centre = Vec3::new((na.0 as f32 + 0.5) * C, 0.0, (na.1 as f32 + 0.5) * C);
        Well { origin: centre - (u + v) * (C / 2.0), u, v }
    }
    pub fn point(&self, u: f32, v: f32) -> Vec3 {
        self.origin + self.u * u + self.v * v
    }
    fn bx(&self, u0: f32, u1: f32, v0: f32, v1: f32, y0: f32, y1: f32) -> Aabb {
        let (a, b) = (self.point(u0, v0), self.point(u1, v1));
        Aabb::new(Vec3::new(a.x, y0, a.z), Vec3::new(b.x, y1, b.z))
    }
}

pub fn landing_dir(plot: &Plot) -> Dir {
    dir_between(plot.core[1], plot.core[0]).expect("stair beside its landing")
}

const ALL: Faces = Faces::ALL;
const TOP: Faces = Faces { top: true, bottom: false, px: false, nx: false, pz: false, nz: false };
const BOTTOM: Faces = Faces { top: false, bottom: true, px: false, nx: false, pz: false, nz: false };
const SIDES: Faces = Faces { top: false, bottom: false, px: true, nx: true, pz: true, nz: true };

fn only(d: Dir) -> Faces {
    let mut f = Faces { top: false, bottom: false, px: false, nx: false, pz: false, nz: false };
    match d {
        Dir::N => f.nz = true,
        Dir::S => f.pz = true,
        Dir::E => f.px = true,
        Dir::W => f.nx = true,
    }
    f
}

fn opposite(d: Dir) -> Dir {
    match d {
        Dir::N => Dir::S,
        Dir::S => Dir::N,
        Dir::E => Dir::W,
        Dir::W => Dir::E,
    }
}

fn dir_between(a: Cell, b: Cell) -> Option<Dir> {
    Dir::ALL.into_iter().find(|&d| {
        let (di, dj) = d.delta();
        a.0 as i32 + di == b.0 as i32 && a.1 as i32 + dj == b.1 as i32
    })
}

/// Paint for corridor walls: institutional greens, pinks and greys, grubby.
fn paint(plot: u32, f: i32) -> [f32; 3] {
    const P: &[[u8; 3]] = &[[150, 176, 150], [186, 150, 150], [160, 160, 150], [140, 160, 170], [178, 170, 140]];
    let c = P[(hash(plot, f as u32, 41) * P.len() as f32) as usize % P.len()];
    srgb(c[0], c[1], c[2])
}

fn concrete(k: f32) -> [f32; 3] {
    let c = srgb(88, 86, 82);
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Lane cells built over at upper floors: `(cell, bottom, top)` in metres.
/// Chosen in patches (smooth noise), only where buildings flank the lane on both
/// sides, and never over a bridge crossing or right beside the Yamen.
pub fn overbuild(city: &City, year: u16) -> Vec<(Cell, f32, f32)> {
    let spans: std::collections::HashSet<Cell> = city.bridges.iter().flat_map(|b| b.span.iter().copied()).collect();
    let mut out = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) != Ground::Alley || spans.contains(&c) {
                continue;
            }
            if crate::citymesh::near_yamen(city, c) {
                continue;
            }
            let h = |d: Dir| city.step(c, d).map_or(0, |n| if city.ground_at(n) == Ground::Plot { city.height_at(n, year) } else { 0 });
            let top = h(Dir::E).min(h(Dir::W)).max(h(Dir::N).min(h(Dir::S)));
            if top < 4 {
                continue;
            }
            // Patchy: neighbouring lane cells mostly agree.
            let n = value_noise(i as f32 / 6.0, j as f32 / 6.0, city.params.seed as u32);
            if n < 0.5 {
                continue;
            }
            let top = (top as f32 - (hash(i as u32, j as u32, 5) * 3.0).floor()).max(3.0);
            out.push((c, OVERBUILD_FROM, top * S));
        }
    }
    out
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |a: f32, b: f32| hash(a as i32 as u32, b as i32 as u32, seed ^ 0x77);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (a, b, c, d) = (h(xi, yi), h(xi + 1.0, yi), h(xi, yi + 1.0), h(xi + 1.0, yi + 1.0));
    let top = a + (b - a) * s(fx);
    let bot = c + (d - c) * s(fx);
    top + (bot - top) * s(fy)
}

/// Height (m) of the solid top surface on a cell: roofs, the Yamen, overbuilds.
fn top_of(city: &City, c: Cell, year: u16, over: &HashMap<Cell, (f32, f32)>) -> f32 {
    match city.ground_at(c) {
        Ground::Plot => city.height_at(c, year) as f32 * S,
        Ground::Yamen => crate::citymesh::YAMEN_H,
        Ground::Alley => over.get(&c).map_or(0.0, |o| o.1),
        _ => 0.0,
    }
}

pub fn build(city: &City, year: u16) -> (WalkWorld, MeshData) {
    let mut b = Builder { boxes: vec![], ladders: vec![], mesh: MeshData::default() };
    let over: HashMap<Cell, (f32, f32)> = overbuild(city, year).into_iter().map(|(c, a, t)| (c, (a, t))).collect();
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|br| br.year <= year).collect();
    // Does a bridge leave cell `c` through face `d` at floor `f`?
    let bridge_through = |c: Cell, d: Dir, f: i32| -> bool {
        bridges.iter().any(|br| {
            br.floor as i32 == f && {
                let first = |from: Cell, to: Cell| dir_between(from, br.span.first().copied().unwrap_or(to));
                (br.a == c && first(br.a, br.b) == Some(d)) || (br.b == c && {
                    let last = br.span.last().copied().unwrap_or(br.a);
                    dir_between(br.b, last) == Some(d)
                })
            }
        })
    };

    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let (x0, z0, x1, z1) = cell_rect(c);
            match city.ground_at(c) {
                Ground::Yamen => {
                    b.boxes.push(Aabb::new(Vec3::new(x0, 0.0, z0), Vec3::new(x1, crate::citymesh::YAMEN_H, z1)));
                }
                Ground::Alley => {
                    if let Some(&(y0, y1)) = over.get(&c) {
                        // Building over the lane: solid above, a dark soffit below.
                        b.mesh.ao = 0.3;
                        b.solid(Aabb::new(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1)), concrete(0.6), BOTTOM);
                        b.mesh.ao = 1.0;
                    }
                }
                Ground::Plot => {
                    let h = city.height_at(c, year) as i32;
                    if h == 0 {
                        continue;
                    }
                    let pid = city.plot_of[city.idx(c)];
                    let plot = &city.plots[pid as usize];
                    if city.role_at(c) == Role::Stair {
                        if c == plot.core[1] {
                            stairwell(&mut b, city, plot, h);
                        }
                        continue;
                    }
                    // Storey by storey: runs of rooms are solid; circulation is hollow.
                    let mut run_start: Option<i32> = None;
                    for f in 0..=h {
                        let room = f < h && !city.circ_at(c, pid, f as u8);
                        match (room, run_start) {
                            (true, None) => run_start = Some(f),
                            (false, Some(f0)) => {
                                b.boxes.push(Aabb::new(Vec3::new(x0, f0 as f32 * S, z0), Vec3::new(x1, f as f32 * S, z1)));
                                run_start = None;
                            }
                            _ => {}
                        }
                        if f < h && !room {
                            circulation_storey(&mut b, city, c, pid, f, h, &bridge_through);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    unit_doors(&mut b, city, year);
    bridge_decks(&mut b, city, &bridges);
    // Roof furniture is drawn by the city mesher; here it only collides.
    let (pieces, ladders) = roof_furniture(city, year);
    b.boxes.extend(pieces.iter().filter(|p| p.solid).map(|p| p.aabb));
    b.ladders.extend(ladders);

    // Spawn outside the South Gate, looking in along the lane behind it.
    let (spawn, spawn_yaw) = lane_view(city, city.lanes.iter().find(|l| l.name == "Lung Chun Road"), 0).unwrap_or_else(|| {
        let g = city.south_gate;
        (Vec3::new((g.0 as f32 + 0.5) * C, 0.0, (g.1 as f32 + 0.5) * C), 0.0)
    });
    let spawn = spawn - Vec3::new(spawn_yaw.cos(), 0.0, spawn_yaw.sin()) * 5.0;
    let mut w = WalkWorld { boxes: b.boxes, ladders: b.ladders, buckets: HashMap::new(), spawn, spawn_yaw };
    w.index();
    (w, b.mesh)
}

/// A standing spot on a lane at cell index `k` (clamped), facing along it.
pub fn lane_view(city: &City, lane: Option<&Lane>, k: usize) -> Option<(Vec3, f32)> {
    let cells = &lane?.cells;
    if cells.len() < 3 {
        return None;
    }
    let k = k.clamp(0, cells.len() - 3);
    let (a, b) = (cells[k], cells[(k + 3).min(cells.len() - 1)]);
    let p = |c: Cell| Vec3::new((c.0 as f32 + 0.5) * C, 0.0, (c.1 as f32 + 0.5) * C);
    let d = p(b) - p(a);
    let _ = city;
    Some((p(a), d.z.atan2(d.x)))
}

/// Corner posts. Where a space turns a corner (both neighbours at a corner are
/// open but the diagonal cell is walled off), the two walls end short of each
/// other; a WALL x WALL post in this cell's corner closes the joint.
fn corner_posts(b: &mut Builder, c: Cell, y0: f32, y1: f32, col: [f32; 3], open: &dyn Fn(Cell) -> bool, city: &City) {
    let (x0, z0, x1, z1) = cell_rect(c);
    for (dx, dz) in [(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
        let (i, j) = (c.0 as i32, c.1 as i32);
        let at = |di: i32, dj: i32| {
            let (a, bb) = (i + di, j + dj);
            city.in_bounds(a, bb).then(|| (a as u16, bb as u16))
        };
        let (Some(n1), Some(n2)) = (at(dx, 0), at(0, dz)) else { continue };
        let diag_open = at(dx, dz).is_some_and(|d| open(d));
        if open(n1) && open(n2) && !diag_open {
            let x = if dx < 0 { (x0, x0 + WALL) } else { (x1 - WALL, x1) };
            let z = if dz < 0 { (z0, z0 + WALL) } else { (z1 - WALL, z1) };
            b.solid(Aabb::new(Vec3::new(x.0, y0, z.0), Vec3::new(x.1, y1, z.1)), col, SIDES);
        }
    }
}

/// Everything but the face towards `d`: walls against a facade or a neighbour's
/// wall don't draw their outer side (it would fight with that surface).
fn except(d: Dir) -> Faces {
    let mut f = ALL;
    match d {
        Dir::N => f.nz = false,
        Dir::S => f.pz = false,
        Dir::E => f.px = false,
        Dir::W => f.nx = false,
    }
    f
}

/// One storey of a landing or corridor cell: floor, ceiling, walls, doorways,
/// the faces of the flats around it, and a fluorescent tube.
fn circulation_storey(b: &mut Builder, city: &City, c: Cell, pid: u32, f: i32, h: i32, bridge_through: &dyn Fn(Cell, Dir, i32) -> bool) {
    let (x0, z0, x1, z1) = cell_rect(c);
    let landing = city.role_at(c) == Role::Core;
    let fl = f as u8;
    let y = f as f32 * S;
    b.mesh.ao = 0.12;
    // Floor, and ceiling slab (the roof's underside on the top storey).
    b.solid(Aabb::new(Vec3::new(x0, y - SLAB, z0), Vec3::new(x1, y, z1)), concrete(0.8), TOP);
    let ceil_col = if f + 1 == h { concrete(0.5) } else { concrete(0.65) };
    b.solid(Aabb::new(Vec3::new(x0, y + S - SLAB, z0), Vec3::new(x1, y + S, z1)), ceil_col, BOTTOM);
    // Fluorescent tube on landings and about half the corridor cells.
    if let Some(lit) = crate::lights::tube(c, f, landing) {
        b.visual(crate::lights::tube_box(c, f), crate::lights::TUBE_COL, if lit { 1.6 } else { 0.0 }, BOTTOM);
    }
    let (y0, y1) = (y, y + S - SLAB);
    let col = paint(pid, f);
    let same_space = |n: Cell| city.circ_at(n, pid, fl) || (landing && city.role_at(n) == Role::Stair && city.plot_of[city.idx(n)] == pid);
    corner_posts(b, c, y0, y1, col, &same_space, city);
    for d in Dir::ALL {
        let n = city.step(c, d);
        let open = n.is_some_and(|n| {
            city.circ_at(n, pid, fl) || (landing && city.role_at(n) == Role::Stair && city.plot_of[city.idx(n)] == pid)
        });
        if open {
            continue;
        }
        if n.is_some_and(|n| city.room_at(n, pid, fl)) {
            // A flat's wall: same thickness and plane as every other corridor wall,
            // so corners meet cleanly.
            b.solid(face_box(c, d, WALL, 0.0, C, y0, y1), col, except(d));
            continue;
        }
        let lane = n.is_some_and(|n| city.ground_at(n) == Ground::Alley);
        if (f == 0 && landing && lane) || bridge_through(c, d, f) {
            // A doorway. The facade has a hole here, so the jambs draw all round.
            let (a0, a1) = ((C - DOOR_W) / 2.0, (C + DOOR_W) / 2.0);
            b.solid(face_box(c, d, WALL, 0.0, a0, y0, y1), col, ALL);
            b.solid(face_box(c, d, WALL, a1, C, y0, y1), col, ALL);
            b.solid(face_box(c, d, WALL, a0, a1, y0 + DOOR_H, y1), col, ALL);
        } else {
            b.solid(face_box(c, d, WALL, 0.0, C, y0, y1), col, except(d));
        }
    }
    b.mesh.ao = 1.0;
}

/// A dog-leg stair in a 3 x 3 m well: from the floor-level strip by the landing,
/// flight A climbs half a storey away from it, turns on the far landing, and
/// flight B climbs back to the strip one storey up.
fn stairwell(b: &mut Builder, city: &City, plot: &Plot, h: i32) {
    let w = Well::of(plot);
    let rise = S / 2.0 / RISERS as f32;
    let tread = RUN / RISERS as f32;
    let half = WELL / 2.0;
    // Terrazzo treads, pale enough to catch the bulbs.
    let step_col = srgb(150, 146, 136);
    b.mesh.ao = 0.12;
    // Floor-level strip by the landing on every floor, the roof (inside the hut)
    // included; the ground floor's is the ground.
    for f in 1..=h {
        let y = f as f32 * S;
        b.solid(w.bx(0.0, NEAR, 0.0, WELL, y - 0.3, y), step_col, ALL);
    }
    for f in 0..h {
        let y = f as f32 * S;
        for k in 1..=RISERS {
            let kf = k as f32;
            let ta = y + kf * rise;
            b.solid(w.bx(NEAR + (kf - 1.0) * tread, NEAR + kf * tread, 0.0, half, ta - 0.3, ta), step_col, ALL);
            let tb = y + S / 2.0 + kf * rise;
            b.solid(w.bx(NEAR + RUN - kf * tread, NEAR + RUN - (kf - 1.0) * tread, half, WELL, tb - 0.3, tb), step_col, ALL);
        }
        let tl = y + S / 2.0;
        b.solid(w.bx(NEAR + RUN, WELL, 0.0, WELL, tl - 0.3, tl), step_col, ALL);
        // A balustrade along the inside edge of each flight, stepping with it;
        // between the two you can see up and down the well.
        let rail = paint(plot.id, f);
        for k in 1..=RISERS {
            let kf = k as f32;
            let (u0, u1) = (NEAR + (kf - 1.0) * tread, NEAR + kf * tread);
            let ta = y + kf * rise;
            b.solid(w.bx(u0, u1, half - 0.08, half - 0.02, ta, ta + 0.9), rail, ALL);
            let tb = y + S / 2.0 + (RISERS - k + 1) as f32 * rise;
            b.solid(w.bx(u0, u1, half + 0.02, half + 0.08, tb, tb + 0.9), rail, ALL);
        }
        if let Some(bulb) = crate::lights::stair_bulb(plot, f) {
            b.visual(bulb, crate::lights::BULB_COL, 2.5, ALL);
        }
    }
    // Walls round the well, open only to the landing.
    let top = h as f32 * S;
    let cells = &plot.core[1..5];
    let in_well = |n: Cell| cells.contains(&n) || n == plot.core[0] || n == plot.core[5];
    // Walls painted storey by storey, matching that floor's landing, so the
    // joints between landing and well line up in colour as well as plane.
    for f in 0..h {
        let (y0, y1) = (f as f32 * S, (f + 1) as f32 * S);
        let col = paint(plot.id, f);
        corner_posts(b, plot.core[1], y0, y1, col, &in_well, city);
        for &c in cells {
            for d in Dir::ALL {
                let n = city.step(c, d);
                if n.is_some_and(|n| cells.contains(&n) || (n == plot.core[0] && c == plot.core[1]) || (n == plot.core[5] && c == plot.core[3])) {
                    continue;
                }
                b.solid(face_box(c, d, WALL, 0.0, C, y0, y1), col, except(d));
            }
        }
    }
    let _ = top;
    b.mesh.ao = 1.0;
}

/// Unit doors (metal gates) on corridors, and shopfronts on the lanes.
fn unit_doors(b: &mut Builder, city: &City, year: u16) {
    for u in city.units_at(year) {
        let c = u.door.cell;
        let d = u.door.facing;
        let y = u.floor as f32 * S;
        let (a0, a1) = ((C - DOOR_W) / 2.0, (C + DOOR_W) / 2.0);
        let r = hash(u.id, 3, 3);
        let outside = city.step(c, d).is_some_and(|n| city.ground_at(n) == Ground::Alley);
        if outside {
            // Shopfront: most of the cell width; open ones glow, shut ones are shutters.
            b.mesh.ao = 1.0;
            let front = face_box_out(c, d, 0.03, 0.08, C - 0.08, y, y + 2.5);
            if u.usage == UnitUse::Temple {
                // Temple: red front glowing with lamps and incense, a lantern either side.
                b.visual(front, srgb(220, 40, 25), 1.2, only(d));
                for a in [0.12, C - 0.32] {
                    let lantern = face_box_out(c, d, 0.35, a, a + 0.2, y + 2.2, y + 2.55);
                    b.visual(lantern, srgb(255, 90, 40), 2.0, ALL);
                }
            } else if u.door_state == DoorState::Open {
                let col = if r < 0.5 { srgb(255, 214, 160) } else { srgb(220, 255, 230) };
                b.visual(front, col, 0.9, only(d));
            } else {
                b.visual(front, srgb(120, 124, 126), 0.0, only(d));
            }
        } else {
            b.mesh.ao = 0.12;
            let door = face_box_out(c, d, WALL + 0.03, a0, a1, y, y + DOOR_H);
            if u.door_state == DoorState::Open {
                b.visual(door, srgb(255, 200, 150), 0.7, only(d));
            } else {
                const GATES: &[[u8; 3]] = &[[70, 110, 80], [120, 60, 50], [60, 80, 120], [140, 130, 110], [90, 90, 90]];
                let g = GATES[(r * GATES.len() as f32) as usize % GATES.len()];
                b.visual(door, srgb(g[0], g[1], g[2]), 0.0, only(d));
            }
        }
    }
    b.mesh.ao = 1.0;
}

fn bridge_decks(b: &mut Builder, city: &City, bridges: &[&Bridge]) {
    for br in bridges {
        if br.span.is_empty() {
            continue;
        }
        let y = br.floor as f32 * S;
        let across = dir_between(br.a, br.span[0]).unwrap_or(Dir::E);
        for &c in &br.span {
            let (x0, z0, x1, z1) = cell_rect(c);
            b.mesh.ao = 0.6;
            b.solid(Aabb::new(Vec3::new(x0, y - SLAB, z0), Vec3::new(x1, y, z1)), concrete(0.7), ALL);
            // Railings along both sides of the crossing.
            let sides = match across {
                Dir::E | Dir::W => [Dir::N, Dir::S],
                _ => [Dir::E, Dir::W],
            };
            for s in sides {
                let mut r = face_box(c, s, 0.05, 0.0, C, y, y + 1.05);
                nudge(&mut r, s, -0.03);
                b.solid(r, srgb(90, 110, 100), ALL);
            }
        }
    }
    b.mesh.ao = 1.0;
    let _ = city;
}

/// Roof furniture: parapets, stair huts and ladders. Drawn by the city mesher
/// (so they're on the skyline in the overview) and collided with when walking.
pub struct Piece {
    pub aabb: Aabb,
    pub col: [f32; 3],
    pub faces: Faces,
    pub solid: bool,
}

pub fn roof_furniture(city: &City, year: u16) -> (Vec<Piece>, Vec<Ladder>) {
    let over: HashMap<Cell, (f32, f32)> = overbuild(city, year).into_iter().map(|(c, a, t)| (c, (a, t))).collect();
    let mut out: Vec<Piece> = vec![];
    let mut ladders = vec![];
    let mut put = |aabb: Aabb, col: [f32; 3], solid: bool| out.push(Piece { aabb, col, faces: ALL, solid });
    let wood = srgb(150, 120, 80);

    // Stair huts over the wells.
    for plot in &city.plots {
        let h = plot.height_at(year);
        if h == 0 {
            continue;
        }
        let top = h as f32 * S;
        let hut = top + 2.4;
        let cells = &plot.core[1..5];
        for &c in cells {
            for side in Dir::ALL {
                let n = city.step(c, side);
                if n.is_some_and(|n| cells.contains(&n)) {
                    continue;
                }
                if (c == plot.core[1] && n == Some(plot.core[0])) || (c == plot.core[3] && n == Some(plot.core[5])) {
                    put(face_box(c, side, WALL, 0.0, C, top + DOOR_H, hut), concrete(0.75), true);
                    continue;
                }
                // Inset a touch so it never shares a plane with a taller neighbour's wall.
                let mut w = face_box(c, side, WALL, 0.0, C, top, hut);
                nudge(&mut w, side, -0.03);
                put(w, concrete(0.75), true);
            }
            let (x0, z0, x1, z1) = cell_rect(c);
            put(Aabb::new(Vec3::new(x0, hut, z0), Vec3::new(x1, hut + 0.15, z1)), concrete(0.7), true);
        }
    }

    // One ladder per pair of neighbouring roofs of different heights.
    let mut laddered: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut ladder_cells: std::collections::HashSet<(Cell, Dir)> = std::collections::HashSet::new();
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) != Ground::Plot || city.role_at(c) == Role::Stair || city.height_at(c, year) == 0 {
                continue;
            }
            let (pid, h) = (city.plot_of[city.idx(c)], city.height_at(c, year));
            for d in Dir::ALL {
                let Some(n) = city.step(c, d) else { continue };
                if city.ground_at(n) != Ground::Plot || city.role_at(n) == Role::Stair {
                    continue;
                }
                let q = city.plot_of[city.idx(n)];
                if q != pid && city.height_at(n, year) > h && laddered.insert((pid.min(q), pid.max(q))) {
                    ladder_cells.insert((c, d));
                }
            }
        }
    }

    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let top = top_of(city, c, year, &over);
            if top <= 0.0 || city.ground_at(c) == Ground::Yamen || city.role_at(c) == Role::Stair {
                continue;
            }
            let is_plot = city.ground_at(c) == Ground::Plot;
            let pid = city.plot_of[city.idx(c)];
            for d in Dir::ALL {
                let n = city.step(c, d);
                let nt = n.map_or(0.0, |n| top_of(city, n, year, &over));
                if n.is_some_and(|n| is_plot && city.plot_of[city.idx(n)] == pid) {
                    continue;
                }
                if ladder_cells.contains(&(c, d)) {
                    // Ladder up the taller neighbour's wall.
                    let off = |mut a: Aabb| {
                        nudge(&mut a, d, -0.06);
                        a
                    };
                    put(off(face_box(c, d, 0.06, 0.45, 0.5, top, nt + 0.9)), wood, false);
                    put(off(face_box(c, d, 0.06, 1.0, 1.05, top, nt + 0.9)), wood, false);
                    let mut y = top + 0.3;
                    while y < nt {
                        put(off(face_box(c, d, 0.06, 0.45, 1.05, y, y + 0.04)), wood, false);
                        y += 0.3;
                    }
                    let (dx, dz) = d.delta();
                    ladders.push(Ladder {
                        volume: face_box(c, d, 0.35, 0.45, 1.05, top, nt + 0.9),
                        facing: Vec3::new(dx as f32, 0.0, dz as f32),
                        top: nt,
                    });
                    continue;
                }
                if nt < top - 0.4 {
                    // A drop: parapet, with a gap where a ladder arrives from below.
                    let col = concrete(0.8);
                    if n.is_some_and(|n| ladder_cells.contains(&(n, opposite(d)))) {
                        put(face_box(c, d, WALL, 0.0, 0.4, top, top + PARAPET), col, true);
                        put(face_box(c, d, WALL, 1.1, C, top, top + PARAPET), col, true);
                    } else {
                        put(face_box(c, d, WALL, 0.0, C, top, top + PARAPET), col, true);
                    }
                }
            }
        }
    }
    (out, ladders)
}
