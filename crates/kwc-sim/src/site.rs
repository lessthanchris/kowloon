//! Grounded site data, projected to local metres.
//!
//! Sources (see RESEARCH.md): OpenStreetMap (© OpenStreetMap contributors, ODbL)
//! and the published 1987 survey figures.
//!
//! Footprint: the Kowloon Walled City Park outline (OSM relation 1650915, 3.3 ha)
//! inset uniformly by 9.7 m until it encloses the documented 2.6 ha. The park is
//! the old city plus a buffer; the inset outline puts the excavated South Gate
//! ruins just inside the edge and the Yamen near the middle, as they should be.

/// (lon, lat) ring, closed.
pub const FOOTPRINT_LONLAT: &[(f64, f64)] = &[
    (114.1890721, 22.3322445), (114.1896509, 22.3324080), (114.1898318, 22.3324734),
    (114.1900080, 22.3325639), (114.1901218, 22.3326279), (114.1903071, 22.3327724),
    (114.1903828, 22.3327922), (114.1906266, 22.3329864), (114.1906267, 22.3330273),
    (114.1907406, 22.3331168), (114.1907842, 22.3331316), (114.1909689, 22.3329671),
    (114.1912743, 22.3324315), (114.1912619, 22.3323450), (114.1914651, 22.3319938),
    (114.1907392, 22.3317112), (114.1906788, 22.3317977), (114.1904323, 22.3316988),
    (114.1904457, 22.3315982), (114.1903705, 22.3315260), (114.1902776, 22.3314819),
    (114.1899514, 22.3313763), (114.1897708, 22.3312563), (114.1897185, 22.3312352),
    (114.1896675, 22.3312332), (114.1896323, 22.3312477), (114.1894560, 22.3314214),
    (114.1894899, 22.3314371), (114.1892624, 22.3319274), (114.1892003, 22.3319004),
    (114.1890932, 22.3320978), (114.1890721, 22.3322445),
];

/// The Yamen's three halls (OSM relation 18506822), convex hull + 1.5 m.
pub const YAMEN_LONLAT: &[(f64, f64)] = &[
    (114.1904243, 22.3320120), (114.1903630, 22.3320758), (114.1903176, 22.3323786),
    (114.1903176, 22.3324905), (114.1904826, 22.3324905), (114.1905963, 22.3322381),
    (114.1905963, 22.3321094), (114.1905846, 22.3320120), (114.1904243, 22.3320120),
];

/// Centre of the excavated South Gate ruins (OSM way 263310044).
pub const SOUTH_GATE_LONLAT: (f64, f64) = (114.190822, 22.331820);

/// Metres per cell edge in plan. Lanes are one cell wide: the survey's
/// "often only 1–2 m".
pub const CELL_M: f32 = 1.5;
/// Metres per storey.
pub const STOREY_M: f32 = 2.8;
/// Kai Tak flight-path height limit, in storeys.
pub const MAX_FLOORS: usize = 14;

pub const START_YEAR: u16 = 1950;
pub const END_YEAR: u16 = 1987;

/// 1987 survey targets, used by tests and the dump tool.
pub mod survey_1987 {
    pub const AREA_M2: f32 = 26_000.0;
    pub const BUILDINGS: usize = 350;
    pub const PREMISES: usize = 8_500;
    pub const HOUSEHOLDS: usize = 10_700;
    pub const RESIDENTS: usize = 33_000;
    pub const WATER_STANDPIPES: usize = 8;
    pub const LIFTS: usize = 2;
}

pub type Ring = Vec<(f32, f32)>;

/// Everything projected to local metres: x east, y south (image-like), origin at
/// the north-west corner of the footprint's bounding box plus a margin.
pub struct Site {
    pub ring_m: Ring,
    pub yamen_m: Ring,
    pub width_m: f32,
    pub depth_m: f32,
    pub south_gate_m: (f32, f32),
}

impl Site {
    pub fn load() -> Site {
        let lat0 = FOOTPRINT_LONLAT.iter().map(|p| p.1).sum::<f64>() / FOOTPRINT_LONLAT.len() as f64;
        let mx = 111_320.0 * lat0.to_radians().cos();
        let my = 110_574.0;
        let min_lon = FOOTPRINT_LONLAT.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let max_lat = FOOTPRINT_LONLAT.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        let margin = 2.0 * CELL_M as f64;
        let proj = |(lon, lat): (f64, f64)| -> (f32, f32) {
            (((lon - min_lon) * mx + margin) as f32, ((max_lat - lat) * my + margin) as f32)
        };
        let ring_m: Ring = FOOTPRINT_LONLAT.iter().map(|&p| proj(p)).collect();
        let width_m = ring_m.iter().map(|p| p.0).fold(0.0, f32::max) + margin as f32;
        let depth_m = ring_m.iter().map(|p| p.1).fold(0.0, f32::max) + margin as f32;
        Site {
            yamen_m: YAMEN_LONLAT.iter().map(|&p| proj(p)).collect(),
            ring_m,
            width_m,
            depth_m,
            south_gate_m: proj(SOUTH_GATE_LONLAT),
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        point_in(&self.ring_m, x, y)
    }

    pub fn area_m2(&self) -> f32 {
        let r = &self.ring_m;
        let mut a = 0.0;
        for i in 0..r.len() {
            let (x0, y0) = r[i];
            let (x1, y1) = r[(i + 1) % r.len()];
            a += x0 * y1 - x1 * y0;
        }
        (a * 0.5).abs()
    }
}

pub fn point_in(r: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = r.len() - 1;
    for i in 0..r.len() {
        let (xi, yi) = r[i];
        let (xj, yj) = r[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
