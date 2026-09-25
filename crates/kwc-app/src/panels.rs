//! The notebook (M) and the ledger (L).

use crate::game::{Game, Met};
use glam::Vec3;
use kwc_engine::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use kwc_sim::society::{Occupant, Society};
use kwc_sim::*;

const PAPER: Color32 = Color32::from_rgb(232, 224, 200);
const INK: Color32 = Color32::from_rgb(40, 34, 28);

fn family_ink(k: usize) -> Color32 {
    let c = crate::lights::FAMILY[k % crate::lights::FAMILY.len()];
    // Muted: a pencil wash of the lane family's colour.
    let mix = |v: u8| ((v as u16 + 2 * 120) / 3) as u8;
    Color32::from_rgb(mix(c[0]), mix(c[1]), mix(c[2]))
}

/// Your notebook map: only what you've walked near this era.
pub fn notebook(ui: &mut egui::Ui, city: &City, soc: &Society, g: &Game, feet: Vec3, yaw: f32) {
    let screen = ui.max_rect();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("notebook")));
    let area = screen.shrink(40.0);
    let cs = (area.width() / city.w as f32).min(area.height() / city.d as f32);
    let origin = Pos2::new(area.center().x - cs * city.w as f32 / 2.0, area.center().y - cs * city.d as f32 / 2.0);
    let page = Rect::from_min_size(origin, egui::vec2(cs * city.w as f32, cs * city.d as f32)).expand(18.0);
    painter.rect_filled(page, 6.0, PAPER);
    let at = |c: Cell| Rect::from_min_size(Pos2::new(origin.x + c.0 as f32 * cs, origin.y + c.1 as f32 * cs), egui::vec2(cs, cs));
    let year = g.year;
    for &c in &g.seen {
        let col = match city.ground_at(c) {
            Ground::Alley => match crate::lights::family(city, &soc.directory, c) {
                Some(k) => family_ink(k),
                None => Color32::from_rgb(150, 140, 120),
            },
            Ground::Plot if city.height_at(c, year) > 0 => {
                let h = city.height_at(c, year) as f32 / MAX_FLOORS as f32;
                let v = (205.0 - 90.0 * h) as u8;
                Color32::from_rgb(v, (v as f32 * 0.95) as u8, (v as f32 * 0.85) as u8)
            }
            Ground::Plot => Color32::from_rgb(214, 208, 180),
            Ground::Well => Color32::from_rgb(120, 150, 180),
            Ground::Yamen => Color32::from_rgb(170, 90, 70),
            Ground::Outside => continue,
        };
        painter.rect_filled(at(c), 0.0, col);
    }
    // Lane names where you've walked a good stretch of them.
    let mut cells_of: std::collections::HashMap<u16, Vec<Cell>> = Default::default();
    for &c in &g.seen {
        if let Some(l) = soc.directory.lane_of[city.idx(c)] {
            cells_of.entry(l).or_default().push(c);
        }
    }
    for (l, mut cs_) in cells_of {
        if cs_.len() < 4 {
            continue;
        }
        cs_.sort_unstable();
        let mid = cs_[cs_.len() / 2];
        painter.text(at(mid).center(), Align2::CENTER_CENTER, &soc.directory.lane_names[l as usize], FontId::proportional(11.0), INK);
    }
    // Landmarks.
    for f in city.features.iter().filter(|f| g.seen.contains(&f.cell)) {
        let (col, label) = match f.kind {
            FeatureKind::WaterStandpipe => (Color32::from_rgb(40, 90, 160), "tap"),
            FeatureKind::NaturalWell => (Color32::from_rgb(30, 70, 140), "Big Well"),
            FeatureKind::Temple => (Color32::from_rgb(180, 40, 30), f.name.as_deref().unwrap_or("temple")),
            FeatureKind::SouthGate => (Color32::from_rgb(90, 70, 50), "South Gate"),
            FeatureKind::Lift => continue,
        };
        let p = at(f.cell).center();
        painter.circle_filled(p, 3.5, col);
        painter.text(p + egui::vec2(5.0, -2.0), Align2::LEFT_BOTTOM, label, FontId::proportional(10.0), col);
    }
    // Job doors, if you've been near them.
    if let Some(j) = &g.job {
        for (unit, col, what) in [(j.from, Color32::from_rgb(40, 150, 70), "collect"), (j.to, Color32::from_rgb(200, 130, 20), "deliver")] {
            if what == "collect" && j.picked {
                continue;
            }
            let c = city.units[unit as usize].door.cell;
            if g.seen.contains(&c) {
                let p = at(c).center();
                painter.circle_stroke(p, 6.0, Stroke::new(2.0, col));
                painter.text(p + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, what, FontId::proportional(11.0), col);
            }
        }
    }
    // You.
    let me = Pos2::new(origin.x + feet.x / CELL_M * cs, origin.y + feet.z / CELL_M * cs);
    let (s, c) = yaw.sin_cos();
    let pt = |fx: f32, fy: f32| Pos2::new(me.x + fx * c - fy * s, me.y + fx * s + fy * c);
    painter.add(egui::Shape::convex_polygon(vec![pt(9.0, 0.0), pt(-6.0, 5.0), pt(-3.0, 0.0), pt(-6.0, -5.0)], Color32::from_rgb(200, 30, 30), Stroke::NONE));
    painter.text(page.left_top() + egui::vec2(12.0, 10.0), Align2::LEFT_TOP, format!("Notebook · {}", g.year), FontId::proportional(18.0), INK);
    painter.text(
        page.left_bottom() + egui::vec2(12.0, -10.0),
        Align2::LEFT_BOTTOM,
        "Only what you've walked. Opening it spoils a memory run. M to close.",
        FontId::proportional(12.0),
        INK,
    );
}

