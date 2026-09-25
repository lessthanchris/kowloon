//! Bridges and knocked-through links between neighbouring buildings. Residents
//! could famously cross the city without touching the ground; this is that.

use crate::city::*;
use crate::rng_for;
use crate::site::END_YEAR;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashSet;

fn walkable(city: &City, c: Cell) -> Option<u32> {
    let k = city.idx(c);
    (city.plot_of[k] != NO_PLOT && matches!(city.role[k], Role::Core | Role::Corridor)).then_some(city.plot_of[k])
}

pub fn add_bridges(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 4);
    let mut cands: Vec<(Cell, Cell, Vec<Cell>)> = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let a = (i, j);
            let Some(pa) = walkable(city, a) else { continue };
            for d in [Dir::E, Dir::S] {
                // Direct: circulation cells of two buildings side by side.
                if let Some(b) = city.step(a, d) {
                    if let Some(pb) = walkable(city, b) {
                        if pb != pa {
                            cands.push((a, b, vec![]));
                        }
                    }
                    // Across a one-cell lane.
                    if city.ground_at(b) == Ground::Alley {
                        if let Some(c) = city.step(b, d) {
                            if let Some(pc) = walkable(city, c) {
                                if pc != pa {
                                    cands.push((a, c, vec![b]));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    cands.shuffle(&mut rng);

    let mut pairs = HashSet::new();
    let mut bridges = vec![];
    for (a, b, span) in cands {
        let (pa, pb) = (city.plot_of[city.idx(a)], city.plot_of[city.idx(b)]);
        let key = (pa.min(pb), pa.max(pb));
        let p = if span.is_empty() { 0.7 } else { 0.6 };
        if pairs.contains(&key) || rng.gen::<f32>() > p {
            continue;
        }
        let (ha, hb) = (city.plots[pa as usize].final_height(), city.plots[pb as usize].final_height());
        let top = ha.min(hb);
        if top < 3 {
            continue;
        }
        let n = rng.gen_range(1..=3);
        for _ in 0..n {
            let floor = rng.gen_range(1..top - 1);
            let ya = city.plots[pa as usize].floor_year[floor as usize];
            let yb = city.plots[pb as usize].floor_year[floor as usize];
            let year = (ya.max(yb) + rng.gen_range(0..4)).min(END_YEAR);
            bridges.push(Bridge { floor, a, b, span: span.clone(), year });
        }
        pairs.insert(key);
    }
    city.bridges = bridges;
}
