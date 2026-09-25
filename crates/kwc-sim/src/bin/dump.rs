//! Headless tuning tool: `cargo run -p kwc-sim --bin dump -- [seed] [out_dir]`
//! Writes plan PNGs across the timeline, compares against the 1987 survey, and
//! writes city.json.

use kwc_sim::export::{render_map, to_json, MapMode};
use kwc_sim::site::survey_1987 as survey;
use kwc_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1987);
    let out = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| "dumps".into()));
    std::fs::create_dir_all(&out).unwrap();

    let t = std::time::Instant::now();
    let city = generate(&Params { seed, ..Default::default() });
    println!("seed {seed}: generated in {:.0?}", t.elapsed());
    let s = stats::measure(&city, END_YEAR);
    println!("grid {}x{} cells of {CELL_M} m, lanes named: {}", city.w, city.d, city.lanes.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", "));
    println!("                    model     1987 survey");
    println!("site area m²      {:7.0}     {:7.0}", s.area_m2, survey::AREA_M2);
    println!("buildings         {:7}     {:7}", s.buildings, survey::BUILDINGS);
    println!("premises (units)  {:7}     {:7}", s.units, survey::PREMISES);
    println!("residents (est.)  {:7}     {:7}", s.residents, survey::RESIDENTS);
    println!("mean unit m²      {:7.1}     {:7.1}", s.mean_unit_m2, 23.0);
    println!("bldgs >= 10 storeys {:5.0}%     'almost all'", s.frac_10_plus * 100.0);
    println!("lane share        {:6.1}%", s.lane_share * 100.0);
    println!("light wells       {:6.1}%", s.well_share * 100.0);
    println!("corridor share    {:6.1}% of building cells", s.corridor_share * 100.0);
    println!("bridges {}, features {}", city.bridges.len(), city.features.len());

    println!("\nyear  settled  mean_h  at_cap  units  ~people");
    for year in START_YEAR..=END_YEAR {
        if year % 5 == 0 || year == END_YEAR {
            let s = stats::measure(&city, year);
            println!("{year}  {:7}  {:6.1}  {:6}  {:5}  {:7}", s.settled, s.mean_height, s.at_cap, s.units, s.residents);
            render_map(&city, year, MapMode::Height).save(out.join(format!("height_{year}.png"))).unwrap();
        }
    }
    render_map(&city, END_YEAR, MapMode::Decade).save(out.join("decade_1987.png")).unwrap();
    for f in [0u8, 1, 5, 10, 13] {
        render_map(&city, END_YEAR, MapMode::Slice(f)).save(out.join(format!("slice_{f:02}.png"))).unwrap();
    }
    let bad = walk::unreachable_units(&city, END_YEAR);
    println!("\nunreachable units in {END_YEAR}: {}", bad.len());
    std::fs::write(out.join("city.json"), to_json(&city)).unwrap();
    println!("wrote {}", out.display());
}
