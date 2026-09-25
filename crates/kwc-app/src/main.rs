//! Kowloon Walled City — grow it, map it, walk it.
//!
//!   kowloon                       open the window
//!   kowloon --shot out.png [--year 1987] [--seed 1987] [--night] [--decade] [--view yaw,pitch,dist]
//!   kowloon --shot out.png --walk [--lane N] [--at x,y,z,yaw,pitch] [--torch]
//!                                 render one frame headless and save it

mod autopilot;
mod citymesh;
mod course;
mod game;
mod lighting;
mod lights;
mod panels;
mod people;
mod plane;
mod prefs;
mod runs;
mod player;
mod signtext;
mod wayfinding;
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
use kwc_engine::{egui, Camera, FrameParams, GpuMesh, Gpu, Gui, OrbitCamera, Renderer, TextMesh};
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
    /// Memory mode (G): no place line, door names or arrow; only the street plaques.
    memory: bool,
    /// The notebook map (M) and the ledger (L).
    map_open: bool,
    ledger_open: bool,
    ledger_pick: Option<usize>,
    /// What the autopilot is up to, while it's driving.
    demo_status: Option<String>,
    /// Frames wait for the display (V toggles).
    vsync: bool,
    /// The course code, when racing one.
    course: Option<String>,
    /// Title screen: the code typed in, and the run category chosen.
    title_code: String,
    memory_run: bool,
    /// The settings panel, and the action waiting for a key to be pressed.
    settings_open: bool,
    rebinding: Option<prefs::Action>,
    /// The controls line, in the player's own keys.
    keys_line: String,
}

struct World {
    city: Arc<City>,
    society: society::Society,
    mesh: Option<GpuMesh>,
    built_for: Option<(u16, ColourMode, u64)>,
}

