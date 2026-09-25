//! Kowloon Walled City — grow it, map it, walk it.
//!
//!   kowloon                       open the window
//!   kowloon --shot out.png [--year 1987] [--seed 1987] [--night] [--view yaw,pitch,dist]
//!                                 render one frame headless and save it

mod citymesh;
mod lighting;

use citymesh::ColourMode;
use kwc_engine::winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};
use kwc_engine::{egui, GpuMesh, Gpu, Gui, OrbitCamera, Renderer};
use kwc_sim::*;
use std::sync::Arc;
use std::time::Instant;

struct Settings {
    seed: u64,
    year: f32,
    playing: bool,
    speed: f32,
    mode: ColourMode,
    night: bool,
}

struct World {
    city: City,
    mesh: Option<GpuMesh>,
    built_for: Option<(u16, ColourMode, u64)>,
}

impl World {
    fn new(seed: u64) -> World {
        World { city: generate(&Params { seed, ..Default::default() }), mesh: None, built_for: None }
    }
    fn ensure_mesh(&mut self, gpu: &Gpu, year: u16, mode: ColourMode) {
        let key = (year, mode, self.city.params.seed);
        if self.built_for == Some(key) {
            return;
        }
        let t = Instant::now();
        let data = citymesh::build(&self.city, year, mode);
        self.mesh = GpuMesh::upload(&gpu.device, &data);
        log::info!("mesh {year}: {} verts in {:.0?}", data.vertices.len(), t.elapsed());
        self.built_for = Some(key);
    }
}

fn default_orbit(city: &City) -> OrbitCamera {
    OrbitCamera { target: citymesh::centre(city) + glam::Vec3::Y * 12.0, yaw: 2.3, pitch: 0.42, dist: 330.0 }
}

struct Running {
    window: Arc<Window>,
    gpu: Gpu,
    renderer: Renderer,
    gui: Gui,
}

struct App {
    run: Option<Running>,
    world: World,
    settings: Settings,
    orbit: OrbitCamera,
    drag: Option<MouseButton>,
    last_cursor: Option<(f64, f64)>,
    last_frame: Instant,
    fps: f32,
    frames: u64,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.run.is_some() {
            return;
        }
        let window = Arc::new(
            el.create_window(Window::default_attributes().with_title("Kowloon Walled City").with_inner_size(LogicalSize::new(1600.0, 900.0)))
                .expect("window"),
        );
        let gpu = Gpu::for_window(window.clone());
        let renderer = Renderer::new(&gpu);
        let gui = Gui::new(&gpu, &window);
        self.run = Some(Running { window, gpu, renderer, gui });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(run) = self.run.as_mut() else { return };
        let consumed = run.gui.on_event(&run.window, &event);
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                run.gpu.resize(size.width, size.height);
                run.renderer.resize(&run.gpu.device, size.width, size.height);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && !run.gui.wants_pointer() {
                    self.drag = Some(button);
                } else if state == ElementState::Released {
                    self.drag = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x, position.y);
                if let (Some(b), Some(l)) = (self.drag, self.last_cursor) {
                    let (dx, dy) = ((p.0 - l.0) as f32, (p.1 - l.1) as f32);
                    match b {
                        MouseButton::Left => self.orbit.rotate(dx, dy),
                        _ => self.orbit.pan(dx, dy),
                    }
                }
                self.last_cursor = Some(p);
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed && !run.gui.wants_pointer() => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                self.orbit.zoom(steps);
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(run) = &self.run {
            run.window.request_redraw();
        }
    }
}

impl App {
    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.fps = self.fps * 0.95 + (1.0 / dt.max(1e-4)) * 0.05;
        self.frames += 1;
        if self.frames % 120 == 0 {
            log::debug!("frame {} fps {:.0} year {:.1}", self.frames, self.fps, self.settings.year);
        }

        let s = &mut self.settings;
        if s.playing {
            s.year += dt * s.speed;
            if s.year >= END_YEAR as f32 {
                s.year = END_YEAR as f32;
                s.playing = false;
            }
        }
        if self.world.city.params.seed != s.seed {
            self.world = World::new(s.seed);
        }
        let year = s.year.floor() as u16;
        let run = self.run.as_mut().unwrap();
        self.world.ensure_mesh(&run.gpu, year, s.mode);

