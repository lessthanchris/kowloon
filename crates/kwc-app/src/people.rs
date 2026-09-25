//! People: figures in the lanes, on doorsteps, at the standpipes and on the
//! roofs. Who they are comes from the society sim, where they stand from the
//! city, and how many there are and what they wear from the year.
//!
//! Figures are low-poly box people rebuilt each frame near the player (so
//! walkers can walk), lit by the same baked lamp light and sky openness as
//! the walls around them.

use crate::citymesh::{hash, srgb, Sky};
use crate::lights::PointLight;
use crate::plane::obox;
use crate::world::{Aabb, WalkWorld, S};
use glam::{Mat3, Vec3};
use kwc_engine::mesh::MeshData;
use kwc_sim::society::{Occupant, Sex, Society};
use kwc_sim::*;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const C: f32 = CELL_M;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pose {
    Stand,
    Sit,
    Walk,
}

/// What they're carrying.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prop {
    None,
    /// A shoulder pole with a basket at each end.
    Pole,
    /// A bucket in each hand (the standpipe run).
    Buckets,
    Bag,
}

pub struct Figure {
    /// Name, and a line about them (age, where they live or work).
    pub name: String,
    pub about: String,
    scale: f32,
    stoop: f32,
    top: [f32; 3],
    bottom: [f32; 3],
    skin: [f32; 3],
    hair: [f32; 3],
    long_hair: bool,
    pose: Pose,
    prop: Prop,
    /// Where they are, or the route they pace up and down.
    path: Vec<Vec3>,
    cum: Vec<f32>,
    yaw: f32,
    s: f32,
    dir: f32,
    speed: f32,
    /// Lamp light at each path point, and how much sky they see.
    light: Vec<Vec3>,
    ao: f32,
    /// Indoors by night.
    day_only: bool,
}

/// Who someone is, for a figure.
struct Identity {
    name: String,
    about: String,
    age: u16,
    female: bool,
}

impl Figure {
    fn new(id: &Identity, path: Vec<Vec3>, yaw: f32, pose: Pose, prop: Prop, year: u16, rng: &mut ChaCha8Rng) -> Figure {
        let age = id.age as f32;
        let mut scale = if age < 14.0 { (0.5 + age * 0.037).clamp(0.55, 1.0) } else if id.female { 0.94 } else { 1.0 };
        scale *= 1.0 + rng.gen_range(-0.04..0.04);
        let stoop = if age > 64.0 { 0.28 } else { 0.03 };
        let (top, bottom) = clothes(year, id.female, age, rng);
        let tone = rng.gen_range(0.85..1.1);
        let skin = [0.55 * tone, 0.36 * tone, 0.22 * tone];
        let hair = if age > 60.0 { srgb(170, 168, 162) } else { srgb(22, 20, 20) };
        let mut cum = vec![0.0];
        for w in path.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let speed = if age < 14.0 { 1.4 } else if age > 64.0 { 0.7 } else { rng.gen_range(0.9..1.3) } * if prop == Prop::Pole { 1.15 } else { 1.0 };
        let total = *cum.last().unwrap();
        Figure {
            name: id.name.clone(),
            about: id.about.clone(),
            scale,
            stoop,
            top,
            bottom,
            skin,
            hair,
            long_hair: id.female && rng.gen_bool(0.6),
            pose,
            prop,
            light: vec![Vec3::ZERO; path.len()],
            path,
            cum,
            yaw,
            s: rng.gen_range(0.0..total.max(0.001)),
            dir: if rng.gen_bool(0.5) { 1.0 } else { -1.0 },
            speed,
            ao: 1.0,
            day_only: false,
        }
    }

    /// Where they are now, which way they face, and the nearest path point.
    fn at(&self) -> (Vec3, f32, usize) {
        if self.path.len() < 2 {
            return (self.path[0], self.yaw, 0);
        }
        let i = self.cum.partition_point(|&c| c <= self.s).clamp(1, self.path.len() - 1) - 1;
        let len = (self.cum[i + 1] - self.cum[i]).max(1e-4);
        let f = ((self.s - self.cum[i]) / len).clamp(0.0, 1.0);
        let (a, b) = (self.path[i], self.path[i + 1]);
        let d = (b - a) * self.dir;
        (a.lerp(b, f), d.z.atan2(d.x), if f < 0.5 { i } else { i + 1 })
    }

