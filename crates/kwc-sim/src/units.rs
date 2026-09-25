//! Split every floor of every building into units (flats, shops, workshops...).
//! Units are the hook for interiors: each has a stable id, cells and a door.

use crate::city::*;
use crate::rng_for;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::VecDeque;

pub fn subdivide(city: &mut City) {
    let mut units = vec![];
    for p in 0..city.plots.len() {
        let floors = city.plots[p].final_height();
        for f in 0..floors {
            let mut rng = rng_for(city.params.seed, 0x5000_0000 + ((p as u64) << 8) + f as u64);
            floor_units(city, p, f, &mut rng, &mut units);
        }
    }
    for (i, u) in units.iter_mut().enumerate() {
        u.id = i as u32;
    }
    city.units = units;
}

fn is_circulation(city: &City, c: Cell, plot: u32, floor: u8) -> bool {
    city.circ_at(c, plot, floor)
}

fn floor_units(city: &City, p: usize, floor: u8, rng: &mut impl Rng, out: &mut Vec<Unit>) {
    let pid = p as u32;
    let is_room = |c: Cell| city.room_at(c, pid, floor);
    let doorable = |c: Cell| {
        city.neighbours(c).any(|(_, n)| is_circulation(city, n, pid, floor) || (floor == 0 && city.ground_at(n) == Ground::Alley))
    };
    let rooms: Vec<Cell> = city.plots[p].cells.iter().copied().filter(|&c| is_room(c)).collect();
    // Units start from cells with a way out, then grow into back rooms.
    let mut seeds: Vec<Cell> = rooms.iter().copied().filter(|&c| doorable(c)).collect();
    seeds.shuffle(rng);
    let mut owner: std::collections::HashMap<Cell, usize> = std::collections::HashMap::new();
    let mut groups: Vec<Vec<Cell>> = vec![];
    for &start in &seeds {
        if owner.contains_key(&start) {
            continue;
        }
        let g = groups.len();
        let size = rng.gen_range(16..=32); // ~23 m² average flat (1987 survey)
        let mut cells = vec![start];
        owner.insert(start, g);
        let mut q = VecDeque::from([start]);
        while let Some(c) = q.pop_front() {
            let mut ns: Vec<Cell> = city.neighbours(c).map(|x| x.1).collect();
            ns.shuffle(rng);
            for n in ns {
                if cells.len() < size && is_room(n) && !owner.contains_key(&n) {
                    owner.insert(n, g);
                    cells.push(n);
                    q.push_back(n);
                }
            }
        }
        groups.push(cells);
    }
    // Back rooms nobody claimed join a neighbouring unit.
    loop {
        let mut changed = false;
        for &c in &rooms {
            if owner.contains_key(&c) {
                continue;
            }
            if let Some(g) = city.neighbours(c).find_map(|(_, n)| owner.get(&n).copied()) {
                owner.insert(c, g);
                groups[g].push(c);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Pokey leftovers (under ~18 m²) knock through into their smallest neighbour.
    const MIN_UNIT: usize = 12;
    let mut lonely = std::collections::HashSet::new();
    while let Some(g) = (0..groups.len()).find(|&g| !groups[g].is_empty() && groups[g].len() < MIN_UNIT && !lonely.contains(&g)) {
        let mut best: Option<usize> = None;
        for &c in &groups[g] {
            for (_, n) in city.neighbours(c) {
                if let Some(&o) = owner.get(&n) {
                    if o != g && best.map_or(true, |b| groups[o].len() < groups[b].len()) {
                        best = Some(o);
                    }
                }
            }
        }
        match best {
            Some(o) => {
                let cells = std::mem::take(&mut groups[g]);
                for &c in &cells {
                    owner.insert(c, o);
                }
                groups[o].extend(cells);
            }
            None => {
                lonely.insert(g); // nothing to join: stays a tiny room
            }
        }
    }

    for cells in groups.into_iter().filter(|g| !g.is_empty()) {

        // Door: onto the lane for ground-floor frontage (usually), else onto the landing.
        let mut alley_doors = vec![];
        let mut inner_doors = vec![];
        for &c in &cells {
            for (d, n) in city.neighbours(c) {
                if floor == 0 && city.ground_at(n) == Ground::Alley {
                    alley_doors.push(Door { cell: c, facing: d });
                } else if is_circulation(city, n, pid, floor) {
                    inner_doors.push(Door { cell: c, facing: d });
                }
            }
        }
        let on_lane = !alley_doors.is_empty() && (inner_doors.is_empty() || rng.gen::<f32>() < 0.75);
        let door = if on_lane {
            *alley_doors.choose(rng).unwrap()
        } else if let Some(d) = inner_doors.choose(rng) {
            *d
        } else {
            continue; // unreachable by construction; skip rather than make a sealed unit
        };

        let usage = pick_use(floor, on_lane, rng);
        let open_p = if on_lane { 0.35 } else { 0.04 };
        let door_state = if rng.gen::<f32>() < open_p { DoorState::Open } else { DoorState::Closed };
        out.push(Unit { id: 0, plot: pid, floor, cells, door, usage, door_state, name: None });
    }
}

fn pick_use(floor: u8, on_lane: bool, rng: &mut impl Rng) -> UnitUse {
    use UnitUse::*;
    let table: &[(UnitUse, u32)] = if on_lane {
        &[(Shop, 30), (Workshop, 20), (FishballFactory, 10), (Dentist, 15), (Clinic, 5), (Restaurant, 15), (Flat, 3)]
    } else if floor <= 3 {
        &[(Flat, 60), (Workshop, 18), (FishballFactory, 6), (Dentist, 8), (Clinic, 4), (Shop, 4)]
    } else {
        &[(Flat, 88), (Workshop, 8), (Clinic, 2), (Dentist, 2)]
    };
    let total: u32 = table.iter().map(|t| t.1).sum();
    let mut r = rng.gen_range(0..total);
    for &(u, w) in table {
        if r < w {
            return u;
        }
        r -= w;
    }
    Flat
}
