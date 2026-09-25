//! The delivery game: doors you can knock on, jobs, tips. Relaxed: nothing fails.

use crate::world::{WalkWorld, S};
use glam::Vec3;
use kwc_sim::society::{Occupant, Sex, Society};
use kwc_sim::*;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const C: f32 = CELL_M;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotKind {
    Unit(u32),
    /// A building's street door (its stair landing).
    Building(u32),
    /// A street plaque (lane index in the directory).
    Plaque(u16),
}

/// Somewhere with a name: a flat or shop door, or a building's entrance.
pub struct Spot {
    pub kind: SpotKind,
    /// Where you stand to knock.
    pub stand: Vec3,
    /// Where the name label floats.
    pub label: Vec3,
}

fn face_point(c: Cell, d: Dir) -> Vec3 {
    let (dx, dz) = d.delta();
    Vec3::new((c.0 as f32 + 0.5) * C + dx as f32 * C * 0.5, 0.0, (c.1 as f32 + 0.5) * C + dz as f32 * C * 0.5)
}

pub fn spots(city: &City, soc: &Society, year: u16) -> Vec<Spot> {
    let mut out = vec![];
    for (b, lane, out_dir) in crate::lights::plaques(city, &soc.directory, year) {
        let c = (b.min + b.max) * 0.5;
        out.push(Spot { kind: SpotKind::Plaque(lane), stand: c + out_dir * 0.6 - Vec3::Y * c.y, label: c + out_dir * 0.1 + Vec3::Y * 0.35 });
    }
    for u in city.units_at(year) {
        let (dx, dz) = u.door.facing.delta();
        let out_dir = Vec3::new(dx as f32, 0.0, dz as f32);
        let y = u.floor as f32 * S;
        let p = face_point(u.door.cell, u.door.facing);
        let shop = city.step(u.door.cell, u.door.facing).is_some_and(|n| city.ground_at(n) == Ground::Alley);
        out.push(Spot {
            kind: SpotKind::Unit(u.id),
            stand: p + out_dir * 0.45 + Vec3::Y * y,
            label: p + out_dir * 0.2 + Vec3::Y * (y + if shop { 2.75 } else { 2.3 }),
        });
    }
    for p in city.plots.iter().filter(|p| p.height_at(year) > 0) {
        let l = p.core[0];
        if let Some((d, _)) = city.neighbours(l).find(|(_, n)| city.ground_at(*n) == Ground::Alley) {
            let (dx, dz) = d.delta();
            let out_dir = Vec3::new(dx as f32, 0.0, dz as f32);
            let fp = face_point(l, d);
            out.push(Spot { kind: SpotKind::Building(p.id), stand: fp + out_dir * 0.45, label: fp + out_dir * 0.2 + Vec3::Y * 2.45 });
        }
    }
    out
}

/// What a label says: a name and an address line.
pub fn describe(city: &City, soc: &Society, year: u16, kind: SpotKind) -> (String, String) {
    match kind {
        SpotKind::Unit(u) => {
            let addr = soc.directory.address[u as usize].line();
            let occ = soc.occupants(u, year);
            let name = match occ.first() {
                None => "(empty)".to_string(),
                Some(o) if occ.len() > 1 => format!("{} & {}", soc.describe(*o), soc.describe(occ[1]).trim_start_matches("the ")),
                Some(&Occupant::Temple) => city.units[u as usize].name.clone().unwrap_or("Temple".into()),
                Some(&o) => soc.describe(o),
            };
            (name, addr)
        }
        SpotKind::Plaque(l) => (soc.directory.lane_names[l as usize].clone(), "street sign".into()),
        SpotKind::Building(p) => {
            let (lane, num) = soc.directory.building[p as usize];
            let h = city.plots[p as usize].height_at(year);
            (format!("{num} {}", soc.directory.lane_names[lane as usize]), format!("{h} {}", if h == 1 { "storey" } else { "storeys" }))
        }
    }
}

pub struct Job {
    pub item: String,
    pub from: u32,
    pub to: u32,
    pub from_name: String,
    pub to_name: String,
    pub picked: bool,
    pub tip: u32,
}

