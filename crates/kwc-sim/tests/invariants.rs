use kwc_sim::site::survey_1987 as survey;
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

/// The 1987 survey: 2.6 ha, ~350 buildings "almost all between 10 and 14
/// storeys", 8,500 premises, 33,000 residents, ~23 m² flats, 1–2 m lanes.
#[test]
fn matches_1987_survey() {
    for seed in [1, 2, 1987] {
        let c = city(seed);
        let s = stats::measure(&c, END_YEAR);
        let near = |v: f32, target: f32, tol: f32| (v - target).abs() / target <= tol;
        assert!(near(s.area_m2, survey::AREA_M2, 0.03), "area {}", s.area_m2);
        assert!(near(s.buildings as f32, survey::BUILDINGS as f32, 0.15), "buildings {}", s.buildings);
        assert!(near(s.units as f32, survey::PREMISES as f32, 0.15), "premises {}", s.units);
        assert!(near(s.residents as f32, survey::RESIDENTS as f32, 0.15), "residents {}", s.residents);
        assert!(near(s.mean_unit_m2, 23.0, 0.15), "unit m² {}", s.mean_unit_m2);
        assert!(s.frac_10_plus >= 0.9, "10+ storeys {}", s.frac_10_plus);
        assert!((0.08..=0.18).contains(&s.lane_share), "lane share {}", s.lane_share);
    }
}

#[test]
fn documented_features_present() {
    let c = city(1987);
    let n = |k: FeatureKind| c.features.iter().filter(|f| f.kind == k).count();
    assert_eq!(n(FeatureKind::WaterStandpipe), survey::WATER_STANDPIPES);
    assert_eq!(n(FeatureKind::Lift), survey::LIFTS);
    assert_eq!(n(FeatureKind::NaturalWell), 1);
    assert_eq!(n(FeatureKind::Temple), 2);
    for name in ["Lung Chun Road", "Lung Chun Back Road", "Lo Yan Street", "Sai Shing Road", "Tai Chang Street"] {
        assert!(c.lanes.iter().any(|l| l.name == name), "missing lane {name}");
    }
}
