//! Named, documented places: the 8 municipal standpipes, the last natural well
//! off Tai Chang ("Big Well") Street, the only two lifts, and the Tin Hau and
//! Fuk Tak temples. Positions are invented; counts and kinds are from sources.

use crate::city::*;
use crate::rng_for;
use crate::site::{survey_1987, END_YEAR};
use rand::seq::SliceRandom;
use rand::Rng;

fn dist2(a: Cell, b: Cell) -> i32 {
    let (di, dj) = (a.0 as i32 - b.0 as i32, a.1 as i32 - b.1 as i32);
    di * di + dj * dj
}

pub fn place(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 5);
    let lanes: Vec<Cell> = (0..city.w * city.d)
        .filter(|&k| city.ground[k] == Ground::Alley)
        .map(|k| ((k % city.w) as u16, (k / city.w) as u16))
        .collect();

    // Standpipes: farthest-point spread over the lane network.
    let mut pipes: Vec<Cell> = vec![lanes[rng.gen_range(0..lanes.len())]];
    while pipes.len() < survey_1987::WATER_STANDPIPES {
        let next = *lanes.iter().max_by_key(|&&c| pipes.iter().map(|&p| dist2(c, p)).min().unwrap()).unwrap();
        pipes.push(next);
    }
    for c in pipes {
        city.features.push(Feature { kind: FeatureKind::WaterStandpipe, cell: c, name: None, plot: None });
    }

    // The natural well: part-way along Tai Chang Street.
    if let Some(l) = city.lanes.iter().find(|l| l.name == "Tai Chang Street") {
        let c = l.cells[l.cells.len() / 2];
        city.features.push(Feature { kind: FeatureKind::NaturalWell, cell: c, name: Some("Big Well".into()), plot: None });
    }

    // Two lifts, in two of the largest full-height buildings.
    let mut tall: Vec<&Plot> = city.plots.iter().filter(|p| p.height_at(END_YEAR) as usize >= crate::MAX_FLOORS).collect();
    tall.sort_by_key(|p| std::cmp::Reverse(p.cells.len()));
    let lifts: Vec<(Cell, u32)> = tall.iter().take(survey_1987::LIFTS).map(|p| (p.core[0], p.id)).collect();
    for (c, pid) in lifts {
        city.features.push(Feature { kind: FeatureKind::Lift, cell: c, name: None, plot: Some(pid) });
    }

    // Trades by street: roast meats along Lo Yan Street; fishball makers off
    // Kwong Ming Street (lower floors, within ~15 m of it).
    let lane_cells = |name: &str| -> std::collections::HashSet<Cell> {
        city.lanes.iter().filter(|l| l.name == name).flat_map(|l| l.cells.iter().copied()).collect()
    };
    let lo_yan = lane_cells("Lo Yan Street");
    let kwong_ming = lane_cells("Kwong Ming Street");
    for i in 0..city.units.len() {
        let u = &city.units[i];
        let front = city.step(u.door.cell, u.door.facing);
        if u.floor == 0 && front.is_some_and(|f| lo_yan.contains(&f)) && rng.gen::<f32>() < 0.6 {
            city.units[i].usage = UnitUse::Restaurant;
        } else if u.floor <= 3
            && matches!(u.usage, UnitUse::Workshop | UnitUse::Flat)
            && kwong_ming.iter().any(|&k| dist2(k, u.door.cell) <= 100)
            && rng.gen::<f32>() < 0.5
        {
            city.units[i].usage = UnitUse::FishballFactory;
        }
    }

    // Temples: two lane-facing ground-floor units, well apart.
    let mut shopfronts: Vec<usize> = (0..city.units.len())
        .filter(|&i| {
            let u = &city.units[i];
            u.floor == 0 && city.step(u.door.cell, u.door.facing).is_some_and(|n| city.ground_at(n) == Ground::Alley)
        })
        .collect();
    shopfronts.shuffle(&mut rng);
    let mut placed: Vec<Cell> = vec![];
    for name in ["Tin Hau Temple", "Fuk Tak Temple"] {
        if let Some(&i) = shopfronts.iter().find(|&&i| placed.iter().all(|&p| dist2(p, city.units[i].door.cell) > 40 * 40)) {
            let u = &mut city.units[i];
            u.usage = UnitUse::Temple;
            u.name = Some(name.into());
            u.door_state = DoorState::Open;
            placed.push(u.door.cell);
            city.features.push(Feature { kind: FeatureKind::Temple, cell: u.door.cell, name: Some(name.into()), plot: Some(u.plot) });
        }
    }
}