/// The chapters of the game: you move on when you choose, once you know the place.
pub const ERAS: [u16; 8] = [1950, 1955, 1960, 1965, 1970, 1975, 1980, 1987];
/// Knowing an era: this much of its lanes walked, and this many deliveries made
/// by memory alone (memory mode on, notebook shut).
pub const KNOW_COVERAGE: f32 = 0.5;
pub const KNOW_CLEAN: u32 = 3;

/// Someone you delivered to.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Met {
    /// 0 = household, 1 = business.
    pub kind: u8,
    pub id: u32,
    pub unit: u32,
    pub year: u16,
}

impl Met {
    pub fn occupant(&self) -> Occupant {
        if self.kind == 0 { Occupant::Household(self.id) } else { Occupant::Business(self.id) }
    }
}

pub struct Game {
    pub year: u16,
    pub delivered: u32,
    pub tips: u32,
    pub job: Option<Job>,
    /// A short message and how long it stays up (s).
    pub toast: Option<(String, f32)>,
    rng: ChaCha8Rng,
    pub seed: u64,
    /// Cells you've walked near this era: what your notebook shows.
    pub seen: std::collections::HashSet<Cell>,
    /// Deliveries this era made by memory alone.
    pub clean: u32,
    /// Did you lean on the notebook, names or arrow during the current job?
    pub job_helped: bool,
    pub met: Vec<Met>,
    /// This era is understood (shown once).
    pub understood: bool,
    /// Where to put you when this era's walk is next built (from a save).
    pub resume: Option<[f32; 4]>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Save {
    pub seed: u64,
    pub year: u16,
    pub delivered: u32,
    pub tips: u32,
    pub clean: u32,
    pub seen: Vec<Cell>,
    pub met: Vec<Met>,
    /// Where you were standing (x, y, z, yaw), to carry on from there.
    #[serde(default)]
    pub pos: Option<[f32; 4]>,
}

fn item_for(trade: UnitUse, rng: &mut impl Rng) -> &'static str {
    use UnitUse::*;
    let list: &[&'static str] = match trade {
        Restaurant => &["a box of roast pork", "two bowls of congee", "a tray of noodles", "a flask of milk tea"],
        Shop => &["a sack of rice", "a tin of cooking oil", "cigarettes", "a bag of groceries", "a crate of soda"],
        FishballFactory => &["a basket of fishballs", "a bucket of fish paste"],
        Workshop => &["a box of watchstraps", "a bundle of plastic parts", "a roll of cloth", "a parcel of printing"],
        Dentist => &["a set of dentures", "a bottle of mouthwash"],
        Clinic => &["a packet of herbal medicine", "a bottle of liniment"],
        _ => &["a parcel"],
    };
    list.choose(rng).unwrap()
}

/// How to address the recipient: someone from the household, or the business.
fn recipient(soc: &Society, year: u16, occ: Occupant, rng: &mut impl Rng) -> String {
    match occ {
        Occupant::Household(h) => {
            let adults: Vec<_> = soc.members_in(h, year).into_iter().filter(|p| p.age(year) >= 18).collect();
            match adults.choose(rng) {
                Some(p) if p.sex == Sex::F && p.spouse.is_some() => {
                    let husband = &soc.people[p.spouse.unwrap() as usize];
                    format!("Mrs {}", husband.surname)
                }
                Some(p) => p.name(),
                None => soc.describe(occ),
            }
        }
        _ => soc.describe(occ),
    }
}

impl Game {
    pub fn new(seed: u64, year: u16) -> Game {
        Game {
            year,
            delivered: 0,
            tips: 0,
            job: None,
            toast: None,
            rng: ChaCha8Rng::seed_from_u64(seed ^ 0xDE11 ^ year as u64),
            seed,
            seen: Default::default(),
            clean: 0,
            job_helped: false,
            met: vec![],
            understood: false,
            resume: None,
        }
    }

    pub fn save(&self) -> Save {
        let mut seen: Vec<Cell> = self.seen.iter().copied().collect();
        seen.sort_unstable();
        Save { seed: self.seed, year: self.year, delivered: self.delivered, tips: self.tips, clean: self.clean, seen, met: self.met.clone(), pos: None }
    }

