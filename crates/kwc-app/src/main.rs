//! Kowloon Walled City — grow it, map it, walk it.
//!
//!   kowloon                       open the window
//!   kowloon --shot out.png [--year 1987] [--seed 1987] [--night] [--decade] [--view yaw,pitch,dist]
//!   kowloon --shot out.png --walk [--lane N] [--at x,y,z,yaw,pitch] [--torch]
//!                                 render one frame headless and save it

mod citymesh;
mod lighting;
mod player;
mod world;

use citymesh::ColourMode;
use glam::Vec3;
use kwc_engine::winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};
use kwc_engine::{egui, Camera, FrameParams, GpuMesh, Gpu, Gui, OrbitCamera, Renderer};
use kwc_sim::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

struct Settings {
    seed: u64,
    year: f32,
    playing: bool,
    speed: f32,
    mode: ColourMode,
    night: bool,
    torch: bool,
    want_walk: bool,
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

/// Everything needed to walk around one year's city.
struct Walk {
    world: world::WalkWorld,
    interior: Option<GpuMesh>,
    player: player::Player,
    year: u16,
}

impl Walk {
    fn new(gpu: &Gpu, city: &City, year: u16) -> Walk {
        let t = Instant::now();
        let (world, mesh) = world::build(city, year);
        log::info!("walk world {year}: {} boxes, {} verts in {:.0?}", world.boxes.len(), mesh.vertices.len(), t.elapsed());
        let player = player::Player::new(world.spawn, world.spawn_yaw);
        Walk { interior: GpuMesh::upload(&gpu.device, &mesh), world, player, year }
    }
}

fn default_orbit(city: &City) -> OrbitCamera {
    OrbitCamera { target: citymesh::centre(city) + Vec3::Y * 12.0, yaw: 2.3, pitch: 0.42, dist: 330.0 }
}

fn walk_params(cam: &Camera, aspect: f32, night: bool, torch: bool) -> FrameParams {
    let mut p = lighting::params(cam, aspect, night);
    // Up close the lanes are darker and the air thicker.
    p.fog_density *= 2.5;
    p.canyon_depth = 30.0;
    p.canyon_strength = 0.8;
    if torch {
        p.torch_col = Vec3::new(0.75, 0.7, 0.6);
        p.torch_range = 8.0;
    }
    p
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
    walk: Option<Walk>,
    keys: HashSet<KeyCode>,
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

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, Some(w)) = (event, self.walk.as_mut()) {
            w.player.look(delta.0 as f32, delta.1 as f32);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(run) = self.run.as_mut() else { return };
        let walking = self.walk.is_some();
        let consumed = !walking && run.gui.on_event(&run.window, &event);
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                run.gpu.resize(size.width, size.height);
                run.renderer.resize(&run.gpu.device, size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if event.state == ElementState::Pressed {
                    if !event.repeat {
                        match code {
                            KeyCode::Tab => {
                                if walking {
                                    self.leave_walk();
                                } else {
                                    self.settings.want_walk = true;
                                }
                            }
                            KeyCode::Escape if walking => self.leave_walk(),
                            KeyCode::KeyT => self.settings.torch = !self.settings.torch,
                            KeyCode::KeyN => self.settings.night = !self.settings.night,
                            _ => {}
                        }
                    }
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
            }
            WindowEvent::MouseInput { state, button, .. } if !walking => {
                if state == ElementState::Pressed && !run.gui.wants_pointer() {
                    self.drag = Some(button);
                } else if state == ElementState::Released {
                    self.drag = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x, position.y);
                if let (false, Some(b), Some(l)) = (walking, self.drag, self.last_cursor) {
                    let (dx, dy) = ((p.0 - l.0) as f32, (p.1 - l.1) as f32);
                    match b {
                        MouseButton::Left => self.orbit.rotate(dx, dy),
                        _ => self.orbit.pan(dx, dy),
                    }
                }
                self.last_cursor = Some(p);
            }
            WindowEvent::MouseWheel { delta, .. } if !walking && !consumed && !run.gui.wants_pointer() => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                self.orbit.zoom(steps);
            }
            WindowEvent::Focused(false) => self.keys.clear(),
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
    fn enter_walk(&mut self) {
        let run = self.run.as_ref().unwrap();
        let year = self.settings.year.floor() as u16;
        self.settings.playing = false;
        self.walk = Some(Walk::new(&run.gpu, &self.world.city, year));
        let w = &run.window;
        let _ = w.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
        w.set_cursor_visible(false);
    }

    fn leave_walk(&mut self) {
        self.walk = None;
        self.keys.clear();
        if let Some(run) = &self.run {
            let _ = run.window.set_cursor_grab(CursorGrabMode::None);
            run.window.set_cursor_visible(true);
        }
    }

    fn input(&self) -> player::Input {
        let k = |c| self.keys.contains(&c);
        let axis = |a, b| (k(a) as i32 - k(b) as i32) as f32;
        player::Input {
            forward: axis(KeyCode::KeyW, KeyCode::KeyS),
            strafe: axis(KeyCode::KeyD, KeyCode::KeyA),
            run: k(KeyCode::ShiftLeft) || k(KeyCode::ShiftRight),
            jump: k(KeyCode::Space),
        }
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.fps = self.fps * 0.95 + (1.0 / dt.max(1e-4)) * 0.05;
        self.frames += 1;
        if self.frames % 240 == 0 {
            log::debug!("frame {} fps {:.0}", self.frames, self.fps);
        }

        if std::mem::take(&mut self.settings.want_walk) {
            self.enter_walk();
        }
        let input = self.input();
        if let Some(w) = self.walk.as_mut() {
            w.player.update(&w.world, input, dt);
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
        let year = self.walk.as_ref().map_or(s.year.floor() as u16, |w| w.year);
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
        let mut meshes: Vec<&GpuMesh> = self.world.mesh.iter().collect();
        let params = match &self.walk {
            Some(w) => {
                meshes.extend(w.interior.iter());
                walk_params(&w.player.camera(), run.gpu.aspect(), s.night, s.torch)
            }
            None => lighting::params(&self.orbit.camera(), run.gpu.aspect(), s.night),
        };
        run.renderer.render(&run.gpu, &mut enc, &view, &meshes, &params);

        let stats = stats::measure(&self.world.city, year);
        let fps = self.fps;
        let walk = self.walk.as_ref();
        let cmds = run.gui.draw(&run.gpu, &run.window, &mut enc, &view, |ui| match walk {
            Some(w) => hud(ui, w, s, fps),
            None => panel(ui, s, &stats, fps),
        });
        run.gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        run.window.pre_present_notify();
        run.gpu.queue.present(frame);
    }
}

fn hud(ui: &mut egui::Ui, w: &Walk, s: &Settings, fps: f32) {
    let p = &w.player;
    let floor = (p.pos.y / world::S + 0.25).floor() as i32;
    let place = if p.pos.y < 0.5 { "street level".to_string() } else { format!("floor {floor}") };
    egui::Area::new(egui::Id::new("hud")).fixed_pos([16.0, 16.0]).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(egui::Color32::from_black_alpha(150)).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
            ui.colored_label(egui::Color32::from_rgb(230, 225, 210), format!("{} · {}", w.year, place));
            ui.small(format!(
                "WASD walk · Shift run · Space jump · W on a ladder to climb · T torch ({}) · N night · Tab/Esc overview · {fps:.0} fps",
                if s.torch { "on" } else { "off" }
            ));
        });
    });
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
        if ui.button("🚶 Walk the city (Tab)").clicked() {
            s.want_walk = true;
        }
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

