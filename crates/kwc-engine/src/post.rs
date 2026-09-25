//! HDR post-processing: bloom (bright-pass + mip chain) and the tonemapped
//! composite onto the output target.

pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const LEVELS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
    texel: [f32; 2],
    threshold: f32,
    knee: f32,
    bloom: f32,
    exposure: f32,
    _pad: [f32; 2],
}

struct Pass {
    bind: wgpu::BindGroup,
    ubuf: wgpu::Buffer,
    texel: [f32; 2],
}

pub struct Post {
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bright: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    dummy: wgpu::TextureView,
    /// Resolved HDR scene; the world pass resolves into this.
    pub scene: wgpu::TextureView,
    levels: Vec<wgpu::TextureView>,
    passes: Vec<Pass>, // bright, down 1.., up .., composite
}

fn hdr_tex(device: &wgpu::Device, w: u32, h: u32, label: &str) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Post {
    pub fn new(device: &wgpu::Device, out_format: wgpu::TextureFormat, w: u32, h: u32) -> Post {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                tex_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex_entry(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipe = |entry: &str, format: wgpu::TextureFormat, additive: bool| {
            let blend = additive.then_some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut post = Post {
            bright: pipe("fs_bright", HDR, false),
            down: pipe("fs_down", HDR, false),
            up: pipe("fs_up", HDR, true),
            composite: pipe("fs_composite", out_format, false),
            dummy: hdr_tex(device, 1, 1, "post dummy"),
            scene: hdr_tex(device, 1, 1, "scene"),
            levels: vec![],
            passes: vec![],
            bgl,
            sampler,
        };
        post.resize(device, w, h);
        post
    }

    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        self.scene = hdr_tex(device, w, h, "scene");
        self.levels = (0..LEVELS).map(|i| hdr_tex(device, (w >> (i + 1)).max(1), (h >> (i + 1)).max(1), "bloom")).collect();
        let size = |i: usize| ((w >> (i + 1)).max(1) as f32, (h >> (i + 1)).max(1) as f32);
        let mk = |src: &wgpu::TextureView, bloom: &wgpu::TextureView, texel: [f32; 2]| {
            let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post uniform"),
                size: std::mem::size_of::<PostUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(bloom) },
                ],
            });
            Pass { bind, ubuf, texel }
        };
        let mut passes = vec![mk(&self.scene, &self.dummy, [1.0 / w as f32, 1.0 / h as f32])];
        for i in 1..LEVELS {
            let (sw, sh) = size(i - 1);
            passes.push(mk(&self.levels[i - 1], &self.dummy, [1.0 / sw, 1.0 / sh]));
        }
        for i in (1..LEVELS).rev() {
            let (sw, sh) = size(i);
            passes.push(mk(&self.levels[i], &self.dummy, [1.0 / sw, 1.0 / sh]));
        }
        passes.push(mk(&self.scene, &self.levels[0], [1.0 / w as f32, 1.0 / h as f32]));
        self.passes = passes;
    }

    fn draw(encoder: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, pass: &Pass, target: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, &pass.bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Bloom the resolved scene and tonemap it onto `target`.
    pub fn run(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, threshold: f32, bloom: f32, exposure: f32) {
        for p in &self.passes {
            let u = PostUniform { texel: p.texel, threshold, knee: threshold * 0.5, bloom, exposure, _pad: [0.0; 2] };
            queue.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&u));
        }
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        let mut k = 0;
        Self::draw(encoder, &self.bright, &self.passes[k], &self.levels[0], clear);
        k += 1;
        for i in 1..LEVELS {
            Self::draw(encoder, &self.down, &self.passes[k], &self.levels[i], clear);
            k += 1;
        }
        for i in (1..LEVELS).rev() {
            Self::draw(encoder, &self.up, &self.passes[k], &self.levels[i - 1], wgpu::LoadOp::Load);
            k += 1;
        }
        Self::draw(encoder, &self.composite, &self.passes[k], target, clear);
    }
}
