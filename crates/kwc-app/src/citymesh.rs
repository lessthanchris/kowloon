//! Turns a City at a given year into low-poly geometry: exposed wall faces
//! storey by storey (with windows), roofs, lanes, wells, empty plots, the Yamen.

use glam::Vec3;
use kwc_engine::mesh::{Faces, MeshData};
use kwc_sim::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourMode {
    Grime,
    Decade,
}

/// sRGB 0–255 → linear.
pub fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    let f = |c: u8| (c as f32 / 255.0).powf(2.2);
    [f(r), f(g), f(b)]
}

fn mul(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Cheap deterministic hash → [0,1).
pub fn hash(a: u32, b: u32, c: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65536.0
}

/// Facade palettes: grubby concrete, stained mosaic tile, faded paint.
fn facade(plot: u32) -> [f32; 3] {
    const P: &[[u8; 3]] = &[
        [120, 116, 106],
        [138, 132, 118],
        [104, 104, 98],
        [126, 134, 124], // pale green tile
        [140, 124, 118], // pink tile
        [116, 124, 136], // faded blue
        [150, 142, 120], // cream paint
        [96, 92, 86],
    ];
    let c = P[(hash(plot, 1, 7) * P.len() as f32) as usize % P.len()];
    srgb(c[0], c[1], c[2])
}

fn decade(year: u16) -> [f32; 3] {
    let c = match year {
        0..=1954 => [110, 84, 60],
        1955..=1959 => [150, 110, 62],
        1960..=1964 => [178, 146, 70],
        1965..=1969 => [160, 170, 88],
        1970..=1974 => [96, 150, 120],
        1975..=1979 => [80, 120, 160],
        _ => [130, 96, 170],
    };
    srgb(c[0], c[1], c[2])
}

pub const YAMEN_H: f32 = 4.5;

/// Height (m) of whatever stands on a cell.
fn col_height(city: &City, c: Cell, year: u16) -> f32 {
    match city.ground_at(c) {
        Ground::Plot => city.height_at(c, year) as f32 * STOREY_M,
        Ground::Yamen => yamen_height(city, c),
        _ => 0.0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum YamenPart {
    Hall,
    Wall,
    Yard,
}

/// The Yamen compound: three low halls across its long axis, courtyards
/// between them, a boundary wall with a gate at the south end.
pub fn yamen_part(city: &City, c: Cell) -> YamenPart {
    static BOX: std::sync::OnceLock<(i32, i32, i32, i32)> = std::sync::OnceLock::new();
    let &(i0, i1, j0, j1) = BOX.get_or_init(|| {
        let cells: Vec<Cell> = (0..city.w * city.d).filter(|&k| city.ground[k] == Ground::Yamen).map(|k| ((k % city.w) as u16, (k / city.w) as u16)).collect();
        let f = |g: fn(&Cell) -> u16| cells.iter().map(g).collect::<Vec<u16>>();
        let (is, js) = (f(|c| c.0), f(|c| c.1));
        (*is.iter().min().unwrap() as i32, *is.iter().max().unwrap() as i32, *js.iter().min().unwrap() as i32, *js.iter().max().unwrap() as i32)
    });
    let (i, j) = (c.0 as i32, c.1 as i32);
    let edge = city.neighbours(c).any(|(_, n)| city.ground_at(n) != Ground::Yamen) || city.neighbours(c).count() < 4;
    let mid_i = (i0 + i1) / 2;
    if edge {
        // The gate: a two-cell gap in the south wall.
        return if j >= j1 - 1 && (i - mid_i).abs() <= 1 { YamenPart::Yard } else { YamenPart::Wall };
    }
    let len = (j1 - j0 + 1) as f32;
    let r = (j - j0) as f32 / len;
    let hall = [(0.12, 0.30), (0.44, 0.60), (0.72, 0.84)].iter().any(|&(a, b)| r >= a && r < b);
    if hall { YamenPart::Hall } else { YamenPart::Yard }
}

pub fn yamen_height(city: &City, c: Cell) -> f32 {
    match yamen_part(city, c) {
        YamenPart::Hall => YAMEN_H,
        YamenPart::Wall => 1.8,
        YamenPart::Yard => 0.0,
    }
}

pub fn near_yamen(city: &City, c: Cell) -> bool {
    (-1..=1).any(|dj: i32| {
        (-1..=1).any(|di: i32| {
            let (i, j) = (c.0 as i32 + di, c.1 as i32 + dj);
            city.in_bounds(i, j) && city.ground_at((i as u16, j as u16)) == Ground::Yamen
        })
    })
}

/// Storey faces left open in facades: ground-floor stair entrances onto lanes,
/// and bridge ends.
fn openings(city: &City, year: u16) -> std::collections::HashSet<(Cell, Dir, i32)> {
    let mut o = std::collections::HashSet::new();
    for p in &city.plots {
        if p.height_at(year) == 0 {
            continue;
        }
        // Both landing cells open onto the lane at street level.
        for l in [p.core[0], p.core[5]] {
            for (d, n) in city.neighbours(l) {
                if city.ground_at(n) == Ground::Alley {
                    o.insert((l, d, 0));
                }
            }
        }
    }
    let dir = |a: Cell, b: Cell| Dir::ALL.into_iter().find(|&d| city.step(a, d) == Some(b));
    for br in city.bridges.iter().filter(|b| b.year <= year) {
        let fa = br.span.first().copied().unwrap_or(br.b);
        let fb = br.span.last().copied().unwrap_or(br.a);
        if let Some(d) = dir(br.a, fa) {
            o.insert((br.a, d, br.floor as i32));
        }
        if let Some(d) = dir(br.b, fb) {
            o.insert((br.b, d, br.floor as i32));
        }
    }
    o
}

/// How much sky a point sees, 0..1, from the heights around it: open ground
/// in 1950 is bright, a lane at the foot of 1987's canyons is dim. Looks out
/// 15 m in eight directions (or only the half in front of a wall facing `n`).
pub struct Sky {
    w: i32,
    d: i32,
    h: Vec<f32>,
}

impl Sky {
    pub fn new(city: &City, year: u16, over: &std::collections::HashMap<Cell, (f32, f32)>) -> Sky {
        let mut h = vec![0.0; city.w * city.d];
        for j in 0..city.d as u16 {
            for i in 0..city.w as u16 {
                let c = (i, j);
                h[city.idx(c)] = col_height(city, c, year).max(over.get(&c).map_or(0.0, |o| o.1));
            }
        }
        Sky { w: city.w as i32, d: city.d as i32, h }
    }

    pub fn open(&self, p: Vec3, n: Option<Vec3>) -> f32 {
        let (mut sum, mut count) = (0.0f32, 0.0f32);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let (dx, dz) = (a.cos(), a.sin());
            if n.is_some_and(|n| n.x * dx + n.z * dz < -0.1) {
                continue;
            }
            let mut best = 0.0f32;
            for step in 1..=10 {
                let r = step as f32 * CELL_M;
                let (i, j) = (((p.x + dx * r) / CELL_M).floor() as i32, ((p.z + dz * r) / CELL_M).floor() as i32);
                if i < 0 || j < 0 || i >= self.w || j >= self.d {
                    break;
                }
                best = best.max((self.h[(j * self.w + i) as usize] - p.y) / r);
            }
            sum += best / (1.0 + best * best).sqrt();
            count += 1.0;
        }
        1.0 - sum / count.max(1.0)
    }

    /// As a vertex occlusion factor.
    pub fn ao(&self, p: Vec3, n: Option<Vec3>) -> f32 {
        0.18 + 0.82 * self.open(p, n)
    }
}

pub fn build(city: &City, year: u16, mode: ColourMode) -> MeshData {
    let mut m = MeshData::default();
    let s = CELL_M;
    let holes = openings(city, year);
    let over: std::collections::HashMap<Cell, (f32, f32)> =
        crate::world::overbuild(city, year).into_iter().map(|(c, a, t)| (c, (a, t))).collect();
    let sky = Sky::new(city, year, &over);

    // Surroundings: a dark plane under everything.
    let (w, d) = (city.w as f32 * s, city.d as f32 * s);
    let pad = 2500.0;
    m.quad(
        [
            Vec3::new(-pad, -0.05, -pad),
            Vec3::new(-pad, -0.05, d + pad),
            Vec3::new(w + pad, -0.05, d + pad),
            Vec3::new(w + pad, -0.05, -pad),
        ],
        srgb(38, 40, 36),
        0.0,
    );

    for j in 0..city.d as u16 {
        for i in 0..city.w as u16 {
            let c = (i, j);
            let g = city.ground_at(c);
            if g == Ground::Outside {
                continue;
            }
            let (x0, z0) = (i as f32 * s, j as f32 * s);
            let (x1, z1) = (x0 + s, z0 + s);
            let h = col_height(city, c, year);

            if h <= 0.0 {
                // Lanes are packed earth out in the open and wet, dark
                // concrete where the buildings close in.
                let open = sky.ao(Vec3::new(x0 + s / 2.0, 0.3, z0 + s / 2.0), None);
                let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
                let t = ((open - 0.3) / 0.6).clamp(0.0, 1.0);
                let col = match g {
                    Ground::Yamen => srgb(120, 112, 98), // stone-paved courtyard
                    Ground::Alley => lerp(srgb(46, 46, 48), srgb(118, 104, 84), t),
                    Ground::Well => srgb(30, 32, 34),
                    _ => lerp(srgb(58, 62, 44), srgb(96, 94, 64), t), // empty plot: scrub and huts' footprints
                };
                let n = 0.9 + 0.2 * hash(i as u32, j as u32, 3);
                m.ao = if over.contains_key(&c) { 0.35f32.min(open) } else { open };
                m.quad([Vec3::new(x0, 0.0, z0), Vec3::new(x0, 0.0, z1), Vec3::new(x1, 0.0, z1), Vec3::new(x1, 0.0, z0)], mul(col, n), 0.0);
                m.ao = 1.0;
                if let Some(&(y0, y1)) = over.get(&c) {
                    overbuild_box(&mut m, city, c, y0, y1, &over);
                }
                continue;
            }

            let (base, roof) = match g {
                Ground::Yamen if yamen_part(city, c) == YamenPart::Wall => (srgb(140, 130, 116), srgb(110, 100, 90)),
                Ground::Yamen => (srgb(150, 60, 45), srgb(60, 66, 70)), // red walls, grey tile roofs
                _ => {
                    let p = city.plot_at(c).unwrap();
                    let b = match mode {
                        ColourMode::Grime => facade(p.id),
                        ColourMode::Decade => decade(if p.rebuilt != 0 && p.rebuilt <= year { p.rebuilt } else { p.founded }),
                    };
                    let r = mul(srgb(92, 90, 86), 0.8 + 0.4 * hash(p.id, 2, 9));
                    (b, r)
                }
            };

            // Roof (the stair's is its hut, drawn with the roof furniture).
            if city.role_at(c) != Role::Stair {
                m.cuboid(Vec3::new(x0, h, z0), Vec3::new(x1, h, z1), roof, 0.0, Faces { top: true, bottom: false, px: false, nx: false, pz: false, nz: false });
            }

            // Walls: only where the neighbour is lower, storey by storey.
            let pid = city.plot_of[city.idx(c)];
            for dir in Dir::ALL {
                let (hn, nb_plot) = match city.step(c, dir) {
                    Some(n) => (col_height(city, n, year), city.plot_of[city.idx(n)]),
                    None => (0.0, NO_PLOT),
                };
                if hn >= h {
                    continue;
                }
                // Walls inside one building's footprint are never exposed at the same height,
                // so an exposed face here is a facade or a party wall above a neighbour.
                let exterior = nb_plot != pid;
                let face = match dir {
                    Dir::N => Faces { top: false, bottom: false, px: false, nx: false, pz: false, nz: true },
                    Dir::S => Faces { top: false, bottom: false, px: false, nx: false, pz: true, nz: false },
                    Dir::E => Faces { top: false, bottom: false, px: true, nx: false, pz: false, nz: false },
                    Dir::W => Faces { top: false, bottom: false, px: false, nx: true, pz: false, nz: false },
                };
                let floors = (h / STOREY_M).round() as i32;
                let first = (hn / STOREY_M).floor() as i32;
                for f in first.max(0)..floors.max(1) {
                    let y0 = (f as f32 * STOREY_M).max(hn);
                    let y1 = ((f + 1) as f32 * STOREY_M).min(h);
                    if y1 <= y0 || holes.contains(&(c, dir, f)) {
                        continue;
                    }
                    let stain = 0.82 + 0.3 * hash(i as u32 ^ (f as u32) << 16, j as u32, dir as u32);
                    let (dx, dz) = dir.delta();
                    let nrm = Vec3::new(dx as f32, 0.0, dz as f32);
                    m.ao = sky.ao(Vec3::new(x0 + s / 2.0, (y0 + y1) / 2.0, z0 + s / 2.0) + nrm * (s / 2.0 + 0.2), Some(nrm));
                    m.cuboid(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1), mul(base, stain), 0.0, face);

                    // No windows at street level: that's shopfronts and doors.
                    if g == Ground::Plot && exterior && y1 - y0 > 2.0 && f > 0 {
                        window(&mut m, (x0, z0, x1, z1), dir, y0, i, j, f, year);
                    }
                }
            }
        }
    }
    let centre = |a: &crate::world::Aabb| (a.min + a.max) * 0.5;
    for fx in crate::lights::fixtures(city, year) {
        m.ao = sky.ao(centre(&fx.aabb), None);
        m.cuboid(fx.aabb.min, fx.aabb.max, fx.col, fx.emit, Faces::ALL);
    }
    let (mut pieces, _) = crate::world::roof_furniture(city, year);
    pieces.extend(crate::world::squatters(city, year));
    pieces.extend(crate::world::surroundings(city, year));
    for p in pieces {
        m.ao = sky.ao(centre(&p.aabb), None);
        m.cuboid(p.aabb.min, p.aabb.max, p.col, p.emit, p.faces);
    }
    m.ao = 1.0;
    m
}

/// A lane cell built over: roof on top, blank party walls where it ends.
fn overbuild_box(m: &mut MeshData, city: &City, c: Cell, y0: f32, y1: f32, over: &std::collections::HashMap<Cell, (f32, f32)>) {
    let s = CELL_M;
    let (x0, z0) = (c.0 as f32 * s, c.1 as f32 * s);
    let (x1, z1) = (x0 + s, z0 + s);
    let col = facade(hash(c.0 as u32, c.1 as u32, 11).to_bits());
    m.cuboid(Vec3::new(x0, y1, z0), Vec3::new(x1, y1, z1), mul(srgb(92, 90, 86), 0.9), 0.0, Faces { top: true, bottom: false, px: false, nx: false, pz: false, nz: false });
    for d in Dir::ALL {
        let Some(n) = city.step(c, d) else { continue };
        // Exposed where the neighbour is open lane (or a lower overbuild).
        let nt = match city.ground_at(n) {
            Ground::Alley => over.get(&n).map_or(0.0, |o| o.1),
            Ground::Plot => city.height_at(n, u16::MAX) as f32 * STOREY_M,
            _ => 0.0,
        };
        if city.ground_at(n) == Ground::Plot || nt >= y1 {
            continue;
        }
        let lo = if over.contains_key(&n) { nt.max(y0) } else { y0 };
        let face = match d {
            Dir::N => Faces { top: false, bottom: false, px: false, nx: false, pz: false, nz: true },
            Dir::S => Faces { top: false, bottom: false, px: false, nx: false, pz: true, nz: false },
            Dir::E => Faces { top: false, bottom: false, px: true, nx: false, pz: false, nz: false },
            Dir::W => Faces { top: false, bottom: false, px: false, nx: true, pz: false, nz: false },
        };
        m.cuboid(Vec3::new(x0, lo, z0), Vec3::new(x1, y1, z1), mul(col, 0.85), 0.0, face);
    }
}

/// A window on a storey's wall face: dark glass, or lit (warm tungsten or cold
/// fluorescent) at random. Sits slightly proud of the wall.
fn window(m: &mut MeshData, (x0, z0, x1, z1): (f32, f32, f32, f32), dir: Dir, y0: f32, i: u16, j: u16, f: i32, year: u16) {
    let r = hash(i as u32 * 31 + dir as u32, j as u32, f as u32);
    if r < 0.18 {
        return; // blank wall / blocked up
    }
    let lit = hash(i as u32, j as u32 * 7 + dir as u32, f as u32 + year as u32 % 3) < 0.42;
    let (col, emit) = if lit {
        if hash(i as u32, j as u32, 99 + f as u32) < 0.55 {
            (srgb(255, 236, 190), 1.0) // fluorescent tube
        } else {
            (srgb(255, 170, 90), 1.0) // bulb
        }
    } else {
        (srgb(24, 28, 32), 0.0)
    };
    let (wy0, wy1) = (y0 + 0.9, y0 + 2.0);
    let e = 0.04;
    let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
    let hw = 0.45;
    let only = |px, nx, pz, nz| Faces { top: false, bottom: false, px, nx, pz, nz };
    match dir {
        Dir::N => m.cuboid(Vec3::new(cx - hw, wy0, z0 - e), Vec3::new(cx + hw, wy1, z0), col, emit, only(false, false, false, true)),
        Dir::S => m.cuboid(Vec3::new(cx - hw, wy0, z1), Vec3::new(cx + hw, wy1, z1 + e), col, emit, only(false, false, true, false)),
        Dir::E => m.cuboid(Vec3::new(x1, wy0, cz - hw), Vec3::new(x1 + e, wy1, cz + hw), col, emit, only(true, false, false, false)),
        Dir::W => m.cuboid(Vec3::new(x0 - e, wy0, cz - hw), Vec3::new(x0, wy1, cz + hw), col, emit, only(false, true, false, false)),
    }
}

/// Centre of the site in world space.
pub fn centre(city: &City) -> Vec3 {
    let (mut sx, mut sz, mut n) = (0.0, 0.0, 0.0);
    for j in 0..city.d {
        for i in 0..city.w {
            if city.ground[j * city.w + i] != Ground::Outside {
                sx += i as f32;
                sz += j as f32;
                n += 1.0;
            }
        }
    }
    Vec3::new(sx / n * CELL_M, 0.0, sz / n * CELL_M)
}
