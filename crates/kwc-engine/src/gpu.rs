use std::sync::Arc;
use winit::window::Window;

/// Device, queue and (optionally) a window surface.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    })
}

async fn device(adapter: &wgpu::Adapter) -> (wgpu::Device, wgpu::Queue) {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("kwc device"),
            // Big city meshes: take the adapter's real buffer limit, not the portable default.
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: 8192,
                max_buffer_size: adapter.limits().max_buffer_size,
                ..wgpu::Limits::default()
            },
            ..Default::default()
        })
        .await
        .expect("no suitable GPU device")
}

impl Gpu {
    /// For a window: picks an sRGB surface format so shader maths stays linear.
    pub fn for_window(window: Arc<Window>) -> Gpu {
        pollster::block_on(async {
            let instance = instance();
            let surface = instance.create_surface(window.clone()).expect("create surface");
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                })
                .await
                .expect("no GPU adapter");
            let (device, queue) = device(&adapter).await;
            let size = window.inner_size();
            let mut config = surface
                .get_default_config(&adapter, size.width.max(1), size.height.max(1))
                .expect("surface unsupported by adapter");
            let caps = surface.get_capabilities(&adapter);
            if let Some(f) = caps.formats.iter().find(|f| f.is_srgb()) {
                config.format = *f;
            }
            config.present_mode = wgpu::PresentMode::AutoVsync;
            surface.configure(&device, &config);
            log::info!("GPU: {:?}, surface {:?}", adapter.get_info().name, config.format);
            Gpu { instance, adapter, device, queue, surface: Some(surface), config }
        })
    }

    /// Headless (screenshots / tests): no surface, a fixed sRGB target format.
    pub fn headless(width: u32, height: u32) -> Gpu {
        pollster::block_on(async {
            let instance = instance();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                })
                .await
                .expect("no GPU adapter");
            let (device, queue) = device(&adapter).await;
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                width,
                height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                color_space: Default::default(),
            };
            Gpu { instance, adapter, device, queue, surface: None, config }
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }

    /// Vsync on: frames wait for the display. Off: as fast as the GPU goes.
    pub fn set_vsync(&mut self, on: bool) {
        self.config.present_mode = if on { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
        self.reconfigure();
    }

    pub fn vsync(&self) -> bool {
        self.config.present_mode == wgpu::PresentMode::AutoVsync
    }

    pub fn reconfigure(&self) {
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }
}