fn who(soc: &Society, m: &Met) -> String {
    soc.describe(m.occupant())
}

/// Everyone you've delivered to, and their story.
pub fn ledger(ui: &mut egui::Ui, soc: &Society, g: &Game, selected: &mut Option<usize>) {
    egui::Window::new("Ledger").anchor(Align2::CENTER_CENTER, [0.0, 0.0]).fixed_size([760.0, 480.0]).collapsible(false).show(ui.ctx(), |ui| {
        if g.met.is_empty() {
            ui.label("Nobody yet. Everyone you deliver to is written down here, with their family's story.");
            return;
        }
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("met").max_width(300.0).max_height(440.0).show(ui, |ui| {
                for (k, m) in g.met.iter().enumerate().rev() {
                    let text = format!("{} · {}", m.year, who(soc, m));
                    if ui.selectable_label(*selected == Some(k), text).clicked() {
                        *selected = Some(k);
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("story").max_height(440.0).show(ui, |ui| {
                let Some(m) = selected.and_then(|k| g.met.get(k)) else {
                    ui.label("Pick someone.");
                    return;
                };
                story(ui, soc, m, g.year);
            });
        });
    });
}

fn story(ui: &mut egui::Ui, soc: &Society, m: &Met, now: u16) {
    let addr = |u: u32| soc.directory.address[u as usize].line();
    match m.occupant() {
        Occupant::Household(h) => {
            let hh = &soc.households[h as usize];
            ui.heading(soc.describe(m.occupant()));
            ui.label(format!("You first delivered to them in {} at {}.", m.year, addr(m.unit)));
            ui.add_space(6.0);
            ui.strong(format!("At home in {now}"));
            for p in soc.members_in(h, now) {
                ui.label(format!("  {} ({}), born {}", p.name(), p.age(now), p.born));
            }
            if soc.members_in(h, now).is_empty() {
                ui.label(format!("  The household ended in {}.", hh.ended.unwrap_or(now)));
            }
            ui.add_space(6.0);
            ui.strong("The family line");
            for &l in soc.lineage(h).iter().rev() {
                let x = &soc.households[l as usize];
                if x.founded > now {
                    continue;
                }
                let head = &soc.people[x.head as usize];
                let how = if x.arrived_from_outside { "arrived" } else { "set up home" };
                ui.label(format!("  {} {how} in {}", head.name(), x.founded));
                for (u, from, to) in soc.homes_of(l).into_iter().filter(|h| h.1 <= now) {
                    let to = to.filter(|&t| t <= now).map_or(String::new(), |t| format!("-{t}"));
                    ui.small(format!("      lived at {} ({from}{to})", addr(u)));
                }
                for b in soc.businesses.iter().filter(|b| b.opened <= now && b.owners.iter().any(|o| o.0 == l)) {
                    ui.small(format!("      ran {} (from {})", b.name, b.opened));
                }
            }
        }
        Occupant::Business(b) => {
            let bz = &soc.businesses[b as usize];
            ui.heading(&bz.name);
            ui.label(format!("Trading since {} at {}.", bz.opened, addr(m.unit)));
            if let Some(c) = bz.closed.filter(|&c| c <= now) {
                ui.label(format!("Closed in {c}."));
            }
            ui.add_space(6.0);
            ui.strong("Run by");
            for &(h, from) in bz.owners.iter().filter(|o| o.1 <= now) {
                let head = &soc.people[soc.households[h as usize].head as usize];
                ui.label(format!("  {} ({}), from {from}", soc.describe(Occupant::Household(h)), head.name()));
            }
        }
        Occupant::Temple => {}
    }
}