impl World {
    fn new(seed: u64) -> World {
        let city = generate(&Params { seed, ..Default::default() });
        let society = society::generate(&city);
        World { city: Arc::new(city), society, mesh: None, built_for: None }
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
    world: Arc<world::WalkWorld>,
    interior: Vec<GpuMesh>,
    player: player::Player,
    year: u16,
    spots: Vec<game::Spot>,
    plates: Vec<game::Plate>,
    /// Every name, painted on the city.
    text: Option<TextMesh>,
    /// The current job's two doors, painted again in their highlight colours.
    job_text: Option<TextMesh>,
    job_key: Option<(u32, u32, bool)>,
    crowd: people::Crowd,
}

impl Walk {
    fn new(gpu: &Gpu, renderer: &mut Renderer, signs: &signtext::SignText, city: &City, soc: &society::Society, year: u16) -> Walk {
        let t = Instant::now();
        let (world, mut mesh) = world::build(city, year);
        let lamps = lights::collect(city, year);
        lights::bake(city, year, &lamps, &mut [&mut mesh]);
        let crowd = people::Crowd::new(city, soc, &world, year, &lamps);
        log::info!("{} people about in {year}", crowd.figures.len());
        log::info!("walk world {year}: {} boxes, {} verts in {:.0?}", world.boxes.len(), mesh.vertices.len(), t.elapsed());
        let player = player::Player::new(world.spawn, world.spawn_yaw);
        let plates = game::plates(city, soc, year);
        let (tv, ti) = signs.build(&plates, None, None);
        let (aw, ah, atlas) = signs.atlas();
        renderer.set_text_atlas(gpu, aw, ah, &atlas);
        Walk {
            interior: GpuMesh::upload_chunked(&gpu.device, &mesh),
            world: Arc::new(world),
            player,
            year,
            spots: game::spots(city, soc, year),
            text: TextMesh::upload(&gpu.device, &tv, &ti),
            plates,
            job_text: None,
            job_key: None,
            crowd,
        }
    }
}

fn default_orbit(city: &City) -> OrbitCamera {
    OrbitCamera { target: citymesh::centre(city) + Vec3::Y * 12.0, yaw: 2.3, pitch: 0.42, dist: 330.0 }
}

fn walk_params(cam: &Camera, aspect: f32, night: bool, torch: bool) -> FrameParams {
    let mut p = lighting::params(cam, aspect, night);
    // Up close the lanes are darker and the air thicker.
    p.fog_density *= 2.5;
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
    /// The walk you left with Tab: picked up again, where you stood, if the
    /// year hasn't changed.
    parked: Option<Walk>,
    /// The delivery game (None = free roaming).
    game: Option<game::Game>,
    signs: signtext::SignText,
    /// Moving on: the timelapse runs to this year, then you're back on foot.
    pending_era: Option<u16>,
    /// Demo mode (P): the courier does the rounds by itself.
    autopilot: Option<autopilot::Autopilot>,
    /// Where the game saves (a trial of another era keeps its own).
    save_file: String,
    /// The title screen is up (the city turns slowly behind it).
    title: bool,
    /// What the campaign save holds, for the title's Continue button.
    save_summary: Option<String>,
    /// A run in progress, and personal bests.
    race: Option<runs::Run>,
    records: runs::Records,
    prefs: prefs::Prefs,
    started: Instant,
    plane: Option<GpuMesh>,
    /// Everyone near you, posed for this frame.
    people: Option<GpuMesh>,
    keys: HashSet<KeyCode>,
    drag: Option<MouseButton>,
    last_cursor: Option<(f64, f64)>,
    last_frame: Instant,
    clock: Clock,
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
        let mut gpu = Gpu::for_window(window.clone());
        gpu.set_vsync(self.prefs.vsync);
        self.settings.vsync = self.prefs.vsync;
        if self.prefs.fullscreen {
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
        self.settings.keys_line = keys_line(&self.prefs);
        let renderer = Renderer::new(&gpu);
        let gui = Gui::new(&gpu, &window);
        self.run = Some(Running { window, gpu, renderer, gui });
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if self.settings.map_open || self.settings.ledger_open || self.autopilot.is_some() {
            return;
        }
        if let (DeviceEvent::MouseMotion { delta }, Some(w)) = (event, self.walk.as_mut()) {
            let k = self.prefs.sensitivity;
            w.player.look(delta.0 as f32 * k, delta.1 as f32 * k * if self.prefs.invert_y { -1.0 } else { 1.0 });
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(run) = self.run.as_mut() else { return };
        let walking = self.walk.is_some();
        let panel = self.settings.map_open || self.settings.ledger_open;
        let consumed = (!walking || panel) && run.gui.on_event(&run.window, &event);
        match event {
            WindowEvent::CloseRequested => {
                self.save_game();
                el.exit()
            }
            WindowEvent::Resized(size) => {
                run.gpu.resize(size.width, size.height);
                run.renderer.resize(&run.gpu.device, size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if event.state == ElementState::Pressed {
                    if !event.repeat {
                        // Settings: the next key pressed is the new binding.
                        if let Some(a) = self.settings.rebinding.take() {
                            if code != KeyCode::Escape && prefs::bindable(code) {
                                self.prefs.bind(a, code);
                                self.prefs.save();
                                self.settings.keys_line = keys_line(&self.prefs);
                            }
                            return;
                        }
                        if code == KeyCode::F11 {
                            self.prefs.fullscreen = !self.prefs.fullscreen;
                            self.prefs.save();
                            run.window.set_fullscreen(self.prefs.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
                        }
                        let action = self.prefs.action(code);
                        use prefs::Action as A;
                        match code {
                            // On the title screen, keys are for typing a code.
                            _ if self.title => {}
                            // No bird's-eye view mid-race.
                            KeyCode::Tab if self.race.is_some() => {}
                            KeyCode::Tab => {
                                if walking {
                                    self.leave_walk();
                                } else {
                                    self.settings.want_walk = true;
                                }
                            }
                            KeyCode::Escape if !self.title => self.to_title(),
                            _ => match action {
                            Some(A::Restart) if self.race.is_some() && walking => self.restart_run(),
                            // Races are on your own feet and in their own era, and a
                            // Memory race means no help at all.
                            Some(A::Autopilot | A::MoveOn) if self.race.is_some() => {}
                            Some(A::Memory | A::Notebook | A::Ledger) if self.race.as_ref().is_some_and(|r| r.cat == runs::Category::Memory) => {}
                            Some(A::Torch) => self.settings.torch = !self.settings.torch,
                            Some(A::Night) => self.settings.night = !self.settings.night,
                            Some(A::Vsync) => {
                                if let Some(run) = self.run.as_mut() {
                                    let on = !run.gpu.vsync();
                                    run.gpu.set_vsync(on);
                                    self.settings.vsync = on;
                                    self.prefs.vsync = on;
                                    self.prefs.save();
                                }
                            }
                            Some(A::Knock) if walking => self.interact(),
                            Some(A::Help) => self.settings.help = !self.settings.help,
                            Some(A::Memory) => self.settings.memory = !self.settings.memory,
                            Some(A::Autopilot) if walking && self.game.is_some() => {
                                self.autopilot = match self.autopilot.take() {
                                    Some(_) => None,
                                    None => {
                                        self.settings.help = false;
                                        Some(autopilot::Autopilot::default())
                                    }
                                };
                            }
                            Some(A::Notebook) if walking => {
                                self.settings.map_open = !self.settings.map_open;
                                self.settings.ledger_open = false;
                                self.sync_cursor();
                            }
                            Some(A::Ledger) if walking => {
                                self.settings.ledger_open = !self.settings.ledger_open;
                                self.settings.map_open = false;
                                self.sync_cursor();
                            }
                            // With Shift: skip ahead without knowing this era (for trying later years).
                            Some(A::MoveOn) if walking => {
                                let skip = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                                self.move_on(skip)
                            }
                            _ => {}
                            },
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
        let before = self.game.as_ref().map_or(0, |g| g.delivered);
        self.knock();
        if self.game.as_ref().is_some_and(|g| g.delivered > before) {
            if let Some(r) = self.race.as_mut() {
                r.split(&mut self.records);
            }
        }
    }

    fn knock(&mut self) {
        let (Some(g), Some(w)) = (self.game.as_mut(), self.walk.as_mut()) else { return };
        // Whoever lives there comes to the door.
        if let Some(u) = g.interact(&self.world.city, &self.world.society, &w.spots, w.player.pos) {
            if let Some(sp) = w.spots.iter().find(|s| s.kind == game::SpotKind::Unit(u)) {
                w.crowd.greet(&self.world.society, w.year, u, sp.stand, sp.label, w.player.pos);
            }
        }
        if g.delivered > 0 {
            self.settings.help = false;
        }
        self.save_game();
    }

    fn save_game(&self) {
        // Races aren't saved: they always start from the beginning.
        if self.race.is_some() {
            return;
        }
        if let Some(g) = &self.game {
            let mut save = g.save();
            save.pos = self.walk.as_ref().or(self.parked.as_ref()).filter(|w| w.year == g.year).map(|w| {
                let p = w.player.pos;
                [p.x, p.y, p.z, w.player.yaw]
            });
            if let Ok(json) = serde_json::to_string(&save) {
                let _ = std::fs::write(&self.save_file, json);
            }
        }
    }

    /// Back to the title screen (the campaign is saved; a race is dropped).
    fn to_title(&mut self) {
        self.save_game();
        self.walk = None;
        self.parked = None;
        self.race = None;
        self.autopilot = None;
        self.pending_era = None;
        self.settings.map_open = false;
        self.settings.ledger_open = false;
        self.settings.playing = false;
        self.title = true;
        self.save_summary = campaign_summary();
        self.keys.clear();
        self.sync_cursor();
    }

    fn use_city(&mut self, seed: u64) {
        if self.world.city.params.seed != seed {
            self.world = World::new(seed);
        }
        self.settings.seed = seed;
        self.walk = None;
        self.parked = None;
    }

    fn start_campaign(&mut self, fresh: bool) {
        self.use_city(1987);
        self.save_file = SAVE_FILE.to_string();
        let saved = (!fresh).then(|| std::fs::read_to_string(SAVE_FILE).ok()).flatten().and_then(|s| serde_json::from_str::<game::Save>(&s).ok());
        let g = saved.map_or_else(|| game::Game::new(1987, START_YEAR), game::Game::load);
        self.settings.year = g.year as f32;
        self.game = Some(g);
        self.race = None;
        self.settings.memory = false;
        self.settings.course = None;
        self.title = false;
        self.settings.want_walk = true;
    }

    fn start_run(&mut self, course: course::Course, cat: runs::Category) {
        self.use_city(course.city_seed());
        self.game = Some(game::Game::new(course.seed as u64, course.era));
        self.settings.year = course.era as f32;
        self.race = Some(runs::Run::new(course, cat, &self.records));
        self.settings.memory = cat == runs::Category::Memory;
        self.settings.help = false;
        self.settings.course = None;
        self.autopilot = None;
        self.title = false;
        self.settings.want_walk = true;
    }

    /// R: the same course again from the gate.
    fn restart_run(&mut self) {
        let Some(r) = self.race.as_ref() else { return };
        let (course, cat) = (r.course, r.cat);
        self.game = Some(game::Game::new(course.seed as u64, course.era));
        if let Some(w) = self.walk.as_mut() {
            w.player = player::Player::new(w.world.spawn, w.world.spawn_yaw);
        }
        self.race = Some(runs::Run::new(course, cat, &self.records));
    }

    fn sync_cursor(&self) {
        let Some(run) = &self.run else { return };
        let w = &run.window;
        if self.settings.map_open || self.settings.ledger_open || self.walk.is_none() {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
            w.set_cursor_visible(false);
        }
    }

    /// Y: once you know this era, watch the city grow to the next and carry on.
    fn move_on(&mut self, skip: bool) {
        let city = &self.world.city;
        let Some(g) = self.game.as_ref() else { return };
        let Some(next) = g.next_era() else {
            if let Some(g) = self.game.as_mut() {
                g.toast("This is the last year: 1987. The clearance is coming.");
            }
            return;
        };
        if !skip && !g.knows_era(city) {
            let msg = format!(
                "Not yet: walk more of the lanes ({:.0}%/{:.0}%) and make {} deliveries by memory ({} so far). Shift+Y skips ahead anyway.",
                g.coverage(city) * 100.0,
                game::KNOW_COVERAGE * 100.0,
                game::KNOW_CLEAN,
                g.clean
            );
            if let Some(g) = self.game.as_mut() {
                g.toast(msg);
            }
            return;
        }
        self.settings.year = g.year as f32;
        self.settings.speed = 1.5;
        self.settings.playing = true;
        self.settings.map_open = false;
        self.settings.ledger_open = false;
        self.pending_era = Some(next);
        self.leave_walk();
    }

    fn enter_walk(&mut self) {
        let run = self.run.as_mut().unwrap();
        let year = self.game.as_ref().map_or(self.settings.year.floor() as u16, |g| g.year);
        self.settings.playing = false;
        self.walk = match self.parked.take() {
            // Back from the overview: carry on exactly where you were.
            Some(w) if w.year == year => Some(w),
            _ => {
                let mut w = Walk::new(&run.gpu, &mut run.renderer, &self.signs, &self.world.city, &self.world.society, year);
                // Or where you were when you last saved.
                if let Some(p) = self.game.as_mut().and_then(|g| g.resume.take()) {
                    w.player.pos = Vec3::new(p[0], p[1], p[2]);
                    w.player.yaw = p[3];
                }
                w.player.unstick(&w.world);
                Some(w)
            }
        };
        let w = &run.window;
        let _ = w.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
        w.set_cursor_visible(false);
    }

    fn leave_walk(&mut self) {
        self.parked = self.walk.take();
        self.keys.clear();
        if let Some(run) = &self.run {
            let _ = run.window.set_cursor_grab(CursorGrabMode::None);
            run.window.set_cursor_visible(true);
        }
    }

    fn input(&self) -> player::Input {
        use prefs::Action as A;
        let k = |a: A| self.keys.contains(&self.prefs.key(a));
        let axis = |a, b| (k(a) as i32 - k(b) as i32) as f32;
        // Either Shift jogs while jog is on a Shift.
        let jog = self.prefs.key(A::Jog);
        let shift = matches!(jog, KeyCode::ShiftLeft | KeyCode::ShiftRight) && (self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight));
        player::Input { forward: axis(A::Forward, A::Back), strafe: axis(A::Right, A::Left), run: k(A::Jog) || shift, jump: k(A::Jump) }
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
        // Demo mode drives until you touch the keys.
        if self.autopilot.is_some() && (input.forward != 0.0 || input.strafe != 0.0) {
            self.autopilot = None;
            if let Some(g) = self.game.as_mut() {
                g.toast("You take over.");
            }
        }
        // The world moves on a fixed tick, the same at any frame rate (fair
        // runs, replayable inputs); frames draw between the last two ticks.
        for _ in 0..self.clock.advance(dt) {
            let mut step = input;
            let mut act = autopilot::Act::Walk;
            if let (Some(ap), Some(w), Some(g)) = (self.autopilot.as_mut(), self.walk.as_mut(), self.game.as_ref()) {
                let target = autopilot::target(g, &w.spots);
                (step, act) = ap.drive(&w.world, &self.world.city, w.year, &mut w.player, target, TICK);
            }
            if let Some(w) = self.walk.as_mut() {
                w.player.update(&w.world, step, TICK);
                w.crowd.update(TICK, w.player.pos);
                if let Some(r) = self.race.as_mut() {
                    r.tick(input.forward != 0.0 || input.strafe != 0.0);
                    r.record(w.player.pos, w.player.yaw);
                }
            }
            match act {
                autopilot::Act::Knock => self.interact(),
                autopilot::Act::Lost => {
                    if let Some(g) = self.game.as_mut() {
                        g.job = None;
                        g.toast("The courier couldn't find a way there, and passed the job on.");
                    }
                }
                autopilot::Act::Walk => {}
            }
        }
        if let Some(w) = self.walk.as_mut() {
            w.player.alpha = self.clock.alpha();
            w.player.base_fov = self.prefs.fov;
            w.player.bob_scale = self.prefs.head_bob;
        }
        self.settings.demo_status = self.autopilot.as_ref().map(|a| a.status.clone());
        if let Some(g) = self.game.as_mut() {
            g.tick(dt);
            if g.job.is_none() {
                g.new_job(&self.world.city, &self.world.society);
            }
            if let Some(w) = &self.walk {
                // The autopilot's walking doesn't count as you learning the lanes.
                if self.autopilot.is_none() {
                    g.observe(&self.world.city, w.player.pos);
                }
                // Any help during a job means it wasn't done from memory.
                if g.job.is_some() && (!self.settings.memory || self.settings.map_open || self.autopilot.is_some()) {
                    g.job_helped = true;
                }
                if self.race.is_none() && !g.understood && g.knows_era(&self.world.city) {
                    g.understood = true;
                    let next = g.next_era().map_or("the end".to_string(), |n| n.to_string());
                    g.toast(format!("You know {}'s Walled City. Press Y to move on to {next}, or keep delivering.", g.year));
                }
            }
        }
        // The timelapse between eras: when it reaches the next era, back on foot.
        if let Some(next) = self.pending_era {
            if self.settings.year >= next as f32 {
                self.settings.year = next as f32;
                self.settings.playing = false;
                self.pending_era = None;
                if let Some(g) = self.game.as_mut() {
                    g.enter_era(&self.world.city, next);
                }
                self.save_game();
                self.settings.want_walk = true;
            }
        }
        if let (Some(w), Some(run)) = (self.walk.as_mut(), self.run.as_ref()) {
            let key = self.game.as_ref().and_then(|g| g.job.as_ref()).map(|j| (j.from, j.to, j.picked));
            if key != w.job_key {
                w.job_key = key;
                w.job_text = key.and_then(|(from, to, picked)| {
                    let (unit, tint) = if picked { (to, [255, 205, 90]) } else { (from, [120, 240, 150]) };
                    let ids: Vec<usize> = w.plates.iter().enumerate().filter(|(_, p)| p.kind == game::SpotKind::Unit(unit)).map(|(k, _)| k).collect();
                    let (v, i) = self.signs.build(&w.plates, Some(&ids), Some(tint));
                    TextMesh::upload(&run.gpu.device, &v, &i)
                });
            }
        }

        if self.title {
            self.orbit.yaw += dt * 0.04;
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
            self.parked = None;
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
        // The Kai Tak jet, rebuilt where it is this frame.
        let t = self.started.elapsed().as_secs_f32();
        self.plane = plane::mesh(t, citymesh::centre(&self.world.city)).and_then(|m| GpuMesh::upload(&run.gpu.device, &m));
        let race = self.race.as_ref();
        self.people = self.walk.as_ref().and_then(|w| {
            let cam = w.player.camera();
            let mut m = w.crowd.mesh(cam.eye, (cam.target - cam.eye).normalize(), s.night, t);
            // Your best run, racing you.
            if let Some(g) = race.and_then(|r| r.ghost.as_ref().map(|g| (g, r.ticks))).map(|(g, ticks)| g.at(ticks)) {
                people::ghost_mesh(&mut m, g.0, g.1, g.2, g.3, t);
            }
            GpuMesh::upload(&run.gpu.device, &m)
        });
        let mut meshes: Vec<&GpuMesh> = self.world.mesh.iter().chain(self.plane.iter()).chain(self.people.iter()).collect();
        let params = match &self.walk {
            Some(w) => {
                meshes.extend(w.interior.iter());
                walk_params(&w.player.camera(), run.gpu.aspect(), s.night, s.torch)
            }
            None => lighting::params(&self.orbit.camera(), run.gpu.aspect(), s.night),
        };
        let texts: Vec<&TextMesh> = self.walk.as_ref().map_or(vec![], |w| w.text.iter().chain(w.job_text.iter()).collect());
        run.renderer.render_with_text(&run.gpu, &mut enc, &view, &meshes, &texts, &params);

        let stats = stats::measure(&self.world.city, year);
        let fps = self.fps;
        let walk = self.walk.as_ref();
        let info = walk.map(|w| hud_info(&self.world, w, self.game.as_ref(), &params, s.memory, s.night));
        let game = self.game.as_ref();
        let (city, soc) = (&self.world.city, &self.world.society);
        let progress = game.map(|g| (g.coverage(city), g.clean, g.knows_era(city), g.next_era()));
        let pending = self.pending_era;
        let (title, race, records, summary) = (self.title, self.race.as_ref(), &self.records, self.save_summary.as_deref());
        let prefs = &mut self.prefs;
        let mut prefs_changed = false;
        let mut action = None;
        let cmds = run.gui.draw(&run.gpu, &run.window, &mut enc, &view, |ui| match (walk, &info) {
            (Some(w), Some(info)) => {
                hud(ui, w, s, game, info, fps, progress);
                if let Some(r) = race {
                    runs::hud(ui, r);
                }
                if let (true, Some(g)) = (s.map_open, game) {
                    panels::notebook(ui, city, soc, g, w.player.pos, w.player.yaw);
                }
                if let (true, Some(g)) = (s.ledger_open, game) {
                    panels::ledger(ui, soc, g, &mut s.ledger_pick);
                }
            }
            _ if title && s.settings_open => prefs_changed = settings_panel(ui, s, prefs),
            _ if title => action = title_screen(ui, s, records, summary),
            _ => {
                panel(ui, s, &stats, fps);
                if let Some(n) = pending {
                    ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("era"))).text(
                        ui.max_rect().center_top() + egui::vec2(0.0, 60.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{:.0} → {n}", s.year.floor()),
                        egui::FontId::proportional(40.0),
                        egui::Color32::from_rgb(240, 230, 200),
                    );
                }
            }
        });
        run.gpu.queue.submit(cmds.into_iter().chain([enc.finish()]));
        run.window.pre_present_notify();
        run.gpu.queue.present(frame);
        if prefs_changed {
            self.prefs.save();
            self.settings.keys_line = keys_line(&self.prefs);
            let run = self.run.as_mut().unwrap();
            if run.gpu.vsync() != self.prefs.vsync {
                run.gpu.set_vsync(self.prefs.vsync);
                self.settings.vsync = self.prefs.vsync;
            }
            run.window.set_fullscreen(self.prefs.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
        }
        match action {
            Some(TitleAction::Continue) => self.start_campaign(false),
            Some(TitleAction::NewGame) => self.start_campaign(true),
            Some(TitleAction::Race(c, cat)) => self.start_run(c, cat),
            None => {}
        }
    }
}

/// The controls line, in the player's own keys.
fn keys_line(p: &prefs::Prefs) -> String {
    use prefs::{key_name as n, Action as A};
    format!(
        "{}{}{}{} walk · {} jog · {} hop/vault · {} knock · {} autopilot · {} notebook · {} ledger · {} memory · {} move on · {} torch · {} night · {} help · Esc menu · Tab overview · {} vsync",
        n(p.key(A::Forward)),
        n(p.key(A::Left)),
        n(p.key(A::Back)),
        n(p.key(A::Right)),
        n(p.key(A::Jog)),
        n(p.key(A::Jump)),
        n(p.key(A::Knock)),
        n(p.key(A::Autopilot)),
        n(p.key(A::Notebook)),
        n(p.key(A::Ledger)),
        n(p.key(A::Memory)),
        n(p.key(A::MoveOn)),
        n(p.key(A::Torch)),
        n(p.key(A::Night)),
        n(p.key(A::Help)),
        n(p.key(A::Vsync)),
    )
}

/// Settings: mouse, view, display and keys. Returns true if anything changed.
fn settings_panel(ui: &mut egui::Ui, s: &mut Settings, p: &mut prefs::Prefs) -> bool {
    use egui::{Align2, Color32, RichText};
    let before = serde_json::to_string(&*p).unwrap_or_default();
    egui::Area::new(egui::Id::new("settings")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(16, 14, 12, 240)).inner_margin(24.0).corner_radius(8.0).show(ui, |ui| {
            ui.set_width(520.0);
            ui.colored_label(Color32::from_rgb(236, 228, 208), RichText::new("Settings").size(22.0));
            ui.add_space(8.0);
            egui::Grid::new("prefs").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label("Mouse sensitivity");
                ui.add(egui::Slider::new(&mut p.sensitivity, 0.2..=3.0).fixed_decimals(2));
                ui.end_row();
                ui.label("Invert mouse Y");
                ui.checkbox(&mut p.invert_y, "");
                ui.end_row();
                ui.label("Field of view");
                ui.add(egui::Slider::new(&mut p.fov, 60.0..=95.0).suffix("°").fixed_decimals(0));
                ui.end_row();
                ui.label("Head bob");
                ui.add(egui::Slider::new(&mut p.head_bob, 0.0..=1.5).fixed_decimals(2));
                ui.end_row();
                ui.label("Vsync");
                ui.checkbox(&mut p.vsync, "");
                ui.end_row();
                ui.label("Fullscreen (F11)");
                ui.checkbox(&mut p.fullscreen, "");
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.colored_label(Color32::from_rgb(236, 228, 208), RichText::new("Keys").size(16.0));
            ui.colored_label(Color32::from_gray(150), RichText::new("Click a key, then press the new one (Esc cancels). Keys already in use swap over.").small());
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for a in prefs::Action::ALL {
                        ui.label(a.label());
                        let text = if s.rebinding == Some(a) { "press a key…".to_string() } else { prefs::key_name(p.key(a)) };
                        if ui.add(egui::Button::new(RichText::new(text).monospace()).min_size(egui::vec2(120.0, 0.0))).clicked() {
                            s.rebinding = Some(a);
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Reset keys").clicked() {
                    p.reset_keys();
                }
                if ui.button("Done").clicked() {
                    s.settings_open = false;
                    s.rebinding = None;
                }
            });
        });
    });
    serde_json::to_string(&*p).unwrap_or_default() != before
}

enum TitleAction {
    Continue,
    NewGame,
    Race(course::Course, runs::Category),
}

/// What the campaign save holds, for the Continue button.
fn campaign_summary() -> Option<String> {
    let s = serde_json::from_str::<game::Save>(&std::fs::read_to_string(SAVE_FILE).ok()?).ok()?;
    Some(format!("{} · {} delivered", s.year, s.delivered))
}

/// The title: carry on the campaign, or race a course.
fn title_screen(ui: &mut egui::Ui, s: &mut Settings, records: &runs::Records, summary: Option<&str>) -> Option<TitleAction> {
    use egui::{Align2, Color32, RichText};
    let paper = Color32::from_rgb(236, 228, 208);
    let dim = Color32::from_gray(160);
    let mut act = None;
    egui::Area::new(egui::Id::new("title")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(16, 14, 12, 235)).inner_margin(28.0).corner_radius(8.0).show(ui, |ui| {
            ui.set_width(440.0);
            ui.vertical_centered(|ui| {
                ui.colored_label(paper, RichText::new("KOWLOON WALLED CITY").size(30.0).strong());
                ui.colored_label(dim, "1950 – 1987 · a courier's city");
            });
            ui.add_space(18.0);
            ui.colored_label(Color32::from_rgb(255, 200, 90), RichText::new("Campaign").size(18.0));
            ui.colored_label(dim, RichText::new("Deliver until you know the city, then watch it grow.").small());
            ui.horizontal(|ui| {
                if let Some(sum) = summary {
                    if ui.button(format!("Continue ({sum})")).clicked() {
                        act = Some(TitleAction::Continue);
                    }
                }
                if ui.button(if summary.is_some() { "Start again" } else { "Start" }).clicked() {
                    act = Some(TitleAction::NewGame);
                }
            });
            ui.add_space(16.0);
            ui.colored_label(Color32::from_rgb(120, 200, 255), RichText::new("Race").size(18.0));
            ui.colored_label(dim, RichText::new(format!("{} deliveries against the clock. Same code, same city, same jobs.", runs::DELIVERIES)).small());
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.memory_run, false, "Rounds (arrow and names)");
                ui.radio_value(&mut s.memory_run, true, "Memory (plaques only)");
            });
            let cat = if s.memory_run { runs::Category::Memory } else { runs::Category::Rounds };
            let best = |c: course::Course| records.best(c, cat).and_then(|b| b.last()).map_or(String::new(), |&t| format!(" · best {}", runs::clock(t)));
            let daily = course::Course::daily();
            ui.horizontal(|ui| {
                if ui.button(format!("Today's course {}{}", daily.code(), best(daily))).clicked() {
                    act = Some(TitleAction::Race(daily, cat));
                }
                if ui.button("Random").clicked() {
                    act = Some(TitleAction::Race(course::Course::random(course::Course::daily().era, false), cat));
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.title_code).hint_text("KWC-1965-7F3A").desired_width(160.0).font(egui::TextStyle::Monospace));
                let parsed = course::Course::parse(&s.title_code);
                if ui.add_enabled(parsed.is_some(), egui::Button::new("Race this code")).clicked() {
                    act = parsed.map(|c| TitleAction::Race(c, cat));
                }
                if let Some(c) = parsed {
                    ui.colored_label(dim, RichText::new(best(c).trim_start_matches(" · ").to_string()).small());
                } else if !s.title_code.trim().is_empty() {
                    ui.colored_label(Color32::from_rgb(240, 120, 110), RichText::new("not a code").small());
                }
            });
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                if ui.button("Settings").clicked() {
                    s.settings_open = true;
                }
                ui.add_space(6.0);
                ui.colored_label(dim, RichText::new("In game: Esc comes back here · F11 fullscreen").small());
                ui.add_space(6.0);
                ui.colored_label(
                    Color32::from_gray(130),
                    RichText::new("Kowloon Walled City was real: some 33,000 people lived in 2.6 hectares until it was cleared in 1993. The layout here is generated and every name is invented.").small().italics(),
                );
            });
        });
    });
    act
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
    /// The person under the crosshair: name, and who they are.
    person: Option<(String, String)>,
}