fn floats(s: &str) -> Vec<f32> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
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
    let walk;
    let mut meshes: Vec<&GpuMesh> = world.mesh.iter().collect();
    let params = if args.iter().any(|a| a == "--walk") {
        walk = Walk::new(&gpu, &world.city, year);
        let mut p = player::Player::new(walk.world.spawn, walk.world.spawn_yaw);
        if let Some(n) = arg::<usize>(args, "--lane") {
            let lane = world.city.lanes.get(n);
            println!("lane: {}", lane.map_or("?", |l| l.name.as_str()));
            if let Some((pos, yaw)) = world::lane_view(&world.city, lane, lane.map_or(0, |l| l.cells.len() / 2)) {
                (p.pos, p.yaw) = (pos, yaw);
            }
        }
        if let Some(n) = arg::<usize>(args, "--plot") {
            // Stand on that building's landing at --floor F (or on its roof with --floor 99),
            // looking away from the stair.
            let plot = &world.city.plots[n.min(world.city.plots.len() - 1)];
            let h = plot.height_at(year) as f32;
            let f = arg::<f32>(args, "--floor").unwrap_or(1.0).min(h);
            let l = plot.core[0];
            let d = world::landing_dir(plot).delta();
            // `d` points stair -> landing: back up to the far side of the landing, face the stair.
            let back = Vec3::new(d.0 as f32, 0.0, d.1 as f32) * 0.45;
            p.pos = Vec3::new((l.0 as f32 + 0.5) * CELL_M, f * STOREY_M, (l.1 as f32 + 0.5) * CELL_M) + back;
            p.yaw = (-d.1 as f32).atan2(-d.0 as f32);
            p.pitch = arg::<f32>(args, "--pitch").unwrap_or(0.0);
            println!("plot {} height {h} floor {f}", plot.id);
        }
        if let Some(v) = arg::<String>(args, "--at").map(|v| floats(&v)) {
            if v.len() >= 3 {
                p.pos = Vec3::new(v[0], v[1], v[2]);
            }
            if v.len() >= 5 {
                (p.yaw, p.pitch) = (v[3], v[4]);
            }
        }
        meshes.extend(walk.interior.iter());
        walk_params(&p.camera(), gpu.aspect(), night, args.iter().any(|a| a == "--torch"))
    } else {
        let mut orbit = default_orbit(&world.city);
        if let Some(v) = arg::<String>(args, "--view").map(|v| floats(&v)) {
            if v.len() == 3 {
                (orbit.yaw, orbit.pitch, orbit.dist) = (v[0], v[1], v[2]);
            }
        }
        lighting::params(&orbit.camera(), gpu.aspect(), night)
    };
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
    // `kowloon --walk` starts on foot in 1987.
    let walk_now = args.iter().any(|a| a == "--walk");
    let mut app = App {
        run: None,
        world,
        settings: Settings {
            seed: 1987,
            year: if walk_now { END_YEAR as f32 } else { START_YEAR as f32 },
            playing: !walk_now,
            speed: 2.0,
            mode: ColourMode::Grime,
            night: false,
            torch: true,
            want_walk: walk_now,
        },
        orbit,
        walk: None,
        keys: HashSet::new(),
        drag: None,
        last_cursor: None,
        last_frame: Instant::now(),
        fps: 60.0,
        frames: 0,
    };
    let el = EventLoop::new().expect("event loop");
    el.run_app(&mut app).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steer the player through waypoints; returns false if it gets stuck.
    fn walk_to(p: &mut player::Player, w: &world::WalkWorld, pts: &[(f32, f32)]) -> bool {
        for &(x, z) in pts {
            let mut t = 0.0;
            while (Vec3::new(x, 0.0, z) - Vec3::new(p.pos.x, 0.0, p.pos.z)).length() > 0.12 {
                p.yaw = (z - p.pos.z).atan2(x - p.pos.x);
                p.update(w, player::Input { forward: 1.0, ..Default::default() }, 1.0 / 60.0);
                t += 1.0 / 60.0;
                if t > 8.0 {
                    return false;
                }
            }
        }
        true
    }

    fn settle(p: &mut player::Player, w: &world::WalkWorld) {
        for _ in 0..60 {
            p.update(w, player::Input::default(), 1.0 / 60.0);
        }
    }

    /// Up every flight of a tall building, then out of the stair hut onto the roof.
    #[test]
    fn climb_stairs_to_the_roof() {
        let city = generate(&Params::default());
        let (w, _) = world::build(&city, END_YEAR);
        let plot = city.plots.iter().find(|p| p.final_height() >= 12).unwrap();
        let h = plot.final_height() as i32;
        let (landing, stair) = (plot.core[0], plot.core[1]);
        let d = world::landing_dir(plot);
        let lc = ((landing.0 as f32 + 0.5) * CELL_M, (landing.1 as f32 + 0.5) * CELL_M);
        let mut p = player::Player::new(Vec3::new(lc.0, 0.0, lc.1), 0.0);
        settle(&mut p, &w);
        assert!(p.pos.y.abs() < 0.05, "should stand on the ground-floor landing, y = {}", p.pos.y);
        for floor in 1..=h {
            let pt = |u: f32, v: f32| world::stair_point(stair, d, u, v);
            let route = [pt(0.05, 0.4), pt(1.15, 0.4), pt(1.15, 1.1), pt(0.05, 1.1), lc];
            assert!(walk_to(&mut p, &w, &route), "stuck on the stair to floor {floor} at {:?}", p.pos);
            assert!((p.pos.y - floor as f32 * STOREY_M).abs() < 0.1, "floor {floor}: y = {}", p.pos.y);
        }
        settle(&mut p, &w);
        assert!((p.pos.y - h as f32 * STOREY_M).abs() < 0.1, "on the roof: y = {}", p.pos.y);
    }

    /// Ladders get you up onto a taller neighbour's roof.
    #[test]
    fn climb_a_ladder() {
        let city = generate(&Params::default());
        let (w, _) = world::build(&city, END_YEAR);
        let mut ok = 0;
        for l in w.ladders.iter().filter(|l| l.top - l.volume.min.y > 2.0).take(10) {
            let c = (l.volume.min + l.volume.max) * 0.5;
            let mut p = player::Player::new(Vec3::new(c.x, l.volume.min.y, c.z) - l.facing * 0.3, l.facing.z.atan2(l.facing.x));
            settle(&mut p, &w);
            for _ in 0..(60.0 * 30.0) as i32 {
                p.update(&w, player::Input { forward: 1.0, ..Default::default() }, 1.0 / 60.0);
                if p.pos.y >= l.top - 0.05 && p.on_ground {
                    break;
                }
            }
            if (p.pos.y - l.top).abs() < 0.15 {
                ok += 1;
            } else {
                eprintln!("ladder at {:?} to {}: ended at {:?}", l.volume.min, l.top, p.pos);
            }
        }
        assert!(ok >= 8, "only {ok}/10 ladders climbed");
    }
}
