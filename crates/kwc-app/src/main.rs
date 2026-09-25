//! Kowloon Walled City — grow it, map it, walk it.
//!
//!   kowloon                       open the window
//!   kowloon --shot out.png [--year 1987] [--seed 1987] [--night] [--decade] [--view yaw,pitch,dist]
//!   kowloon --shot out.png --walk [--lane N] [--at x,y,z,yaw,pitch] [--torch]
//!                                 render one frame headless and save it

mod citymesh;
mod game;
mod lighting;
mod lights;
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
    /// The how-to card (H).
    help: bool,
}

struct World {
    city: City,
    society: society::Society,
    mesh: Option<GpuMesh>,
    built_for: Option<(u16, ColourMode, u64)>,
}

impl World {
    fn new(seed: u64) -> World {
        let city = generate(&Params { seed, ..Default::default() });
        let society = society::generate(&city);
        World { city, society, mesh: None, built_for: None }
    }
    fn ensure_mesh(&mut self, gpu: &Gpu, year: u16, mode: ColourMode) {
        let key = (year, mode, self.city.params.seed);
        if self.built_for == Some(key) {
            return;
        }
        let t = Instant::now();
        let mut data = citymesh::build(&self.city, year, mode);
        let lamps = lights::collect(&self.city, year);
        lights::bake(&self.city, year, &lamps, &mut [&mut data]);
        self.mesh = GpuMesh::upload(&gpu.device, &data);
        log::info!("mesh {year}: {} verts in {:.0?}", data.vertices.len(), t.elapsed());
        self.built_for = Some(key);
    }
}

/// Everything needed to walk around one year's city.
struct Walk {
    world: world::WalkWorld,
    interior: Vec<GpuMesh>,
    player: player::Player,
    year: u16,
    spots: Vec<game::Spot>,
}

impl Walk {
    fn new(gpu: &Gpu, city: &City, year: u16) -> Walk {
        let t = Instant::now();
        let (world, mut mesh) = world::build(city, year);
        let lamps = lights::collect(city, year);
        lights::bake(city, year, &lamps, &mut [&mut mesh]);
        log::info!("walk world {year}: {} boxes, {} verts in {:.0?}", world.boxes.len(), mesh.vertices.len(), t.elapsed());
        let player = player::Player::new(world.spawn, world.spawn_yaw);
        Walk { interior: GpuMesh::upload_chunked(&gpu.device, &mesh), world, player, year, spots: game::spots(city, year) }
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
        p.torch_col = Vec3::new(0.45, 0.42, 0.36);
        p.torch_range = 7.0;
    }
    p.exposure *= 1.15;
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
    /// The delivery game (None = free roaming).
    game: Option<game::Game>,
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
                            KeyCode::KeyE if walking => self.interact(),
                            KeyCode::KeyH => self.settings.help = !self.settings.help,
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
    fn interact(&mut self) {
        let (Some(g), Some(w)) = (self.game.as_mut(), self.walk.as_ref()) else { return };
        g.interact(&self.world.city, &self.world.society, &w.spots, w.player.pos);
        if g.delivered > 0 {
            self.settings.help = false;
        }
    }

