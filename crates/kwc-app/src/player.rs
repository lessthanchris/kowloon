//! First-person walker: an upright box that steps up stairs, climbs ladders,
//! falls, and never passes through walls.

use crate::world::{Aabb, WalkWorld};
use glam::Vec3;
use kwc_engine::Camera;

pub const RADIUS: f32 = 0.22;
pub const HEIGHT: f32 = 1.7;
pub const EYE: f32 = 1.58;
const STEP: f32 = 0.42;
/// A brisk courier's walk, and (Shift) a steady jog: not a sprint.
const WALK: f32 = 2.5;
const RUN: f32 = 3.9;
const CLIMB: f32 = 1.8;
/// Sliding down a ladder, hands on the rails.
const SLIDE: f32 = 4.0;
const GRAVITY: f32 = 18.0;
/// A short hop (about half a metre), not a leap.
const JUMP: f32 = 4.2;
/// A vault takes this long whatever it clears: never quicker than going round.
const VAULT_TIME: f32 = 0.55;

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
    /// Where the feet were at the start of the last tick, and how far
    /// (0..1) the frame being drawn is between that tick and the next:
    /// the simulation runs on a fixed tick, the view is interpolated.
    prev: Vec3,
    pub alpha: f32,
    /// View only (never affects where you are): eye height smoothed over
    /// stair treads, head bob phase, the dip on landing, the jog's wider view.
    eye_y: f32,
    prev_eye_y: f32,
    bob: f32,
    prev_bob: f32,
    bob_amp: f32,
    dip: f32,
    dip_v: f32,
    fov: f32,
    /// Mid-vault: from, to, the height to clear, and progress 0..1.
    vault: Option<(Vec3, Vec3, f32, f32)>,
    /// From the player's settings: field of view and head bob strength.
    pub base_fov: f32,
    pub bob_scale: f32,
}

impl Player {
    pub fn new(pos: Vec3, yaw: f32) -> Player {
        Player {
            pos,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            on_ground: false,
            climbing: false,
            prev: pos,
            alpha: 1.0,
            eye_y: pos.y,
            prev_eye_y: pos.y,
            bob: 0.0,
            prev_bob: 0.0,
            bob_amp: 0.0,
            dip: 0.0,
            dip_v: 0.0,
            fov: 72.0,
            vault: None,
            base_fov: 72.0,
            bob_scale: 1.0,
        }
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
        // Between ticks (unless we've just been moved a long way, e.g. unstuck).
        let near = self.prev.distance(self.pos) < 2.0;
        let mut at = if near { self.prev.lerp(self.pos, self.alpha) } else { self.pos };
        at.y = if near { self.prev_eye_y + (self.eye_y - self.prev_eye_y) * self.alpha } else { self.eye_y };
        // Head bob: a soft rise and fall per step (a smooth cosine, no
        // bounce at the bottom), the barest sway per stride; drawn between
        // ticks like everything else.
        let phase = self.prev_bob + (self.bob - self.prev_bob) * self.alpha;
        let side = Vec3::new(-self.yaw.sin(), 0.0, self.yaw.cos());
        let amp = self.bob_amp * self.bob_scale;
        let bob = Vec3::Y * (amp * 0.5 * (1.0 - (phase * 2.0).cos())) + side * (amp * 0.25 * phase.sin());
        let eye = at + Vec3::Y * (EYE - self.dip) + bob;
        let dir = Vec3::new(self.yaw.cos() * self.pitch.cos(), self.pitch.sin(), self.yaw.sin() * self.pitch.cos());
        Camera { eye, target: eye + dir, fov_y: self.fov.to_radians(), near: 0.05, far: 2500.0 }
    }

    pub fn update(&mut self, w: &WalkWorld, input: Input, dt: f32) {
        self.prev = self.pos;
        self.prev_eye_y = self.eye_y;
        let falling = self.vel.y;
        let was_ground = self.on_ground;
        if self.vault.is_some() {
            self.vaulting(dt);
        } else if input.jump && self.on_ground && self.try_vault(w) {
            // Vaulting instead of hopping.
        } else {
            // Fixed small substeps keep thin walls and stair edges honest.
            let mut left = dt.min(0.1);
            while left > 0.0 {
                let h = left.min(1.0 / 240.0);
                self.step(w, input, h);
                left -= h;
            }
        }
        self.feel(dt, input, !was_ground && self.on_ground, falling);
    }

