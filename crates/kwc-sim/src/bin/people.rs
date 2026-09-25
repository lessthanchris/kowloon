//! Read the people: `cargo run -p kwc-sim --bin people -- [seed] [lane substring]`
//! Population by era, a street directory in two years, and one family's story.

use kwc_sim::society::{self, Occupant, Society};
use kwc_sim::*;

fn directory(city: &City, s: &Society, lane: &str, year: u16) {
    println!("\n== {lane} in {year} ==");
    let mut rows: Vec<(String, u32, String)> = vec![];
    for u in city.units_at(year) {
        let a = &s.directory.address[u.id as usize];
        if a.lane != lane {
            continue;
        }
        let who: Vec<String> = s
            .occupants(u.id, year)
            .into_iter()
            .map(|o| match o {
                Occupant::Household(h) => {
                    let m = s.members_in(h, year);
                    let head = &s.people[s.households[h as usize].head as usize];
                    let n = m.len();
                    format!("{} ({n} {}, head {})", s.describe(o), if n == 1 { "person" } else { "people" }, head.name())
                }
                _ => s.describe(o),
            })
            .collect();
        let who = if who.is_empty() { "(empty)".to_string() } else { who.join(" + ") };
        rows.push((a.building(), a.floor as u32 * 100 + a.flat.map_or(0, |c| c as u32), format!("{:<28} {}", a.line(), who)));
    }
    rows.sort();
    for r in rows.iter().take(40) {
        println!("  {}", r.2);
    }
    if rows.len() > 40 {
        println!("  ... {} more", rows.len() - 40);
    }
}

fn story(city: &City, s: &Society) {
    // The longest-running family line still here in 1987.
    let best = s
        .households
        .iter()
        .filter(|h| h.ended.is_none())
        .max_by_key(|h| (s.lineage(h.id).len(), std::cmp::Reverse(s.households[*s.lineage(h.id).last().unwrap() as usize].founded)))
        .unwrap();
    let line = s.lineage(best.id);
    println!("\n== A family line ({} generations of households) ==", line.len());
    for &h in line.iter().rev() {
        let hh = &s.households[h as usize];
        let head = &s.people[hh.head as usize];
        let how = if hh.arrived_from_outside { "arrived" } else { "set up home" };
        println!("  {} {how} in {} ({}, born {})", s.describe(Occupant::Household(h)), hh.founded, head.name(), head.born);
        for (unit, from, to) in s.homes_of(h) {
            let a = &s.directory.address[unit as usize];
            let to = to.map_or("".into(), |t| format!("-{t}"));
            println!("      lived at {} ({from}{to})", a.line());
        }
        for b in s.businesses.iter().filter(|b| b.owners.iter().any(|o| o.0 == h)) {
            println!("      ran {} ({})", b.name, b.opened);
        }
        if let Some(end) = hh.ended {
            println!("      household ended {end}");
        }
    }
    let _ = city;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1987);
    let lane = args.get(2).cloned().unwrap_or_else(|| "Lo Yan Street".into());
    let city = generate(&Params { seed, ..Default::default() });
    let t = std::time::Instant::now();
    let s = society::generate(&city);
    println!("society in {:.0?}: {} people ever, {} households, {} businesses", t.elapsed(), s.people.len(), s.households.len(), s.businesses.len());
    println!("\nyear  residents  households  businesses  shared flats");
    for y in (START_YEAR..=END_YEAR).filter(|y| y % 5 == 0 || *y == END_YEAR) {
        let hh = s.households.iter().filter(|h| h.founded <= y && h.ended.map_or(true, |e| e > y)).count();
        let biz = s.businesses.iter().filter(|b| b.opened <= y && b.closed.map_or(true, |c| c > y)).count();
        let shared = city.units_at(y).filter(|u| s.occupants(u.id, y).iter().filter(|o| matches!(o, Occupant::Household(_))).count() > 1).count();
        println!("{y}  {:9}  {:10}  {:10}  {:12}", s.population(y), hh, biz, shared);
    }
    println!("\nlanes named: {}", s.directory.lane_names.len());
    let exact = s.directory.lane_names.iter().find(|n| n.contains(&lane)).cloned().unwrap_or(lane);
    directory(&city, &s, &exact, 1960);
    directory(&city, &s, &exact, 1987);
    story(&city, &s);
}
