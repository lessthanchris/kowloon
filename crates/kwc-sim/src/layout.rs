//! Ground plan: footprint raster, the Yamen, the lane network, plots, and each
//! plot's stair core + corridors.
//!
//! Grounding (RESEARCH.md): lanes "often only 1–2 m" wide; "no real entrances,
//! just narrow openings between shops"; Lung Chun Back Road one of the few lanes
//! running east–west across the city; Sai Shing ("West City") Road on the west;
//! Lo Yan Street entered from Tung Tau Tsuen Road on the north side.

use crate::city::*;
use crate::site::{point_in, Site, CELL_M, MAX_FLOORS};
use crate::{rng_for, Params};
use rand::seq::SliceRandom;
use rand::Rng;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

/// Minimum plot size in cells (~27 m²); smaller leftovers merge or become wells.
const MIN_PLOT: usize = 12;
/// How far (cells, through rooms) a room may sit from a corridor or stair.
const ROOM_REACH: u32 = 7;

pub fn build(params: &Params) -> City {
    let site = Site::load();
    let w = (site.width_m / CELL_M).ceil() as usize;
    let d = (site.depth_m / CELL_M).ceil() as usize;
    let mut city = City {
        params: params.clone(),
        w,
        d,
        ground: vec![Ground::Outside; w * d],
        plot_of: vec![NO_PLOT; w * d],
        role: vec![Role::None; w * d],
        corr: vec![0; w * d],
        plots: vec![],
        units: vec![],
        bridges: vec![],
        lanes: vec![],
        features: vec![],
        south_gate: (0, 0),
        ring_m: site.ring_m.clone(),
        yamen_m: site.yamen_m.clone(),
    };

    // Raster: a cell is inside if its centre is.
    for j in 0..d {
        for i in 0..w {
            let (x, y) = centre((i as u16, j as u16));
            if site.contains(x, y) {
                city.ground[j * w + i] = if point_in(&site.yamen_m, x, y) { Ground::Yamen } else { Ground::Plot };
            }
        }
    }
    // Keep a lane around the Yamen compound so it stays reachable.
    let yamen: Vec<Cell> = all_cells(&city).into_iter().filter(|&c| city.ground_at(c) == Ground::Yamen).collect();
    for c in yamen {
        for n in ring8(&city, c) {
            let k = city.idx(n);
            if city.ground[k] == Ground::Plot {
                city.ground[k] = Ground::Alley;
            }
        }
    }

    city.south_gate = nearest(&city, site.south_gate_m, |c| city.ground_at(c) == Ground::Plot && is_perimeter(&city, c));
    city.features.push(Feature { kind: FeatureKind::SouthGate, cell: city.south_gate, name: Some("South Gate".into()), plot: None });
    carve_lanes(&mut city);
    make_plots(&mut city);
    city
}

pub fn centre(c: Cell) -> (f32, f32) {
    ((c.0 as f32 + 0.5) * CELL_M, (c.1 as f32 + 0.5) * CELL_M)
}

fn all_cells(city: &City) -> Vec<Cell> {
    let mut v = Vec::with_capacity(city.w * city.d);
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            v.push((i, j));
        }
    }
    v
}

fn ring8(city: &City, c: Cell) -> Vec<Cell> {
    let mut v = vec![];
    for dj in -1..=1 {
        for di in -1..=1 {
            let (i, j) = (c.0 as i32 + di, c.1 as i32 + dj);
            if (di, dj) != (0, 0) && city.in_bounds(i, j) {
                v.push((i as u16, j as u16));
            }
        }
    }
    v
}

fn nearest(city: &City, (x, y): (f32, f32), ok: impl Fn(Cell) -> bool) -> Cell {
    all_cells(city)
        .into_iter()
        .filter(|&c| ok(c))
        .min_by(|&a, &b| {
            let (ax, ay) = centre(a);
            let (bx, by) = centre(b);
            ((ax - x).powi(2) + (ay - y).powi(2)).total_cmp(&((bx - x).powi(2) + (by - y).powi(2)))
        })
        .expect("no matching cell")
}