/// Work out what the HUD shows this frame: where you are, and the names of
/// the doors you can see nearby.
fn hud_info(wd: &World, w: &Walk, g: Option<&game::Game>, params: &FrameParams, memory: bool, night: bool) -> HudInfo {
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
        .filter(|_| false)
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
    let arrow = g.filter(|_| !memory).and_then(|g| g.job.as_ref()).and_then(|j| {
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
    let place = if memory { "Memory mode · plaques only (G to turn off)".to_string() } else { game::place_name(city, soc, year, w.player.pos) };
    let person = w.crowd.looking_at(cam.eye, look, night, &w.world).map(|(n, a)| (n.to_string(), a.to_string()));
    HudInfo { place, labels, from_addr, to_addr, arrow, person }
}

fn hud(ui: &mut egui::Ui, w: &Walk, s: &Settings, g: Option<&game::Game>, info: &HudInfo, fps: f32, progress: Option<(f32, u32, bool, Option<u16>)>) {
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

    // Where you are, on a dark band so it reads against a bright sky.
    let at = Pos2::new(screen.center().x, screen.top() + 22.0);
    let r = painter.text(at, Align2::CENTER_CENTER, &info.place, FontId::proportional(20.0), Color32::TRANSPARENT);
    painter.rect_filled(r.expand2(egui::vec2(14.0, 5.0)), 6.0, Color32::from_black_alpha(150));
    painter.text(at, Align2::CENTER_CENTER, &info.place, FontId::proportional(20.0), Color32::from_rgba_unmultiplied(236, 228, 208, 235));
    if let Some(st) = &s.demo_status {
        let at = Pos2::new(screen.center().x, screen.top() + 52.0);
        let line = format!("AUTOPILOT · {st} · P or WASD to take over");
        let r = painter.text(at, Align2::CENTER_CENTER, &line, FontId::proportional(14.0), Color32::TRANSPARENT);
        painter.rect_filled(r.expand2(egui::vec2(10.0, 4.0)), 5.0, Color32::from_rgba_unmultiplied(90, 60, 10, 190));
        painter.text(at, Align2::CENTER_CENTER, &line, FontId::proportional(14.0), Color32::from_rgb(255, 214, 120));
    }
    // Crosshair.
    painter.circle_filled(screen.center(), 2.0, Color32::from_white_alpha(140));
    // Whoever you're looking at.
    if let Some((name, about)) = &info.person {
        let c = screen.center() + egui::vec2(0.0, 34.0);
        let a = painter.text(c, Align2::CENTER_CENTER, name, FontId::proportional(18.0), Color32::TRANSPARENT);
        let b = painter.text(c + egui::vec2(0.0, 20.0), Align2::CENTER_CENTER, about, FontId::proportional(13.0), Color32::TRANSPARENT);
        painter.rect_filled(a.union(b).expand2(egui::vec2(10.0, 4.0)), 5.0, Color32::from_black_alpha(150));
        painter.text(c, Align2::CENTER_CENTER, name, FontId::proportional(18.0), Color32::from_rgb(240, 232, 212));
        painter.text(c + egui::vec2(0.0, 20.0), Align2::CENTER_CENTER, about, FontId::proportional(13.0), Color32::from_rgb(180, 172, 156));
    }

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
                line(ui, "Or press P and watch the courier do the rounds.");
                line(ui, "6. To know the city: walk its lanes (M shows your notebook) and make deliveries by memory, with G memory mode on and the notebook shut. Then Y moves you on a few years, and the city grows.");
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
                if let Some(code) = &s.course {
                    ui.colored_label(Color32::from_rgb(150, 200, 255), egui::RichText::new(format!("Course {code}")).small().monospace());
                }
                if let (Some((cov, clean, knows, next)), None) = (progress, &s.course) {
                    let line = if knows {
                        match next {
                            Some(n) => format!("You know this city. Y: move on to {n}"),
                            None => "You know the city as it was at the end.".into(),
                        }
                    } else {
                        format!("Lanes walked {:.0}% / {:.0}% · By memory {clean}/{}", cov * 100.0, game::KNOW_COVERAGE * 100.0, game::KNOW_CLEAN)
                    };
                    ui.colored_label(if knows { Color32::from_rgb(255, 215, 120) } else { Color32::from_rgb(170, 165, 150) }, egui::RichText::new(line).small());
                }
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
            egui::RichText::new(format!("{} · torch {} · {fps:.0} fps", s.keys_line, if s.torch { "on" } else { "off" }))
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

const SAVE_FILE: &str = "save.json";
/// The simulation tick: 120 per second, whatever the frame rate.
pub const TICK: f32 = 1.0 / 120.0;

/// Turns frame time into whole ticks, carrying the remainder.
#[derive(Default)]
struct Clock {
    acc: f64,
}

impl Clock {
    /// Ticks to run for a frame of `dt` seconds.
    fn advance(&mut self, dt: f32) -> u32 {
        self.acc += dt as f64;
        let n = (self.acc / TICK as f64).floor();
        self.acc -= n * TICK as f64;
        // Far behind (a hitch): drop the backlog rather than spiral.
        n.min(12.0) as u32
    }

    /// How far the frame being drawn is between the last tick and the next.
    fn alpha(&self) -> f32 {
        (self.acc / TICK as f64) as f32
    }
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
    let walk: Option<Walk>;
    let people_mesh: Option<GpuMesh>;
    let plane_mesh = arg::<f32>(args, "--plane").and_then(|t| plane::mesh(t, citymesh::centre(&world.city))).and_then(|m| GpuMesh::upload(&gpu.device, &m));
    let mut meshes: Vec<&GpuMesh> = world.mesh.iter().chain(plane_mesh.iter()).collect();
    let params = if args.iter().any(|a| a == "--walk") {
        let signs = signtext::SignText::new();
        walk = Some(Walk::new(&gpu, &mut renderer, &signs, &world.city, &world.society, year));
        let wk = walk.as_ref().unwrap();
        let mut p = player::Player::new(wk.world.spawn, wk.world.spawn_yaw);
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
        meshes.extend(wk.interior.iter());
        let cam = p.camera();
        let look = (cam.target - cam.eye).normalize();
        let mut pm = wk.crowd.mesh(cam.eye, look, night, 0.0);
        // `--ghost`: a ghost courier a few steps ahead, mid-stride (to check how it looks).
        if args.iter().any(|a| a == "--ghost") {
            let at = Vec3::new(cam.eye.x + look.x * 3.5, p.pos.y, cam.eye.z + look.z * 3.5);
            people::ghost_mesh(&mut pm, at, p.yaw + 0.4, 0.4, true, 0.0);
        }
        people_mesh = GpuMesh::upload(&gpu.device, &pm);
        meshes.extend(people_mesh.iter());
        walk_params(&cam, gpu.aspect(), night, args.iter().any(|a| a == "--torch"))
    } else {
        walk = None;
        let mut orbit = default_orbit(&world.city);
        if let Some(v) = arg::<String>(args, "--view").map(|v| floats(&v)) {
            if v.len() == 3 {
                (orbit.yaw, orbit.pitch, orbit.dist) = (v[0], v[1], v[2]);
            }
        }
        lighting::params(&orbit.camera(), gpu.aspect(), night)
    };
    let texts: Vec<&TextMesh> = walk.as_ref().map_or(vec![], |w| w.text.iter().collect());
    // `--bench N`: time N frames of this view, part by part.
    if let Some(n) = arg::<u32>(args, "--bench") {
        let (mut t_stats, mut t_people, mut t_gpu, mut t_look) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let look = (params.view_proj.inverse() * glam::Vec4::new(0.0, 0.0, 1.0, 1.0)).truncate().normalize();
        for i in 0..n {
            let t = Instant::now();
            std::hint::black_box(stats::measure(&world.city, year));
            t_stats += t.elapsed().as_secs_f64();
            let t = Instant::now();
            let pm = walk.as_ref().and_then(|w| GpuMesh::upload(&gpu.device, &w.crowd.mesh(params.cam_pos, look, night, i as f32 / 60.0)));
            t_people += t.elapsed().as_secs_f64();
            let t = Instant::now();
            if let Some(w) = walk.as_ref() {
                std::hint::black_box(w.crowd.looking_at(params.cam_pos, look, night, &w.world));
            }
            t_look += t.elapsed().as_secs_f64();
            let mut ms: Vec<&GpuMesh> = meshes.clone();
            ms.extend(pm.iter());
            let t = Instant::now();
            std::hint::black_box(renderer.capture_with_text(&gpu, &ms, &texts, &params));
            t_gpu += t.elapsed().as_secs_f64();
        }
        let per = |x: f64| x * 1000.0 / n as f64;
        let verts: u64 = meshes.iter().map(|m| m.count as u64).sum();
        println!(
            "per frame: stats {:.2} ms, people {:.2} ms, look-at {:.2} ms, render+readback {:.2} ms ({} meshes, {} indices)",
            per(t_stats), per(t_people), per(t_look), per(t_gpu), meshes.len(), verts
        );
        return;
    }
    let (w, h, px) = renderer.capture_with_text(&gpu, &meshes, &texts, &params);
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
    // A course (`--code KWC-1965-7F3A`, `--daily`, `--random`): its own city,
    // era and delivery list, always from the start.
    let course = if let Some(c) = arg::<String>(&args, "--code") {
        match course::Course::parse(&c) {
            Some(c) => Some(c),
            None => {
                eprintln!("Not a course code: {c} (they look like KWC-1965-7F3A)");
                return;
            }
        }
    } else if args.iter().any(|a| a == "--daily") {
        Some(course::Course::daily())
    } else if args.iter().any(|a| a == "--random") {
        Some(course::Course::random(arg::<u16>(&args, "--era").filter(|e| game::ERAS.contains(e)).unwrap_or(START_YEAR), args.iter().any(|a| a == "--wild")))
    } else {
        None
    };
    if let Some(c) = course {
        println!("Course {}", c.code());
    }
    let seed = course.map_or(course::CITY, |c| c.city_seed());
    let world = World::new(seed);
    let orbit = default_orbit(&world.city);
    // Default: the delivery game, on foot in 1950. `--walk`: roam 1987 freely.
    // `--free`: the growth overview.
    let free_walk = args.iter().any(|a| a == "--walk");
    let overview = args.iter().any(|a| a == "--free");
    let walk_now = !overview && !args.iter().skip(1).all(|a| !a.starts_with("--"));
    // `--era 1970`: try a later era, with its own save so the real one is untouched.
    let era = course.map(|c| c.era).or_else(|| arg::<u16>(&args, "--era").map(|y| game::ERAS.iter().copied().filter(|&e| e <= y.max(START_YEAR)).last().unwrap_or(START_YEAR)));
    let save_file = match (course, era) {
        (Some(c), _) => format!("save-{}.json", c.code()),
        (None, Some(e)) => format!("save-{e}.json"),
        _ => SAVE_FILE.to_string(),
    };
    // Carry on from the save unless asked for a fresh start (--new); a
    // course always starts from the beginning.
    let saved = (!args.iter().any(|a| a == "--new") && course.is_none())
        .then(|| std::fs::read_to_string(&save_file).ok())
        .flatten()
        .and_then(|s| serde_json::from_str::<game::Save>(&s).ok());
    // Plain launch: the title screen. Flags go straight in.
    let title = !args.iter().skip(1).any(|a| a.starts_with("--"));
    let game = (!free_walk && !overview && !title).then(|| match saved {
        Some(s) => game::Game::load(s),
        None => game::Game::new(seed, era.unwrap_or(START_YEAR)),
    });
    let start_year = game.as_ref().map_or(START_YEAR, |g| g.year);
    let mut app = App {
        run: None,
        world,
        settings: Settings {
            seed,
            year: if free_walk { END_YEAR as f32 } else { start_year as f32 },
            playing: !walk_now,
            speed: 2.0,
            mode: ColourMode::Grime,
            night: false,
            torch: true,
            want_walk: walk_now,
            help: true,
            memory: false,
            map_open: false,
            ledger_open: false,
            ledger_pick: None,
            demo_status: None,
            vsync: true,
            course: None,
            title_code: String::new(),
            memory_run: false,
            settings_open: false,
            rebinding: None,
            keys_line: String::new(),
        },
        orbit,
        walk: None,
        parked: None,
        game,
        signs: signtext::SignText::new(),
        pending_era: None,
        autopilot: args.iter().any(|a| a == "--demo").then(autopilot::Autopilot::default),
        save_file,
        title,
        save_summary: campaign_summary(),
        race: None,
        records: runs::Records::load(),
        prefs: prefs::Prefs::load().0,
        started: Instant::now(),
        plane: None,
        people: None,
        keys: HashSet::new(),
        drag: None,
        last_cursor: None,
        last_frame: Instant::now(),
        clock: Clock::default(),
        fps: 60.0,
        frames: 0,
    };
    // A course from the command line is a race (`--memory` for the Memory category).
    if let Some(c) = course {
        let cat = if args.iter().any(|a| a == "--memory") { runs::Category::Memory } else { runs::Category::Rounds };
        app.start_run(c, cat);
    }
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

    /// Z-fighting detector: coplanar faces pointing the same way that overlap.
    /// Checks a sample of whole buildings, inside and out.
    #[test]
    fn no_visible_z_fighting() {
        // 1987: a few tall buildings' stair cores. 1950: the squatter huts.
        let hits = z_fight_hits(END_YEAR, &|p: &kwc_sim::Plot| p.final_height() >= 8) + z_fight_hits(START_YEAR, &|p: &kwc_sim::Plot| p.height_at(START_YEAR) == 0);
        assert!(hits == 0, "{hits} visible z-fighting overlaps");
    }

    fn z_fight_hits(year: u16, pick: &dyn Fn(&kwc_sim::Plot) -> bool) -> usize {
        use std::collections::HashMap;
        let city = generate(&Params::default());
        let (walk, mut mesh) = world::build(&city, year);
        // Inside and outside meet at walls: check both meshes together.
        mesh.append(&citymesh::build(&city, year, citymesh::ColourMode::Grime));
        let areas: Vec<(Vec3, Vec3)> = city
            .plots
            .iter()
            .filter(|p| pick(p))
            .take(6)
            .map(|p| {
                let xs = p.cells.iter().map(|c| c.0 as f32 * CELL_M);
                let zs = p.cells.iter().map(|c| c.1 as f32 * CELL_M);
                let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min) - 0.1, xs.fold(f32::MIN, f32::max) + CELL_M + 0.1);
                let (z0, z1) = (zs.clone().fold(f32::MAX, f32::min) - 0.1, zs.fold(f32::MIN, f32::max) + CELL_M + 0.1);
                (Vec3::new(x0, -1.0, z0), Vec3::new(x1, 60.0, z1))
            })
            .collect();
        let inside = |p: Vec3| areas.iter().any(|(a, b)| p.cmpge(*a).all() && p.cmple(*b).all());
        // Bucket axis-aligned quads by (axis, facing, plane offset).
        let mut buckets: HashMap<(usize, bool, i64), Vec<([f32; 2], [f32; 2], [f32; 3])>> = HashMap::new();
        for q in mesh.vertices.chunks_exact(4) {
            let n = Vec3::from(q[0].normal);
            let Some(axis) = (0..3).find(|&k| n[k].abs() > 0.99) else { continue };
            let pts: Vec<Vec3> = q.iter().map(|v| Vec3::from(v.pos)).collect();
            let c = (pts[0] + pts[2]) * 0.5;
            if !inside(c) {
                continue;
            }
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let lo = [pts.iter().map(|p| p[a]).fold(f32::MAX, f32::min), pts.iter().map(|p| p[b]).fold(f32::MAX, f32::min)];
            let hi = [pts.iter().map(|p| p[a]).fold(f32::MIN, f32::max), pts.iter().map(|p| p[b]).fold(f32::MIN, f32::max)];
            let key = (axis, n[axis] > 0.0, (pts[0][axis] * 1000.0).round() as i64);
            buckets.entry(key).or_default().push((lo, hi, q[0].color));
        }
        let mut hits = vec![];
        for (key, qs) in &buckets {
            for i in 0..qs.len() {
                for j in i + 1..qs.len() {
                    let (a, b) = (&qs[i], &qs[j]);
                    let w = a.1[0].min(b.1[0]) - a.0[0].max(b.0[0]);
                    let h = a.1[1].min(b.1[1]) - a.0[1].max(b.0[1]);
                    if w > 0.01 && h > 0.01 && a.2 != b.2 {
                        // Only surfaces you can see flicker: skip overlaps whose front
                        // side is buried inside something solid.
                        let (ax, bx) = ((key.0 + 1) % 3, (key.0 + 2) % 3);
                        let mut p = Vec3::ZERO;
                        p[key.0] = key.2 as f32 / 1000.0 + if key.1 { 0.01 } else { -0.01 };
                        p[ax] = (a.0[0].max(b.0[0]) + a.1[0].min(b.1[0])) / 2.0;
                        p[bx] = (a.0[1].max(b.0[1]) + a.1[1].min(b.1[1])) / 2.0;
                        if walk.blocked(&world::Aabb::new(p - Vec3::splat(0.002), p + Vec3::splat(0.002))) {
                            continue;
                        }
                        if std::env::var("ZDEBUG").is_ok() && hits.len() < 5 {
                            let c = ((p.x / CELL_M) as u16, (p.z / CELL_M) as u16);
                            eprintln!("  front {p:?} cell {c:?} {:?} | A {:?}-{:?} | B {:?}-{:?}", city.ground_at(c), a.0, a.1, b.0, b.1);
                        }
                        hits.push((*key, w * h, a.2, b.2));
                    }
                }
            }
        }
        for h in hits.iter().take(12) {
            eprintln!("coplanar overlap: axis {} +{} at {:.3} m, {:.3} m2, colours {:?} / {:?}", h.0 .0, h.0 .1, h.0 .2 as f32 / 1000.0, h.1, h.2, h.3);
        }
        hits.len()
    }

    /// People turn up in every era, busier as the city fills, and none of
    /// them stands inside a wall.
    #[test]
    fn people_stand_in_open_space() {
        let city = generate(&Params::default());
        let soc = kwc_sim::society::generate(&city);
        let mut counts = vec![];
        for year in [START_YEAR, 1965, END_YEAR] {
            let (w, _) = world::build(&city, year);
            let lamps = lights::collect(&city, year);
            let crowd = people::Crowd::new(&city, &soc, &w, year, &lamps);
            for p in crowd.points() {
                let q = world::Aabb::new(p + Vec3::new(-0.2, 0.05, -0.2), p + Vec3::new(0.2, 1.5, 0.2));
                assert!(!w.blocked(&q), "{year}: someone inside a wall at {p:?}");
            }
            counts.push(crowd.figures.len());
        }
        assert!(counts[0] > 200, "{counts:?}");
        assert!(counts[2] > counts[0], "1987 should be busier than 1950: {counts:?}");
    }

    /// The autopilot does real rounds, on foot, in an open 1950 and in the
    /// 1987 maze of stairs and corridors.
    #[test]
    fn autopilot_delivers() {
        let city = Arc::new(generate(&Params::default()));
        let soc = kwc_sim::society::generate(&city);
        for year in [START_YEAR, END_YEAR] {
            let w = Arc::new(world::build(&city, year).0);
            let spots = game::spots(&city, &soc, year);
            let mut g = game::Game::new(1987, year);
            let mut p = player::Player::new(w.spawn, w.spawn_yaw);
            let mut ap = autopilot::Autopilot::default();
            let (dt, mut t, mut lost) = (1.0 / 60.0, 0.0, 0);
            let wall = std::time::Instant::now();
            while g.delivered < 3 && t < 900.0 {
                if g.job.is_none() {
                    g.new_job(&city, &soc);
                }
                // Planning happens on another thread: game time stands still meanwhile.
                let planning = ap.planning();
                let (input, act) = ap.drive(&w, &city, year, &mut p, autopilot::target(&g, &spots), if planning { 0.0 } else { dt });
                if planning {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                } else {
                    p.update(&w, input, dt);
                    t += dt;
                }
                match act {
                    autopilot::Act::Knock => {
                        g.interact(&city, &soc, &spots, p.pos);
                        eprintln!("  {year} t {t:.0}s knock, delivered {} ({:.0?} wall)", g.delivered, wall.elapsed());
                    }
                    autopilot::Act::Lost => {
                        lost += 1;
                        if let Some(tg) = autopilot::target(&g, &spots) {
                            let c = ((tg.stand.x / CELL_M) as u16, (tg.stand.z / CELL_M) as u16);
                            let u = &city.units[tg.unit as usize];
                            eprintln!("    {}", autopilot::explain(&w, &city, year, p.pos, tg.stand));
                            eprintln!("    lost: from {:?} to {:?} cell {c:?} {:?} unit floor {} door {:?} picked {}", p.pos, tg.stand, city.ground_at(c), u.floor, u.door, tg.picked);
                        }
                        g.job = None;
                        eprintln!("  {year} t {t:.0}s lost ({:.0?} wall)", wall.elapsed());
                    }
                    autopilot::Act::Walk => {}
                }
            }
            eprintln!("{year}: {} deliveries in {t:.0} s, {lost} given up", g.delivered);
            assert!(g.delivered >= 3 && lost <= 1, "{year}: {} deliveries, {lost} lost, status {}", g.delivered, ap.status);
        }
    }

    /// The same inputs, tick by tick, give the same walk whatever the frame
    /// rate: 30 fps and 240 fps end in exactly the same place.
    #[test]
    fn fixed_tick_is_frame_rate_independent() {
        let city = generate(&Params::default());
        let (w, _) = world::build(&city, END_YEAR);
        let run = |fps: f32| {
            let mut p = player::Player::new(w.spawn, w.spawn_yaw);
            let mut clock = Clock::default();
            let mut tick = 0u32;
            while tick < 1200 {
                for _ in 0..clock.advance(1.0 / fps) {
                    // A scripted walk: forward, a turn, a run, a strafe.
                    let input = player::Input { forward: 1.0, strafe: if tick % 400 > 300 { 1.0 } else { 0.0 }, run: tick % 600 > 200, jump: false };
                    p.yaw += if tick % 300 < 60 { 0.01 } else { 0.0 };
                    p.update(&w, input, TICK);
                    tick += 1;
                    if tick == 1200 {
                        break;
                    }
                }
            }
            p.pos
        };
        let (slow, fast) = (run(30.0), run(240.0));
        assert_eq!(slow, fast, "30 fps and 240 fps disagree");
        assert!(slow.distance(w.spawn) > 5.0, "the walk went nowhere");
    }

    /// Space at a drum vaults you over it (and takes its fixed time); at open
    /// ground it's only a short hop.
    #[test]
    fn vault_over_clutter() {
        let city = generate(&Params::default());
        let (w, _) = world::build(&city, START_YEAR);
        let mut vaulted = 0;
        for piece in world::squatters(&city, START_YEAR).iter().filter(|p| p.solid && p.aabb.max.y < 1.0 && p.aabb.max.y > 0.4) {
            let b = piece.aabb;
            let z = (b.min.z + b.max.z) / 2.0;
            let start = Vec3::new(b.max.x + 0.3, 0.0, z);
            let beyond = Vec3::new(b.min.x - 0.9, 0.0, z);
            if w.blocked(&player::Player::aabb_at(start)) || w.blocked(&player::Player::aabb_at(beyond)) {
                continue;
            }
            let mut p = player::Player::new(start, std::f32::consts::PI);
            settle(&mut p, &w);
            p.update(&w, player::Input { jump: true, ..Default::default() }, TICK);
            if !p.is_vaulting() {
                continue;
            }
            let mut ticks = 1;
            while p.is_vaulting() && ticks < 240 {
                p.update(&w, player::Input::default(), TICK);
                ticks += 1;
            }
            // Always the vault's fixed time (0.55 s), and over to the far side.
            let secs = ticks as f32 * TICK;
            assert!((secs - 0.55).abs() < 0.02, "vault took {secs} s");
            assert!(p.pos.x < b.min.x, "vault ended short at {:?} (drum {:?})", p.pos, b);
            vaulted += 1;
            if vaulted >= 3 {
                break;
            }
        }
        assert!(vaulted >= 3, "only {vaulted} vaults over 1950 clutter");
    }

    /// A course code fixes the whole course: the same deliveries in the same
    /// order every time, and a different seed gives a different city and list.
    #[test]
    fn same_code_same_course() {
        let jobs = |code: &str| {
            let c = course::Course::parse(code).unwrap();
            let city = generate(&Params { seed: c.city_seed(), ..Default::default() });
            let soc = kwc_sim::society::generate(&city);
            let mut g = game::Game::new(c.seed as u64, c.era);
            (0..10)
                .map(|_| {
                    g.new_job(&city, &soc);
                    let j = g.job.take().unwrap();
                    (j.from, j.to, j.item)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(jobs("KWC-1965-7F3A"), jobs("kwc 1965 7f3a"));
        assert_ne!(jobs("KWC-1965-7F3A"), jobs("KWC-1965-0042"));
    }

    /// The fairness soak: the autopilot does rounds on many seeds and eras
    /// and reports every job it had to give up and every place it snagged.
    /// Slow; run with `cargo test --release -p kwc-app soak -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn soak() {
        let seeds: Vec<u64> = std::env::var("SOAK_SEEDS").ok().map_or(vec![1987, 17, 5488, 10959, 16430, 21901], |s| s.split(',').filter_map(|x| x.parse().ok()).collect());
        let (mut all_lost, mut all_stuck, mut all_done) = (0, 0, 0);
        for seed in seeds {
            let city = Arc::new(generate(&Params { seed, ..Default::default() }));
            let soc = kwc_sim::society::generate(&city);
            for year in [1950u16, 1960, 1970, 1987] {
                let w = Arc::new(world::build(&city, year).0);
                let spots = game::spots(&city, &soc, year);
                let mut g = game::Game::new(seed, year);
                let mut p = player::Player::new(w.spawn, w.spawn_yaw);
                let mut ap = autopilot::Autopilot::default();
                let (mut t, mut lost) = (0.0f32, 0);
                while g.delivered < 4 && t < 1200.0 {
                    if g.job.is_none() {
                        g.new_job(&city, &soc);
                    }
                    let planning = ap.planning();
                    let (input, act) = ap.drive(&w, &city, year, &mut p, autopilot::target(&g, &spots), if planning { 0.0 } else { TICK });
                    if planning {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    } else {
                        p.update(&w, input, TICK);
                        t += TICK;
                    }
                    match act {
                        autopilot::Act::Knock => {
                            g.interact(&city, &soc, &spots, p.pos);
                        }
                        autopilot::Act::Lost => {
                            lost += 1;
                            if let Some(tg) = autopilot::target(&g, &spots) {
                                eprintln!("    LOST seed {seed} {year}: from {:?} to {:?} :: {}", p.pos, tg.stand, autopilot::explain(&w, &city, year, p.pos, tg.stand));
                            }
                            g.job = None;
                        }
                        autopilot::Act::Walk => {}
                    }
                }
                for s in &ap.stuck_at {
                    let c = ((s.x / CELL_M) as u16, (s.z / CELL_M) as u16);
                    let what = if (c.0 as usize) < city.w && (c.1 as usize) < city.d { format!("{:?}/{:?}", city.ground_at(c), city.role_at(c)) } else { "outside".into() };
                    eprintln!("    snag seed {seed} {year} at {s:?} cell {c:?} {what}");
                }
                eprintln!("seed {seed} {year}: {} delivered in {t:.0} s, {lost} lost, {} snags", g.delivered, ap.stuck_at.len());
                all_lost += lost;
                all_stuck += ap.stuck_at.len();
                all_done += g.delivered;
            }
        }
        eprintln!("TOTAL: {all_done} delivered, {all_lost} lost, {all_stuck} snags");
    }

    /// A save from inside what is now a hut gets you out, not stuck.
    #[test]
    fn unstick_from_solid() {
        let city = generate(&Params::default());
        let (w, _) = world::build(&city, START_YEAR);
        let free = |p: Vec3| !w.blocked(&player::Player::aabb_at(p));
        let saved = Vec3::new(195.35, 0.0, 92.97);
        assert!(!free(saved), "the save spot is no longer inside anything");
        let mut p = player::Player::new(saved, 0.0);
        p.unstick(&w);
        assert!(free(p.pos), "still stuck at {:?}", p.pos);
        assert!(p.pos.distance(Vec3::new(195.35, 0.0, 92.97)) < 5.0);
    }

    /// Knowing an era needs both: enough lanes walked, and deliveries by memory.
    /// Moving on resets the memory count and blanks only what was rebuilt.
    #[test]
    fn era_progression() {
        let city = generate(&Params::default());
        let mut g = game::Game::new(1987, 1950);
        assert!(!g.knows_era(&city));
        for k in 0..city.w * city.d {
            if city.ground[k] == Ground::Alley {
                g.seen.insert(((k % city.w) as u16, (k / city.w) as u16));
            }
        }
        assert!(!g.knows_era(&city), "walking alone isn't enough");
        g.clean = game::KNOW_CLEAN;
        assert!(g.knows_era(&city));
        assert_eq!(g.next_era(), Some(1955));
        let before = g.seen.len();
        g.enter_era(&city, 1955);
        assert_eq!((g.year, g.clean), (1955, 0));
        assert!(g.seen.len() <= before && !g.seen.is_empty(), "lanes that didn't change stay in the notebook");
        // Saving and loading keeps it all.
        let back = game::Game::load(serde_json::from_str(&serde_json::to_string(&g.save()).unwrap()).unwrap());
        assert_eq!((back.year, back.seen.len()), (g.year, g.seen.len()));
    }
}
