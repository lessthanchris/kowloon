//! Year-by-year growth, 1950 → 1987.
//!
//! Two pressures drive it: land take-up (squatters settle empty plots) and
//! vertical demand (the average building height the city "wants" each year).
//! Buildings rise in jumps, prefer to catch up with taller neighbours, and a big
//! jump is a rebuild: the old low building is replaced wholesale.

use crate::city::*;
use crate::rng_for;
use crate::site::{END_YEAR, MAX_FLOORS, START_YEAR};
use rand::Rng;

/// Fraction of plots settled by `year`.
fn settled_fraction(year: u16) -> f32 {
    let t = (year as f32 - 1950.0) / 16.0;
    (0.35 + 0.65 * t).clamp(0.0, 1.0)
}

/// Target mean storeys over all plots by `year` (before the demand multiplier).
fn target_height(year: u16) -> f32 {
    const KEYS: &[(f32, f32)] = &[
        (1950.0, 0.5),
        (1955.0, 0.9),
        (1960.0, 1.8),
        (1965.0, 3.8),
        (1970.0, 6.5),
        (1975.0, 9.2),
        (1980.0, 11.4),
        (1987.0, 12.6),
    ];
    let y = year as f32;
    for w in KEYS.windows(2) {
        let (a, b) = (w[0], w[1]);
        if y <= b.0 {
            return a.1 + (b.1 - a.1) * ((y - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
        }
    }
    KEYS.last().unwrap().1
}

fn neighbour_plots(city: &City, p: usize) -> Vec<usize> {
    let mut v: Vec<usize> = vec![];
    for &c in &city.plots[p].cells {
        for (_, n) in city.neighbours(c) {
            let q = city.plot_of[city.idx(n)];
            if q != NO_PLOT && q as usize != p && !v.contains(&(q as usize)) {
                v.push(q as usize);
            }
        }
    }
    v
}

pub fn grow(city: &mut City) {
    let mut rng = rng_for(city.params.seed, 3);
    let n = city.plots.len();
    let neigh: Vec<Vec<usize>> = (0..n).map(|p| neighbour_plots(city, p)).collect();
    let frontage: Vec<bool> = (0..n)
        .map(|p| city.plots[p].cells.iter().any(|&c| city.neighbours(c).any(|(_, x)| city.ground_at(x) == Ground::Outside)))
        .collect();
    let near_gate: Vec<f32> = (0..n)
        .map(|p| {
            let c = city.plots[p].core[0];
            let g = city.south_gate;
            let d = ((c.0 as f32 - g.0 as f32).powi(2) + (c.1 as f32 - g.1 as f32).powi(2)).sqrt();
            1.0 / (1.0 + d / 15.0)
        })
        .collect();

    for p in &mut city.plots {
        p.ambition = if rng.gen::<f32>() < 0.55 { MAX_FLOORS as u8 } else { rng.gen_range(10..=13) };
    }

    let mut heights = vec![0u8; n];
    for year in START_YEAR..=END_YEAR {
        // 1. Settle empty plots, favouring ones beside existing houses and near the gate.
        let target_settled = (settled_fraction(year) * n as f32).round() as usize;
        let mut settled = heights.iter().filter(|&&h| h > 0).count();
        while settled < target_settled {
            let mut best = (f32::MIN, usize::MAX);
            for p in 0..n {
                if heights[p] > 0 {
                    continue;
                }
                let built_nb = neigh[p].iter().filter(|&&q| heights[q] > 0).count() as f32;
                let s = built_nb * 0.5 + near_gate[p] * 1.0 + if frontage[p] { 1.0 } else { 0.0 } + rng.gen::<f32>() * 6.0;
                if s > best.0 {
                    best = (s, p);
                }
            }
            let p = best.1;
            let h = if rng.gen::<f32>() < 0.3 { 2 } else { 1 };
            set_height(city, p, &mut heights, h, year, true);
            city.plots[p].founded = year;
            settled += 1;
        }

        // 2. Build upward until the city meets this year's height demand.
        let demand = city.params.demand.max(0.1);
        let target_floors = (target_height(year) * demand).min(MAX_FLOORS as f32 - 0.5) * n as f32;
        let mut floors: f32 = heights.iter().map(|&h| h as f32).sum();
        let mut guard = 0;
        while floors < target_floors && guard < 100_000 {
            guard += 1;
            // Weighted pick: catching up with taller neighbours is the strongest pull.
            let weights: Vec<f32> = (0..n)
                .map(|p| {
                    let h = heights[p];
                    if h == 0 || h >= city.plots[p].ambition {
                        return 0.0;
                    }
                    let nb_max = neigh[p].iter().map(|&q| heights[q]).max().unwrap_or(0) as f32;
                    let gap = (nb_max - h as f32).max(0.0);
                    0.3 + gap * 1.2 + near_gate[p] * 0.5
                })
                .collect();
            let total: f32 = weights.iter().sum();
            if total <= 0.0 {
                break;
            }
            let mut r = rng.gen::<f32>() * total;
            let mut p = 0;
            for (i, w) in weights.iter().enumerate() {
                r -= w;
                if r <= 0.0 {
                    p = i;
                    break;
                }
            }
            let h = heights[p];
            let late = ((year as f32 - 1958.0) / 20.0).clamp(0.0, 1.0);
            let jump = if rng.gen::<f32>() < 0.25 + 0.45 * late { rng.gen_range(3..=8) } else { rng.gen_range(1..=2) };
            let new_h = (h + jump).min(city.plots[p].ambition);
            let rebuild = new_h - h >= 3;
            floors += (new_h - h) as f32;
            set_height(city, p, &mut heights, new_h, year, rebuild);
        }
    }
}

fn set_height(city: &mut City, p: usize, heights: &mut [u8], h: u8, year: u16, rebuild: bool) {
    // floor_year records when each storey first stood, so the timeline never loses
    // floors; a rebuild is remembered separately (it drives "decade" colouring).
    let old = heights[p] as usize;
    let plot = &mut city.plots[p];
    for f in old..h as usize {
        plot.floor_year[f] = year;
    }
    if rebuild {
        plot.rebuilt = year;
    }
    heights[p] = h;
}
