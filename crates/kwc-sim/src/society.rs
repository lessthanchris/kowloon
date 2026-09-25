//! The people: households and businesses in every unit, simulated year by year
//! alongside the buildings. Families arrive (the 1950s refugee waves), marry,
//! have children, grow old; grown children set up their own households in the
//! new flats as the towers rise (or double up with their parents when there's
//! nowhere to go, as 10,700 households did in 8,500 premises); businesses open,
//! pass from parent to child, get sold, close.
//!
//! Deterministic from the city's seed.

use crate::address::{self, Directory};
use crate::city::*;
use crate::names;
use crate::rng_for;
use crate::site::{END_YEAR, START_YEAR};
use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sex {
    F,
    M,
}

#[derive(Clone, Debug)]
pub struct Person {
    pub id: u32,
    pub surname: String,
    pub given: String,
    pub sex: Sex,
    pub born: u16,
    pub died: Option<u16>,
    pub mother: Option<u32>,
    pub father: Option<u32>,
    pub spouse: Option<u32>,
}

impl Person {
    /// "Chan Ka-ming"
    pub fn name(&self) -> String {
        format!("{} {}", self.surname, self.given)
    }
    pub fn alive_in(&self, year: u16) -> bool {
        self.born <= year && self.died.map_or(true, |d| d > year)
    }
    pub fn age(&self, year: u16) -> u16 {
        year.saturating_sub(self.born)
    }
}

#[derive(Clone, Debug)]
pub struct Household {
    pub id: u32,
    pub surname: String,
    pub head: u32,
    pub founded: u16,
    pub ended: Option<u16>,
    /// The household the founder grew up in (None for arrivals).
    pub parent: Option<u32>,
    pub arrived_from_outside: bool,
}

