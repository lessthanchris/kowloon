use kwc_sim::society::{self, Occupant};
use kwc_sim::*;
use std::collections::HashSet;

fn world(seed: u64) -> (City, society::Society) {
    let c = generate(&Params { seed, ..Default::default() });
    let s = society::generate(&c);
    (c, s)
}

#[test]
fn deterministic() {
    let (_, a) = world(5);
    let (_, b) = world(5);
    let names = |s: &society::Society| s.people.iter().take(200).map(|p| p.name()).collect::<Vec<_>>();
    assert_eq!(a.people.len(), b.people.len());
    assert_eq!(names(&a), names(&b));
    assert_eq!(a.businesses.iter().map(|b| b.name.clone()).collect::<Vec<_>>(), b.businesses.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
}

/// Survey: ~33,000 residents in 1987; the sim lands a little under.
#[test]
fn population_in_range() {
    for seed in [1, 1987] {
        let (_, s) = world(seed);
        let p = s.population(END_YEAR);
        assert!((24_000..=40_000).contains(&p), "seed {seed}: {p} residents in 1987");
        assert!(s.population(1960) < s.population(1975), "the city should fill up");
    }
}

#[test]
fn flats_are_lived_in() {
    let (c, s) = world(1987);
    let flats: Vec<&Unit> = c.units_at(END_YEAR).filter(|u| u.usage == UnitUse::Flat).collect();
    let lived = flats.iter().filter(|u| s.occupants(u.id, END_YEAR).iter().any(|o| matches!(o, Occupant::Household(_)))).count();
    assert!(lived as f32 >= 0.9 * flats.len() as f32, "{lived}/{} flats occupied", flats.len());
    // And the premises carry on trading.
    let shops: Vec<&Unit> = c.units_at(END_YEAR).filter(|u| !matches!(u.usage, UnitUse::Flat | UnitUse::Temple)).collect();
    let open = shops.iter().filter(|u| s.occupants(u.id, END_YEAR).iter().any(|o| matches!(o, Occupant::Business(_)))).count();
    assert!(open as f32 >= 0.9 * shops.len() as f32, "{open}/{} premises trading", shops.len());
}

/// Families carry on across the eras: some 1987 households descend from 1950s arrivals.
#[test]
fn family_lines_span_generations() {
    let (_, s) = world(1987);
    let now: Vec<u32> = s.households.iter().filter(|h| h.ended.is_none()).map(|h| h.id).collect();
    let old_roots = now.iter().filter(|&&h| s.households[*s.lineage(h).last().unwrap() as usize].founded <= 1955).count();
    assert!(old_roots * 20 >= now.len(), "only {old_roots}/{} households descend from the 1950s", now.len());
    assert!(now.iter().any(|&h| s.lineage(h).len() >= 3), "no three-generation lines");
    let handed_down = s.businesses.iter().filter(|b| b.owners.len() > 1).count();
    assert!(handed_down >= 20, "only {handed_down} businesses changed hands");
}

/// Every standing unit has a distinct, complete address each year.
#[test]
fn addresses_are_unique() {
    let (c, s) = world(1987);
    for year in [1960, END_YEAR] {
        let mut seen = HashSet::new();
        for u in c.units_at(year) {
            let a = &s.directory.address[u.id as usize];
            assert!(!a.lane.is_empty() && a.number > 0, "unit {} has no address", u.id);
            assert!(seen.insert(a.line()), "duplicate address {} in {year}", a.line());
        }
    }
}
