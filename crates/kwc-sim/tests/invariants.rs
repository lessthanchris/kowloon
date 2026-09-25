use kwc_sim::*;

fn city(seed: u64) -> City {
    generate(&Params { seed, ..Default::default() })
}

#[test]
fn deterministic() {
    let a = export::to_json(&city(7));
    let b = export::to_json(&city(7));
    assert_eq!(a, b);
    assert_ne!(a, export::to_json(&city(8)));
}

#[test]
fn height_cap_and_yamen() {
    for seed in [1, 2, 3] {
        let c = city(seed);
        for p in &c.plots {
            assert!(p.final_height() as usize <= MAX_FLOORS);
            assert!(p.ambition as usize <= MAX_FLOORS);
        }
        for (k, g) in c.ground.iter().enumerate() {
            if *g == Ground::Yamen {
                assert_eq!(c.plot_of[k], NO_PLOT, "yamen cell built on");
            }
        }
        assert!(c.ground.iter().any(|g| *g == Ground::Yamen));
    }
}

#[test]
fn every_unit_reachable_every_era() {
    for seed in [1, 2, 3, 1987] {
        let c = city(seed);
        for year in [1955, 1965, 1975, END_YEAR] {
            let bad = walk::unreachable_units(&c, year);
            assert!(bad.is_empty(), "seed {seed} year {year}: {} unreachable units", bad.len());
        }
    }
}

#[test]
fn timeline_never_loses_floors() {
    let c = city(3);
    for p in &c.plots {
        let mut last = 0;
        for y in START_YEAR..=END_YEAR {
            let h = p.height_at(y);
            assert!(h >= last, "plot {} shrank in {y}", p.id);
            last = h;
        }
    }
}

#[test]
fn plausible_scale() {
    let c = city(1987);
    let n = c.units_at(END_YEAR).count();
    assert!((5_000..=16_000).contains(&n), "unit count {n}");
    let mean = c.plots.iter().map(|p| p.final_height() as f32).sum::<f32>() / c.plots.len() as f32;
    assert!(mean > 10.0, "mean height {mean}");
}
