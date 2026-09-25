use crate::gpu::Gpu;
use crate::mesh::{GpuMesh, Vertex};
use glam::{Mat4, Vec3};

pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MSAA: u32 = 4;

/// Per-frame lighting / camera state.
#[derive(Clone, Copy, Debug)]
pub struct FrameParams {
    pub view_proj: Mat4,
    pub cam_pos: Vec3,
    pub sun_dir: Vec3,
    pub sun_col: Vec3,
    pub sky_col: Vec3,
    pub gnd_col: Vec3,
    pub fog_col: Vec3,
    pub fog_density: f32,
    pub fog_height_falloff: f32,
    pub fog_base: f32,
    pub emissive_gain: f32,
    pub canyon_depth: f32,
    pub canyon_strength: f32,
    /// Head torch colour (pre-multiplied intensity) and range in metres; range 0 = off.
    pub torch_col: Vec3,
    pub torch_range: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_col: [f32; 4],
    sky_col: [f32; 4],
    gnd_col: [f32; 4],
    fog_col: [f32; 4],
    fog: [f32; 4],
    misc: [f32; 4],
    torch: [f32; 4],
}

fn v4(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
}

fn targets(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> (wgpu::TextureView, wgpu::TextureView) {
    let mk = |fmt, label| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: MSAA,
                dimension: wgpu::TextureDimension::D2,
                format: fmt,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    (mk(format, "msaa color"), mk(DEPTH, "depth"))
}

impl Renderer {
    pub fn new(gpu: &Gpu) -> Renderer {
        let device = &gpu.device;
        let format = gpu.config.format;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::layout())],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let (w, h) = (gpu.config.width, gpu.config.height);
        let (color_msaa, depth) = targets(device, format, w, h);
        Renderer { pipeline, ubuf, bind, color_msaa, depth, size: (w, h), format }
    }

    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if (w, h) != self.size && w > 0 && h > 0 {
            let (c, d) = targets(device, self.format, w, h);
            self.color_msaa = c;
            self.depth = d;
            self.size = (w, h);
        }
    }

    /// Draw the world into `target` (resolved from MSAA), clearing to the fog colour.
    pub fn render(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, meshes: &[&GpuMesh], p: &FrameParams) {
        let u = Uniforms {
            view_proj: p.view_proj.to_cols_array_2d(),
            cam_pos: v4(p.cam_pos, 1.0),
            sun_dir: v4(p.sun_dir.normalize(), 0.0),
            sun_col: v4(p.sun_col, 0.0),
            sky_col: v4(p.sky_col, 0.0),
            gnd_col: v4(p.gnd_col, 0.0),
            fog_col: v4(p.fog_col, 0.0),
            fog: [p.fog_density, p.fog_height_falloff, p.fog_base, p.emissive_gain],
            misc: [p.canyon_depth, p.canyon_strength, 0.0, 0.0],
            torch: v4(p.torch_col, p.torch_range),
        };
        gpu.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let fc = p.fog_col;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_msaa,
                resolve_target: Some(target),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: fc.x as f64, g: fc.y as f64, b: fc.z as f64, a: 1.0 }),
                    store: wgpu::StoreOp::Discard,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        for m in meshes {
            pass.set_vertex_buffer(0, m.vbuf.slice(..));
            pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..m.count, 0, 0..1);
        }
    }

    /// Render one frame offscreen and read it back as tightly packed RGBA8.
    pub fn capture(&mut self, gpu: &Gpu, meshes: &[&GpuMesh], p: &FrameParams) -> (u32, u32, Vec<u8>) {
        let (w, h) = (gpu.config.width, gpu.config.height);
        self.resize(&gpu.device, w, h);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
        self.render(gpu, &mut enc, &view, meshes, p);
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.map_async(wgpu::MapMode::Read, .., |r| r.expect("map readback"));
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buf.get_mapped_range(..).expect("mapped range");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row) as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        (w, h, out)
    }
}
