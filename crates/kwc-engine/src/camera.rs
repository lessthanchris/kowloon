use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye, self.target, Vec3::Y);
        let proj = Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far);
        proj * view
    }
}

/// Orbit around a target: left-drag rotates, right-drag pans, wheel zooms.
#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl OrbitCamera {
    pub fn camera(&self) -> Camera {
        let dir = Vec3::new(self.yaw.cos() * self.pitch.cos(), self.pitch.sin(), self.yaw.sin() * self.pitch.cos());
        Camera { eye: self.target + dir * self.dist, target: self.target, fov_y: 45f32.to_radians(), near: 0.5, far: 6000.0 }
    }

    pub fn rotate(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.006;
        self.pitch = (self.pitch + dy * 0.006).clamp(0.05, 1.5);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        let fwd = Vec3::new(-self.yaw.cos(), 0.0, -self.yaw.sin());
        let right = fwd.cross(Vec3::Y).normalize();
        let k = self.dist * 0.0015;
        self.target += right * (-dx * k) + fwd * (dy * k);
    }

    pub fn zoom(&mut self, steps: f32) {
        self.dist = (self.dist * (1.0 - steps * 0.1)).clamp(15.0, 1200.0);
    }
}
