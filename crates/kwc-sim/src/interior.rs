//! Interiors. v1 only furnishes units whose door is open; making every room
//! enterable later means calling the same generator for any unit on demand
//! (deterministic from seed + unit id), so nothing here needs to change shape.

use crate::city::*;
use crate::rng_for;
use crate::site::{CELL_M, STOREY_M};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropKind {
    Bed,
    Table,
    Stool,
    Shelf,
    Counter,
    Workbench,
    Machine,
    Vat,
    DentistChair,
    Altar,
    Crate,
    Fridge,
    Tv,
}

/// Box-shaped prop in unit-local metres (origin = unit bbox min corner, y up).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Prop {
    pub kind: PropKind,
    pub pos: [f32; 3],
    pub size: [f32; 3],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Light {
    pub pos: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InteriorLayout {
    pub props: Vec<Prop>,
    pub lights: Vec<Light>,
}

pub trait InteriorGenerator {
    fn generate(&self, city: &City, unit: &Unit, rng: &mut ChaCha8Rng) -> InteriorLayout;
}

/// Local-space bbox of a unit (metres): (width x, depth z).
pub fn unit_extent(unit: &Unit) -> ([u16; 2], [f32; 2]) {
    let mi = unit.cells.iter().map(|c| c.0).min().unwrap();
    let mj = unit.cells.iter().map(|c| c.1).min().unwrap();
    let xi = unit.cells.iter().map(|c| c.0).max().unwrap();
    let xj = unit.cells.iter().map(|c| c.1).max().unwrap();
    ([mi, mj], [(xi - mi + 1) as f32 * CELL_M, (xj - mj + 1) as f32 * CELL_M])
}

/// Picks a template by the unit's use. Props are placed per cell, against the
/// walls, leaving the cell centre clear so the room stays walkable.
pub struct TemplateInteriors;

impl InteriorGenerator for TemplateInteriors {
    fn generate(&self, _city: &City, unit: &Unit, rng: &mut ChaCha8Rng) -> InteriorLayout {
        use PropKind::*;
        let (min, _) = unit_extent(unit);
        let mut out = InteriorLayout::default();
        let fluoro = [0.75, 1.0, 0.85];
        let warm = [1.0, 0.72, 0.45];
        for (n, c) in unit.cells.iter().enumerate() {
            let ox = (c.0 - min[0]) as f32 * CELL_M;
            let oz = (c.1 - min[1]) as f32 * CELL_M;
            let wall = |along: f32, d: f32, w: f32, h: f32| -> ([f32; 3], [f32; 3]) {
                ([ox + along, 0.0, oz + 0.1], [w, h, d])
            };
            let items: &[(PropKind, f32, f32, f32)] = match unit.usage {
                UnitUse::Flat => &[(Bed, 1.9, 0.9, 0.5), (Shelf, 0.9, 0.4, 1.8), (Table, 0.8, 0.8, 0.75), (Tv, 0.5, 0.4, 0.45)],
                UnitUse::Shop => &[(Counter, 2.0, 0.6, 1.0), (Shelf, 1.2, 0.4, 2.0), (Crate, 0.6, 0.6, 0.5)],
                UnitUse::Workshop => &[(Workbench, 2.0, 0.8, 0.9), (Machine, 0.9, 0.9, 1.3), (Crate, 0.6, 0.6, 0.6)],
                UnitUse::FishballFactory => &[(Vat, 1.0, 1.0, 1.0), (Workbench, 2.2, 0.8, 0.9), (Fridge, 0.8, 0.7, 1.8)],
                UnitUse::Dentist | UnitUse::Clinic => &[(DentistChair, 1.6, 0.8, 1.0), (Shelf, 1.0, 0.4, 1.6), (Stool, 0.4, 0.4, 0.5)],
                UnitUse::Restaurant => &[(Table, 0.9, 0.9, 0.75), (Stool, 0.4, 0.4, 0.45), (Counter, 1.8, 0.6, 1.0)],
                UnitUse::Temple => &[(Altar, 1.6, 0.6, 1.2), (Stool, 0.4, 0.4, 0.4)],
            };
            let (k, w, d, h) = items[(n + rng.gen_range(0..items.len())) % items.len()];
            let along = rng.gen_range(0.05..(CELL_M - w).max(0.1));
            let (pos, size) = wall(along, d, w, h);
            out.props.push(Prop { kind: k, pos, size });
            if n == 0 || rng.gen::<f32>() < 0.4 {
                let col = if matches!(unit.usage, UnitUse::Flat | UnitUse::Temple | UnitUse::Restaurant) { warm } else { fluoro };
                out.lights.push(Light { pos: [ox + CELL_M * 0.5, STOREY_M - 0.2, oz + CELL_M * 0.5], color: col, intensity: 1.0 });
            }
        }
        out
    }
}

/// Deterministic interior for a unit.
pub fn interior_for(city: &City, unit: &Unit, gen: &dyn InteriorGenerator) -> InteriorLayout {
    let mut rng = rng_for(city.params.seed, 0x1_0000_0000 + unit.id as u64);
    gen.generate(city, unit, &mut rng)
}
