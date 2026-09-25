//! Addresses. Every lane gets a name: the main lanes have theirs, and side
//! alleys are named after the lane they branch from, as the city's own were
//! (龍津一巷, "Lung Chun 1st Lane"). Buildings are numbered along their lane;
//! flats are lettered per floor. Floors use Hong Kong notation (G/F, 1/F, ...).

use crate::city::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Address {
    pub lane: String,
    pub number: u16,
    pub floor: u8,
    /// 'A', 'B', ... on that floor.
    pub flat: Option<char>,
    /// A shopfront on the lane ("Shop A, G/F, ...").
    pub shop: bool,
}

impl Address {
    pub fn floor_name(floor: u8) -> String {
        if floor == 0 { "G/F".into() } else { format!("{floor}/F") }
    }
    /// "Flat 4B, 12 Lo Yan Street" or "G/F, 12 Lo Yan Street".
    pub fn line(&self) -> String {
        let fl = Address::floor_name(self.floor);
        match (self.flat, self.shop) {
            (Some(l), true) => format!("Shop {l}, {fl}, {} {}", self.number, self.lane),
            (Some(l), false) if self.floor > 0 => format!("Flat {}{}, {} {}", self.floor, l, self.number, self.lane),
            (Some(l), false) => format!("Unit {l}, {fl}, {} {}", self.number, self.lane),
            (None, _) => format!("{fl}, {} {}", self.number, self.lane),
        }
    }
    pub fn building(&self) -> String {
        format!("{} {}", self.number, self.lane)
    }
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

pub struct Directory {
    /// Lane name per cell (None for non-lane cells).
    pub lane_of: Vec<Option<u16>>,
    pub lane_names: Vec<String>,
    /// Per plot: (lane index, building number).
    pub building: Vec<(u16, u16)>,
    /// Per unit.
    pub address: Vec<Address>,
}

impl Directory {
    pub fn lane_at(&self, city: &City, c: Cell) -> Option<&str> {
        self.lane_of[city.idx(c)].map(|k| self.lane_names[k as usize].as_str())
    }
}

pub fn build(city: &City) -> Directory {
    let n = city.w * city.d;
    let mut lane_of: Vec<Option<u16>> = vec![None; n];
    let mut lane_names: Vec<String> = vec![];

    // Named lanes claim their cells (earlier lanes win at crossings).
    for l in &city.lanes {
        let k = lane_names.len() as u16;
        lane_names.push(l.name.clone());
        for &c in &l.cells {
            let i = city.idx(c);
            if lane_of[i].is_none() && city.ground[i] == Ground::Alley {
                lane_of[i] = Some(k);
            }
        }
    }
    // Every other lane cell belongs to the nearest named lane (through the lanes)...
    let mut parent: Vec<Option<u16>> = lane_of.clone();
    let mut q: VecDeque<Cell> = (0..n).filter(|&i| lane_of[i].is_some()).map(|i| ((i % city.w) as u16, (i / city.w) as u16)).collect();
    while let Some(c) = q.pop_front() {
        let p = parent[city.idx(c)];
        for (_, nb) in city.neighbours(c) {
            let i = city.idx(nb);
            if city.ground[i] == Ground::Alley && parent[i].is_none() {
                parent[i] = p;
                q.push_back(nb);
            }
        }
    }
    // ...and each connected run of unnamed cells is a side lane off it: "1st Lane", "2nd Lane".
    let mut seen = vec![false; n];
    let mut count: HashMap<u16, usize> = HashMap::new();
    for i in 0..n {
        if city.ground[i] != Ground::Alley || lane_of[i].is_some() || seen[i] {
            continue;
        }
        let Some(p) = parent[i] else { continue };
        let k = *count.entry(p).and_modify(|x| *x += 1).or_insert(1);
        let id = lane_names.len() as u16;
        lane_names.push(format!("{} {} Lane", lane_names[p as usize], ordinal(k)));
        let mut q = VecDeque::from([((i % city.w) as u16, (i / city.w) as u16)]);
        seen[i] = true;
        while let Some(c) = q.pop_front() {
            lane_of[city.idx(c)] = Some(id);
            for (_, nb) in city.neighbours(c) {
                let j = city.idx(nb);
                if !seen[j] && city.ground[j] == Ground::Alley && lane_of[j].is_none() && parent[j] == Some(p) {
                    seen[j] = true;
                    q.push_back(nb);
                }
            }
        }
    }

    // Buildings: numbered along the lane their stair door opens onto, in order of
    // distance from that lane's start.
    let mut entries: HashMap<u16, Vec<(u32, u32)>> = HashMap::new(); // lane -> (distance, plot)
    let dist_along = |lane: u16| -> HashMap<Cell, u32> {
        let cells: Vec<Cell> = (0..n).filter(|&i| lane_of[i] == Some(lane)).map(|i| ((i % city.w) as u16, (i / city.w) as u16)).collect();
        let start = *cells.iter().min_by_key(|c| (c.1, c.0)).unwrap();
        let mut d = HashMap::from([(start, 0u32)]);
        let mut q = VecDeque::from([start]);
        while let Some(c) = q.pop_front() {
            for (_, nb) in city.neighbours(c) {
                if lane_of[city.idx(nb)] == Some(lane) && !d.contains_key(&nb) {
                    d.insert(nb, d[&c] + 1);
                    q.push_back(nb);
                }
            }
        }
        d
    };
    let mut along: HashMap<u16, HashMap<Cell, u32>> = HashMap::new();
    for p in &city.plots {
        let door = city.neighbours(p.core[0]).find_map(|(_, nb)| lane_of[city.idx(nb)].map(|l| (l, nb)));
        let Some((l, cell)) = door else { continue };
        let d = along.entry(l).or_insert_with(|| dist_along(l)).get(&cell).copied().unwrap_or(0);
        entries.entry(l).or_default().push((d, p.id));
    }
    let mut building = vec![(0u16, 0u16); city.plots.len()];
    for (l, mut list) in entries {
        list.sort_unstable();
        for (k, (_, pid)) in list.into_iter().enumerate() {
            building[pid as usize] = (l, k as u16 + 1);
        }
    }

    // Flats: lettered per building and floor, in unit order.
    let mut per_floor: HashMap<(u32, u8), Vec<u32>> = HashMap::new();
    for u in &city.units {
        per_floor.entry((u.plot, u.floor)).or_default().push(u.id);
    }
    let mut address = vec![Address { lane: String::new(), number: 0, floor: 0, flat: None, shop: false }; city.units.len()];
    for ((plot, floor), mut ids) in per_floor {
        ids.sort_unstable();
        let (l, num) = building[plot as usize];
        for (k, id) in ids.iter().enumerate() {
            let u = &city.units[*id as usize];
            let shopfront = city.step(u.door.cell, u.door.facing).is_some_and(|nb| city.ground_at(nb) == Ground::Alley);
            // Shops use their building's address too (a shop letter keeps them apart).
            let (lane, number) = (l, num);
            address[*id as usize] = Address {
                lane: lane_names[lane as usize].clone(),
                number,
                floor,
                flat: Some((b'A' + (k as u8).min(25)) as char),
                shop: shopfront,
            };
        }
    }
    Directory { lane_of, lane_names, building, address }
}