/// Person `person` lived in household `household` over [from, to).
#[derive(Clone, Debug)]
pub struct Membership {
    pub person: u32,
    pub household: u32,
    pub from: u16,
    pub to: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct Business {
    pub id: u32,
    pub name: String,
    pub trade: UnitUse,
    pub opened: u16,
    pub closed: Option<u16>,
    /// (owning household, from year).
    pub owners: Vec<(u32, u16)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Occupant {
    Household(u32),
    Business(u32),
    Temple,
}

/// `occupant` held `unit` over [from, to).
#[derive(Clone, Debug)]
pub struct Tenancy {
    pub unit: u32,
    pub occupant: Occupant,
    pub from: u16,
    pub to: Option<u16>,
}

impl Tenancy {
    pub fn during(&self, year: u16) -> bool {
        self.from <= year && self.to.map_or(true, |t| t > year)
    }
}

pub struct Society {
    pub people: Vec<Person>,
    pub households: Vec<Household>,
    pub members: Vec<Membership>,
    pub businesses: Vec<Business>,
    pub tenancies: Vec<Tenancy>,
    pub directory: Directory,
    by_unit: HashMap<u32, Vec<usize>>,
    by_household: HashMap<u32, Vec<usize>>,
}

impl Society {
    pub fn occupants(&self, unit: u32, year: u16) -> Vec<Occupant> {
        self.by_unit.get(&unit).map_or(vec![], |v| v.iter().map(|&k| &self.tenancies[k]).filter(|t| t.during(year)).map(|t| t.occupant).collect())
    }
    pub fn members_in(&self, household: u32, year: u16) -> Vec<&Person> {
        let mut v: Vec<&Person> = self
            .by_household
            .get(&household)
            .map_or(vec![], |ms| ms.iter().map(|&k| &self.members[k]).filter(|m| m.from <= year && m.to.map_or(true, |t| t > year)).map(|m| &self.people[m.person as usize]).collect());
        v.sort_by_key(|p| p.born);
        v
    }
    pub fn home_of(&self, household: u32, year: u16) -> Option<u32> {
        self.tenancies.iter().find(|t| t.occupant == Occupant::Household(household) && t.during(year)).map(|t| t.unit)
    }
    /// All homes a household ever had, in order.
    pub fn homes_of(&self, household: u32) -> Vec<(u32, u16, Option<u16>)> {
        self.tenancies.iter().filter(|t| t.occupant == Occupant::Household(household)).map(|t| (t.unit, t.from, t.to)).collect()
    }
    /// The family line: this household, its parent household, and so on back.
    pub fn lineage(&self, household: u32) -> Vec<u32> {
        let mut v = vec![household];
        while let Some(p) = self.households[*v.last().unwrap() as usize].parent {
            v.push(p);
        }
        v
    }
    pub fn population(&self, year: u16) -> usize {
        self.members.iter().filter(|m| m.from <= year && m.to.map_or(true, |t| t > year) && self.people[m.person as usize].alive_in(year)).count()
    }
    /// Short description of who's at a unit: "the Chan family", "Wing Kee Store".
    pub fn describe(&self, occ: Occupant) -> String {
        match occ {
            Occupant::Household(h) => format!("the {} family", self.households[h as usize].surname),
            Occupant::Business(b) => self.businesses[b as usize].name.clone(),
            Occupant::Temple => "temple".into(),
        }
    }
}

// ---------------------------------------------------------------------------

struct Sim<'a> {
    city: &'a City,
    rng: ChaCha8Rng,
    s: Society,
    /// Current household of each living, housed person.
    person_hh: HashMap<u32, u32>,
    hh_members: HashMap<u32, Vec<u32>>,
    /// Open tenancy index per (unit, occupant).
    open: HashMap<(u32, Occupant), usize>,
    open_member: HashMap<u32, usize>,
    unit_households: HashMap<u32, Vec<u32>>,
    unit_business: HashMap<u32, u32>,
    business_unit: HashMap<u32, u32>,
    hh_home: HashMap<u32, u32>,
}

fn mortality(age: u16) -> f64 {
    match age {
        0 => 0.02,
        1..=4 => 0.004,
        5..=39 => 0.0015,
        40..=54 => 0.005,
        55..=64 => 0.012,
        65..=74 => 0.035,
        75..=84 => 0.09,
        _ => 0.2,
    }
}

impl<'a> Sim<'a> {
    fn person(&mut self, surname: &str, sex: Sex, born: u16, mother: Option<u32>, father: Option<u32>) -> u32 {
        let id = self.s.people.len() as u32;
        let given = names::given(&mut self.rng, sex == Sex::F);
        self.s.people.push(Person { id, surname: surname.to_string(), given, sex, born, died: None, mother, father, spouse: None });
        id
    }

    fn join(&mut self, person: u32, hh: u32, year: u16) {
        if let Some(old) = self.person_hh.insert(person, hh) {
            self.leave_only(person, old, year);
        }
        self.hh_members.entry(hh).or_default().push(person);
        let k = self.s.members.len();
        self.s.members.push(Membership { person, household: hh, from: year, to: None });
        self.open_member.insert(person, k);
    }

    fn leave_only(&mut self, person: u32, hh: u32, year: u16) {
        if let Some(v) = self.hh_members.get_mut(&hh) {
            v.retain(|&p| p != person);
        }
        if let Some(k) = self.open_member.remove(&person) {
            self.s.members[k].to = Some(year);
        }
    }

    fn start_tenancy(&mut self, unit: u32, occ: Occupant, year: u16) {
        let k = self.s.tenancies.len();
        self.s.tenancies.push(Tenancy { unit, occupant: occ, from: year, to: None });
        self.open.insert((unit, occ), k);
    }

    fn end_tenancy(&mut self, unit: u32, occ: Occupant, year: u16) {
        if let Some(k) = self.open.remove(&(unit, occ)) {
            self.s.tenancies[k].to = Some(year);
        }
    }

    fn household(&mut self, surname: &str, head: u32, year: u16, parent: Option<u32>, outside: bool) -> u32 {
        let id = self.s.households.len() as u32;
        self.s.households.push(Household { id, surname: surname.to_string(), head, founded: year, ended: None, parent, arrived_from_outside: outside });
        id
    }

