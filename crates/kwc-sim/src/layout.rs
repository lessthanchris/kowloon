//! Ground plan: footprint raster, the Yamen, the alley network, plots, and each
//! plot's stair core + corridors.

use crate::city::*;
use crate::site::{Site, CELL_M, MAX_FLOORS};
use crate::{rng_for, Params};
use rand::seq::SliceRandom;
use rand::Rng;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

const LANE_NAMES: &[&str] = &[
    "Lung Chun Road",
    "Tai Chang Street",
    "Lo Yan Street",
    "Kwong Ming Street",
    "Sai Shing Road",
    "Tung Tsing Road",
    "Lung Shing Road",
    "Tin Hau Temple Lane",
];

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
        plots: vec![],
        units: vec![],
        bridges: vec![],
        lanes: vec![],
        south_gate: (0, 0),
        ring_m: site.ring_m.clone(),
    };

    // Raster: a cell is inside if its centre is.
    for j in 0..d {
        for i in 0..w {
            let (x, y) = ((i as f32 + 0.5) * CELL_M, (j as f32 + 0.5) * CELL_M);
            if site.contains(x, y) {
                city.ground[j * w + i] = Ground::Plot;
            }
        }
    }

    place_yamen(&mut city);
    city.south_gate = nearest_inside(&city, site.south_gate_m);
    carve_alleys(&mut city);
    make_plots(&mut city);
    city
}

fn inside_cells(city: &City) -> Vec<Cell> {
    let mut v = vec![];
    for j in 0..city.d {
        for i in 0..city.w {
            if city.ground[j * city.w + i] != Ground::Outside {
                v.push((i as u16, j as u16));
            }
        }
    }
    v
}

fn nearest_inside(city: &City, (x, y): (f32, f32)) -> Cell {
    *inside_cells(city)
        .iter()
        .min_by(|a, b| {
            let da = ((a.0 as f32 + 0.5) * CELL_M - x).powi(2) + ((a.1 as f32 + 0.5) * CELL_M - y).powi(2);
            let db = ((b.0 as f32 + 0.5) * CELL_M - x).powi(2) + ((b.1 as f32 + 0.5) * CELL_M - y).powi(2);
            da.total_cmp(&db)
        })
        .unwrap()
}

fn is_perimeter(city: &City, c: Cell) -> bool {
    Dir::ALL.iter().any(|&d| match city.step(c, d) {
        None => true,
        Some(n) => city.ground_at(n) == Ground::Outside,
    })
}

/// The Yamen compound near the middle, ringed by a lane so it stays reachable.
fn place_yamen(city: &mut City) {
    let cells = inside_cells(city);
    let n = cells.len() as f32;
    let ci = cells.iter().map(|c| c.0 as f32).sum::<f32>() / n;
    let cj = cells.iter().map(|c| c.1 as f32).sum::<f32>() / n;
    let (hw, hd) = (5i32, 4i32); // ~30 m x 24 m
    for dj in -hd - 1..=hd {
        for di in -hw - 1..=hw {
            let (i, j) = (ci as i32 + di, cj as i32 + dj);
            if !city.in_bounds(i, j) {
                continue;
            }
            let c = (i as u16, j as u16);
            if city.ground_at(c) == Ground::Outside {
                continue;
            }
            let ring = di == -hw - 1 || di == hw || dj == -hd - 1 || dj == hd;
            let k = city.idx(c);
            city.ground[k] = if ring { Ground::Alley } else { Ground::Yamen };
        }
    }
}

/// Dijkstra from `from` until any alley cell is reached, over buildable land.
/// Costs are noisy (so lanes wander) and penalise hugging existing lanes.
fn noisy_path(city: &City, noise: &[f32], from: Cell) -> Option<Vec<Cell>> {
    let n = city.w * city.d;
    let mut dist = vec![f32::MAX; n];
    let mut prev = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let key = |f: f32| Reverse((f * 1000.0) as u64);
    dist[city.idx(from)] = 0.0;
    heap.push((key(0.0), from));
    while let Some((Reverse(_), c)) = heap.pop() {
        let ci = city.idx(c);
        if city.ground[ci] == Ground::Alley && c != from {
            let mut path = vec![c];
            let mut k = ci;
            while prev[k] != u32::MAX {
                k = prev[k] as usize;
                path.push(((k % city.w) as u16, (k / city.w) as u16));
            }
            return Some(path);
        }
        for (_, nb) in city.neighbours(c) {
            let g = city.ground_at(nb);
            if g != Ground::Plot && g != Ground::Alley {
                continue;
            }
            let hugging = g == Ground::Plot
                && city.neighbours(nb).any(|(_, x)| city.ground_at(x) == Ground::Alley)
                && !city.neighbours(nb).any(|(_, x)| x == c);
            let cost = 1.0 + noise[city.idx(nb)] * 7.0 + if hugging { 2.0 } else { 0.0 };
            let nd = dist[ci] + cost;
            let ni = city.idx(nb);
            if nd < dist[ni] {
                dist[ni] = nd;
                prev[ni] = ci as u32;
                heap.push((key(nd), nb));
            }
        }
    }
    None
}

