use crate::gpu::Gpu;
use crate::mesh::{GpuMesh, Vertex};
use crate::post::{Post, HDR};
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
    /// Tonemap exposure, bloom strength and the HDR level where bloom starts.
    pub exposure: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    /// Strength of the baked lamp spill.
    pub spill: f32,
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

/// A vertex of world-space text (glyph quads that sample the font atlas).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

pub struct TextMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
}

impl TextMesh {
    pub fn upload(device: &wgpu::Device, verts: &[TextVertex], idx: &[u32]) -> Option<TextMesh> {
        use wgpu::util::DeviceExt;
        if idx.is_empty() {
            return None;
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("text v"), contents: bytemuck::cast_slice(verts), usage: wgpu::BufferUsages::VERTEX });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("text i"), contents: bytemuck::cast_slice(idx), usage: wgpu::BufferUsages::INDEX });
        Some(TextMesh { vbuf, ibuf, count: idx.len() as u32 })
    }
}

pub struct Renderer {
    text_pipeline: wgpu::RenderPipeline,
    text_bgl: wgpu::BindGroupLayout,
    text_bind: Option<wgpu::BindGroup>,
    pipeline: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    post: Post,
}

fn targets(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::TextureView, wgpu::TextureView) {
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
    (mk(HDR, "msaa hdr"), mk(DEPTH, "depth"))
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
                targets: &[Some(wgpu::ColorTargetState { format: HDR, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        // Text: world-space glyph quads, depth-tested (not written), alpha-blended.
        let text_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text.wgsl").into()),
        });
        let text_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text layout"),
            bind_group_layouts: &[Some(&bgl), Some(&text_bgl)],
            immediate_size: 0,
        });
        let text_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text"),
            layout: Some(&text_layout),
            vertex: wgpu::VertexState {
                module: &text_module,
                entry_point: Some("vs_text"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &text_attrs,
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &text_module,
                entry_point: Some("fs_text"),
                targets: &[Some(wgpu::ColorTargetState { format: HDR, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let (w, h) = (gpu.config.width, gpu.config.height);
        let (color_msaa, depth) = targets(device, w, h);
        let post = Post::new(device, format, w, h);
        Renderer { text_pipeline, text_bgl, text_bind: None, pipeline, ubuf, bind, color_msaa, depth, size: (w, h), format, post }
    }

    /// The font atlas the text meshes' UVs point into (RGBA8, coverage in alpha).
    pub fn set_text_atlas(&mut self, gpu: &Gpu, width: u32, height: u32, rgba: &[u8]) {
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text atlas"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.text_bind = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text atlas"),
            layout: &self.text_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        }));
    }

    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if (w, h) != self.size && w > 0 && h > 0 {
            let (c, d) = targets(device, w, h);
            self.color_msaa = c;
            self.depth = d;
            self.size = (w, h);
            self.post.resize(device, w, h);
        }
    }

    /// Draw the world (HDR, MSAA), bloom it, and tonemap onto `target`.
    pub fn render(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, meshes: &[&GpuMesh], p: &FrameParams) {
        self.render_with_text(gpu, encoder, target, meshes, &[], p)
    }

    /// As `render`, plus world-space text drawn after the city, hidden by walls.
    pub fn render_with_text(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, meshes: &[&GpuMesh], texts: &[&TextMesh], p: &FrameParams) {
        let u = Uniforms {
            view_proj: p.view_proj.to_cols_array_2d(),
            cam_pos: v4(p.cam_pos, 1.0),
            sun_dir: v4(p.sun_dir.normalize(), 0.0),
            sun_col: v4(p.sun_col, 0.0),
            sky_col: v4(p.sky_col, 0.0),
            gnd_col: v4(p.gnd_col, 0.0),
            fog_col: v4(p.fog_col, 0.0),
            fog: [p.fog_density, p.fog_height_falloff, p.fog_base, p.emissive_gain],
            misc: [p.canyon_depth, p.canyon_strength, p.spill, 0.0],
            torch: v4(p.torch_col, p.torch_range),
        };
        gpu.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let fc = p.fog_col;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_msaa,
                resolve_target: Some(&self.post.scene),
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
        if let Some(tb) = &self.text_bind {
            pass.set_pipeline(&self.text_pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_bind_group(1, tb, &[]);
            for t in texts {
                pass.set_vertex_buffer(0, t.vbuf.slice(..));
                pass.set_index_buffer(t.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..t.count, 0, 0..1);
            }
        }
        drop(pass);
        self.post.run(&gpu.queue, encoder, target, p.bloom_threshold, p.bloom, p.exposure);
    }

    /// Render one frame offscreen and read it back as tightly packed RGBA8.
    pub fn capture(&mut self, gpu: &Gpu, meshes: &[&GpuMesh], p: &FrameParams) -> (u32, u32, Vec<u8>) {
        self.capture_with_text(gpu, meshes, &[], p)
    }

    pub fn capture_with_text(&mut self, gpu: &Gpu, meshes: &[&GpuMesh], texts: &[&TextMesh], p: &FrameParams) -> (u32, u32, Vec<u8>) {
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
        self.render_with_text(gpu, &mut enc, &view, meshes, texts, p);
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