    fn move_in(&mut self, hh: u32, unit: u32, year: u16) {
        if let Some(old) = self.hh_home.insert(hh, unit) {
            self.end_tenancy(old, Occupant::Household(hh), year);
            if let Some(v) = self.unit_households.get_mut(&old) {
                v.retain(|&h| h != hh);
            }
        }
        self.unit_households.entry(unit).or_default().push(hh);
        self.start_tenancy(unit, Occupant::Household(hh), year);
    }

    /// A family arriving from outside (in the early years, refugees from the mainland).
    fn arrivals(&mut self, unit: u32, year: u16) {
        let surname = names::surname(&mut self.rng).to_string();
        let r = &mut self.rng;
        let husband_age = r.gen_range(22..=55u16);
        let wife_age = (husband_age as i32 - r.gen_range(0..=6)).max(18) as u16;
        let h = self.person(&surname, Sex::M, year - husband_age, None, None);
        let wsur = names::surname(&mut self.rng).to_string();
        let w = self.person(&wsur, Sex::F, year - wife_age, None, None);
        self.s.people[h as usize].spouse = Some(w);
        self.s.people[w as usize].spouse = Some(h);
        let hh = self.household(&surname, h, year, None, true);
        self.join(h, hh, year);
        self.join(w, hh, year);
        let kids = *[0, 1, 2, 2, 3, 3, 4, 5].choose(&mut self.rng).unwrap();
        for _ in 0..kids {
            let max_age = (wife_age.saturating_sub(18)).min(16);
            let age = self.rng.gen_range(0..=max_age);
            let sex = if self.rng.gen_bool(0.5) { Sex::F } else { Sex::M };
            let c = self.person(&surname, sex, year - age, Some(w), Some(h));
            self.join(c, hh, year);
        }
        // Sometimes a widowed grandparent comes too.
        if self.rng.gen_bool(0.35) {
            let sex = if self.rng.gen_bool(0.6) { Sex::F } else { Sex::M };
            let age = self.rng.gen_range(58..=78);
            let g = self.person(&surname, sex, year - age, None, None);
            self.join(g, hh, year);
        }
        self.move_in(hh, unit, year);
    }

    fn end_household(&mut self, hh: u32, year: u16) {
        self.s.households[hh as usize].ended = Some(year);
        if let Some(unit) = self.hh_home.remove(&hh) {
            self.end_tenancy(unit, Occupant::Household(hh), year);
            if let Some(v) = self.unit_households.get_mut(&unit) {
                v.retain(|&h| h != hh);
            }
        }
    }

