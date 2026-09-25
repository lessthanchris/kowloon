//! Summary numbers for comparing a generated city against the 1987 survey.

use crate::city::*;
use crate::site::{CELL_M, MAX_FLOORS};

/// Residents per premises: 33,000 residents / 8,500 premises (1987 survey).
pub const PEOPLE_PER_UNIT: f32 = 33_000.0 / 8_500.0;

#[derive(Debug, Clone)]
pub struct Stats {
    pub area_m2: f32,
    pub buildings: usize,
    pub settled: usize,
    pub units: usize,
    pub residents: usize,
    pub mean_unit_m2: f32,
    pub mean_height: f32,
    pub at_cap: usize,
    pub frac_10_plus: f32,
    pub lane_share: f32,
    pub well_share: f32,
    pub corridor_share: f32,
}

pub fn measure(city: &City, year: u16) -> Stats {
    let cell_m2 = CELL_M * CELL_M;
    let count = |g: Ground| city.ground.iter().filter(|&&x| x == g).count() as f32;
    let inside = city.ground.len() as f32 - count(Ground::Outside);
    let hs: Vec<u8> = city.plots.iter().map(|p| p.height_at(year)).collect();
    let settled = hs.iter().filter(|&&h| h > 0).count();
    let units: Vec<&Unit> = city.units_at(year).collect();
    let bcells = city.role.iter().filter(|r| **r != Role::None).count() as f32;
    Stats {
        area_m2: inside * cell_m2,
        buildings: city.plots.len(),
        settled,
        units: units.len(),
        residents: (units.len() as f32 * PEOPLE_PER_UNIT) as usize,
        mean_unit_m2: units.iter().map(|u| u.cells.len() as f32).sum::<f32>() / units.len().max(1) as f32 * cell_m2,
        mean_height: hs.iter().map(|&h| h as f32).sum::<f32>() / hs.len() as f32,
        at_cap: hs.iter().filter(|&&h| h as usize >= MAX_FLOORS).count(),
        frac_10_plus: hs.iter().filter(|&&h| h >= 10).count() as f32 / hs.len() as f32,
        lane_share: count(Ground::Alley) / inside,
        well_share: count(Ground::Well) / inside,
        corridor_share: city.corr.iter().map(|m| m.count_ones()).sum::<u32>() as f32 / (bcells * MAX_FLOORS as f32),
    }
}
