//! Demo mode: the courier does the rounds by itself. Routes are found on a
//! fine grid over the real collision world (so stairs, doorways and bridges
//! work exactly as they do for the player), smoothed, then walked with the
//! ordinary player controller: no teleporting, no shortcuts through walls.

use crate::game::{Game, Spot, SpotKind};
use crate::player::{Input, Player, RADIUS};
use crate::world::{Aabb, WalkWorld};
use glam::Vec3;
use std::cmp::Reverse;
use kwc_sim::*;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

/// Grid spacing: fine enough for the gap past a stair's handrail (barely
/// wider than you), and nodes either side of a wall are never neighbours.
const FINE: f32 = 0.15;
/// Highest step the player can take (see player.rs), and furthest drop.
const UP: f32 = 0.4;
const DOWN: f32 = 0.9;

/// Where the player would stand at (x, z) coming from height `y`: on the
/// highest surface under their whole footprint (on a stair, the footprint
/// spans two treads and rests on the upper one), within a step's reach.
fn floor_at(w: &WalkWorld, x: f32, z: f32, y: f32) -> Option<f32> {
    let (lo, hi) = (y - DOWN, y + UP);
    let r = RADIUS + 0.005;
    let q = Aabb::new(Vec3::new(x - r, lo - 0.01, z - r), Vec3::new(x + r, hi + 0.01, z + r));
    let mut best = (lo <= 0.0 && hi >= 0.0).then_some(0.0f32);
    for b in w.near(&q) {
        if b.min.x < x + r && b.max.x > x - r && b.min.z < z + r && b.max.z > z - r && b.max.y >= lo && b.max.y <= hi {
            best = Some(best.map_or(b.max.y, |v| v.max(b.max.y)));
        }
    }
    best
}

fn stands(w: &WalkWorld, p: Vec3) -> bool {
    !w.blocked(&Player::aabb_at(p + Vec3::Y * 0.02))
}

type Key = (i32, i32, i32);

fn key(x: i32, z: i32, y: f32) -> Key {
    (x, z, (y * 20.0).round() as i32)
}


/// A route from `from` to `to`: first cell by cell over the city's walk
/// graph (lanes, stairs, corridors, bridges), then on a fine grid through
/// just those cells and their neighbours.
pub fn route(w: &WalkWorld, city: &City, year: u16, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
    // Steps to the goal over the walk graph (lanes, stairs, corridors,
    // bridges; not the roof ladders) guide the fine search. A guide, not a
    // fence: the graph is a little optimistic (a ground-floor corridor beside
    // a lane counts as open to it), so the search may leave it when the
    // building disagrees.
    let goal = graph_node(city, year, to).or_else(|| nearby_node(city, year, to));
    let dist = goal.map(|g| {
        kwc_sim::walk::distances(city, year, g, &|(c, lv)| city.ground_at(c) != Ground::Plot || lv < city.height_at(c, year) || (c, lv) == g)
    });
    let flat = |p: Vec3| Vec3::new(p.x - to.x, 0.0, p.z - to.z).length() + (p.y - to.y).abs() * 1.5;
    // Per cell and storey: graph steps to go, for points on the graph, or
    // from the nearest graph node for points just off it (open ground,
    // outside the gate). Straight-line distance is used only when there's no
    // graph at all, since mixing the two would pull the search off the lanes.
    let cache: std::cell::RefCell<HashMap<(i32, i32, i32), Option<f32>>> = Default::default();
    let h = |p: Vec3| -> f32 {
        let Some(m) = dist.as_ref() else { return flat(p) };
        let k = ((p.x / CELL_M).floor() as i32, (p.z / CELL_M).floor() as i32, ((p.y + 0.1) / STOREY_M).floor() as i32);
        let d = *cache.borrow_mut().entry(k).or_insert_with(|| {
            let steps = |n: kwc_sim::walk::Node| m.get(&n).map(|&d| d as f32 * CELL_M);
            graph_node(city, year, p).and_then(steps).or_else(|| {
                let n = nearby_node(city, year, p)?;
                let c = Vec3::new((n.0 .0 as f32 + 0.5) * CELL_M, p.y, (n.0 .1 as f32 + 0.5) * CELL_M);
                steps(n).map(|d| d + c.distance(p))
            })
        });
        d.map_or(flat(p) * 2.0, |d| d.max(flat(p)))
    };
    grid_route(w, from, to, FINE, &h, 600_000)
}