        let frame = match run.gpu.surface.as_ref().unwrap().get_current_texture() {
            kwc_engine::wgpu::CurrentSurfaceTexture::Success(f) | kwc_engine::wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            kwc_engine::wgpu::CurrentSurfaceTexture::Outdated | kwc_engine::wgpu::CurrentSurfaceTexture::Lost => {
                run.gpu.reconfigure();
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = run.gpu.device.create_command_encoder(&Default::default());
        let cam = self.orbit.camera();
        let params = lighting::params(&cam, run.gpu.aspect(), s.night);
        let meshes: Vec<&GpuMesh> = self.world.mesh.iter().collect();
        run.renderer.render(&run.gpu, &mut enc, &view, &meshes, &params);

        let stats = stats::measure(&self.world.city, year);
        let fps = self.fps;
        let cmds = run.gui.draw(&run.gpu, &run.window, &mut enc, &view, |ui| panel(ui, s, &stats, fps));
        run.gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        run.window.pre_present_notify();
        run.gpu.queue.present(frame);
    }
}

fn panel(ui: &mut egui::Ui, s: &mut Settings, st: &stats::Stats, fps: f32) {
    egui::Window::new("Kowloon Walled City").default_pos([16.0, 16.0]).resizable(false).show(ui.ctx(), |ui| {
        ui.horizontal(|ui| {
            if ui.button(if s.playing { "⏸ Pause" } else { "▶ Play" }).clicked() {
                if !s.playing && s.year >= END_YEAR as f32 {
                    s.year = START_YEAR as f32;
                }
                s.playing = !s.playing;
            }
            // Slide a whole-year copy: a stepped slider would snap playback's fractional year back.
            let mut y = s.year.floor() as u16;
            if ui.add(egui::Slider::new(&mut y, START_YEAR..=END_YEAR).text("year")).changed() {
                s.year = y as f32;
            }
        });
        ui.add(egui::Slider::new(&mut s.speed, 0.5..=8.0).text("years / sec"));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Colour");
            ui.radio_value(&mut s.mode, ColourMode::Grime, "grime");
            ui.radio_value(&mut s.mode, ColourMode::Decade, "decade built");
        });
        ui.checkbox(&mut s.night, "Night");
        ui.horizontal(|ui| {
            ui.label("Seed");
            let mut seed = s.seed;
            ui.add(egui::DragValue::new(&mut seed));
            if ui.button("New city").clicked() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) % 100_000;
            }
            s.seed = seed;
        });
        ui.separator();
        egui::Grid::new("stats").num_columns(2).show(ui, |ui| {
            ui.label("Buildings standing");
            ui.label(format!("{} / {}", st.settled, st.buildings));
            ui.end_row();
            ui.label("Premises");
            ui.label(format!("{}", st.units));
            ui.end_row();
            ui.label("Residents (est.)");
            ui.label(format!("{}", st.residents));
            ui.end_row();
            ui.label("Mean storeys");
            ui.label(format!("{:.1}", st.mean_height));
            ui.end_row();
            ui.label("At 14-storey cap");
            ui.label(format!("{}", st.at_cap));
            ui.end_row();
        });
        ui.separator();
        ui.small("Left-drag: orbit · Right-drag: pan · Wheel: zoom");
        ui.small(format!("{fps:.0} fps"));
    });
}

fn arg<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn screenshot(args: &[String], out: &str) {
    let year: u16 = arg(args, "--year").unwrap_or(END_YEAR);
    let seed: u64 = arg(args, "--seed").unwrap_or(1987);
    let night = args.iter().any(|a| a == "--night");
    let mode = if args.iter().any(|a| a == "--decade") { ColourMode::Decade } else { ColourMode::Grime };
    let gpu = Gpu::headless(1600, 900);
    let mut renderer = Renderer::new(&gpu);
    let mut world = World::new(seed);
    world.ensure_mesh(&gpu, year, mode);
    let mut orbit = default_orbit(&world.city);
    if let Some(v) = arg::<String>(args, "--view") {
        let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if p.len() == 3 {
            (orbit.yaw, orbit.pitch, orbit.dist) = (p[0], p[1], p[2]);
        }
    }
    let params = lighting::params(&orbit.camera(), gpu.aspect(), night);
    let meshes: Vec<&GpuMesh> = world.mesh.iter().collect();
    let (w, h, px) = renderer.capture(&gpu, &meshes, &params);
    image::RgbaImage::from_raw(w, h, px).unwrap().save(out).expect("save png");
    println!("wrote {out}");
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn")).init();
    let args: Vec<String> = std::env::args().collect();
    if let Some(out) = arg::<String>(&args, "--shot") {
        screenshot(&args, &out);
        return;
    }
    let world = World::new(1987);
    let orbit = default_orbit(&world.city);
    let mut app = App {
        run: None,
        world,
        settings: Settings { seed: 1987, year: START_YEAR as f32, playing: true, speed: 2.0, mode: ColourMode::Grime, night: false },
        orbit,
        drag: None,
        last_cursor: None,
        last_frame: Instant::now(),
        fps: 60.0,
        frames: 0,
    };
    let el = EventLoop::new().expect("event loop");
    el.run_app(&mut app).expect("run");
}