    fn head(&self) -> Vec3 {
        let (p, _, _) = self.at();
        p + Vec3::Y * (if self.pose == Pose::Sit { 1.15 } else { 1.52 }) * self.scale
    }
}

fn clothes(year: u16, female: bool, age: f32, rng: &mut ChaCha8Rng) -> ([f32; 3], [f32; 3]) {
    let pick = |rng: &mut ChaCha8Rng, p: &[[u8; 3]]| {
        let c = p.choose(rng).unwrap();
        srgb(c[0], c[1], c[2])
    };
    if year < 1966 {
        if female && age >= 14.0 {
            // Samfu: a matching jacket and trousers.
            let c = pick(rng, &[[28, 30, 36], [40, 52, 80], [120, 140, 160], [150, 130, 110], [70, 60, 56]]);
            (c, if rng.gen_bool(0.7) { c } else { srgb(26, 26, 30) })
        } else {
            let t = pick(rng, &[[225, 222, 210], [225, 222, 210], [150, 160, 170], [90, 100, 120], [170, 150, 120]]);
            (t, pick(rng, &[[30, 30, 34], [44, 50, 70], [120, 108, 84], [60, 58, 54]]))
        }
    } else {
        let t = pick(rng, &[[230, 228, 220], [90, 130, 190], [190, 60, 50], [220, 190, 80], [80, 140, 90], [200, 150, 160], [60, 60, 70]]);
        let b = if female && rng.gen_bool(0.4) {
            pick(rng, &[[40, 40, 50], [140, 60, 70], [60, 80, 120]])
        } else {
            pick(rng, &[[50, 70, 110], [40, 42, 48], [110, 100, 80], [60, 80, 120]])
        };
        (t, b)
    }
}

/// A local frame: origin and axes (columns x right, y up, z forward), scaled.
#[derive(Clone, Copy)]
struct Frame {
    o: Vec3,
    m: Mat3,
}

impl Frame {
    /// A child frame at `pivot`, pitched about its x axis (+ tips y towards z).
    fn child(&self, pivot: Vec3, pitch: f32) -> Frame {
        Frame { o: self.o + self.m * pivot, m: self.m * Mat3::from_rotation_x(pitch) }
    }

    /// A box centred at `c`, half-size (width, height, depth).
    fn part(&self, mesh: &mut MeshData, c: Vec3, h: Vec3, col: [f32; 3]) {
        let (x, y, z) = (self.m.x_axis, self.m.y_axis, self.m.z_axis);
        obox(mesh, self.o + self.m * c, z, -x, y, Vec3::new(h.z, h.x, h.y), col, 0.0);
    }
}

