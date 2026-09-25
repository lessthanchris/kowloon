//! Headless tuning tool: `cargo run -p kwc-sim --bin dump -- [seed] [out_dir]`
//! Writes plan PNGs across the timeline plus a stats table and city.json.

use kwc_sim::export::{render_map, to_json, MapMode};
use kwc_sim::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1987);
    let out = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| "dumps".into()));
    std::fs::create_dir_all(&out).unwrap();

    let t = std::time::Instant::now();
    let city = generate(&Params { seed, ..Default::default() });
    println!("seed {seed}: generated in {:.0?}", t.elapsed());
    println!(
        "grid {}x{} ({:.0} x {:.0} m), plots {}, lanes {}, bridges {}, units {}",
        city.w,
        city.d,
        city.w as f32 * CELL_M,
        city.d as f32 * CELL_M,
        city.plots.len(),
        city.lanes.len(),
        city.bridges.len(),
        city.units.len()
    );
    let count = |g: Ground| city.ground.iter().filter(|&&x| x == g).count();
    println!(
        "cells: plot {}  alley {}  well {}  yamen {}",
        count(Ground::Plot),
        count(Ground::Alley),
        count(Ground::Well),
        count(Ground::Yamen)
    );

    println!("\nyear  settled  mean_h  at_cap  units   ~people");
    for year in (START_YEAR..=END_YEAR).step_by(1) {
        let hs: Vec<u8> = city.plots.iter().map(|p| p.height_at(year)).collect();
        let settled = hs.iter().filter(|&&h| h > 0).count();
        let mean = hs.iter().map(|&h| h as f32).sum::<f32>() / hs.len() as f32;
        let cap = hs.iter().filter(|&&h| h as usize >= MAX_FLOORS).count();
        let units = city.units_at(year).count();
        if year % 5 == 0 || year == END_YEAR {
            println!("{year}  {settled:7}  {mean:6.1}  {cap:6}  {units:5}  {:8}", (units as f32 * 3.4) as u32);
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