    fn enter_walk(&mut self) {
        let run = self.run.as_ref().unwrap();
        let year = self.game.as_ref().map_or(self.settings.year.floor() as u16, |g| g.year);
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
        if let Some(g) = self.game.as_mut() {
            g.tick(dt);
            if g.job.is_none() {
                g.new_job(&self.world.city, &self.world.society);
            }
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
        let info = walk.map(|w| hud_info(&self.world, w, self.game.as_ref(), &params));
        let game = self.game.as_ref();
        let cmds = run.gui.draw(&run.gpu, &run.window, &mut enc, &view, |ui| match (walk, &info) {
            (Some(w), Some(info)) => hud(ui, w, s, game, info, fps),
            _ => panel(ui, s, &stats, fps),
        });
        run.gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        run.window.pre_present_notify();
        run.gpu.queue.present(frame);
    }
}

/// A name floating over a door, already projected to the screen.
struct Label {
    ndc: (f32, f32),
    title: String,
    sub: String,
    /// "COLLECT" / "DELIVER" if it's where the job wants you.
    tag: Option<&'static str>,
    fade: f32,
}

struct HudInfo {
    place: String,
    labels: Vec<Label>,
    /// Addresses for the job card.
    from_addr: String,
    to_addr: String,
    /// Pointer to the current target: screen angle (0 = straight ahead,
    /// clockwise) and a hint ("24 m · up to 3/F").
    arrow: Option<(f32, String)>,
}

/// Work out what the HUD shows this frame: where you are, and the names of
/// the doors you can see nearby.
fn hud_info(wd: &World, w: &Walk, g: Option<&game::Game>, params: &FrameParams) -> HudInfo {
    let (city, soc, year) = (&wd.city, &wd.society, w.year);
    let cam = w.player.camera();
    let look = (cam.target - cam.eye).normalize();
    let job_unit = |u: u32| -> Option<&'static str> {
        let job = g?.job.as_ref()?;
        if !job.picked && job.from == u {
            Some("COLLECT")
        } else if job.picked && job.to == u {
            Some("DELIVER")
        } else {
            None
        }
    };
    let mut cands: Vec<(f32, &game::Spot)> = w
        .spots
        .iter()
        .filter_map(|sp| {
            let d = sp.label - cam.eye;
            let dist = d.length();
            let tagged = matches!(sp.kind, game::SpotKind::Unit(u) if job_unit(u).is_some());
            let range = if tagged { 25.0 } else { 9.0 };
            (dist < range && d.normalize().dot(look) > 0.3 && (sp.label.y - cam.eye.y).abs() < 3.5).then_some((dist, sp))
        })
        .collect();
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut labels = vec![];
    for (dist, sp) in cands {
        if labels.len() >= 7 {
            break;
        }
        let back = (sp.label - cam.eye).normalize() * 0.2;
        if !game::line_clear(&w.world, cam.eye, sp.label - back) {
            continue;
        }
        let clip = params.view_proj * sp.label.extend(1.0);
        if clip.w <= 0.0 {
            continue;
        }
        let (title, sub) = game::describe(city, soc, year, sp.kind);
        let tag = match sp.kind {
            game::SpotKind::Unit(u) => job_unit(u),
            _ => None,
        };
        labels.push(Label { ndc: (clip.x / clip.w, clip.y / clip.w), title, sub, tag, fade: (1.0 - (dist - 5.0).max(0.0) / 4.0).clamp(0.35, 1.0) });
    }
    let addr = |u: u32| soc.directory.address[u as usize].line();
    let (from_addr, to_addr) = g.and_then(|g| g.job.as_ref()).map_or((String::new(), String::new()), |j| (addr(j.from), addr(j.to)));
    // The arrow: which way, how far, and which floor.
    let arrow = g.and_then(|g| g.job.as_ref()).and_then(|j| {
        let unit = if j.picked { j.to } else { j.from };
        let spot = w.spots.iter().find(|sp| sp.kind == game::SpotKind::Unit(unit))?;
        let p = &w.player;
        let d = spot.stand - p.pos;
        let fwd = Vec3::new(p.yaw.cos(), 0.0, p.yaw.sin());
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        let angle = d.dot(right).atan2(d.dot(fwd));
        let dist = Vec3::new(d.x, 0.0, d.z).length();
        let want = city.units[unit as usize].floor as i32;
        let here = if p.pos.y < 0.5 { 0 } else { (p.pos.y / world::S + 0.25).floor() as i32 };
        let floor = address::Address::floor_name(want as u8);
        let vertical = if want > here {
            format!(" · up to {floor}")
        } else if want < here {
            format!(" · down to {floor}")
        } else if want > 0 {
            format!(" · on {floor}")
        } else {
            String::new()
        };
        let what = if j.picked { "Deliver" } else { "Collect" };
        Some((angle, format!("{what} · {dist:.0} m{vertical}")))
    });
    HudInfo { place: game::place_name(city, soc, year, w.player.pos), labels, from_addr, to_addr, arrow }
}

