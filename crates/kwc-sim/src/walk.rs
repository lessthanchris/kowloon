//! The walkable graph: lanes, stair cores, corridors, bridges and rooftops.
//! Used to prove every unit can be reached from the street, and later by the
//! engine for spawn points / navigation.

use crate::city::*;
use std::collections::{HashSet, VecDeque};

/// A place you can stand: a cell at a level. Level 0 is the ground; level `f` is
/// floor `f`; level `h` on a building of height `h` is its roof.
pub type Node = (Cell, u8);

/// Everywhere reachable from the lane openings on the edge of the site.
pub fn reachable(city: &City, year: u16) -> HashSet<Node> {
    let mut starts = vec![];
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            if city.ground_at(c) == Ground::Alley
                && crate::Dir::ALL.iter().any(|&d| {
                    city.step(c, d)
                        .map_or(true, |n| city.ground_at(n) == Ground::Outside)
                })
            {
                starts.push((c, 0u8));
            }
        }
    }
    search(city, year, starts, &|_| true)
}

/// Breadth-first over the walkable graph, only through nodes `allow` accepts.
pub fn search(
    city: &City,
    year: u16,
    starts: Vec<Node>,
    allow: &dyn Fn(Node) -> bool,
) -> HashSet<Node> {
    let mut seen: HashSet<Node> = starts.iter().copied().collect();
    let mut q: VecDeque<Node> = starts.into();
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|b| b.year <= year).collect();

    while let Some(node) = q.pop_front() {
        for nd in next_nodes(city, year, &bridges, node) {
            if allow(nd) && seen.insert(nd) {
                q.push_back(nd);
            }
        }
    }
    seen
}

/// The shortest walk (in steps) from `from` to `to`, both ends included,
/// only through nodes `allow` accepts.
pub fn path(city: &City, year: u16, from: Node, to: Node, allow: &dyn Fn(Node) -> bool) -> Option<Vec<Node>> {
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|b| b.year <= year).collect();
    let mut parent: std::collections::HashMap<Node, Node> =
        std::collections::HashMap::from([(from, from)]);
    let mut q: VecDeque<Node> = VecDeque::from([from]);
    while let Some(node) = q.pop_front() {
        if node == to {
            let mut out = vec![node];
            let mut cur = node;
            while cur != from {
                cur = parent[&cur];
                out.push(cur);
            }
            out.reverse();
            return Some(out);
        }
        for nd in next_nodes(city, year, &bridges, node) {
            if allow(nd) && !parent.contains_key(&nd) {
                parent.insert(nd, node);
                q.push_back(nd);
            }
        }
    }
    None
}

/// Steps from `from` to every node reachable through nodes `allow` accepts.
pub fn distances(city: &City, year: u16, from: Node, allow: &dyn Fn(Node) -> bool) -> std::collections::HashMap<Node, u32> {
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|b| b.year <= year).collect();
    let mut dist = std::collections::HashMap::from([(from, 0u32)]);
    let mut q: VecDeque<Node> = VecDeque::from([from]);
    while let Some(node) = q.pop_front() {
        let d = dist[&node];
        for nd in next_nodes(city, year, &bridges, node) {
            if allow(nd) && !dist.contains_key(&nd) {
                dist.insert(nd, d + 1);
                q.push_back(nd);
            }
        }
    }
    dist
}

/// Where you can go in one step from a node.
fn next_nodes(city: &City, year: u16, bridges: &[&Bridge], (c, lv): Node) -> Vec<Node> {
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
                    Ground::Plot
                        if city.role_at(n) == Role::Core && city.height_at(n, year) > 0 =>
                    {
                        next.push((n, 0))
                    }
                    _ => {}
                }
            }
        }
        Ground::Plot if lv < h => {
            // Inside: move along this floor's circulation, up/down the core.
            for (_, n) in city.neighbours(c) {
                if city.circ_at(n, pid, lv) {
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
            for b in bridges {
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
    next
}

/// Units standing in `year` whose door can't be reached from the street.
pub fn unreachable_units(city: &City, year: u16) -> Vec<u32> {
    let r = reachable(city, year);
    city.units_at(year)
        .filter(|u| {
            let outside = city.step(u.door.cell, u.door.facing).unwrap();
            let lv = if city.ground_at(outside) == Ground::Alley {
                0
            } else {
                u.floor
            };
            !r.contains(&(outside, lv))
        })
        .map(|u| u.id)
        .collect()
}