fn figure_mesh(m: &mut MeshData, f: &Figure, t: f32, detail: bool) {
    let (pos, yaw, _) = f.at();
    let fwd = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let rot = Mat3::from_cols(Vec3::Y.cross(fwd), Vec3::Y, fwd) * f.scale;
    let root = Frame { o: pos, m: rot };
    let walking = f.pose == Pose::Walk && f.path.len() > 1;
    let phase = if walking { f.s * 3.2 } else { 0.0 };
    let swing = if walking { phase.sin() * 0.45 } else { 0.0 };
    // A little life when still: breathing, shifting weight.
    let idle = if walking { 0.0 } else { (t * 0.9 + f.cum.len() as f32 + pos.x).sin() * 0.03 };
    let shoe = srgb(30, 28, 26);

    let hip_y = if f.pose == Pose::Sit { 0.45 } else { 0.84 };
    if f.pose == Pose::Sit {
        root.part(m, Vec3::new(0.0, 0.21, -0.05), Vec3::new(0.17, 0.21, 0.17), srgb(140, 100, 64));
        for side in [-1.0f32, 1.0] {
            root.part(m, Vec3::new(side * 0.09, hip_y, 0.2), Vec3::new(0.075, 0.08, 0.22), f.bottom);
            root.part(m, Vec3::new(side * 0.09, 0.225, 0.4), Vec3::new(0.07, 0.225, 0.075), f.bottom);
            if detail {
                root.part(m, Vec3::new(side * 0.09, 0.03, 0.45), Vec3::new(0.07, 0.03, 0.1), shoe);
            }
        }
    } else {
        for side in [-1.0f32, 1.0] {
            let leg = root.child(Vec3::new(side * 0.09, hip_y, 0.0), swing * side);
            leg.part(m, Vec3::new(0.0, -0.42, 0.0), Vec3::new(0.075, 0.42, 0.08), f.bottom);
            if detail {
                leg.part(m, Vec3::new(0.0, -0.81, 0.04), Vec3::new(0.07, 0.03, 0.11), shoe);
            }
        }
    }
    let torso = root.child(Vec3::new(0.0, hip_y, 0.0), f.stoop + idle);
    torso.part(m, Vec3::new(0.0, 0.28, 0.0), Vec3::new(0.19, 0.28, 0.11), f.top);
    torso.part(m, Vec3::new(0.0, 0.68, 0.01), Vec3::new(0.095, 0.11, 0.105), f.skin);
    torso.part(m, Vec3::new(0.0, 0.78, -0.01), Vec3::new(0.105, 0.03, 0.115), f.hair);
    if f.long_hair {
        torso.part(m, Vec3::new(0.0, 0.66, -0.09), Vec3::new(0.1, 0.13, 0.03), f.hair);
    }
    // Arms: swinging, down with buckets, or resting forward when sitting.
    let arm_pitch = match (f.pose, f.prop) {
        (Pose::Sit, _) => -0.6,
        (_, Prop::Buckets) => 0.0,
        (_, Prop::Pole) => -1.2,
        _ => 0.0,
    };
    for side in [-1.0f32, 1.0] {
        let sw = if f.prop == Prop::Buckets || f.prop == Prop::Pole { 0.0 } else { -swing * side * 0.8 };
        let pitch = if f.prop == Prop::Pole && side > 0.0 { arm_pitch } else if f.pose == Pose::Sit { arm_pitch } else { sw };
        let arm = torso.child(Vec3::new(side * 0.25, 0.52, 0.0), pitch);
        arm.part(m, Vec3::new(0.0, -0.28, 0.0), Vec3::new(0.055, 0.28, 0.06), f.top);
        if detail {
            arm.part(m, Vec3::new(0.0, -0.6, 0.0), Vec3::new(0.045, 0.05, 0.045), f.skin);
        }
    }
    // What they carry.
    match f.prop {
        Prop::Pole => {
            let bob = (phase * 2.0).sin() * 0.03;
            let pole = torso.child(Vec3::new(0.2, 0.6, 0.0), 0.0);
            pole.part(m, Vec3::ZERO, Vec3::new(0.025, 0.025, 0.85), srgb(176, 152, 96));
            for end in [-0.8f32, 0.8] {
                root.part(m, Vec3::new(0.2, 0.55 + bob, end), Vec3::new(0.22, 0.17, 0.22), srgb(150, 120, 70));
                if detail {
                    root.part(m, Vec3::new(0.2, 1.05 + bob, end), Vec3::new(0.01, 0.33, 0.01), srgb(120, 100, 70));
                }
            }
        }
        Prop::Buckets => {
            for side in [-1.0f32, 1.0] {
                root.part(m, Vec3::new(side * 0.33, 0.1 + 0.16, 0.0), Vec3::new(0.13, 0.16, 0.13), srgb(150, 152, 150));
            }
        }
        Prop::Bag => {
            root.part(m, Vec3::new(0.32, 0.25 + 0.12, 0.0), Vec3::new(0.06, 0.16, 0.2), srgb(120, 80, 50));
        }
        Prop::None => {}
    }
}

pub struct Crowd {
    pub figures: Vec<Figure>,
    /// Someone at a door you just knocked on: the figure, and time left.
    greeter: Option<(Figure, f32)>,
}

/// Walk down from `p` to the floor under it (if there is one within a storey).
fn settle(w: &WalkWorld, p: Vec3) -> Option<Vec3> {
    let mut y = p.y + 0.6;
    while y > p.y - 1.4 {
        let q = Aabb::new(Vec3::new(p.x - 0.1, y - 0.02, p.z - 0.1), Vec3::new(p.x + 0.1, y, p.z + 0.1));
        if y - 0.02 < 0.0 || w.blocked(&q) {
            return Some(Vec3::new(p.x, y, p.z));
        }
        y -= 0.02;
    }
    None
}

