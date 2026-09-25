//! Run mode: race a course's deliveries against the clock. The clock counts
//! simulation ticks (not wall time), so a run is timed the same on any PC,
//! and it starts on your first step, not when the city appears.

use crate::course::Course;
use crate::TICK;
use egui::{Align2, Color32, FontId, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deliveries in a run.
pub const DELIVERIES: usize = 5;
const RECORDS: &str = "runs.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Arrow, door names and notebook all on.
    Rounds,
    /// Nothing but the street plaques: you find the way.
    Memory,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Rounds => "Rounds",
            Category::Memory => "Memory",
        }
    }
}

/// Personal bests: for each course and category, the time at each split.
#[derive(Serialize, Deserialize, Default)]
pub struct Records {
    best: HashMap<String, Vec<u32>>,
}

impl Records {
    pub fn load() -> Records {
        std::fs::read_to_string(RECORDS).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self) {
        if cfg!(test) {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(RECORDS, json);
        }
    }

    pub fn best(&self, course: Course, cat: Category) -> Option<&Vec<u32>> {
        self.best.get(&key(course, cat))
    }
}

fn key(course: Course, cat: Category) -> String {
    format!("{}:{}", course.code(), cat.name())
}

pub struct Run {
    pub course: Course,
    pub cat: Category,
    /// Ticks since the clock started.
    pub ticks: u32,
    pub started: bool,
    /// The clock at each delivery.
    pub splits: Vec<u32>,
    best: Option<Vec<u32>>,
    pub new_best: bool,
}

impl Run {
    pub fn new(course: Course, cat: Category, records: &Records) -> Run {
        Run { course, cat, ticks: 0, started: false, splits: vec![], best: records.best(course, cat).cloned(), new_best: false }
    }

    pub fn finished(&self) -> bool {
        self.splits.len() >= DELIVERIES
    }

    /// One simulation tick. The clock starts when you first move.
    pub fn tick(&mut self, moving: bool) {
        self.started |= moving;
        if self.started && !self.finished() {
            self.ticks += 1;
        }
    }

    /// A delivery made: record the split, and the run if it's a new best.
    pub fn split(&mut self, records: &mut Records) {
        if self.finished() {
            return;
        }
        self.splits.push(self.ticks);
        if self.finished() && self.best.as_ref().is_none_or(|b| self.ticks < *b.last().unwrap_or(&u32::MAX)) {
            self.new_best = true;
            records.best.insert(key(self.course, self.cat), self.splits.clone());
            records.save();
        }
    }

    /// Ahead (negative) or behind the best run at split `i`, in seconds.
    fn delta(&self, i: usize) -> Option<f32> {
        let b = self.best.as_ref()?.get(i)?;
        Some((self.splits[i] as f32 - *b as f32) * TICK)
    }
}

pub fn clock(ticks: u32) -> String {
    let t = ticks as f32 * TICK;
    format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0)
}

fn signed(d: f32) -> String {
    format!("{}{:.2}", if d < 0.0 { "−" } else { "+" }, d.abs())
}

/// The clock and splits (top left), and the results card once it's done.
pub fn hud(ui: &mut egui::Ui, r: &Run) {
    let paper = Color32::from_rgb(236, 228, 208);
    egui::Area::new(egui::Id::new("run")).anchor(Align2::LEFT_TOP, [16.0, 16.0]).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(20, 18, 16, 225)).inner_margin(12.0).corner_radius(4.0).show(ui, |ui| {
            ui.colored_label(Color32::from_rgb(150, 200, 255), RichText::new(format!("{} · {}", r.course.code(), r.cat.name())).small().monospace());
            let colour = if r.finished() { Color32::from_rgb(255, 215, 120) } else { paper };
            ui.colored_label(colour, RichText::new(clock(r.ticks)).font(FontId::monospace(30.0)));
            if !r.started {
                ui.colored_label(Color32::from_gray(170), RichText::new("The clock starts on your first step").small());
            }
            for (i, &t) in r.splits.iter().enumerate() {
                let d = r.delta(i);
                let (txt, col) = match d {
                    Some(d) if d < 0.0 => (signed(d), Color32::from_rgb(120, 220, 140)),
                    Some(d) => (signed(d), Color32::from_rgb(240, 120, 110)),
                    None => (String::new(), paper),
                };
                ui.horizontal(|ui| {
                    ui.colored_label(paper, RichText::new(format!("{}. {}", i + 1, clock(t))).monospace());
                    ui.colored_label(col, RichText::new(txt).monospace());
                });
            }
            for i in r.splits.len()..crate::runs::DELIVERIES {
                ui.colored_label(Color32::from_gray(110), RichText::new(format!("{}. –", i + 1)).monospace());
            }
        });
    });
    if r.finished() {
        egui::Area::new(egui::Id::new("results")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(20, 18, 16, 240)).inner_margin(24.0).corner_radius(6.0).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.colored_label(paper, RichText::new(format!("{} deliveries", DELIVERIES)).size(16.0));
                    ui.colored_label(Color32::from_rgb(255, 215, 120), RichText::new(clock(r.ticks)).font(FontId::monospace(44.0)));
                    let line = match (&r.best, r.new_best) {
                        (None, _) => "First run on this course".to_string(),
                        (Some(_), true) => format!("New best, by {:.2} s", -r.delta(DELIVERIES - 1).unwrap_or(0.0)),
                        (Some(b), false) => format!("Best {}", clock(*b.last().unwrap_or(&0))),
                    };
                    ui.colored_label(if r.new_best { Color32::from_rgb(120, 220, 140) } else { paper }, line);
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(150, 200, 255), RichText::new(format!("Course {} · {}", r.course.code(), r.cat.name())).monospace());
                    ui.colored_label(Color32::from_gray(170), RichText::new("Share the code and race a friend · R run it again · Esc menu").small());
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_starts_on_the_first_step_and_stops_at_the_last_delivery() {
        let course = Course { seed: 1, era: 1950 };
        let mut records = Records::default();
        let mut r = Run::new(course, Category::Rounds, &records);
        for _ in 0..50 {
            r.tick(false);
        }
        assert_eq!(r.ticks, 0, "standing still doesn't start the clock");
        r.tick(true);
        for _ in 0..DELIVERIES {
            for _ in 0..120 {
                r.tick(false);
            }
            r.split(&mut records);
        }
        assert!(r.finished());
        let t = r.ticks;
        r.tick(true);
        assert_eq!(r.ticks, t, "the clock stops at the last delivery");
        assert_eq!(clock(120 * 65 + 30), "1:05.25");
    }
}
