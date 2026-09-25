//! First-person walker: an upright box that steps up stairs, climbs ladders,
//! falls, and never passes through walls.

use crate::world::{Aabb, WalkWorld};
use glam::Vec3;
use kwc_engine::Camera;

pub const RADIUS: f32 = 0.22;
pub const HEIGHT: f32 = 1.7;
pub const EYE: f32 = 1.58;
const STEP: f32 = 0.42;
const WALK: f32 = 2.3;
const RUN: f32 = 4.8;
const CLIMB: f32 = 1.8;
const GRAVITY: f32 = 18.0;
const JUMP: f32 = 5.2;

#[derive(Default, Clone, Copy)]
pub struct Input {
    pub forward: f32,
    pub strafe: f32,
    pub run: bool,
    pub jump: bool,
}

pub struct Player {
    /// Feet position.
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub climbing: bool,
}

impl Player {
    pub fn new(pos: Vec3, yaw: f32) -> Player {
        Player { pos, vel: Vec3::ZERO, yaw, pitch: 0.0, on_ground: false, climbing: false }
    }

    pub fn aabb_at(p: Vec3) -> Aabb {
        Aabb::new(Vec3::new(p.x - RADIUS, p.y, p.z - RADIUS), Vec3::new(p.x + RADIUS, p.y + HEIGHT, p.z + RADIUS))
    }

    /// If standing inside something solid (a save from before a hut went up
    /// there, say), move to the nearest free spot: same level first, then the ground.
    pub fn unstick(&mut self, w: &WalkWorld) {
        if !w.blocked(&Self::aabb_at(self.pos)) {
            return;
        }
        for y in [self.pos.y, 0.0] {
            for ring in 1..=80 {
                let r = ring as f32 * 0.25;
                let n = 8 * ring;
                for k in 0..n {
                    let a = k as f32 / n as f32 * std::f32::consts::TAU;
                    let p = Vec3::new(self.pos.x + r * a.cos(), y, self.pos.z + r * a.sin());
                    if !w.blocked(&Self::aabb_at(p)) {
                        self.pos = p;
                        self.vel = Vec3::ZERO;
                        return;
                    }
                }
            }
        }
        self.pos = w.spawn;
    }

    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.0022;
        self.pitch = (self.pitch - dy * 0.0022).clamp(-1.5, 1.5);
    }

    pub fn camera(&self) -> Camera {
        let eye = self.pos + Vec3::Y * EYE;
        let dir = Vec3::new(self.yaw.cos() * self.pitch.cos(), self.pitch.sin(), self.yaw.sin() * self.pitch.cos());
        Camera { eye, target: eye + dir, fov_y: 72f32.to_radians(), near: 0.05, far: 2500.0 }
    }

    pub fn update(&mut self, w: &WalkWorld, input: Input, dt: f32) {
        // Fixed small substeps keep thin walls and stair edges honest.
        let mut left = dt.min(0.1);
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            self.step(w, input, h);
            left -= h;
        }
    }

    fn step(&mut self, w: &WalkWorld, input: Input, dt: f32) {
        let fwd = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        let mut wish = fwd * input.forward + right * input.strafe;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }
        let speed = if input.run { RUN } else { WALK };

        self.climbing = w.ladder_at(&Self::aabb_at(self.pos)).is_some() && input.forward.abs() > 0.1;
        if self.climbing {
            self.vel = wish * speed * 0.5;
            self.vel.y = CLIMB * input.forward.signum();
        } else {
            // Snappy on the ground, a little drift in the air.
            let target = wish * speed;
            let k = if self.on_ground { 14.0 } else { 2.0 };
            let blend = (k * dt).min(1.0);
            self.vel.x += (target.x - self.vel.x) * blend;
            self.vel.z += (target.z - self.vel.z) * blend;
            self.vel.y -= GRAVITY * dt;
            if input.jump && self.on_ground {
                self.vel.y = JUMP;
            }
        }

        // Horizontal, one axis at a time, stepping up small ledges.
        for axis in [0usize, 2] {
            let d = self.vel[axis] * dt;
            if d == 0.0 {
                continue;
            }
            let mut p = self.pos;
            p[axis] += d;
            if !w.blocked(&Self::aabb_at(p)) {
                self.pos = p;
                continue;
            }
            if self.on_ground || self.climbing {
                let mut up = p;
                up.y += STEP;
                if !w.blocked(&Self::aabb_at(self.pos + Vec3::Y * STEP)) && !w.blocked(&Self::aabb_at(up)) {
                    // Settle back down onto whatever we stepped onto.
                    let mut drop = STEP;
                    while drop > 0.0 {
                        let s = drop.min(0.02);
                        let mut down = up;
                        down.y -= s;
                        if w.blocked(&Self::aabb_at(down)) {
                            break;
                        }
                        up = down;
                        drop -= s;
                    }
                    self.pos = up;
                    continue;
                }
            }
            self.vel[axis] = 0.0;
        }

        // Vertical.
        let dy = self.vel.y * dt;
        let mut p = self.pos;
        p.y += dy;
        if w.blocked(&Self::aabb_at(p)) {
            if dy < 0.0 {
                // Land: find the surface in small increments.
                let mut y = self.pos.y;
                while y > p.y {
                    let t = (y - 0.01).max(p.y);
                    if w.blocked(&Self::aabb_at(Vec3::new(p.x, t, p.z))) {
                        break;
                    }
                    y = t;
                }
                self.pos.y = y;
                self.on_ground = true;
            }
            self.vel.y = 0.0;
        } else {
            self.pos = p;
            // Still on the ground if there's floor just beneath (keeps stairs smooth going down).
            let below = Self::aabb_at(self.pos - Vec3::Y * 0.05);
            self.on_ground = dy <= 0.0 && w.blocked(&below);
            if !self.on_ground && dy <= 0.0 && !self.climbing {
                // Snap down small steps when walking down stairs.
                let snap = Self::aabb_at(self.pos - Vec3::Y * STEP);
                if self.vel.y > -3.0 && w.blocked(&snap) {
                    let mut y = self.pos.y;
                    for _ in 0..(STEP / 0.02) as i32 {
                        let t = y - 0.02;
                        if w.blocked(&Self::aabb_at(Vec3::new(self.pos.x, t, self.pos.z))) {
                            break;
                        }
                        y = t;
                    }
                    self.pos.y = y;
                    self.on_ground = true;
                    self.vel.y = 0.0;
                }
            }
        }
    }
}