fn free(w: &WalkWorld, p: Vec3, r: f32, h: f32) -> bool {
    !w.blocked(&Aabb::new(Vec3::new(p.x - r, p.y + 0.05, p.z - r), Vec3::new(p.x + r, p.y + h, p.z + r)))
}

fn cell_centre(c: Cell) -> Vec3 {
    Vec3::new((c.0 as f32 + 0.5) * C, 0.0, (c.1 as f32 + 0.5) * C)
}

fn person<'a>(soc: &'a Society, id: u32) -> Option<&'a society::Person> {
    soc.people.get(id as usize).filter(|p| p.id == id).or_else(|| soc.people.iter().find(|p| p.id == id))
}

/// Someone from the unit: a household member, or the shopkeeper.
fn resident(soc: &Society, unit: u32, year: u16, rng: &mut ChaCha8Rng, adult: bool) -> Option<Identity> {
    let occ = soc.occupants(unit, year);
    let addr = soc.directory.address[unit as usize].line();
    match *occ.first()? {
        Occupant::Household(h) => {
            let members = soc.members_in(h, year);
            let pool: Vec<_> = members.iter().filter(|p| !adult || p.age(year) >= 16).collect();
            let p = pool.choose(rng)?;
            Some(Identity { name: p.name(), about: format!("{} · lives at {addr}", p.age(year)), age: p.age(year), female: p.sex == Sex::F })
        }
        Occupant::Business(b) => {
            let biz = &soc.businesses[b as usize];
            let owner = biz.owners.iter().rev().find(|o| o.1 <= year)?;
            let head = person(soc, soc.households.get(owner.0 as usize)?.head)?;
            Some(Identity { name: head.name(), about: format!("{} · keeps {}", head.age(year), biz.name), age: head.age(year), female: head.sex == Sex::F })
        }
        Occupant::Temple => None,
    }
}

/// Someone from outside the society's records: a squatter family.
fn squatter(rng: &mut ChaCha8Rng, age: u16, surname: &str) -> Identity {
    let female = rng.gen_bool(0.5);
    let about = if age < 14 { format!("{age} · a squatter's child") } else { format!("{age} · lives in a hut") };
    Identity { name: format!("{surname} {}", names::given(rng, female)), about, age, female }
}