fn smooth_noise(city: &City, rng: &mut impl Rng) -> Vec<f32> {
    let mut v: Vec<f32> = (0..city.w * city.d).map(|_| rng.gen::<f32>()).collect();
    for _ in 0..3 {
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

fn carve(city: &mut City, path: &[Cell]) {
    for &c in path {
        let k = city.idx(c);
        if city.ground[k] == Ground::Plot {
            city.ground[k] = Ground::Alley;
        }
    }
}

fn carve_alleys(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 1);
    let noise = smooth_noise(city, &mut rng);

    // Entrances: the South Gate plus a spread of perimeter openings.
    let mut perim: Vec<Cell> = inside_cells(city)
        .into_iter()
        .filter(|&c| city.ground_at(c) == Ground::Plot && is_perimeter(city, c))
        .collect();
    perim.shuffle(&mut rng);
    let mut entrances = vec![city.south_gate];
    for c in perim {
        if entrances.len() >= 9 {
            break;
        }
        if entrances.iter().all(|e| (e.0 as i32 - c.0 as i32).abs() + (e.1 as i32 - c.1 as i32).abs() > 18) {
            entrances.push(c);
        }
    }

    // Trunk lanes: each entrance to the network (the Yamen ring first).
    for (n, &e) in entrances.iter().enumerate() {
        let k = city.idx(e);
        city.ground[k] = Ground::Plot; // start from land so noisy_path walks inward
        if let Some(path) = noisy_path(city, &noise, e) {
            carve(city, &path);
            let name = LANE_NAMES[n % LANE_NAMES.len()].to_string();
            city.lanes.push(Lane { name, cells: path });
        }
        let k = city.idx(e);
        city.ground[k] = Ground::Alley;
    }

    // Coverage: keep carving from the cell farthest from any lane until every
    // buildable cell is within `alley_spacing` of one.
    let spacing = city.params.alley_spacing.max(2);
    loop {
        let dist = distance_to_alley(city);
        let mut far: Vec<(u32, Cell)> = inside_cells(city)
            .into_iter()
            .filter(|&c| city.ground_at(c) == Ground::Plot)
            .map(|c| (dist[city.idx(c)], c))
            .filter(|&(d, _)| d > spacing && d != u32::MAX)
            .collect();
        if far.is_empty() {
            break;
        }
        let maxd = far.iter().map(|f| f.0).max().unwrap();
        far.retain(|f| f.0 == maxd);
        let (_, c) = far[rng.gen_range(0..far.len())];
        match noisy_path(city, &noise, c) {
            Some(path) => carve(city, &path),
            None => {
                let k = city.idx(c);
                city.ground[k] = Ground::Well;
            }
        }
    }
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

fn touches_alley(city: &City, c: Cell) -> bool {
    city.neighbours(c).any(|(_, n)| city.ground_at(n) == Ground::Alley)
}

fn make_plots(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 2);
    let land: Vec<Cell> = inside_cells(city).into_iter().filter(|&c| city.ground_at(c) == Ground::Plot).collect();

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
        caps.push(rng.gen_range(6..=14));
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
        if same < 2 && plot_cells[p as usize].len() > 2 && rng.gen::<f32>() < 0.8 {
            frontier.push((p, c));
            continue;
        }
        city.plot_of[k] = p;
        plot_cells[p as usize].push(c);
        for (_, n) in city.neighbours(c) {
            frontier.push((p, n));
        }
    }

    // Leftover land: components touching a lane become their own plot, others wells.
    for &c in &land {
        if city.plot_of[city.idx(c)] != NO_PLOT || city.ground_at(c) != Ground::Plot {
            continue;
        }
        let mut comp = vec![c];
        let mut q = VecDeque::from([c]);
        let mut seen = std::collections::HashSet::from([c]);
        while let Some(x) = q.pop_front() {
            for (_, n) in city.neighbours(x) {
                let k = city.idx(n);
                if city.ground[k] == Ground::Plot && city.plot_of[k] == NO_PLOT && seen.insert(n) {
                    comp.push(n);
                    q.push_back(n);
                }
            }
        }
        if comp.len() >= 3 && comp.iter().any(|&x| touches_alley(city, x)) {
            let id = plot_cells.len() as u32;
            for &x in &comp {
                let k = city.idx(x);
                city.plot_of[k] = id;
            }
            plot_cells.push(comp);
        } else {
            for &x in &comp {
                let k = city.idx(x);
                city.ground[k] = Ground::Well;
                city.plot_of[k] = u32::MAX - 1; // mark visited
            }
        }
    }
    for k in 0..city.plot_of.len() {
        if city.plot_of[k] == u32::MAX - 1 {
            city.plot_of[k] = NO_PLOT;
        }
    }

    // Tiny plots become light wells (some always; small ones by tolerance).
    let mut keep = vec![true; plot_cells.len()];
    for (p, cells) in plot_cells.iter().enumerate() {
        if cells.len() < 3 || (cells.len() <= 4 && rng.gen::<f32>() < city.params.well_tolerance) {
            keep[p] = false;
        }
    }

    // Renumber and build Plot records with core + corridors.
    let mut remap = vec![NO_PLOT; plot_cells.len()];
    for (p, cells) in plot_cells.into_iter().enumerate() {
        if !keep[p] {
            for c in cells {
                let k = city.idx(c);
                city.ground[k] = Ground::Well;
                city.plot_of[k] = NO_PLOT;
            }
            continue;
        }
        let id = city.plots.len() as u32;
        remap[p] = id;
        city.plots.push(Plot {
            id,
            cells,
            core: (0, 0),
            founded: 0,
            rebuilt: 0,
            floor_year: [0; MAX_FLOORS],
            ambition: 0,
        });
    }
    for k in 0..city.plot_of.len() {
        let p = city.plot_of[k];
        if p != NO_PLOT {
            city.plot_of[k] = remap[p as usize];
        }
    }
    for p in 0..city.plots.len() {
        lay_out_circulation(city, p, &mut rng);
    }
}

