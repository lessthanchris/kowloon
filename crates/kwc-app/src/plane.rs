//! The Kai Tak approach: every so often a jet comes in low over the city,
//! always on the same heading (runway 13: from the north-west, down to the
//! south-east), so it doubles as a compass.

use crate::citymesh::srgb;
use glam::Vec3;
use kwc_engine::mesh::MeshData;

/// Seconds between passes, and how long each pass takes to cross.
const PERIOD: f32 = 75.0;
const CROSSING: f32 = 14.0;

/// Direction of travel: bearing ~130°, i.e. east-south-east (x east, z south).
pub fn heading() -> Vec3 {
    Vec3::new(0.766, -0.02, 0.643).normalize()
}

/// An oriented box: centre, axes (right-handed: f × r = u, or the faces
/// come out inside-out) and half-sizes along them.
pub fn obox(m: &mut MeshData, c: Vec3, f: Vec3, r: Vec3, u: Vec3, h: Vec3, col: [f32; 3], emit: f32) {
    let p = |sf: f32, sr: f32, su: f32| c + f * (h.x * sf) + r * (h.y * sr) + u * (h.z * su);
    let faces = [
        [p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.), p(1., -1., 1.)],
        [p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.)],
        [p(-1., 1., -1.), p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.)],
        [p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.)],
        [p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)],
        [p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.), p(1., -1., -1.)],
    ];
    for q in faces {
        m.quad(q, col, emit);
    }
}

/// The jet at time `t` (seconds since start), if one is passing.
pub fn mesh(t: f32, centre: Vec3) -> Option<MeshData> {
    let phase = t % PERIOD;
    if phase > CROSSING {
        return None;
    }
    let f = heading();
    let s = phase / CROSSING;
    // From 700 m out to 700 m past, descending from ~75 m to ~45 m.
    let along = -700.0 + 1400.0 * s;
    let pos = centre + Vec3::new(f.x, 0.0, f.z) * along + Vec3::Y * (75.0 - 30.0 * s);
    let fwd = Vec3::new(f.x, 0.0, f.z).normalize();
    let right = Vec3::Y.cross(fwd).normalize();
    let up = Vec3::Y;
    let mut m = MeshData::default();
    let body = srgb(200, 200, 205);
    obox(&mut m, pos, fwd, right, up, Vec3::new(30.0, 3.2, 3.4), body, 0.0);
    obox(&mut m, pos + fwd * 2.0 - up * 1.0, fwd, right, up, Vec3::new(5.0, 30.0, 0.5), body, 0.0);
    obox(&mut m, pos - fwd * 26.0, fwd, right, up, Vec3::new(3.0, 10.0, 0.4), body, 0.0);
    obox(&mut m, pos - fwd * 27.0 + up * 5.0, fwd, right, up, Vec3::new(3.5, 0.4, 5.0), srgb(170, 40, 40), 0.0);
    // Engines.
    for side in [-1.0f32, 1.0] {
        obox(&mut m, pos + fwd * 4.0 + right * (side * 11.0) - up * 2.2, fwd, right, up, Vec3::new(3.0, 1.2, 1.2), srgb(150, 150, 155), 0.0);
    }
    // Lights: landing lights ahead, red and green wingtips, a red beacon.
    obox(&mut m, pos + fwd * 30.0 - up * 1.5, fwd, right, up, Vec3::new(0.6, 1.4, 0.6), srgb(255, 250, 235), 6.0);
    obox(&mut m, pos + fwd * 2.0 - right * 30.0 - up * 1.0, fwd, right, up, Vec3::splat(0.5), srgb(255, 40, 40), 4.0);
    obox(&mut m, pos + fwd * 2.0 + right * 30.0 - up * 1.0, fwd, right, up, Vec3::splat(0.5), srgb(40, 255, 80), 4.0);
    if (t * 1.2).fract() < 0.15 {
        obox(&mut m, pos + up * 3.6, fwd, right, up, Vec3::splat(0.4), srgb(255, 30, 30), 6.0);
    }
    Some(m)
}