impl Crowd {
    pub fn new(city: &City, soc: &Society, w: &WalkWorld, year: u16, lamps: &[PointLight]) -> Crowd {
        let mut rng = ChaCha8Rng::seed_from_u64(0x9e0_0000 + year as u64);
        let mut figs: Vec<Figure> = vec![];
        let t = ((year as f32 - 1950.0) / 37.0).clamp(0.0, 1.0);
        let units: Vec<&Unit> = city.units_at(year).collect();
        let homes: Vec<u32> = units
            .iter()
            .filter(|u| matches!(soc.occupants(u.id, year).first(), Some(Occupant::Household(_))))
            .map(|u| u.id)
            .collect();
        let anyone = |rng: &mut ChaCha8Rng| -> Option<Identity> {
            for _ in 0..6 {
                if let Some(id) = homes.choose(rng).and_then(|&u| resident(soc, u, year, rng, false)) {
                    return Some(id);
                }
            }
            None
        };
        let face = |from: Vec3, to: Vec3| (to.z - from.z).atan2(to.x - from.x);

        // Squatter families at their hut doors.
        for (door, out) in crate::world::hut_doors(city, year) {
            if !rng.gen_bool(0.3) {
                continue;
            }
            let surname = names::surname(&mut rng);
            let along = Vec3::new(-out.z, 0.0, out.x);
            let seat = door + out * 0.45 + along * 0.55;
            if free(w, seat, 0.25, 1.2) {
                let age = rng.gen_range(25..75);
                let id = squatter(&mut rng, age, surname);
                figs.push(Figure::new(&id, vec![seat], face(seat, seat + out), Pose::Sit, Prop::None, year, &mut rng));
            }
            if rng.gen_bool(0.3) {
                let kid = door + out * 1.1 - along * 0.3;
                if free(w, kid, 0.2, 1.3) {
                    let age = rng.gen_range(4..12);
                    let id = squatter(&mut rng, age, surname);
                    let mut f = Figure::new(&id, vec![kid], rng.gen_range(0.0..6.28), Pose::Stand, Prop::None, year, &mut rng);
                    f.day_only = true;
                    figs.push(f);
                }
            }
        }

        // Shopkeepers on stools by the shop door; residents at their doors.
        for u in &units {
            let (dx, dz) = u.door.facing.delta();
            let out = Vec3::new(dx as f32, 0.0, dz as f32);
            let along = Vec3::new(-out.z, 0.0, out.x);
            let door = cell_centre(u.door.cell) + out * (C * 0.5) + Vec3::Y * (u.floor as f32 * S);
            let shop = u.floor == 0
                && city.step(u.door.cell, u.door.facing).is_some_and(|n| city.ground_at(n) == Ground::Alley)
                && matches!(soc.occupants(u.id, year).first(), Some(Occupant::Business(_)));
            let (p, pose) = if shop && rng.gen_bool(0.55) {
                (door + out * 0.4 + along * if rng.gen_bool(0.5) { 0.6 } else { -0.6 }, Pose::Sit)
            } else if rng.gen_bool(0.05) {
                (door + out * 0.4, Pose::Stand)
            } else {
                continue;
            };
            let Some(p) = settle(w, p) else { continue };
            if !free(w, p, 0.22, 1.5) {
                continue;
            }
            let Some(id) = resident(soc, u.id, year, &mut rng, shop) else { continue };
            let yaw = face(p, p + out) + rng.gen_range(-0.5..0.5);
            let mut f = Figure::new(&id, vec![p], yaw, pose, Prop::None, year, &mut rng);
            f.day_only = !shop && rng.gen_bool(0.5);
            figs.push(f);
        }

        // Walkers, pacing a stretch of alley there and back.
        let alleys: Vec<Cell> = (0..city.d as u16).flat_map(|j| (0..city.w as u16).map(move |i| (i, j))).filter(|&c| city.ground_at(c) == Ground::Alley).collect();
        let is_alley = |c: Cell| city.ground_at(c) == Ground::Alley;
        let count = (alleys.len() as f32 * (0.03 + 0.2 * t)).round() as usize;
        for _ in 0..count {
            let mut c = *alleys.choose(&mut rng).unwrap();
            let mut d = *Dir::ALL.choose(&mut rng).unwrap();
            let len = rng.gen_range(6..24);
            let side = rng.gen_range(-0.25f32..0.25);
            let mut path: Vec<Vec3> = vec![];
            for _ in 0..len {
                let (dx, dz) = d.delta();
                let p = cell_centre(c) + Vec3::new(-dz as f32, 0.0, dx as f32) * side;
                if !free(w, p, 0.25, 1.75) {
                    break;
                }
                path.push(p);
                // Carry on straight mostly; turn where the alley does.
                let ahead = city.step(c, d).filter(|&n| is_alley(n));
                let turns: Vec<Dir> = Dir::ALL.into_iter().filter(|&t| t != d && t != crate::world::opposite(d) && city.step(c, t).is_some_and(is_alley)).collect();
                d = match ahead {
                    Some(_) if turns.is_empty() || rng.gen_bool(0.8) => d,
                    _ => match turns.choose(&mut rng) {
                        Some(&t) => t,
                        None => break,
                    },
                };
                c = city.step(c, d).unwrap();
            }
            if path.len() < 3 {
                continue;
            }
            let Some(id) = anyone(&mut rng) else { continue };
            let prop = match rng.gen::<f32>() {
                r if year < 1966 && r < 0.18 => Prop::Pole,
                r if r < 0.26 => Prop::Bag,
                r if year < 1976 && r < 0.33 => Prop::Buckets,
                _ => Prop::None,
            };
            let prop = if id.age < 14 { Prop::None } else { prop };
            let mut f = Figure::new(&id, path, 0.0, Pose::Walk, prop, year, &mut rng);
            f.day_only = rng.gen_bool(0.55);
            figs.push(f);
        }

        // Knots of people talking where lanes meet; children at play in the open.
        for j in 0..city.d as u16 {
            for i in 0..city.w as u16 {
                let c = (i, j);
                let g = city.ground_at(c);
                let open_plot = g == Ground::Plot && city.height_at(c, year) == 0;
                let junction = g == Ground::Alley && city.neighbours(c).filter(|(_, n)| city.ground_at(*n) == Ground::Alley).count() >= 3;
                let r = hash(i as u32 * 5, j as u32 * 11, 800 + year as u32);
                let kids = open_plot && r < 0.02 * (1.0 - t);
                if !(junction && r < 0.08 + 0.15 * t) && !kids {
                    continue;
                }
                let centre = cell_centre(c);
                let k = rng.gen_range(2..=3);
                let a0 = rng.gen_range(0.0..6.28f32);
                for m in 0..k {
                    let a = a0 + m as f32 * 6.28 / k as f32 + rng.gen_range(-0.3..0.3);
                    let p = centre + Vec3::new(a.cos(), 0.0, a.sin()) * 0.55;
                    if !free(w, p, 0.22, 1.7) {
                        continue;
                    }
                    let id = if kids {
                        let s = names::surname(&mut rng);
                        let age = rng.gen_range(5..12);
                        squatter(&mut rng, age, s)
                    } else {
                        let Some(id) = anyone(&mut rng) else { continue };
                        id
                    };
                    let mut f = Figure::new(&id, vec![p], face(p, centre), Pose::Stand, Prop::None, year, &mut rng);
                    f.day_only = kids || rng.gen_bool(0.5);
                    figs.push(f);
                }
            }
        }

        // Queues at the standpipes, buckets in hand.
        for feat in city.features.iter().filter(|f| f.kind == FeatureKind::WaterStandpipe) {
            let Some((d, _)) = city.neighbours(feat.cell).find(|(_, n)| city.ground_at(*n) == Ground::Alley) else { continue };
            let (dx, dz) = d.delta();
            let away = Vec3::new(dx as f32, 0.0, dz as f32);
            let pipe = cell_centre(feat.cell);
            let n = ((5.0 - 4.0 * t) * rng.gen_range(0.6..1.2)).round() as usize;
            for k in 0..n {
                let p = pipe + away * (0.9 + k as f32 * 0.65);
                if !free(w, p, 0.22, 1.7) {
                    break;
                }
                let Some(id) = anyone(&mut rng) else { continue };
                let mut f = Figure::new(&id, vec![p], face(p, pipe), Pose::Stand, Prop::Buckets, year, &mut rng);
                f.day_only = true;
                figs.push(f);
            }
        }

        // Up on the roofs: the city's open space, once there was nowhere else.
        for plot in city.plots.iter().filter(|p| p.height_at(year) >= 3) {
            if !rng.gen_bool((0.03 + 0.12 * t) as f64) {
                continue;
            }
            let c = *plot.cells.choose(&mut rng).unwrap();
            if city.role_at(c) == Role::Stair {
                continue;
            }
            let top = plot.height_at(year) as f32 * S;
            let Some(p) = settle(w, cell_centre(c) + Vec3::Y * (top + 0.2)) else { continue };
            if (p.y - top).abs() > 0.3 || !free(w, p, 0.22, 1.7) {
                continue;
            }
            let Some(id) = anyone(&mut rng) else { continue };
            let mut f = Figure::new(&id, vec![p], rng.gen_range(0.0..6.28), Pose::Stand, Prop::None, year, &mut rng);
            f.day_only = rng.gen_bool(0.7);
            figs.push(f);
        }

        // Keep clear of where you arrive.
        figs.retain(|f| f.path.iter().all(|p| p.distance(w.spawn) > 2.0));
        let mut crowd = Crowd { figures: figs, greeter: None };
        crowd.light(city, year, w, lamps);
        crowd
    }