    pub fn load(s: Save) -> Game {
        let mut g = Game::new(s.seed, s.year);
        g.delivered = s.delivered;
        g.tips = s.tips;
        g.clean = s.clean;
        g.seen = s.seen.into_iter().collect();
        g.met = s.met;
        g.resume = s.pos;
        g
    }

    /// Mark what's around you as seen (a few metres; further from rooftops).
    pub fn observe(&mut self, city: &City, feet: Vec3) {
        let r = if feet.y > 3.0 && place_is_roof(city, self.year, feet) { 5 } else { 2 };
        let (ci, cj) = ((feet.x / C).floor() as i32, (feet.z / C).floor() as i32);
        for dj in -r..=r {
            for di in -r..=r {
                let (i, j) = (ci + di, cj + dj);
                if city.in_bounds(i, j) {
                    self.seen.insert((i as u16, j as u16));
                }
            }
        }
    }

    /// Share of this era's lanes you've walked.
    pub fn coverage(&self, city: &City) -> f32 {
        let lanes: Vec<Cell> = (0..city.w * city.d).filter(|&k| city.ground[k] == Ground::Alley).map(|k| ((k % city.w) as u16, (k / city.w) as u16)).collect();
        lanes.iter().filter(|c| self.seen.contains(c)).count() as f32 / lanes.len().max(1) as f32
    }

    pub fn knows_era(&self, city: &City) -> bool {
        self.clean >= KNOW_CLEAN && self.coverage(city) >= KNOW_COVERAGE
    }

    pub fn next_era(&self) -> Option<u16> {
        ERAS.iter().copied().find(|&e| e > self.year)
    }

    /// Move on to `year`. What was rebuilt since is blank in the notebook again.
    pub fn enter_era(&mut self, city: &City, year: u16) {
        let old = self.year;
        self.seen.retain(|&c| city.height_at(c, old) == city.height_at(c, year));
        self.year = year;
        self.clean = 0;
        self.job = None;
        self.job_helped = false;
        self.understood = false;
        self.resume = None;
        self.rng = ChaCha8Rng::seed_from_u64(self.seed ^ 0xDE11 ^ year as u64);
        self.toast(format!("{year}. The city has grown; your notebook is out of date."));
    }

    /// Offer a new job: collect from a business, take it to a household or another business.
    pub fn new_job(&mut self, city: &City, soc: &Society) {
        let y = self.year;
        let units: Vec<&Unit> = city.units_at(y).collect();
        let shops: Vec<(u32, u32)> = units
            .iter()
            .filter_map(|u| soc.occupants(u.id, y).into_iter().find_map(|o| if let Occupant::Business(b) = o { Some((u.id, b)) } else { None }))
            .collect();
        let homes: Vec<(u32, Occupant)> = units
            .iter()
            .filter_map(|u| soc.occupants(u.id, y).into_iter().find(|o| *o != Occupant::Temple).map(|o| (u.id, o)))
            .collect();
        let (Some(&(from, b)), true) = (shops.choose(&mut self.rng), !homes.is_empty()) else { return };
        let (to, occ) = loop {
            let &(to, occ) = homes.choose(&mut self.rng).unwrap();
            if to != from {
                break (to, occ);
            }
        };
        let item = item_for(soc.businesses[b as usize].trade, &mut self.rng).to_string();
        let to_name = recipient(soc, y, occ, &mut self.rng);
        let tip = 2 + self.rng.gen_range(0..4) + (city.units[to as usize].floor as u32) / 2;
        self.job = Some(Job { item, from, to, from_name: soc.businesses[b as usize].name.clone(), to_name, picked: false, tip });
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), 3.5));
    }

    pub fn tick(&mut self, dt: f32) {
        if let Some((_, t)) = &mut self.toast {
            *t -= dt;
            if *t <= 0.0 {
                self.toast = None;
            }
        }
    }

    /// Press E: knock on the nearest door you're standing at.
    /// Knock on the nearest door. Returns the unit if there was one.
    pub fn interact(&mut self, city: &City, soc: &Society, spots: &[Spot], feet: Vec3) -> Option<u32> {
        let near = spots
            .iter()
            .filter(|s| matches!(s.kind, SpotKind::Unit(_)))
            .map(|s| (s, Vec3::new(s.stand.x - feet.x, 0.0, s.stand.z - feet.z).length(), (s.stand.y - feet.y).abs()))
            .filter(|&(_, d, dy)| d < 1.1 && dy < 1.2)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((spot, _, _)) = near else {
            self.toast("There's no door here.");
            return None;
        };
        let SpotKind::Unit(u) = spot.kind else { return None };
        let who = describe(city, soc, self.year, spot.kind).0;
        let Some(job) = &mut self.job else {
            self.toast(format!("{who}: \"Nothing for you today.\""));
            return Some(u);
        };
        if !job.picked && u == job.from {
            job.picked = true;
            let item = job.item.clone();
            self.toast(format!("Collected {item}."));
        } else if job.picked && u == job.to {
            let tip = job.tip;
            let name = job.to_name.clone();
            self.delivered += 1;
            self.tips += tip;
            if let Some(&o) = soc.occupants(u, self.year).first() {
                let (kind, id) = match o {
                    Occupant::Household(h) => (0, h),
                    Occupant::Business(b) => (1, b),
                    Occupant::Temple => (2, 0),
                };
                if kind < 2 && !self.met.iter().any(|m| m.kind == kind && m.id == id) {
                    self.met.push(Met { kind, id, unit: u, year: self.year });
                }
            }
            if !self.job_helped {
                self.clean += 1;
            }
            self.job_helped = false;
            self.job = None;
            self.toast(format!("Delivered to {name}. Tip: HK${tip}."));
            self.new_job(city, soc);
        } else {
            self.toast(format!("{who}: \"Not for us, I'm afraid.\""));
        }
        Some(u)
    }
}

