//! Links between buildings. Residents could cross the city without touching the
//! ground: corridors knocked through party walls into the next building, and
//! bridges thrown across lanes. This is what makes it a labyrinth.

use crate::city::*;
use crate::rng_for;
use crate::site::END_YEAR;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::{HashMap, HashSet, VecDeque};

/// Chance, per pair of neighbouring buildings and per shared upper floor, that
/// a passage was knocked through between them.
const PASSAGE_P: f32 = 0.09;

fn year_link(city: &City, pa: u32, pb: u32, floor: u8, rng: &mut impl Rng) -> u16 {
    let ya = city.plots[pa as usize].floor_year[floor as usize];
    let yb = city.plots[pb as usize].floor_year[floor as usize];
    (ya.max(yb) + rng.gen_range(0..4)).min(END_YEAR)
}

/// Turn a path of rooms into corridor on `floor`, from `from` to the nearest
/// circulation of the same plot. Returns false if there's no way through.
fn corridor_to_circulation(city: &mut City, from: Cell, pid: u32, floor: u8) -> bool {
    if city.circ_at(from, pid, floor) {
        return true;
    }
    let mut prev: HashMap<Cell, Cell> = HashMap::new();
    let mut q = VecDeque::from([from]);
    let mut seen = HashSet::from([from]);
    let mut end = None;
    while let Some(c) = q.pop_front() {
        if city.circ_at(c, pid, floor) {
            end = Some(c);
            break;
        }
        for (_, n) in city.neighbours(c) {
            let k = city.idx(n);
            if city.plot_of[k] == pid && city.role[k] != Role::Stair && seen.insert(n) {
                prev.insert(n, c);
                q.push_back(n);
            }
        }
    }
    let Some(mut c) = end else { return false };
    while let Some(&p) = prev.get(&c) {
        if city.role_at(p) == Role::Room {
            city.set_corridor(p, floor);
        }
        c = p;
    }
    true
}

/// Knock through party walls. Run after growth (needs heights) and before
/// units (it eats into rooms).
pub fn add_passages(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 6);
    // Neighbouring cell pairs across each party wall, grouped by plot pair.
    let mut walls: HashMap<(u32, u32), Vec<(Cell, Cell)>> = HashMap::new();
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let a = (i, j);
            let pa = city.plot_of[city.idx(a)];
            if pa == NO_PLOT || city.role_at(a) == Role::Stair {
                continue;
            }
            for d in [Dir::E, Dir::S] {
                let Some(b) = city.step(a, d) else { continue };
                let pb = city.plot_of[city.idx(b)];
                if pb == NO_PLOT || pb == pa || city.role_at(b) == Role::Stair {
                    continue;
                }
                let key = (pa.min(pb), pa.max(pb));
                let pair = if pa < pb { (a, b) } else { (b, a) };
                walls.entry(key).or_default().push(pair);
            }
        }
    }
    let mut keys: Vec<(u32, u32)> = walls.keys().copied().collect();
    keys.sort_unstable();
    let mut links = vec![];
    for key in keys {
        let top = city.plots[key.0 as usize].final_height().min(city.plots[key.1 as usize].final_height());
        let cands = &walls[&key];
        for floor in 1..top {
            if rng.gen::<f32>() >= PASSAGE_P {
                continue;
            }
            let &(a, b) = cands.choose(&mut rng).unwrap();
            if corridor_to_circulation(city, a, key.0, floor) && corridor_to_circulation(city, b, key.1, floor) {
                if city.role_at(a) == Role::Room {
                    city.set_corridor(a, floor);
                }
                if city.role_at(b) == Role::Room {
                    city.set_corridor(b, floor);
                }
                let year = year_link(city, key.0, key.1, floor, &mut rng);
                links.push(Bridge { floor, a, b, span: vec![], year });
            }
        }
    }
    city.bridges = links;
}

/// Bridges across one-cell lanes, between circulation on the same floor.
pub fn add_bridges(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 4);
    let mut cands: Vec<(Cell, Cell, Cell)> = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let a = (i, j);
            let pa = city.plot_of[city.idx(a)];
            if pa == NO_PLOT || city.role_at(a) == Role::Stair {
                continue;
            }
            for d in [Dir::E, Dir::S] {
                let Some(m) = city.step(a, d) else { continue };
                if city.ground_at(m) != Ground::Alley {
                    continue;
                }
                let Some(b) = city.step(m, d) else { continue };
                let pb = city.plot_of[city.idx(b)];
                if pb != NO_PLOT && pb != pa && city.role_at(b) != Role::Stair {
                    cands.push((a, m, b));
                }
            }
        }
    }
    cands.shuffle(&mut rng);
    let mut pairs = HashSet::new();
    for (a, m, b) in cands {
        let (pa, pb) = (city.plot_of[city.idx(a)], city.plot_of[city.idx(b)]);
        let key = (pa.min(pb), pa.max(pb));
        if pairs.contains(&key) || rng.gen::<f32>() > 0.5 {
            continue;
        }
        let top = city.plots[pa as usize].final_height().min(city.plots[pb as usize].final_height());
        if top < 3 {
            continue;
        }
        // Prefer floors where both ends are already circulation; otherwise carve.
        let floor = rng.gen_range(1..top - 1);
        if corridor_to_circulation(city, a, pa, floor) && corridor_to_circulation(city, b, pb, floor) {
            for (c, p) in [(a, pa), (b, pb)] {
                if city.role_at(c) == Role::Room && !city.circ_at(c, p, floor) {
                    city.set_corridor(c, floor);
                }
            }
            let year = year_link(city, pa, pb, floor, &mut rng);
            city.bridges.push(Bridge { floor, a, b, span: vec![m], year });
            pairs.insert(key);
        }
    }
}