    /// Sample the baked lamp light and sky openness where each figure goes.
    fn light(&mut self, city: &City, year: u16, _w: &WalkWorld, lamps: &[PointLight]) {
        let mut probe = MeshData::default();
        for f in &self.figures {
            for p in &f.path {
                let c = *p + Vec3::Y * 1.2;
                probe.quad([c, c + Vec3::X * 0.01, c + Vec3::new(0.01, 0.0, 0.01), c + Vec3::Z * 0.01], [0.0; 3], 0.0);
            }
        }
        crate::lights::bake(city, year, lamps, &mut [&mut probe]);
        let over = crate::world::overbuild(city, year).into_iter().map(|(c, a, t)| (c, (a, t))).collect();
        let sky = Sky::new(city, year, &over);
        let mut k = 0;
        for f in &mut self.figures {
            for l in f.light.iter_mut() {
                *l = Vec3::from(probe.vertices[k * 4].light);
                k += 1;
            }
            f.ao = sky.ao(f.path[0] + Vec3::Y * 1.0, None);
        }
    }

    /// Every point a figure stands on or walks through (for tests).
    #[cfg(test)]
    pub fn points(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.figures.iter().flat_map(|f| f.path.iter().copied())
    }

    pub fn update(&mut self, dt: f32, player: Vec3) {
        for f in self.figures.iter_mut().filter(|f| f.path.len() > 1) {
            if f.path[0].distance_squared(player) > 90.0 * 90.0 {
                continue;
            }
            // Wait for the player to get out of the way.
            let (p, yaw, _) = f.at();
            let to = player - p;
            if to.length() < 1.1 && to.y.abs() < 1.5 && Vec3::new(yaw.cos(), 0.0, yaw.sin()).dot(to) > 0.0 {
                continue;
            }
            let total = *f.cum.last().unwrap();
            f.s += f.dir * f.speed * dt;
            if f.s >= total || f.s <= 0.0 {
                f.s = f.s.clamp(0.0, total);
                f.dir = -f.dir;
            }
        }
        if let Some((_, left)) = &mut self.greeter {
            *left -= dt;
            if *left <= 0.0 {
                self.greeter = None;
            }
        }
    }