/// Choose the stair core (touching a lane, minimising walking depth) and mark the
/// corridor cells every room needs to reach it.
fn lay_out_circulation(city: &mut City, p: usize, rng: &mut impl Rng) {
    let cells = city.plots[p].cells.clone();
    let pid = p as u32;
    let bfs = |city: &City, from: Cell, shuffle: &mut dyn FnMut(&mut Vec<Cell>)| {
        let mut depth = std::collections::HashMap::from([(from, 0u32)]);
        let mut parent = std::collections::HashMap::new();
        let mut q = VecDeque::from([from]);
        while let Some(c) = q.pop_front() {
            let mut ns: Vec<Cell> = city.neighbours(c).map(|x| x.1).collect();
            shuffle(&mut ns);
            for n in ns {
                if city.plot_of[city.idx(n)] == pid && !depth.contains_key(&n) {
                    depth.insert(n, depth[&c] + 1);
                    parent.insert(n, c);
                    q.push_back(n);
                }
            }
        }
        (depth, parent)
    };

    let mut best: Option<(u32, Cell)> = None;
    let mut cands: Vec<Cell> = cells.iter().copied().filter(|&c| touches_alley(city, c)).collect();
    cands.shuffle(rng);
    for c in cands {
        let (depth, _) = bfs(city, c, &mut |_| {});
        let m = *depth.values().max().unwrap();
        if best.map_or(true, |b| m < b.0) {
            best = Some((m, c));
        }
    }
    let core = best.map(|b| b.1).unwrap_or(cells[0]);
    let (depth, _) = bfs(city, core, &mut |_| {});

    for &c in &cells {
        let k = city.idx(c);
        city.role[k] = Role::Room;
    }
    let k = city.idx(core);
    city.role[k] = Role::Core;

    // Minimal-ish corridor: walk cells outward by depth; a room not already beside
    // the core or a corridor turns one shallower neighbour into corridor. That
    // neighbour was itself satisfied earlier, so corridors always chain to the core.
    let mut by_depth: Vec<Cell> = cells.clone();
    by_depth.shuffle(rng);
    by_depth.sort_by_key(|c| depth[c]);
    for c in by_depth {
        let dc = depth[&c];
        if dc < 2 {
            continue;
        }
        // Served = touches circulation, or sits behind a room that does (a flat can
        // be two rooms deep with one front door).
        let circ = |x: Cell| city.plot_of[city.idx(x)] == pid && matches!(city.role_at(x), Role::Core | Role::Corridor);
        let front = |x: Cell| {
            city.plot_of[city.idx(x)] == pid && city.role_at(x) == Role::Room && city.neighbours(x).any(|(_, y)| circ(y))
        };
        let served = city.neighbours(c).any(|(_, n)| circ(n) || front(n));
        if served {
            continue;
        }
        let ups: Vec<Cell> = city
            .neighbours(c)
            .map(|x| x.1)
            .filter(|n| city.plot_of[city.idx(*n)] == pid && depth.get(n) == Some(&(dc - 1)))
            .collect();
        // `pick` was served earlier: either it touches circulation, or it sits behind
        // a front room that does. Promote whatever keeps the chain to the core intact.
        let pick = *ups.choose(rng).unwrap();
        let mut promote = vec![pick];
        if !city.neighbours(pick).any(|(_, n)| circ(n)) {
            let via = city.neighbours(pick).map(|x| x.1).find(|&n| front(n)).expect("served cell has a front room");
            promote.push(via);
        }
        for x in promote {
            let k = city.idx(x);
            city.role[k] = Role::Corridor;
        }
    }
    city.plots[p].core = core;
}