    fn year(&mut self, y: u16) {
        let city = self.city;
        let standing: Vec<&Unit> = city.units_at(y).collect();

        // Temples.
        for u in standing.iter().filter(|u| u.usage == UnitUse::Temple) {
            if !self.open.contains_key(&(u.id, Occupant::Temple)) {
                self.start_tenancy(u.id, Occupant::Temple, y);
            }
        }

        // Deaths.
        let living: Vec<u32> = self.person_hh.keys().copied().collect();
        let mut living = living;
        living.sort_unstable();
        for p in living {
            let age = self.s.people[p as usize].age(y);
            if self.rng.gen_bool(mortality(age)) {
                self.s.people[p as usize].died = Some(y);
                let hh = self.person_hh.remove(&p).unwrap();
                self.leave_only(p, hh, y);
                if self.hh_members.get(&hh).map_or(true, |v| v.is_empty()) {
                    self.end_household(hh, y);
                }
            }
        }

        // Births.
        let birth_p = 0.22 - 0.12 * ((y - START_YEAR) as f64 / (END_YEAR - START_YEAR) as f64);
        let mut hhs: Vec<u32> = self.hh_members.iter().filter(|(_, v)| !v.is_empty()).map(|(&h, _)| h).collect();
        hhs.sort_unstable();
        for hh in hhs {
            let members = self.hh_members[&hh].clone();
            for &m in &members {
                let p = &self.s.people[m as usize];
                if p.sex != Sex::F || !(18..=42).contains(&p.age(y)) {
                    continue;
                }
                let Some(husband) = p.spouse.filter(|s| self.person_hh.get(s) == Some(&hh)) else { continue };
                if self.rng.gen_bool(birth_p) {
                    let surname = self.s.people[husband as usize].surname.clone();
                    let sex = if self.rng.gen_bool(0.5) { Sex::F } else { Sex::M };
                    let c = self.person(&surname, sex, y, Some(m), Some(husband));
                    self.join(c, hh, y);
                }
            }
        }

        // Grown children marry and set up their own households.
        let mut eligible: Vec<u32> = self
            .person_hh
            .iter()
            .filter(|(&p, &hh)| {
                let pp = &self.s.people[p as usize];
                pp.spouse.is_none() && (21..=34).contains(&pp.age(y)) && self.s.households[hh as usize].head != p
            })
            .map(|(&p, _)| p)
            .collect();
        eligible.sort_unstable();
        eligible.shuffle(&mut self.rng);
        let mut taken = std::collections::HashSet::new();
        for p in eligible.clone() {
            if taken.contains(&p) || !self.rng.gen_bool(0.15) {
                continue;
            }
            let sex = self.s.people[p as usize].sex;
            let my_hh = self.person_hh[&p];
            // Someone from another household in the city, or from outside.
            let local = eligible.iter().copied().find(|&q| {
                !taken.contains(&q) && q != p && self.s.people[q as usize].sex != sex && self.person_hh.get(&q) != Some(&my_hh)
            });
            let partner = match local.filter(|_| self.rng.gen_bool(0.45)) {
                Some(q) => q,
                None => {
                    let age = self.s.people[p as usize].age(y) as i32 + self.rng.gen_range(-3..=3);
                    let sur = names::surname(&mut self.rng).to_string();
                    self.person(&sur, if sex == Sex::M { Sex::F } else { Sex::M }, y - age.max(18) as u16, None, None)
                }
            };
            taken.insert(p);
            taken.insert(partner);
            let (man, woman) = if sex == Sex::M { (p, partner) } else { (partner, p) };
            self.s.people[man as usize].spouse = Some(woman);
            self.s.people[woman as usize].spouse = Some(man);
            let surname = self.s.people[man as usize].surname.clone();
            let parent = self.person_hh.get(&man).or(self.person_hh.get(&woman)).copied();
            let hh = self.household(&surname, man, y, parent, false);
            let old: Vec<u32> = [man, woman].iter().filter_map(|x| self.person_hh.get(x).copied()).collect();
            self.join(man, hh, y);
            self.join(woman, hh, y);
            for o in old {
                if self.hh_members.get(&o).map_or(true, |v| v.is_empty()) {
                    self.end_household(o, y);
                }
            }
            // Doubling up with the parents until a flat comes free.
            if let Some(unit) = parent.and_then(|p| self.hh_home.get(&p).copied()) {
                self.move_in(hh, unit, y);
            }
        }

        // Businesses: succession, sales, closures; new ones in empty premises.
        for b in 0..self.s.businesses.len() {
            if self.s.businesses[b].closed.is_some() {
                continue;
            }
            let owner = self.s.businesses[b].owners.last().unwrap().0;
            let unit = self.business_unit.get(&(b as u32)).copied();
            if self.s.households[owner as usize].ended.is_some() {
                // To a child's household, if there is one; otherwise it closes.
                let heir = self.s.households.iter().find(|h| h.parent == Some(owner) && h.ended.is_none()).map(|h| h.id);
                match heir {
                    Some(h) => self.s.businesses[b].owners.push((h, y)),
                    None => {
                        self.s.businesses[b].closed = Some(y);
                        if let Some(u) = unit {
                            self.end_tenancy(u, Occupant::Business(b as u32), y);
                            self.unit_business.remove(&u);
                            self.business_unit.remove(&(b as u32));
                        }
                    }
                }
            } else if self.rng.gen_bool(0.015) {
                // Sold on, sometimes keeping the name.
                let mut buyers: Vec<u32> = self.hh_members.iter().filter(|(_, v)| !v.is_empty()).map(|(&h, _)| h).collect();
                buyers.sort_unstable();
                if let Some(&buyer) = buyers.choose(&mut self.rng) {
                    self.s.businesses[b].owners.push((buyer, y));
                }
            }
        }
        for u in standing.iter().filter(|u| !matches!(u.usage, UnitUse::Flat | UnitUse::Temple)) {
            if self.unit_business.contains_key(&u.id) {
                continue;
            }
            // Shopkeepers often lived in the same building.
            let upstairs: Vec<u32> = standing
                .iter()
                .filter(|v| v.plot == u.plot)
                .flat_map(|v| self.unit_households.get(&v.id).cloned().unwrap_or_default())
                .collect();
            let owner = match upstairs.choose(&mut self.rng) {
                Some(&h) => h,
                None => {
                    // An arrival who lives elsewhere: find any housed household.
                    let mut all: Vec<u32> = self.hh_home.keys().copied().collect();
                    all.sort_unstable();
                    match all.choose(&mut self.rng) {
                        Some(&h) => h,
                        None => continue,
                    }
                }
            };
            let head = self.s.households[owner as usize].head;
            let (sur, given) = (self.s.people[head as usize].surname.clone(), self.s.people[head as usize].given.clone());
            let name = names::business(&mut self.rng, u.usage, &sur, &given);
            let id = self.s.businesses.len() as u32;
            self.s.businesses.push(Business { id, name, trade: u.usage, opened: y, closed: None, owners: vec![(owner, y)] });
            self.unit_business.insert(u.id, id);
            self.business_unit.insert(id, u.id);
            self.start_tenancy(u.id, Occupant::Business(id), y);
        }

        // Empty flats: households doubled up move out first, then new arrivals.
        let mut vacant: Vec<u32> = standing
            .iter()
            .filter(|u| u.usage == UnitUse::Flat && self.unit_households.get(&u.id).map_or(true, |v| v.is_empty()))
            .map(|u| u.id)
            .collect();
        vacant.shuffle(&mut self.rng);
        let mut doubled: Vec<u32> = self
            .unit_households
            .values()
            .filter(|v| v.len() > 1)
            .flat_map(|v| v[1..].to_vec())
            .collect();
        doubled.sort_unstable();
        doubled.shuffle(&mut self.rng);
        for hh in doubled {
            let Some(unit) = vacant.pop() else { break };
            // Not everyone can afford to move out.
            if self.rng.gen_bool(0.7) {
                self.move_in(hh, unit, y);
            } else {
                vacant.push(unit);
            }
        }
        let fill = if y == START_YEAR { 1.0 } else { 0.85 };
        for unit in vacant {
            if self.rng.gen_bool(fill) {
                self.arrivals(unit, y);
            }
        }
    }
}

pub fn generate(city: &City) -> Society {
    let directory = address::build(city);
    let mut sim = Sim {
        city,
        rng: rng_for(city.params.seed, 0x50C1E7),
        s: Society {
            people: vec![],
            households: vec![],
            members: vec![],
            businesses: vec![],
            tenancies: vec![],
            directory,
            by_unit: HashMap::new(),
            by_household: HashMap::new(),
        },
        person_hh: HashMap::new(),
        hh_members: HashMap::new(),
        open: HashMap::new(),
        open_member: HashMap::new(),
        unit_households: HashMap::new(),
        unit_business: HashMap::new(),
        business_unit: HashMap::new(),
        hh_home: HashMap::new(),
    };
    for y in START_YEAR..=END_YEAR {
        sim.year(y);
    }
    let mut s = sim.s;
    for (k, t) in s.tenancies.iter().enumerate() {
        s.by_unit.entry(t.unit).or_default().push(k);
    }
    for (k, m) in s.members.iter().enumerate() {
        s.by_household.entry(m.household).or_default().push(k);
    }
    s
}
