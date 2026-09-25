//! Lighting moods. Colours are linear.

use glam::Vec3;
use kwc_engine::{Camera, FrameParams};

pub fn params(cam: &Camera, aspect: f32, night: bool) -> FrameParams {
    let base = FrameParams {
        view_proj: cam.view_proj(aspect),
        cam_pos: cam.eye,
        // Late-afternoon sun low in the west-south-west, over Kowloon Tong.
        sun_dir: Vec3::new(-0.75, 0.42, 0.35),
        sun_col: Vec3::new(1.9, 1.3, 0.85),
        sky_col: Vec3::new(0.16, 0.19, 0.26),
        gnd_col: Vec3::new(0.10, 0.09, 0.08),
        fog_col: Vec3::new(0.46, 0.50, 0.56),
        fog_density: 0.0009,
        fog_height_falloff: 0.04,
        fog_base: 0.0,
        emissive_gain: 0.25,
        canyon_depth: 16.0,
        canyon_strength: 0.65,
    };
    if !night {
        return base;
    }
    FrameParams {
        sun_dir: Vec3::new(0.3, 0.8, -0.4),
        sun_col: Vec3::new(0.03, 0.035, 0.06), // moon / city glow
        sky_col: Vec3::new(0.035, 0.04, 0.07),
        gnd_col: Vec3::new(0.02, 0.015, 0.012),
        fog_col: Vec3::new(0.035, 0.035, 0.055),
        fog_density: 0.0016,
        emissive_gain: 2.2,
        canyon_strength: 0.8,
        ..base
    }
}