/// Why a route fails, stage by stage (for tests and debugging).
#[cfg(test)]
pub fn explain(w: &WalkWorld, city: &City, year: u16, from: Vec3, to: Vec3) -> String {
    let near = |p: Vec3| graph_node(city, year, p).or_else(|| nearby_node(city, year, p));
    let (a, b) = (near(from), near(to));
    let coarse = a.zip(b).and_then(|(a, b)| kwc_sim::walk::path(city, year, a, b, &|_| true));
    let fallback = route(w, city, year, from, to).map(|r| r.len());
    format!("from node {a:?} (exact {:?}), to node {b:?}, coarse {:?}, routed {fallback:?}", graph_node(city, year, from), coarse.map(|c| c.len()))
}

/// The walk-graph node for a point: a lane, a floor's corridor, a roof, or a
/// stair (which the graph knows by its first cell, one node per flight).
fn graph_node(city: &City, year: u16, p: Vec3) -> Option<kwc_sim::walk::Node> {
    if p.x < 0.0 || p.z < 0.0 {
        return None;
    }
    let c = ((p.x / CELL_M) as u16, (p.z / CELL_M) as u16);
    if c.0 as usize >= city.w || c.1 as usize >= city.d {
        return None;
    }
    let lv = ((p.y + 0.1) / STOREY_M).floor().max(0.0) as u8;
    match city.ground_at(c) {
        Ground::Alley | Ground::Well => (lv == 0).then_some((c, 0)),
        Ground::Plot => {
            let pid = city.plot_of[city.idx(c)];
            let plot = &city.plots[pid as usize];
            let h = city.height_at(c, year);
            if city.role_at(c) == Role::Stair {
                Some((plot.core[1], ((p.y - 0.05) / STOREY_M).floor().max(0.0) as u8))
            } else if lv == h && h > 0 {
                Some((c, h))
            } else if lv < h && (city.circ_at(c, pid, lv) || city.role_at(c) == Role::Core) {
                Some((c, lv))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Off the graph (outside the gate, on open ground): the nearest node at
/// about the same height.
fn nearby_node(city: &City, year: u16, p: Vec3) -> Option<kwc_sim::walk::Node> {
    (1..=4).find_map(|r| {
        (-r..=r).flat_map(|di| (-r..=r).map(move |dj| (di, dj))).find_map(|(di, dj)| {
            graph_node(city, year, p + Vec3::new(di as f32 * CELL_M, 0.0, dj as f32 * CELL_M))
        })
    })
}

/// A walkable route on a grid of spacing `grid`, searched towards the goal
/// by the estimate `h` (metres to go).
fn grid_route(w: &WalkWorld, from: Vec3, to: Vec3, grid: f32, h: &dyn Fn(Vec3) -> f32, budget: usize) -> Option<Vec<Vec3>> {
    let pt = |x: i32, z: i32, y: f32| Vec3::new(x as f32 * grid, y, z as f32 * grid);
    // Start from the nearest grid point you can stand on.
    let (sx, sz) = ((from.x / grid).round() as i32, (from.z / grid).round() as i32);
    // Nearest grid point you can stand on, spiralling out up to ~0.6 m.
    let reach = (0.6 / grid).ceil() as i32;
    let mut ring: Vec<(i32, i32)> = (-reach..=reach).flat_map(|dx| (-reach..=reach).map(move |dz| (dx, dz))).collect();
    ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
    let start = ring.into_iter().find_map(|(dx, dz)| {
        let (x, z) = (sx + dx, sz + dz);
        let y = floor_at(w, x as f32 * grid, z as f32 * grid, from.y + 0.1)?;
        stands(w, pt(x, z, y)).then_some((x, z, y))
    });
    let start = start?;

    let mut open = BinaryHeap::new();
    let mut came: HashMap<Key, (Key, f32)> = HashMap::new(); // node -> (parent, y)
    let mut cost: HashMap<Key, f32> = HashMap::new();
    let k0 = key(start.0, start.1, start.2);
    cost.insert(k0, 0.0);
    came.insert(k0, (k0, start.2));
    open.push(Reverse(((h(pt(start.0, start.1, start.2)) * 1000.0) as i64, k0)));
    let mut expanded = 0;
    while let Some(Reverse((_, k))) = open.pop() {
        let y = came[&k].1;
        let p = pt(k.0, k.1, y);
        if Vec3::new(p.x - to.x, 0.0, p.z - to.z).length() < 0.5 && (p.y - to.y).abs() < 0.7 {
            // Walk back to the start.
            let mut out = vec![p];
            let mut cur = k;
            while cur != k0 {
                let (prev, _) = came[&cur];
                out.push(pt(prev.0, prev.1, came[&prev].1));
                cur = prev;
            }
            out.reverse();
            out.push(to);
            return Some(smooth(w, out));
        }
        expanded += 1;
        if expanded > budget {
            return None;
        }
        let g0 = cost[&k];
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (x, z) = (k.0 + dx, k.1 + dz);
            let Some(ny) = floor_at(w, x as f32 * grid, z as f32 * grid, y) else { continue };
            let np = pt(x, z, ny);
            let (mx, mz) = ((p.x + np.x) / 2.0, (p.z + np.z) / 2.0);
            let Some(my) = floor_at(w, mx, mz, y) else { continue };
            if !stands(w, np) || !stands(w, Vec3::new(mx, my, mz)) {
                continue;
            }
            let nk = key(x, z, ny);
            let g = g0 + p.distance(np);
            if cost.get(&nk).is_none_or(|&c| g < c) {
                cost.insert(nk, g);
                came.insert(nk, (k, ny));
                // A little greedy: routes come out a touch longer, found far faster.
                open.push(Reverse((((g + 3.0 * h(np)) * 1000.0) as i64, nk)));
            }
        }
    }
    None
}

/// Can you walk straight from a to b (following the floor as it goes)?
fn straight(w: &WalkWorld, a: Vec3, b: Vec3) -> bool {
    let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
    let n = (d.length() / 0.15).ceil().max(1.0) as i32;
    let mut y = a.y;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let (x, z) = (a.x + d.x * t, a.z + d.z * t);
        let Some(ny) = floor_at(w, x, z, y) else { return false };
        if ny < y - 0.45 || !stands(w, Vec3::new(x, ny, z)) {
            return false;
        }
        y = ny;
    }
    (y - b.y).abs() < 0.3
}

/// String-pull the grid path into fewer, straighter legs.
fn smooth(w: &WalkWorld, pts: Vec<Vec3>) -> Vec<Vec3> {
    let mut out = vec![pts[0]];
    let mut i = 0;
    while i < pts.len() - 1 {
        let mut j = (i + 24).min(pts.len() - 1);
        while j > i + 1 && !straight(w, pts[i], pts[j]) {
            j -= 1;
        }
        out.push(pts[j]);
        i = j;
    }
    out
}

/// Where the job wants you next: the unit, where to stand, and the door's
/// direction (you face it to knock).
#[derive(Clone, Copy, PartialEq)]
pub struct Target {
    pub unit: u32,
    pub picked: bool,
    pub stand: Vec3,
    pub door: Vec3,
}

pub fn target(g: &Game, spots: &[Spot]) -> Option<Target> {
    let job = g.job.as_ref()?;
    let unit = if job.picked { job.to } else { job.from };
    let s = spots.iter().find(|s| s.kind == SpotKind::Unit(unit))?;
    let door = Vec3::new(s.label.x - s.stand.x, 0.0, s.label.z - s.stand.z).normalize_or_zero();
    Some(Target { unit, picked: job.picked, stand: s.stand, door })
}

pub enum Act {
    Walk,
    Knock,
    /// No way there: give this job up.
    Lost,
}

#[derive(Default)]
pub struct Autopilot {
    goal: Option<Target>,
    /// A route being worked out on another thread (it can take a second or two).
    pending: Option<std::sync::mpsc::Receiver<Option<Vec<Vec3>>>>,
    path: Vec<Vec3>,
    k: usize,
    /// Standing at the door: time until the knock.
    wait: f32,
    /// Progress watch: best distance to the next waypoint and time since it improved.
    best: f32,
    stuck: f32,
    tries: u32,
    pub status: String,
}

impl Autopilot {
    #[cfg(test)]
    pub fn planning(&self) -> bool {
        self.pending.is_some()
    }

    pub fn drive(&mut self, w: &Arc<WalkWorld>, city: &Arc<City>, year: u16, p: &mut Player, t: Option<Target>, dt: f32) -> (Input, Act) {
        let idle = (Input::default(), Act::Walk);
        let Some(t) = t else { return idle };
        if self.goal != Some(t) {
            self.goal = Some(t);
            self.tries = 0;
            self.plan(w, city, year, p.pos, t);
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(r) => {
                    self.pending = None;
                    self.path = r.unwrap_or_default();
                    self.k = 0;
                    self.best = f32::MAX;
                    self.stuck = 0.0;
                    if self.path.is_empty() {
                        self.tries += 1;
                        if self.tries > 1 {
                            self.goal = None;
                            return (Input::default(), Act::Lost);
                        }
                        self.plan(w, city, year, p.pos, t);
                        return idle;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.status = "Working out the way…".into();
                    // Glance around while thinking.
                    p.pitch += (-0.02 - p.pitch) * (dt * 2.0).min(1.0);
                    return idle;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
        }
        if self.path.is_empty() {
            self.plan(w, city, year, p.pos, t);
            return idle;
        }
        let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
        while self.k < self.path.len() && flat(p.pos, self.path[self.k]) < 0.3 && (p.pos.y - self.path[self.k].y).abs() < 1.0 {
            self.k += 1;
            self.best = f32::MAX;
            self.stuck = 0.0;
        }
        if self.k >= self.path.len() {
            // At the door: turn to it, a beat, knock.
            self.status = if t.picked { "Delivering".into() } else { "Collecting".into() };
            turn_to(p, t.door.z.atan2(t.door.x), dt);
            p.pitch += (-0.05 - p.pitch) * (dt * 4.0).min(1.0);
            self.wait += dt;
            if self.wait > 0.7 {
                self.wait = -1.0; // don't knock again straight away if nothing changes
                return (Input::default(), Act::Knock);
            }
            return idle;
        }
        self.wait = 0.0;
        let next = self.path[self.k];
        let d = flat(p.pos, next);
        if d < self.best - 0.05 {
            self.best = d;
            self.stuck = 0.0;
        } else {
            self.stuck += dt;
            if self.stuck > 2.5 {
                self.status = "Finding another way".into();
                self.plan(w, city, year, p.pos, t);
                return idle;
            }
        }
        let err = turn_to(p, (next.z - p.pos.z).atan2(next.x - p.pos.x), dt);
        // Look where you're going: up and down the stairs too.
        let ahead = self.path[(self.k + 1).min(self.path.len() - 1)];
        let rise = (ahead.y - p.pos.y) / flat(p.pos, ahead).max(1.0);
        p.pitch += ((rise * 0.7).clamp(-0.45, 0.35) - 0.04 - p.pitch) * (dt * 2.5).min(1.0);
        let left: f32 = self.path[self.k..].windows(2).map(|s| s[0].distance(s[1])).sum::<f32>() + d;
        self.status = format!("{} · {left:.0} m", if t.picked { "Delivering" } else { "Collecting" });
        let input = Input { forward: if err.abs() < 0.7 { 1.0 } else { 0.25 }, run: left > 8.0 && err.abs() < 0.3, ..Default::default() };
        (input, Act::Walk)
    }

    fn plan(&mut self, w: &Arc<WalkWorld>, city: &Arc<City>, year: u16, from: Vec3, t: Target) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (w, city) = (w.clone(), city.clone());
        std::thread::spawn(move || {
            let _ = tx.send(route(&w, &city, year, from, t.stand));
        });
        self.pending = Some(rx);
        self.path.clear();
    }
}

/// Turn towards `yaw` at a comfortable rate; returns the remaining error.
fn turn_to(p: &mut Player, yaw: f32, dt: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let err = (yaw - p.yaw + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    let step = err.clamp(-3.5 * dt, 3.5 * dt);
    p.yaw += step;
    err - step
}