fn hud(ui: &mut egui::Ui, w: &Walk, s: &Settings, g: Option<&game::Game>, info: &HudInfo, fps: f32) {
    use egui::{Align2, Color32, FontId, Pos2, Stroke};
    let screen = ui.max_rect();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("labels")));
    let paper = Color32::from_rgb(236, 228, 208);

    // Door and entrance names.
    for l in &info.labels {
        let pos = Pos2::new(screen.left() + (l.ndc.0 * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - l.ndc.1 * 0.5) * screen.height());
        let a = (l.fade * 255.0) as u8;
        let (accent, bg) = match l.tag {
            Some("COLLECT") => (Color32::from_rgb(120, 230, 150), Color32::from_rgba_unmultiplied(20, 60, 30, 210)),
            Some(_) => (Color32::from_rgb(255, 200, 90), Color32::from_rgba_unmultiplied(70, 45, 10, 210)),
            None => (Color32::from_rgba_unmultiplied(236, 228, 208, a), Color32::from_rgba_unmultiplied(12, 12, 14, (a as f32 * 0.7) as u8)),
        };
        let title = painter.layout_no_wrap(l.title.clone(), FontId::proportional(15.0), accent);
        let sub = painter.layout_no_wrap(l.sub.clone(), FontId::proportional(11.5), Color32::from_rgba_unmultiplied(200, 195, 180, a));
        let tag = l.tag.map(|t| painter.layout_no_wrap(t.to_string(), FontId::monospace(11.0), accent));
        let w_ = title.size().x.max(sub.size().x).max(tag.as_ref().map_or(0.0, |t| t.size().x));
        let h = title.size().y + sub.size().y + tag.as_ref().map_or(0.0, |t| t.size().y + 2.0);
        let rect = egui::Rect::from_center_size(pos, egui::vec2(w_ + 14.0, h + 8.0));
        painter.rect_filled(rect, 3.0, bg);
        if l.tag.is_some() {
            painter.rect_stroke(rect, 3.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        }
        let mut y = rect.top() + 4.0;
        if let Some(t) = tag {
            let ts = t.size();
            painter.galley(Pos2::new(pos.x - ts.x / 2.0, y), t, accent);
            y += ts.y + 2.0;
        }
        let ts = title.size();
        painter.galley(Pos2::new(pos.x - ts.x / 2.0, y), title, accent);
        y += ts.y;
        let ss = sub.size();
        painter.galley(Pos2::new(pos.x - ss.x / 2.0, y), sub, paper);
    }

    // Where you are.
    painter.text(
        Pos2::new(screen.center().x, screen.top() + 22.0),
        Align2::CENTER_CENTER,
        &info.place,
        FontId::proportional(20.0),
        Color32::from_rgba_unmultiplied(236, 228, 208, 230),
    );
    // Crosshair.
    painter.circle_filled(screen.center(), 2.0, Color32::from_white_alpha(140));

    // Arrow to the current target.
    if let Some((angle, hint)) = &info.arrow {
        let c = Pos2::new(screen.center().x, screen.top() + 70.0);
        let colour = if hint.starts_with("Deliver") { Color32::from_rgb(255, 200, 90) } else { Color32::from_rgb(120, 230, 150) };
        let (sn, cs) = angle.sin_cos();
        let rot = |x: f32, y: f32| Pos2::new(c.x + x * cs - y * sn, c.y + x * sn + y * cs);
        painter.circle_filled(c, 24.0, Color32::from_black_alpha(140));
        let pts = vec![rot(0.0, -18.0), rot(11.0, 11.0), rot(0.0, 4.0), rot(-11.0, 11.0)];
        painter.add(egui::Shape::convex_polygon(vec![pts[0], pts[1], pts[2]], colour, Stroke::NONE));
        painter.add(egui::Shape::convex_polygon(vec![pts[0], pts[2], pts[3]], colour, Stroke::NONE));
        painter.text(Pos2::new(c.x, c.y + 38.0), Align2::CENTER_CENTER, hint, FontId::proportional(15.0), colour);
    }

    // How to play.
    if s.help {
        egui::Area::new(egui::Id::new("help")).anchor(Align2::CENTER_CENTER, [0.0, 40.0]).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(20, 18, 16, 235)).inner_margin(18.0).corner_radius(6.0).show(ui, |ui| {
                ui.set_max_width(460.0);
                ui.colored_label(paper, egui::RichText::new("Delivering in the Walled City").size(20.0).strong());
                ui.add_space(6.0);
                let line = |ui: &mut egui::Ui, t: &str| {
                    ui.colored_label(Color32::from_rgb(215, 208, 190), t);
                };
                line(ui, "1. Your job is top right: collect something, then deliver it.");
                line(ui, "2. Follow the arrow at the top of the screen. It points at the door you want, and tells you the distance and the floor.");
                line(ui, "3. Names float over doors as you pass. Your pick-up glows green, your drop-off amber.");
                line(ui, "4. Flats are upstairs: go in at the building's street door and take the stairs. \"Flat 3B\" is door B on 3/F; G/F is the ground floor.");
                line(ui, "5. Stand right in front of the door and press E.");
                ui.add_space(6.0);
                ui.colored_label(Color32::from_white_alpha(140), "Mouse look · WASD walk · Shift run · Space jump · T torch · N night · Tab city view · H hide this");
            });
        });
    }

    // The job card.
    if let Some(g) = g {
        egui::Area::new(egui::Id::new("job")).anchor(Align2::RIGHT_TOP, [-16.0, 16.0]).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(20, 18, 16, 225)).inner_margin(12.0).corner_radius(4.0).show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.colored_label(paper, egui::RichText::new(format!("{} · Delivered {} · Tips HK${}", g.year, g.delivered, g.tips)).small());
                ui.separator();
                match &g.job {
                    Some(j) => {
                        let (c1, c2) = if j.picked { (Color32::GRAY, Color32::from_rgb(255, 200, 90)) } else { (Color32::from_rgb(120, 230, 150), Color32::GRAY) };
                        ui.colored_label(c1, egui::RichText::new(format!("1. Collect {}{}", j.item, if j.picked { "  (done)" } else { "" })).strong());
                        ui.colored_label(c1, format!("   from {}", j.from_name));
                        ui.colored_label(c1, egui::RichText::new(format!("   {}", info.from_addr)).small());
                        ui.add_space(4.0);
                        ui.colored_label(c2, egui::RichText::new("2. Deliver it").strong());
                        ui.colored_label(c2, format!("   to {}", j.to_name));
                        ui.colored_label(c2, egui::RichText::new(format!("   {}", info.to_addr)).small());
                    }
                    None => {
                        ui.label("Waiting for work...");
                    }
                }
            });
        });
        if let Some((msg, t)) = &g.toast {
            let a = (t.min(1.0) * 255.0) as u8;
            painter.text(
                Pos2::new(screen.center().x, screen.bottom() - 90.0),
                Align2::CENTER_CENTER,
                msg,
                FontId::proportional(18.0),
                Color32::from_rgba_unmultiplied(255, 240, 200, a),
            );
        }
    }

    // Controls.
    egui::Area::new(egui::Id::new("keys")).anchor(Align2::LEFT_BOTTOM, [12.0, -10.0]).show(ui.ctx(), |ui| {
        ui.colored_label(
            Color32::from_white_alpha(110),
            egui::RichText::new(format!(
                "WASD walk · Shift run · E knock · W on a ladder to climb · T torch ({}) · N night · H help · Tab overview · {fps:.0} fps",
                if s.torch { "on" } else { "off" }
            ))
            .small(),
        );
    });
    let _ = w;
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
            if args.iter().any(|a| a == "--in-well") {
                // Stand on the turning landing, looking back down both flights.
                let w = world::Well::of(plot);
                let at = w.point(2.75, 1.5);
                let back = w.point(0.0, 1.5) - at;
                p.pos = Vec3::new(at.x, f * STOREY_M + STOREY_M / 2.0, at.z);
                p.yaw = back.z.atan2(back.x);
            }
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
    // Default: the delivery game, on foot in 1950. `--walk`: roam 1987 freely.
    // `--free`: the growth overview.
    let free_walk = args.iter().any(|a| a == "--walk");
    let overview = args.iter().any(|a| a == "--free");
    let walk_now = !overview;
    let game = (!free_walk && !overview).then(|| game::Game::new(1987, START_YEAR));
    let mut app = App {
        run: None,
        world,
        settings: Settings {
            seed: 1987,
            year: if free_walk { END_YEAR as f32 } else { START_YEAR as f32 },
            playing: !walk_now,
            speed: 2.0,
            mode: ColourMode::Grime,
            night: false,
            torch: true,
            want_walk: walk_now,
            help: true,
        },
        orbit,
        walk: None,
        game,
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
        let landing = plot.core[0];
        let well = world::Well::of(plot);
        let lc = ((landing.0 as f32 + 0.5) * CELL_M, (landing.1 as f32 + 0.5) * CELL_M);
        let mut p = player::Player::new(Vec3::new(lc.0, 0.0, lc.1), 0.0);
        settle(&mut p, &w);
        assert!(p.pos.y.abs() < 0.05, "should stand on the ground-floor landing, y = {}", p.pos.y);
        for floor in 1..=h {
            let pt = |u: f32, v: f32| {
                let p = well.point(u, v);
                (p.x, p.z)
            };
            let route = [pt(0.25, 0.75), pt(2.7, 0.75), pt(2.7, 2.25), pt(-0.4, 2.25), pt(-0.75, 0.75), lc];
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
