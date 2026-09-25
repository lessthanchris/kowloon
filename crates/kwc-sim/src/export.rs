//! Headless outputs: plan PNGs (height / decade / floor slice) and JSON.

use crate::city::*;
use crate::site::MAX_FLOORS;
use image::{Rgb, RgbImage};

pub const PX: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub enum MapMode {
    Height,
    Decade,
    Slice(u8),
}

fn decade_colour(year: u16) -> [u8; 3] {
    match year {
        0..=1954 => [110, 84, 60],
        1955..=1959 => [150, 110, 62],
        1960..=1964 => [178, 146, 70],
        1965..=1969 => [160, 170, 88],
        1970..=1974 => [96, 150, 120],
        1975..=1979 => [80, 120, 160],
        _ => [130, 96, 170],
    }
}

pub fn render_map(city: &City, year: u16, mode: MapMode) -> RgbImage {
    let mut img = RgbImage::from_pixel(city.w as u32 * PX, city.d as u32 * PX, Rgb([15, 17, 13]));
    let bridges: Vec<&Bridge> = city.bridges.iter().filter(|b| b.year <= year).collect();
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let h = city.height_at(c, year);
            let col: [u8; 3] = match (city.ground_at(c), mode) {
                (Ground::Outside, _) => continue,
                (Ground::Yamen, _) => [150, 58, 44],
                (Ground::Alley, MapMode::Slice(f)) if bridges.iter().any(|b| b.floor == f && b.span.contains(&c)) => [220, 190, 90],
                (Ground::Alley, _) => [38, 36, 40],
                (Ground::Well, _) => [26, 40, 66],
                (Ground::Plot, _) if h == 0 => [52, 58, 44], // empty plot / scrub
                (Ground::Plot, MapMode::Height) => {
                    let t = h as f32 / MAX_FLOORS as f32;
                    let v = (70.0 + 170.0 * t) as u8;
                    if h as usize >= MAX_FLOORS { [v, v, (v as f32 * 0.85) as u8] } else { [v, v, v] }
                }
                (Ground::Plot, MapMode::Decade) => {
                    let p = city.plot_at(c).unwrap();
                    let y = if p.rebuilt != 0 && p.rebuilt <= year { p.rebuilt } else { p.founded };
                    decade_colour(y)
                }
                (Ground::Plot, MapMode::Slice(f)) => {
                    if h <= f {
                        [30, 30, 30]
                    } else {
                        match city.role_at(c) {
                            Role::Core => [220, 150, 50],
                            Role::Stair => [250, 200, 90],
                            _ if city.is_corridor(c, f) => [140, 100, 50],
                            _ => [200, 200, 190],
                        }
                    }
                }
            };
            for y in 0..PX {
                for x in 0..PX {
                    img.put_pixel(i as u32 * PX + x, j as u32 * PX + y, Rgb(col));
                }
            }
        }
    }
    // Plot outlines, so buildings read as separate blocks.
    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let p = city.plot_of[city.idx(c)];
            if p == NO_PLOT || city.height_at(c, year) == 0 {
                continue;
            }
            for (d, n) in city.neighbours(c) {
                if city.plot_of[city.idx(n)] != p {
                    for t in 0..PX {
                        let (x, y) = match d {
                            Dir::N => (t, 0),
                            Dir::S => (t, PX - 1),
                            Dir::W => (0, t),
                            Dir::E => (PX - 1, t),
                        };
                        let px = img.get_pixel_mut(i as u32 * PX + x, j as u32 * PX + y);
                        px.0 = px.0.map(|v| (v as f32 * 0.55) as u8);
                    }
                }
            }
        }
    }
    // Documented features: standpipes (cyan), natural well (blue), lifts (white),
    // temples (red-gold), South Gate (orange).
    for f in &city.features {
        let col = match f.kind {
            FeatureKind::WaterStandpipe => [60, 220, 230],
            FeatureKind::NaturalWell => [40, 90, 255],
            FeatureKind::Lift => [255, 255, 255],
            FeatureKind::Temple => [230, 60, 40],
            FeatureKind::SouthGate => [255, 150, 0],
        };
        let (i, j) = f.cell;
        for y in 0..PX {
            for x in 0..PX {
                if (x == 0 || x == PX - 1) && (y == 0 || y == PX - 1) {
                    continue;
                }
                img.put_pixel(i as u32 * PX + x, j as u32 * PX + y, Rgb(col));
            }
        }
    }
    // Unit doors on the slice view.
    if let MapMode::Slice(f) = mode {
        for u in city.units_at(year).filter(|u| u.floor == f) {
            let (i, j) = u.door.cell;
            let (dx, dy) = match u.door.facing {
                Dir::N => (PX / 2, 0),
                Dir::S => (PX / 2, PX - 1),
                Dir::W => (0, PX / 2),
                Dir::E => (PX - 1, PX / 2),
            };
            let col = if u.door_state == DoorState::Open { [255, 80, 150] } else { [90, 60, 40] };
            img.put_pixel(i as u32 * PX + dx, j as u32 * PX + dy, Rgb(col));
        }
    }
    img
}

pub fn to_json(city: &City) -> String {
    serde_json::to_string(city).expect("serialise city")
}