pub fn is_perimeter(city: &City, c: Cell) -> bool {
    Dir::ALL.iter().any(|&d| city.step(c, d).map_or(true, |n| city.ground_at(n) == Ground::Outside))
}

fn smooth_noise(city: &City, rng: &mut impl Rng) -> Vec<f32> {
    let mut v: Vec<f32> = (0..city.w * city.d).map(|_| rng.gen::<f32>()).collect();
    for _ in 0..5 {
        let old = v.clone();
        for j in 0..city.d {
            for i in 0..city.w {
                let mut s = 0.0;
                let mut c = 0.0;
                for (di, dj) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (a, b) = (i as i32 + di, j as i32 + dj);
                    if city.in_bounds(a, b) {
                        s += old[b as usize * city.w + a as usize];
                        c += 1.0;
                    }
                }
                v[j * city.w + i] = s / c;
            }
        }
    }
    // Stretch back to 0..1 (smoothing squashes it towards 0.5), then square it
    // so there are cheap valleys for lanes to wander along.
    let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    v.iter_mut().for_each(|x| *x = ((*x - lo) / (hi - lo)).powi(2));
    v
}

fn distance_to_alley(city: &City) -> Vec<u32> {
    let mut dist = vec![u32::MAX; city.w * city.d];
    let mut q = VecDeque::new();
    for k in 0..dist.len() {
        if city.ground[k] == Ground::Alley {
            dist[k] = 0;
            q.push_back(((k % city.w) as u16, (k / city.w) as u16));
        }
    }
    while let Some(c) = q.pop_front() {
        let dc = dist[city.idx(c)];
        for (_, n) in city.neighbours(c) {
            let k = city.idx(n);
            if city.ground[k] == Ground::Plot && dist[k] == u32::MAX {
                dist[k] = dc + 1;
                q.push_back(n);
            }
        }
    }
    dist
}

/// Noisy Dijkstra over land (and existing lanes, cheaply) from `from` to the
/// first cell satisfying `goal`. Penalises running close alongside a lane so
/// parallel lanes leave room for buildings between them.
fn noisy_path(city: &City, noise: &[f32], from: Cell, goal: &dyn Fn(Cell) -> bool) -> Option<Vec<Cell>> {
    let near = distance_to_alley(city);
    let n = city.w * city.d;
    let mut dist = vec![f32::MAX; n];
    let mut prev = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let key = |f: f32| Reverse((f * 1000.0) as u64);
    dist[city.idx(from)] = 0.0;
    heap.push((key(0.0), from));
    while let Some((Reverse(kd), c)) = heap.pop() {
        let ci = city.idx(c);
        if (kd as f32) / 1000.0 > dist[ci] + 0.01 {
            continue;
        }
        if c != from && goal(c) {
            let mut path = vec![c];
            let mut k = ci;
            while prev[k] != u32::MAX {
                k = prev[k] as usize;
                path.push(((k % city.w) as u16, (k / city.w) as u16));
            }
            path.reverse();
            return Some(path);
        }
        for (_, nb) in city.neighbours(c) {
            let ni = city.idx(nb);
            let cost = match city.ground[ni] {
                Ground::Alley => 0.4,
                Ground::Plot => {
                    let hug = match near[ni] {
                        1 | 2 => 3.0,
                        3 => 1.0,
                        _ => 0.0,
                    };
                    1.0 + noise[ni] * 6.0 + hug
                }
                _ => continue,
            };
            let nd = dist[ci] + cost;
            if nd < dist[ni] {
                dist[ni] = nd;
                prev[ni] = ci as u32;
                heap.push((key(nd), nb));
            }
        }
    }
    None
}

