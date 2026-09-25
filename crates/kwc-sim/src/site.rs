//! Grounded site data: the real footprint, projected to local metres.
//!
//! Footprint is the outline of Kowloon Walled City Park (OSM relation 1650915,
//! © OpenStreetMap contributors, ODbL), which sits on the old Walled City site.

/// (lon, lat) ring, closed.
pub const FOOTPRINT_LONLAT: &[(f64, f64)] = &[
    (114.188936, 22.3322979), (114.1889772, 22.3322293), (114.1889912, 22.332194),
    (114.1890001, 22.332074), (114.1890383, 22.3320035), (114.1891573, 22.3317843),
    (114.1892142, 22.3318091), (114.1893675, 22.3314787), (114.1892996, 22.3314471),
    (114.1895766, 22.3311743), (114.1896494, 22.3311442), (114.18974, 22.3311479),
    (114.1898176, 22.3311791), (114.1899984, 22.3312993), (114.1900849, 22.3313347),
    (114.1901729, 22.3313613), (114.1903127, 22.3313997), (114.1904235, 22.3314522),
    (114.1904971, 22.331511), (114.1905439, 22.3315767), (114.1905351, 22.3316429),
    (114.1905701, 22.3316563), (114.1905967, 22.3316664), (114.1906294, 22.3316803),
    (114.1906437, 22.3316863), (114.1906923, 22.3316167), (114.1907186, 22.3316077),
    (114.1915303, 22.3319236), (114.1915995, 22.3319493), (114.1914602, 22.3321847),
    (114.1913597, 22.3323613), (114.1913725, 22.3324501), (114.1913398, 22.3325014),
    (114.191301, 22.3325622), (114.1910457, 22.3330207), (114.1908216, 22.3332202),
    (114.1908027, 22.3332219), (114.1907579, 22.3332177), (114.1906916, 22.3331936),
    (114.1905322, 22.3330683), (114.1905321, 22.3330271), (114.1903371, 22.3328718),
    (114.1902619, 22.3328521), (114.1900666, 22.3326998), (114.1899607, 22.3326402),
    (114.1897917, 22.3325534), (114.18962, 22.3324914), (114.1890046, 22.3323175),
    (114.188936, 22.3322979),
];

/// Approximate location of the old South Gate (its foundations are exposed in the park today).
pub const SOUTH_GATE_LONLAT: (f64, f64) = (114.19000, 22.33128);

/// Metres per cell edge in plan.
pub const CELL_M: f32 = 3.0;
/// Metres per storey.
pub const STOREY_M: f32 = 2.8;
/// Kai Tak flight-path height limit, in storeys.
pub const MAX_FLOORS: usize = 14;

pub const START_YEAR: u16 = 1950;
pub const END_YEAR: u16 = 1987;

/// Footprint projected to local metres: x east, y south (image-like), origin at the
/// north-west corner of the bounding box plus one cell of margin.
pub struct Site {
    pub ring_m: Vec<(f32, f32)>,
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
        let margin = CELL_M as f64;
        let proj = |(lon, lat): (f64, f64)| -> (f32, f32) {
            (((lon - min_lon) * mx + margin) as f32, ((max_lat - lat) * my + margin) as f32)
        };
        let ring_m: Vec<(f32, f32)> = FOOTPRINT_LONLAT.iter().map(|&p| proj(p)).collect();
        let width_m = ring_m.iter().map(|p| p.0).fold(0.0, f32::max) + margin as f32;
        let depth_m = ring_m.iter().map(|p| p.1).fold(0.0, f32::max) + margin as f32;
        Site { ring_m, width_m, depth_m, south_gate_m: proj(SOUTH_GATE_LONLAT) }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        let r = &self.ring_m;
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
