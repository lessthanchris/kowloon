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
    for (b, lane, out_dir) in crate::lights::plaques(city, &soc.directory) {
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

pub struct Game {
    pub year: u16,
    pub delivered: u32,
    pub tips: u32,
    pub job: Option<Job>,
    /// A short message and how long it stays up (s).
    pub toast: Option<(String, f32)>,
    rng: ChaCha8Rng,
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
        Game { year, delivered: 0, tips: 0, job: None, toast: None, rng: ChaCha8Rng::seed_from_u64(seed ^ 0xDE11) }
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
    pub fn interact(&mut self, city: &City, soc: &Society, spots: &[Spot], feet: Vec3) {
        let near = spots
            .iter()
            .filter(|s| matches!(s.kind, SpotKind::Unit(_)))
            .map(|s| (s, Vec3::new(s.stand.x - feet.x, 0.0, s.stand.z - feet.z).length(), (s.stand.y - feet.y).abs()))
            .filter(|&(_, d, dy)| d < 1.1 && dy < 1.2)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((spot, _, _)) = near else {
            self.toast("There's no door here.");
            return;
        };
        let SpotKind::Unit(u) = spot.kind else { return };
        let who = describe(city, soc, self.year, spot.kind).0;
        let Some(job) = &mut self.job else {
            self.toast(format!("{who}: \"Nothing for you today.\""));
            return;
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
            self.job = None;
            self.toast(format!("Delivered to {name}. Tip: HK${tip}."));
            self.new_job(city, soc);
        } else {
            self.toast(format!("{who}: \"Not for us, I'm afraid.\""));
        }
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
