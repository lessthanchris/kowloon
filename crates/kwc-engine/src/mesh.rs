use glam::Vec3;
use wgpu::util::DeviceExt;

/// Flat-shaded vertex: every face carries its own normal. `emit` > 0 glows
/// (windows, signs, tubes) and feeds bloom later.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub emit: f32,
    /// Sky exposure: 1 = open air, ~0.1 = deep inside a building.
    pub ao: f32,
}

impl Vertex {
    pub const ATTRS: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32, 4 => Float32];
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[derive(Clone)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Sky exposure stamped onto vertices added from now on.
    pub ao: f32,
}

impl Default for MeshData {
    fn default() -> Self {
        MeshData { vertices: vec![], indices: vec![], ao: 1.0 }
    }
}

/// Which faces of a box to emit.
#[derive(Clone, Copy)]
pub struct Faces {
    pub top: bool,
    pub bottom: bool,
    pub px: bool,
    pub nx: bool,
    pub pz: bool,
    pub nz: bool,
}

impl Faces {
    pub const ALL: Faces = Faces { top: true, bottom: true, px: true, nx: true, pz: true, nz: true };
    pub const NO_BOTTOM: Faces = Faces { top: true, bottom: false, px: true, nx: true, pz: true, nz: true };
}

impl MeshData {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Quad from four corners in counter-clockwise order seen from the front.
    pub fn quad(&mut self, p: [Vec3; 4], color: [f32; 3], emit: f32) {
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        let base = self.vertices.len() as u32;
        for v in p {
            self.vertices.push(Vertex { pos: v.into(), normal: n.into(), color, emit, ao: self.ao });
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Axis-aligned box.
    pub fn cuboid(&mut self, min: Vec3, max: Vec3, color: [f32; 3], emit: f32, faces: Faces) {
        let (a, b) = (min, max);
        let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        if faces.top {
            self.quad([v(a.x, b.y, a.z), v(a.x, b.y, b.z), v(b.x, b.y, b.z), v(b.x, b.y, a.z)], color, emit);
        }
        if faces.bottom {
            self.quad([v(a.x, a.y, a.z), v(b.x, a.y, a.z), v(b.x, a.y, b.z), v(a.x, a.y, b.z)], color, emit);
        }
        if faces.px {
            self.quad([v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(b.x, a.y, b.z)], color, emit);
        }
        if faces.nx {
            self.quad([v(a.x, a.y, a.z), v(a.x, a.y, b.z), v(a.x, b.y, b.z), v(a.x, b.y, a.z)], color, emit);
        }
        if faces.pz {
            self.quad([v(a.x, a.y, b.z), v(b.x, a.y, b.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)], color, emit);
        }
        if faces.nz {
            self.quad([v(a.x, a.y, a.z), v(a.x, b.y, a.z), v(b.x, b.y, a.z), v(b.x, a.y, a.z)], color, emit);
        }
    }

    pub fn append(&mut self, other: &MeshData) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

pub struct GpuMesh {
    pub vbuf: wgpu::Buffer,
    pub ibuf: wgpu::Buffer,
    pub count: u32,
}

impl GpuMesh {
    pub fn upload(device: &wgpu::Device, data: &MeshData) -> Option<GpuMesh> {
        if data.indices.is_empty() {
            return None;
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vbuf"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ibuf"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Some(GpuMesh { vbuf, ibuf, count: data.indices.len() as u32 })
    }
}