    /// Camera feel, from how you're moving (view only).
    fn feel(&mut self, dt: f32, input: Input, landed: bool, falling: f32) {
        // Eye height follows the feet smoothly up and down stairs; big
        // changes (a fall, a ladder) are followed at once.
        let gap = self.pos.y - self.eye_y;
        self.eye_y = if gap.abs() > 1.0 || self.climbing || !self.on_ground { self.pos.y } else { self.eye_y + gap * (dt * 16.0).min(1.0) };
        let speed = Vec3::new(self.vel.x, 0.0, self.vel.z).length();
        let moving = self.on_ground && speed > 0.3 && self.vault.is_none();
        // One bob cycle per two steps (about 1.5 m of stride). Gentle, and
        // eased in and out so starting and stopping don't jolt.
        self.prev_bob = self.bob;
        self.bob += speed * dt * std::f32::consts::TAU / 1.5;
        let amp = if moving { 0.012 + 0.008 * (speed / RUN) } else { 0.0 };
        self.bob_amp += (amp - self.bob_amp) * (dt * 3.0).min(1.0);
        if landed && falling < -3.0 {
            self.dip_v = (falling * 0.06).max(-1.4);
        }
        // A damped spring back to level.
        self.dip_v += (-self.dip * 90.0 - self.dip_v * 14.0) * dt;
        self.dip = (self.dip - self.dip_v * dt).clamp(0.0, 0.3);
        let fov = self.base_fov + if input.run && speed > WALK + 0.2 { 4.0 } else { 0.0 };
        self.fov += (fov - self.fov) * (dt * 4.0).min(1.0);
    }

    /// Facing low clutter (a drum, a crate, a railing) with room beyond:
    /// start a vault over it.
    fn try_vault(&mut self, w: &WalkWorld) -> bool {
        let fwd = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let r = RADIUS;
        let probe = |d: f32, y0: f32, y1: f32| {
            let c = self.pos + fwd * d;
            Aabb::new(Vec3::new(c.x - r, self.pos.y + y0, c.z - r), Vec3::new(c.x + r, self.pos.y + y1, c.z + r))
        };
        // Something in the way at knee-to-waist height, nothing above it.
        let Some(top) = [0.45f32, 0.6, 0.75, 0.9, 1.05].into_iter().find(|&h| w.blocked(&probe(0.45, 0.1, h))) else { return false };
        if w.blocked(&probe(0.45, top + 0.15, top + 0.15 + HEIGHT)) {
            return false;
        }
        // Somewhere to land, about level, a stride or so beyond.
        for d in [1.1f32, 1.4, 1.7] {
            let land = self.pos + fwd * d;
            if w.blocked(&Self::aabb_at(land)) || w.blocked(&Self::aabb_at(land + Vec3::Y * (top + 0.15))) {
                continue;
            }
            let below = Self::aabb_at(land - Vec3::Y * 0.3);
            if !w.blocked(&below) {
                continue;
            }
            self.vault = Some((self.pos, land, top + 0.15, 0.0));
            self.vel = Vec3::ZERO;
            return true;
        }
        false
    }

    #[cfg(test)]
    pub fn is_vaulting(&self) -> bool {
        self.vault.is_some()
    }

    fn vaulting(&mut self, dt: f32) {
        let Some((a, b, clear, t)) = self.vault else { return };
        let t = (t + dt / VAULT_TIME).min(1.0);
        let e = t * t * (3.0 - 2.0 * t);
        let mut p = a.lerp(b, e);
        p.y += (t * std::f32::consts::PI).sin() * clear;
        self.pos = p;
        if t >= 1.0 {
            self.pos = b;
            self.vault = None;
            self.on_ground = false;
            // Out of a vault at a walk, never faster: no chaining into speed.
            let fwd = (b - a).normalize_or_zero();
            self.vel = fwd * WALK;
        } else {
            self.vault = Some((a, b, clear, t));
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
            // Up hand over hand; down, slide.
            self.vel.y = if input.forward > 0.0 { CLIMB } else { -SLIDE };
        } else {
            // Ease into a walk and out of it; little control in the air (a
            // hop never gains speed).
            let target = wish * speed;
            let speeding_up = target.length_squared() > self.vel.x * self.vel.x + self.vel.z * self.vel.z;
            let k = if !self.on_ground { 1.5 } else if speeding_up { 7.0 } else { 10.0 };
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
