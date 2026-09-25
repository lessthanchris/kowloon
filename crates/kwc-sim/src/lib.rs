//! kwc-sim: a deterministic, headless model of Kowloon Walled City growing from
//! scattered village houses (1950) into the solid 14-storey block (1987).
//!
//! No graphics here. The engine consumes a [`City`] and renders/meshes it.

pub mod circulation;
pub mod city;
pub mod export;
pub mod features;
pub mod growth;
pub mod interior;
pub mod layout;
pub mod site;
pub mod stats;
pub mod units;
pub mod walk;

pub use city::*;
pub use site::{CELL_M, END_YEAR, MAX_FLOORS, START_YEAR, STOREY_M};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Params {
    pub seed: u64,
    /// Scales how fast the city rises (1.0 = historical-ish curve).
    pub demand: f32,
    /// Max distance (cells) any building cell may sit from an alley. Lower = denser lane network.
    pub alley_spacing: u32,
    /// Chance a small plot is left as a light well / void.
    pub well_tolerance: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params { seed: 1987, demand: 1.0, alley_spacing: 7, well_tolerance: 0.15 }
    }
}

/// Independent, reproducible RNG stream for one stage of generation.
pub fn rng_for(seed: u64, tag: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Run the whole pipeline.
pub fn generate(params: &Params) -> City {
    let mut city = layout::build(params);
    growth::grow(&mut city);
    units::subdivide(&mut city);
    circulation::add_bridges(&mut city);
    features::place(&mut city);
    city
}