fn carve(city: &mut City, path: &[Cell]) {
    for &c in path {
        let k = city.idx(c);
        if city.ground[k] == Ground::Plot {
            city.ground[k] = Ground::Alley;
        }
    }
}

fn add_lane(city: &mut City, name: &str, path: Vec<Cell>) {
    carve(city, &path);
    city.lanes.push(Lane { name: name.to_string(), cells: path });
}

fn carve_lanes(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 1);
    let noise = smooth_noise(city, &mut rng);
    let perim: Vec<Cell> = all_cells(city).into_iter().filter(|&c| city.ground_at(c) == Ground::Plot && is_perimeter(city, c)).collect();
    let (min_i, max_i) = (perim.iter().map(|c| c.0).min().unwrap(), perim.iter().map(|c| c.0).max().unwrap());
    let (min_j, max_j) = (perim.iter().map(|c| c.1).min().unwrap(), perim.iter().map(|c| c.1).max().unwrap());
    let mid_j = (min_j + max_j) / 2;


    // Lung Chun Road: the old axis from the South Gate up to the Yamen.
    let gate = city.south_gate;
    if let Some(p) = path_to_alley(city, &noise, gate) {
        let mut p = p;
        p.insert(0, gate);
        add_lane(city, "Lung Chun Road", p);
    }

    // Lung Chun Back Road: one of the few lanes running right across, west to east.
    let pick = |rng: &mut rand_chacha::ChaCha8Rng, f: &dyn Fn(&Cell) -> bool| -> Cell {
        let c: Vec<Cell> = perim.iter().copied().filter(|c| f(c)).collect();
        c[rng.gen_range(0..c.len())]
    };
    let west = pick(&mut rng, &|c| c.0 < min_i + (max_i - min_i) / 8 && (c.1 as i32 - mid_j as i32).abs() < 12);
    let east = pick(&mut rng, &|c| c.0 > max_i - (max_i - min_i) / 8);
    if let Some(p) = noisy_path(city, &noise, west, &|c| c == east) {
        add_lane(city, "Lung Chun Back Road", p);
    }

    // Lo Yan Street: in from Tung Tau Tsuen Road on the north side.
    let north = pick(&mut rng, &|c| c.1 < min_j + (max_j - min_j) / 5 && c.0 > min_i + (max_i - min_i) / 3);
    if let Some(mut p) = path_to_alley(city, &noise, north) {
        p.insert(0, north);
        add_lane(city, "Lo Yan Street", p);
    }

    // Sai Shing (West City) Road: north–south through the west side.
    let wn = pick(&mut rng, &|c| c.0 < min_i + (max_i - min_i) / 3 && c.1 < mid_j);
    let ws = pick(&mut rng, &|c| c.0 < min_i + (max_i - min_i) / 3 && c.1 > mid_j);
    if let Some(p) = noisy_path(city, &noise, wn, &|c| c == ws) {
        add_lane(city, "Sai Shing Road", p);
    }

    // Other named lanes, then the many unnamed "narrow openings between shops".
    let mut openings: Vec<Cell> = perim.clone();
    openings.shuffle(&mut rng);
    let mut used: Vec<Cell> = city.lanes.iter().flat_map(|l| [l.cells[0], *l.cells.last().unwrap()]).collect();
    let named = ["Tai Chang Street", "Kwong Ming Street", "Shing Ngam Road", "Mung Chun Road", "Lung Shing Road"];
    let mut n_open = 0;
    for c in openings {
        if n_open >= named.len() + 14 {
            break;
        }
        if city.ground_at(c) != Ground::Plot || used.iter().any(|u| (u.0 as i32 - c.0 as i32).abs() + (u.1 as i32 - c.1 as i32).abs() < 16) {
            continue;
        }
        if let Some(mut p) = path_to_alley(city, &noise, c) {
            p.insert(0, c);
            let name = named.get(n_open).map_or("", |s| s);
            add_lane(city, name, p);
            used.push(c);
            n_open += 1;
        }
    }
    city.lanes.retain(|l| !l.name.is_empty());

    // Coverage: keep carving from the cell farthest from any lane until every
    // buildable cell is within `alley_spacing` of one.
    let spacing = city.params.alley_spacing.max(3);
    loop {
        let dist = distance_to_alley(city);
        let far: Vec<(u32, Cell)> = all_cells(city)
            .into_iter()
            .filter(|&c| city.ground_at(c) == Ground::Plot)
            .map(|c| (dist[city.idx(c)], c))
            .filter(|&(d, _)| d > spacing && d != u32::MAX)
            .collect();
        let Some(maxd) = far.iter().map(|f| f.0).max() else { break };
        let far: Vec<Cell> = far.into_iter().filter(|f| f.0 == maxd).map(|f| f.1).collect();
        let c = far[rng.gen_range(0..far.len())];
        match path_to_alley(city, &noise, c) {
            Some(path) => carve(city, &path),
            None => {
                let k = city.idx(c);
                city.ground[k] = Ground::Well;
            }
        }
    }
}