/// Text painted on a surface: centre, the surface's outward normal, and the
/// in-plane right/up directions (as a reader facing the wall sees them).
pub struct Plate {
    pub centre: Vec3,
    pub right: Vec3,
    pub lines: Vec<String>,
    /// Height of one line of text, and the widest it may be (m).
    pub line_h: f32,
    pub max_w: f32,
    pub colour: [u8; 3],
    pub kind: SpotKind,
}

fn frame(c: Cell, d: Dir) -> (Vec3, Vec3, Vec3) {
    let (dx, dz) = d.delta();
    let n = Vec3::new(dx as f32, 0.0, dz as f32);
    let right = (-n).cross(Vec3::Y);
    (face_point(c, d), n, right)
}

/// Every name in the city, painted where it belongs: business names on shop
/// fascias, flat numbers and family names on doors, addresses over street doors,
/// lane names on the enamel plaques.
pub fn plates(city: &City, soc: &Society, year: u16) -> Vec<Plate> {
    let mut out = vec![];
    for u in city.units_at(year) {
        let (p, n, right) = frame(u.door.cell, u.door.facing);
        let y = u.floor as f32 * S;
        let a = &soc.directory.address[u.id as usize];
        let shop = city.step(u.door.cell, u.door.facing).is_some_and(|nb| city.ground_at(nb) == Ground::Alley);
        let occ = soc.occupants(u.id, year);
        if shop {
            let name = match occ.first() {
                Some(&Occupant::Temple) => u.name.clone().unwrap_or("Temple".into()),
                Some(&o) => soc.describe(o),
                None => continue,
            };
            out.push(Plate { centre: p + n * 0.045 + Vec3::Y * (y + 2.63), right, lines: vec![name], line_h: 0.17, max_w: 1.3, colour: [245, 232, 200], kind: SpotKind::Unit(u.id) });
        } else {
            let flat = format!("{}{}", u.floor, a.flat.unwrap_or(' '));
            let who = match occ.first() {
                Some(&Occupant::Household(h)) => soc.households[h as usize].surname.clone(),
                Some(&o) => soc.describe(o),
                None => String::new(),
            };
            out.push(Plate {
                centre: p + n * 0.15 + Vec3::Y * (y + 1.55),
                right,
                lines: vec![flat, who],
                line_h: 0.11,
                max_w: 0.85,
                colour: [240, 236, 220],
                kind: SpotKind::Unit(u.id),
            });
        }
    }
    for pl in city.plots.iter().filter(|p| p.height_at(year) > 0) {
        let l = pl.core[0];
        if let Some((d, _)) = city.neighbours(l).find(|(_, nb)| city.ground_at(*nb) == Ground::Alley) {
            let (p, n, right) = frame(l, d);
            let (lane, num) = soc.directory.building[pl.id as usize];
            out.push(Plate {
                centre: p + n * 0.03 + Vec3::Y * 2.45,
                right,
                lines: vec![format!("{num} {}", soc.directory.lane_names[lane as usize])],
                line_h: 0.13,
                max_w: 1.35,
                colour: [230, 225, 210],
                kind: SpotKind::Building(pl.id),
            });
        }
    }
    for (b, lane, out_dir) in crate::lights::plaques(city, &soc.directory, year) {
        let right = (-out_dir).cross(Vec3::Y);
        let c = (b.min + b.max) * 0.5;
        out.push(Plate {
            centre: c + out_dir * 0.02,
            right,
            lines: vec![soc.directory.lane_names[lane as usize].clone()],
            line_h: 0.11,
            max_w: 0.64,
            colour: [245, 245, 250],
            kind: SpotKind::Plaque(lane),
        });
    }
    out
}

