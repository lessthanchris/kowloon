//! egui overlay drawn on top of the world.

use crate::gpu::Gpu;
use std::sync::Arc;
use winit::window::Window;

pub struct Gui {
    pub ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl Gui {
    pub fn new(gpu: &Gpu, window: &Window) -> Gui {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, window, Some(window.scale_factor() as f32), None, Some(8192));
        let renderer = egui_wgpu::Renderer::new(&gpu.device, gpu.config.format, egui_wgpu::RendererOptions::default());
        Gui { ctx, state, renderer }
    }

    /// Returns true if egui consumed the event.
    pub fn on_event(&mut self, window: &Window, ev: &winit::event::WindowEvent) -> bool {
        self.state.on_window_event(window, ev).consumed
    }

    /// Run the UI closure and draw it onto `target` (loading what's there).
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        window: &Arc<Window>,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        ui: impl FnMut(&mut egui::Ui),
    ) -> Vec<wgpu::CommandBuffer> {
        let input = self.state.take_egui_input(window);
        let out = self.ctx.run_ui(input, ui);
        self.state.handle_platform_output(window, out.platform_output);
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [gpu.config.width, gpu.config.height], pixels_per_point: out.pixels_per_point };
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let cmds = self.renderer.update_buffers(&gpu.device, &gpu.queue, encoder, &prims, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), &prims, &screen);
        }
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        cmds
    }

    pub fn wants_pointer(&self) -> bool {
        self.ctx.egui_wants_pointer_input()
    }
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }
}
