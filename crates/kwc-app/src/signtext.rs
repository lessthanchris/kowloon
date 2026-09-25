//! Names painted on the city as real geometry: every plate's text laid out
//! once with egui's fonts into world-space glyph quads, drawn in the 3D pass
//! so walls hide it.

use crate::game::Plate;
use glam::Vec3;
use kwc_engine::{egui, TextVertex};

pub struct SignText {
    ctx: egui::Context,
}

fn linear(c: [u8; 3]) -> [f32; 4] {
    let f = |v: u8| (v as f32 / 255.0).powf(2.2);
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

impl SignText {
    pub fn new() -> SignText {
        let ctx = egui::Context::default();
        // One empty pass so the font system exists.
        let _ = ctx.run_ui(Default::default(), |_| {});
        SignText { ctx }
    }

    /// Glyph quads for `plates` (all of them, or just `only`), optionally recoloured.
    pub fn build(&self, plates: &[Plate], only: Option<&[usize]>, tint: Option<[u8; 3]>) -> (Vec<TextVertex>, Vec<u32>) {
        let mut v: Vec<TextVertex> = vec![];
        let mut idx: Vec<u32> = vec![];
        let ids: Vec<usize> = match only {
            Some(o) => o.to_vec(),
            None => (0..plates.len()).collect(),
        };
        for k in ids {
            let pl = &plates[k];
            let colour = linear(tint.unwrap_or(pl.colour));
            let galleys: Vec<_> = pl
                .lines
                .iter()
                .filter(|l| !l.is_empty())
                .map(|l| self.ctx.fonts_mut(|f| f.layout_no_wrap(l.clone(), egui::FontId::proportional(28.0), egui::Color32::WHITE)))
                .collect();
            if galleys.is_empty() {
                continue;
            }
            let gh = galleys[0].size().y;
            let widest = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
            let s = (pl.line_h / gh).min(pl.max_w / widest.max(1.0));
            let total_h = gh * s * galleys.len() as f32;
            for (li, gal) in galleys.iter().enumerate() {
                let origin = pl.centre - pl.right * (gal.size().x * s / 2.0) + Vec3::Y * (total_h / 2.0 - li as f32 * gh * s);
                for row in &gal.rows {
                    let base = v.len() as u32;
                    for gv in &row.visuals.mesh.vertices {
                        let gp = row.pos + gv.pos.to_vec2();
                        let p = origin + pl.right * (gp.x * s) - Vec3::Y * (gp.y * s);
                        // Texel UVs for now: the atlas may grow while we lay text out.
                        v.push(TextVertex { pos: p.into(), uv: [gv.uv.x, gv.uv.y], color: colour });
                    }
                    idx.extend(row.visuals.mesh.indices.iter().map(|i| i + base));
                }
            }
        }
        // Normalise against the atlas as it is once every glyph is in it.
        let size = self.ctx.fonts(|f| f.font_image_size());
        let (un, vn) = (1.0 / size[0] as f32, 1.0 / size[1] as f32);
        for t in &mut v {
            t.uv = [t.uv[0] * un, t.uv[1] * vn];
        }
        (v, idx)
    }

    /// The font atlas as RGBA8 (coverage in alpha). Take it after `build`, so
    /// every glyph used is in it.
    pub fn atlas(&self) -> (u32, u32, Vec<u8>) {
        let img = self.ctx.fonts(|f| f.image());
        let mut rgba = Vec::with_capacity(img.pixels.len() * 4);
        for p in &img.pixels {
            rgba.extend_from_slice(&[255, 255, 255, p.a()]);
        }
        (img.size[0] as u32, img.size[1] as u32, rgba)
    }
}