pub fn touches_alley(city: &City, c: Cell) -> bool {
    city.neighbours(c).any(|(_, n)| city.ground_at(n) == Ground::Alley)
}

fn component(city: &City, start: Cell, ok: impl Fn(Cell) -> bool) -> Vec<Cell> {
    let mut comp = vec![start];
    let mut seen = std::collections::HashSet::from([start]);
    let mut q = VecDeque::from([start]);
    while let Some(x) = q.pop_front() {
        for (_, n) in city.neighbours(x) {
            if ok(n) && seen.insert(n) {
                comp.push(n);
                q.push_back(n);
            }
        }
    }
    comp
}

fn make_plots(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 2);
    let land: Vec<Cell> = all_cells(city).into_iter().filter(|&c| city.ground_at(c) == Ground::Plot).collect();

    // Seeds along the lanes, a few cells apart.
    let mut cand: Vec<Cell> = land.iter().copied().filter(|&c| touches_alley(city, c)).collect();
    cand.shuffle(&mut rng);
    let mut seeds: Vec<Cell> = vec![];
    for c in cand {
        if seeds.iter().all(|s| (s.0 as i32 - c.0 as i32).abs().max((s.1 as i32 - c.1 as i32).abs()) > 2) {
            seeds.push(c);
        }
    }

    // Random multi-source region growth, each plot capped at its own size.
    let mut plot_cells: Vec<Vec<Cell>> = vec![];
    let mut caps: Vec<usize> = vec![];
    let mut frontier: Vec<(u32, Cell)> = vec![];
    for &s in &seeds {
        let id = plot_cells.len() as u32;
        let k = city.idx(s);
        city.plot_of[k] = id;
        plot_cells.push(vec![s]);
        caps.push(rng.gen_range(10..=30));
        for (_, n) in city.neighbours(s) {
            frontier.push((id, n));
        }
    }
    while !frontier.is_empty() {
        let (p, c) = frontier.swap_remove(rng.gen_range(0..frontier.len()));
        let k = city.idx(c);
        if city.ground[k] != Ground::Plot || city.plot_of[k] != NO_PLOT || plot_cells[p as usize].len() >= caps[p as usize] {
            continue;
        }
        // Keep plots compact: defer cells that would only hang off one side.
        let same = city.neighbours(c).filter(|(_, n)| city.plot_of[city.idx(*n)] == p).count();
        if same < 2 && plot_cells[p as usize].len() > 2 && rng.gen::<f32>() < 0.85 {
            frontier.push((p, c));
            continue;
        }
        city.plot_of[k] = p;
        plot_cells[p as usize].push(c);
        for (_, n) in city.neighbours(c) {
            frontier.push((p, n));
        }
    }

    // Leftover land: big enough and on a lane → own plot; otherwise merge into a
    // neighbouring plot, or (rarely, or if landlocked) stay open as a light well.
    for &c in &land {
        let k = city.idx(c);
        if city.plot_of[k] != NO_PLOT || city.ground[k] != Ground::Plot {
            continue;
        }
        let comp = component(city, c, |n| city.ground_at(n) == Ground::Plot && city.plot_of[city.idx(n)] == NO_PLOT);
        let target = if comp.len() >= MIN_PLOT && comp.iter().any(|&x| touches_alley(city, x)) {
            plot_cells.push(vec![]);
            Some(plot_cells.len() as u32 - 1)
        } else if rng.gen::<f32>() < city.params.well_tolerance {
            None
        } else {
            comp.iter().find_map(|&x| city.neighbours(x).find_map(|(_, n)| {
                let p = city.plot_of[city.idx(n)];
                (p != NO_PLOT).then_some(p)
            }))
        };
        for &x in &comp {
            let k = city.idx(x);
            match target {
                Some(p) => {
                    city.plot_of[k] = p;
                    plot_cells[p as usize].push(x);
                }
                None => city.ground[k] = Ground::Well,
            }
        }
    }

    // Plots too small to hold a stair and a room: fold into a neighbour or well.
    for p in 0..plot_cells.len() {
        if plot_cells[p].is_empty() || plot_cells[p].len() >= MIN_PLOT {
            continue;
        }
        let cells = std::mem::take(&mut plot_cells[p]);
        let nb = cells.iter().find_map(|&x| {
            city.neighbours(x).find_map(|(_, n)| {
                let q = city.plot_of[city.idx(n)];
                (q != NO_PLOT && q as usize != p && !plot_cells[q as usize].is_empty()).then_some(q)
            })
        });
        for x in cells {
            let k = city.idx(x);
            match nb {
                Some(q) => {
                    city.plot_of[k] = q;
                    plot_cells[q as usize].push(x);
                }
                None => {
                    city.plot_of[k] = NO_PLOT;
                    city.ground[k] = Ground::Well;
                }
            }
        }
    }

    // Renumber and build Plot records with core + corridors.
    let mut remap = vec![NO_PLOT; plot_cells.len()];
    for (p, cells) in plot_cells.into_iter().enumerate() {
        if cells.is_empty() {
            continue;
        }
        let id = city.plots.len() as u32;
        remap[p] = id;
        city.plots.push(Plot { id, cells, core: vec![], founded: 0, rebuilt: 0, floor_year: [0; MAX_FLOORS], ambition: 0 });
    }
    for k in 0..city.plot_of.len() {
        let p = city.plot_of[k];
        if p != NO_PLOT {
            city.plot_of[k] = remap[p as usize];
        }
    }
    // Circulation. A plot too thin to fit a stair without cutting itself in two
    // is folded into a neighbour, which is then laid out again.
    let mut queue: VecDeque<usize> = (0..city.plots.len()).collect();
    while let Some(p) = queue.pop_front() {
        if city.plots[p].cells.is_empty() || lay_out_circulation(city, p, &mut rng) {
            continue;
        }
        let cells = std::mem::take(&mut city.plots[p].cells);
        let nb = cells.iter().find_map(|&x| {
            city.neighbours(x).find_map(|(_, n)| {
                let q = city.plot_of[city.idx(n)];
                (q != NO_PLOT && q as usize != p).then_some(q as usize)
            })
        });
        for &x in &cells {
            let k = city.idx(x);
            match nb {
                Some(q) => city.plot_of[k] = q as u32,
                None => {
                    city.plot_of[k] = NO_PLOT;
                    city.ground[k] = Ground::Well;
                    city.role[k] = Role::None;
                }
            }
        }
        if let Some(q) = nb {
            city.plots[q].cells.extend(cells);
            queue.push_back(q);
        }
    }
    // Drop emptied plots and renumber.
    let mut remap = vec![NO_PLOT; city.plots.len()];
    let old = std::mem::take(&mut city.plots);
    for mut plot in old {
        if plot.cells.is_empty() {
            continue;
        }
        remap[plot.id as usize] = city.plots.len() as u32;
        plot.id = city.plots.len() as u32;
        city.plots.push(plot);
    }
    for k in 0..city.plot_of.len() {
        let p = city.plot_of[k];
        if p != NO_PLOT {
            city.plot_of[k] = remap[p as usize];
        }
    }
}

