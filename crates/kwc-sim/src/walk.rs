//! The walkable graph: lanes, stair cores, corridors, bridges and rooftops.
//! Used to prove every unit can be reached from the street, and later by the
//! engine for spawn points / navigation.

use crate::city::*;
use std::collections::{HashSet, VecDeque};

/// A place you can stand: a cell at a level. Level 0 is the ground; level `f` is
/// floor `f`; level `h` on a building of height `h` is its roof.
pub type Node = (Cell, u8);

fn circ(city: &City, c: Cell, plot: u32) -> bool {
    let k = city.idx(c);
    city.plot_of[k] == plot && matches!(city.role[k], Role::Core | Role::Corridor)
}

pub fn reachable(city: &City, year: u16) -> HashSet<Node> {
    let mut seen: HashSet<Node> = HashSet::new();
    let mut q = VecDeque::new();
    // Start from every lane cell on the edge of the site.
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) == Ground::Alley
                && crate::Dir::ALL.iter().any(|&d| city.step(c, d).map_or(true, |n| city.ground_at(n) == Ground::Outside))
            {
                seen.insert((c, 0));
                q.push_back((c, 0u8));
            }
        }
    }
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|b| b.year <= year).collect();

    while let Some((c, lv)) = q.pop_front() {
        let mut next: Vec<Node> = vec![];
        let g = city.ground_at(c);
        let pid = city.plot_of[city.idx(c)];
        let h = city.height_at(c, year);
        match g {
            Ground::Alley | Ground::Well if lv == 0 => {
                for (_, n) in city.neighbours(c) {
                    match city.ground_at(n) {
                        Ground::Alley => next.push((n, 0)),
                        // Step into a building's ground-floor stair core.
                        Ground::Plot if city.role_at(n) == Role::Core && city.height_at(n, year) > 0 => next.push((n, 0)),
                        _ => {}
                    }
                }
            }
            Ground::Plot if lv < h => {
                // Inside: move along this floor's circulation, up/down the core.
                for (_, n) in city.neighbours(c) {
                    if circ(city, n, pid) {
                        next.push((n, lv));
                    }
                }
                if lv == 0 {
                    for (_, n) in city.neighbours(c) {
                        if city.ground_at(n) == Ground::Alley {
                            next.push((n, 0));
                        }
                    }
                }
                // Landing <-> stair: the stair at floor lv climbs from landing lv to
                // landing lv+1 (lv+1 == h is the roof).
                let plot = &city.plots[pid as usize];
                match city.role_at(c) {
                    Role::Core => {
                        next.push((plot.core[1], lv));
                        if lv > 0 {
                            next.push((plot.core[1], lv - 1));
                        }
                    }
                    Role::Stair => {
                        next.push((plot.core[0], lv));
                        next.push((plot.core[0], lv + 1));
                    }
                    _ => {}
                }
                for b in &bridges {
                    if b.floor == lv {
                        if b.a == c {
                            next.push((b.b, lv));
                        } else if b.b == c {
                            next.push((b.a, lv));
                        }
                    }
                }
            }
            Ground::Plot if lv == h && h > 0 => {
                // Rooftop: one plane per building, ladders to neighbouring roofs.
                for (_, n) in city.neighbours(c) {
                    if city.ground_at(n) != Ground::Plot {
                        continue;
                    }
                    let hn = city.height_at(n, year);
                    if hn > 0 {
                        next.push((n, hn));
                    }
                }
                if city.role_at(c) == Role::Core {
                    next.push((city.plots[pid as usize].core[1], lv - 1));
                }
            }
            _ => {}
        }
        for nd in next {
            if seen.insert(nd) {
                q.push_back(nd);
            }
        }
    }
    seen
}

/// Units standing in `year` whose door can't be reached from the street.
pub fn unreachable_units(city: &City, year: u16) -> Vec<u32> {
    let r = reachable(city, year);
    city.units_at(year)
        .filter(|u| {
            let outside = city.step(u.door.cell, u.door.facing).unwrap();
            let lv = if city.ground_at(outside) == Ground::Alley { 0 } else { u.floor };
            !r.contains(&(outside, lv))
        })
        .map(|u| u.id)
        .collect()
}