fn place_is_roof(city: &City, year: u16, feet: Vec3) -> bool {
    let (i, j) = ((feet.x / C).floor() as i32, (feet.z / C).floor() as i32);
    city.in_bounds(i, j) && {
        let c = (i as u16, j as u16);
        city.ground_at(c) == Ground::Plot && feet.y + 0.3 >= city.height_at(c, year) as f32 * S
    }
}

/// Is the straight line from `a` to `b` free of walls?
pub fn line_clear(w: &WalkWorld, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let n = (d.length() / 0.2).ceil().max(1.0) as i32;
    (1..n).all(|k| {
        let p = a + d * (k as f32 / n as f32);
        let q = crate::world::Aabb::new(p - Vec3::splat(0.01), p + Vec3::splat(0.01));
        !w.blocked(&q)
    })
}

/// The lane nearest to a cell, searching outward ring by ring (up to ~9 m).
fn nearest_lane<'a>(city: &City, soc: &'a Society, c: Cell) -> Option<&'a str> {
    for r in 1..=6i32 {
        for dj in -r..=r {
            for di in -r..=r {
                if di.abs().max(dj.abs()) != r {
                    continue;
                }
                let (i, j) = (c.0 as i32 + di, c.1 as i32 + dj);
                if city.in_bounds(i, j) {
                    if let Some(name) = soc.directory.lane_at(city, (i as u16, j as u16)) {
                        return Some(name);
                    }
                }
            }
        }
    }
    None
}

/// Where the player is, in words: a lane, or a building and floor.
pub fn place_name(city: &City, soc: &Society, year: u16, feet: Vec3) -> String {
    let (i, j) = ((feet.x / C).floor() as i32, (feet.z / C).floor() as i32);
    if !city.in_bounds(i, j) {
        return "Outside the walls".into();
    }
    let c = (i as u16, j as u16);
    match city.ground_at(c) {
        Ground::Alley | Ground::Well => soc.directory.lane_at(city, c).unwrap_or("A lane").to_string(),
        Ground::Plot => {
            let p = city.plot_of[city.idx(c)];
            let (lane, num) = soc.directory.building[p as usize];
            let h = city.plots[p as usize].height_at(year) as f32;
            if h == 0.0 {
                // Nothing built here yet: name it by the nearest lane, not by the
                // building that will stand here later.
                return match nearest_lane(city, soc, c) {
                    Some(lane) => format!("Open ground off {lane}"),
                    None => "Open ground".into(),
                };
            }
            let lv = (feet.y / S + 0.25).floor();
            let building = format!("{num} {}", soc.directory.lane_names[lane as usize]);
            if feet.y > 0.5 && lv >= h {
                format!("Rooftop · {building}")
            } else {
                format!("{building} · {}", address::Address::floor_name(lv.max(0.0) as u8))
            }
        }
        Ground::Yamen => "The Yamen".into(),
        Ground::Outside => "Outside the walls".into(),
    }
}