/// BFS within one plot from `from`, never entering `block` (the stair shaft).
fn bfs_in_plot(city: &City, pid: u32, from: &[Cell], block: &[Cell]) -> (HashMap<Cell, u32>, HashMap<Cell, Cell>) {
    let mut depth: HashMap<Cell, u32> = from.iter().map(|&c| (c, 0)).collect();
    let mut parent = HashMap::new();
    let mut q: VecDeque<Cell> = from.iter().copied().collect();
    while let Some(c) = q.pop_front() {
        for (_, n) in city.neighbours(c) {
            if city.plot_of[city.idx(n)] == pid && !block.contains(&n) && !depth.contains_key(&n) {
                depth.insert(n, depth[&c] + 1);
                parent.insert(n, c);
                q.push_back(n);
            }
        }
    }
    (depth, parent)
}

/// Stair core: a landing cell opening onto a lane (doors and corridors attach
/// here) and a stair cell beside it holding a steep dog-leg stair (two 0.75 m
/// half-flights side by side). Placed to minimise walking depth. Then corridors
/// so no room is more than ROOM_REACH cells from circulation.
fn lay_out_circulation(city: &mut City, p: usize, rng: &mut impl Rng) -> bool {
    let cells = city.plots[p].cells.clone();
    let pid = p as u32;

    // The stair is a 2 x 2 cell well (3 x 3 m) with a landing two cells wide in
    // front of it (like a real dog-leg): flight A climbs away from the landing,
    // flight B comes back to it one storey up.
    let mut best: Option<(u32, Cell, [Cell; 4], Cell)> = None;
    let mut cands: Vec<Cell> = cells.iter().copied().filter(|&c| touches_alley(city, c)).collect();
    cands.shuffle(rng);
    for (tried, &c) in cands.iter().enumerate() {
        if tried >= 16 && best.is_some() {
            break;
        }
        for (d, na) in city.neighbours(c) {
            for side in [Dir::N, Dir::E, Dir::S, Dir::W] {
                if side == d || side == opposite(d) {
                    continue;
                }
                let (Some(fa), Some(nb), Some(l2)) = (city.step(na, d), city.step(na, side), city.step(c, side)) else { continue };
                let Some(fb) = city.step(nb, d) else { continue };
                let block = [na, fa, nb, fb];
                if block.iter().chain([&l2]).any(|&x| city.plot_of[city.idx(x)] != pid || x == c) {
                    continue;
                }
                let (depth, _) = bfs_in_plot(city, pid, &[c], &block);
                if depth.len() + 4 < cells.len() || !depth.contains_key(&l2) {
                    continue; // the stairwell would cut the plot in two
                }
                let m = *depth.values().max().unwrap();
                if best.map_or(true, |b| m < b.0) {
                    best = Some((m, c, block, l2));
                }
            }
        }
    }
    let Some((_, landing, block, landing2)) = best else { return false };

    for &c in &cells {
        let k = city.idx(c);
        city.role[k] = Role::Room;
        city.corr[k] = 0;
    }
    for l in [landing, landing2] {
        let k = city.idx(l);
        city.role[k] = Role::Core;
    }
    for x in block {
        let k = city.idx(x);
        city.role[k] = Role::Stair;
    }
    city.plots[p].core = vec![landing, block[0], block[1], block[2], block[3], landing2];

    // Every floor gets its own corridors: buildings were fitted out (and
    // re-partitioned) floor by floor, so no two storeys need look the same.
    for f in 0..MAX_FLOORS as u8 {
        lay_out_floor(city, pid, &cells, landing, f, rng);
    }
    true
}