    fn visible(&self, night: bool) -> impl Iterator<Item = &Figure> {
        self.figures.iter().filter(move |f| !(night && f.day_only)).chain(self.greeter.iter().map(|g| &g.0))
    }

    /// Everyone near the camera, posed for time `t`.
    pub fn mesh(&self, eye: Vec3, look: Vec3, night: bool, t: f32) -> MeshData {
        let mut m = MeshData::default();
        for f in self.visible(night) {
            let (p, _, i) = f.at();
            let d = p - eye;
            let dist = d.length();
            if dist > 45.0 || (dist > 3.0 && d.dot(look) < -0.2 * dist) {
                continue;
            }
            let from = m.vertices.len();
            m.ao = f.ao;
            figure_mesh(&mut m, f, t, dist < 15.0);
            let light = f.light.get(i).copied().unwrap_or(Vec3::ZERO);
            for v in &mut m.vertices[from..] {
                v.light = light.into();
            }
        }
        m.ao = 1.0;
        m
    }

    /// The person you're looking straight at, close enough to make out.
    pub fn looking_at(&self, eye: Vec3, look: Vec3, night: bool, w: &WalkWorld) -> Option<(&str, &str)> {
        self.visible(night)
            .filter_map(|f| {
                let h = f.head() - Vec3::Y * 0.2 * f.scale;
                let d = h - eye;
                let dist = d.length();
                let off = (d - look * d.dot(look)).length();
                (dist < 6.0 && d.dot(look) > 0.0 && off < 0.35 && crate::game::line_clear(w, eye, h - d / dist * 0.4)).then_some((dist, f))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, f)| (f.name.as_str(), f.about.as_str()))
    }

    /// Someone comes to the door you knocked on.
    pub fn greet(&mut self, soc: &Society, year: u16, unit: u32, stand: Vec3, label: Vec3, player: Vec3) {
        let mut rng = ChaCha8Rng::seed_from_u64(unit as u64 * 7919 + year as u64);
        let Some(id) = resident(soc, unit, year, &mut rng, true) else { return };
        let out = Vec3::new(stand.x - label.x, 0.0, stand.z - label.z).normalize_or_zero();
        let p = Vec3::new(stand.x, stand.y, stand.z) - out * 0.3;
        let yaw = (player.z - p.z).atan2(player.x - p.x);
        let mut f = Figure::new(&id, vec![p], yaw, Pose::Stand, Prop::None, year, &mut rng);
        f.ao = 0.8;
        self.greeter = Some((f, 8.0));
    }
}
