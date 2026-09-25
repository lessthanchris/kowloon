//! kwc-engine: a small purpose-built engine for the Walled City.
//!
//! winit window + wgpu renderer (flat-shaded low-poly, fog, emissive), cameras,
//! a mesh builder, and an egui overlay. Knows nothing about the city itself.

pub mod camera;
pub mod gpu;
pub mod gui;
pub mod mesh;
pub mod renderer;

pub use camera::{Camera, OrbitCamera};
pub use gpu::Gpu;
pub use gui::Gui;
pub use mesh::{GpuMesh, MeshData, Vertex};
pub use renderer::{FrameParams, Renderer};

pub use egui;
pub use glam;
pub use wgpu;
pub use winit;