/// Corridors for one floor: walk outward from the landing; a room too far from
/// circulation extends a corridor towards it along a (per-floor random) BFS
/// tree, stopping once it touches existing circulation. Then a few dead-end
/// stubs wander off, as corridors in the Walled City did.
fn lay_out_floor(city: &mut City, pid: u32, cells: &[Cell], landing: Cell, f: u8, rng: &mut impl Rng) {
    // Randomised BFS tree from the landing.
    let mut depth: HashMap<Cell, u32> = HashMap::from([(landing, 0)]);
    let mut parent: HashMap<Cell, Cell> = HashMap::new();
    let mut q = VecDeque::from([landing]);
    while let Some(c) = q.pop_front() {
        let mut ns: Vec<Cell> = city.neighbours(c).map(|x| x.1).collect();
        ns.shuffle(rng);
        for n in ns {
            if city.plot_of[city.idx(n)] == pid && city.role_at(n) != Role::Stair && !depth.contains_key(&n) {
                depth.insert(n, depth[&c] + 1);
                parent.insert(n, c);
                q.push_back(n);
            }
        }
    }
    let circ = |city: &City, x: Cell| city.circ_at(x, pid, f);
    let room = |city: &City, x: Cell| city.room_at(x, pid, f);
    // Room-distance from `c` to a room touching circulation (None if > reach).
    let reach_limit = rng.gen_range(5..=ROOM_REACH);
    let reach = |city: &City, c: Cell| -> Option<u32> {
        let mut seen = HashMap::from([(c, 0u32)]);
        let mut q = VecDeque::from([c]);
        while let Some(x) = q.pop_front() {
            let dx = seen[&x];
            if city.neighbours(x).any(|(_, n)| circ(city, n)) {
                return Some(dx);
            }
            if dx >= reach_limit {
                continue;
            }
            for (_, n) in city.neighbours(x) {
                if room(city, n) && !seen.contains_key(&n) {
                    seen.insert(n, dx + 1);
                    q.push_back(n);
                }
            }
        }
        None
    };

    let mut by_depth: Vec<Cell> = cells.iter().copied().filter(|c| depth.contains_key(c) && *c != landing).collect();
    by_depth.shuffle(rng);
    by_depth.sort_by_key(|c| depth[c]);
    for &c in &by_depth {
        if !room(city, c) || reach(city, c).is_some() {
            continue;
        }
        let mut prev = c;
        let mut x = parent[&c];
        loop {
            if !room(city, x) {
                break;
            }
            // Touching circulation *other than* the corridor this chain just laid.
            let touching = city.neighbours(x).any(|(_, n)| n != prev && circ(city, n));
            city.set_corridor(x, f);
            if touching {
                break;
            }
            prev = x;
            x = parent[&x];
        }
    }

    // Dead-end stubs: a corridor pushes a couple of cells further into the rooms.
    if f > 0 && rng.gen::<f32>() < 0.45 {
        let starts: Vec<Cell> = cells.iter().copied().filter(|&c| circ(city, c)).collect();
        if let Some(&s) = starts.choose(rng) {
            let mut x = s;
            for _ in 0..rng.gen_range(1..=3) {
                let next: Vec<Cell> = city.neighbours(x).map(|n| n.1).filter(|&n| room(city, n)).collect();
                let Some(&n) = next.choose(rng) else { break };
                city.set_corridor(n, f);
                x = n;
            }
        }
    }
}

fn path_to_alley(city: &City, noise: &[f32], from: Cell) -> Option<Vec<Cell>> {
    noisy_path(city, noise, from, &|c| city.ground_at(c) == Ground::Alley)
}

fn opposite(d: Dir) -> Dir {
    match d {
        Dir::N => Dir::S,
        Dir::S => Dir::N,
        Dir::E => Dir::W,
        Dir::W => Dir::E,
    }
}
