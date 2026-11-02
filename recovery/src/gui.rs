//! The window: connect the board, choose the firmware, recover. The look follows the node and hub
//! web UIs (the same colour tokens, dark by default and light when the system is light).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};

use crate::device::{self, DeviceInfo, Installed, PortEntry};
use crate::failure::Failure;
use crate::image::{format_size, image_chip};
use crate::recover::{self, Event, Firmware, Job, Outcome, erase_required};
use crate::release::{BootIndex, DEFAULT_BASE_URL, Product, Release, chip_label};
use crate::signature;

// ------------------------------------------------------------------------------------ theme

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    surface: Color32,
    surface2: Color32,
    border: Color32,
    border_strong: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    ok: Color32,
    warn: Color32,
    danger: Color32,
    info: Color32,
}

const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const DARK: Palette = Palette {
    bg: rgb(0x0b0e13),
    surface: rgb(0x12161d),
    surface2: rgb(0x181d26),
    border: rgb(0x252c38),
    border_strong: rgb(0x343d4d),
    text: rgb(0xe7ecf3),
    muted: rgb(0x8a94a6),
    accent: rgb(0xff3b5c),
    ok: rgb(0x2fd08a),
    warn: rgb(0xffb547),
    danger: rgb(0xff6b57),
    info: rgb(0x5aa9ff),
};

const LIGHT: Palette = Palette {
    bg: rgb(0xf5f6f8),
    surface: rgb(0xffffff),
    surface2: rgb(0xeef0f4),
    border: rgb(0xdde1e8),
    border_strong: rgb(0xc6ccd6),
    text: rgb(0x101722),
    muted: rgb(0x5b6474),
    accent: rgb(0xe3263f),
    ok: rgb(0x16a36a),
    warn: rgb(0xc98300),
    danger: rgb(0xd9452f),
    info: rgb(0x2f7fe0),
};

fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

fn visuals(p: Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.surface2;
    v.code_bg_color = p.surface2;
    v.hyperlink_color = p.info;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    let radius = CornerRadius::same(8);
    let states = [
        (&mut v.widgets.noninteractive, p.surface, p.border),
        (&mut v.widgets.inactive, p.surface2, p.border),
        (&mut v.widgets.hovered, p.surface2, p.border_strong),
        (&mut v.widgets.active, p.surface2, p.muted),
        (&mut v.widgets.open, p.surface2, p.border_strong),
    ];
    for (w, fill, stroke) in states {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke = Stroke::new(1.0, p.text);
        w.corner_radius = radius;
    }
    v
}

fn apply_theme(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(LIGHT, false));
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(12.0, 6.0);
            style.spacing.interact_size.y = 30.0;
        });
    }
}

/// The brand mark, as on the web UIs: four infrared bars on a dark tile.
fn icon() -> egui::IconData {
    const SIZE: usize = 64;
    let bg = [0x12, 0x16, 0x1d, 0xff];
    let bar = [0xff, 0x3b, 0x5c, 0xff];
    // the 32-unit favicon, scaled by 2
    let bars = [
        (10.0, 18.0, 14.0, 28.0),
        (30.0, 18.0, 4.8, 28.0),
        (39.2, 18.0, 4.8, 28.0),
        (50.0, 18.0, 4.8, 28.0),
    ];
    let radius = 16.0f32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = fx.clamp(radius, SIZE as f32 - radius);
            let cy = fy.clamp(radius, SIZE as f32 - radius);
            let inside = (fx - cx).powi(2) + (fy - cy).powi(2) <= radius * radius;
            let on_bar = bars
                .iter()
                .any(|(bx, by, bw, bh)| fx >= *bx && fx < bx + bw && fy >= *by && fy < by + bh);
            let px = if !inside {
                [0, 0, 0, 0]
            } else if on_bar {
                bar
            } else {
                bg
            };
            rgba[(y * SIZE + x) * 4..][..4].copy_from_slice(&px);
        }
    }
    egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

fn brand_mark(ui: &mut egui::Ui, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(8),
        p.surface2,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let unit = rect.width() / 32.0;
    for (x, w) in [(5.0, 7.0), (15.0, 2.4), (19.6, 2.4), (25.0, 2.4)] {
        let bar = egui::Rect::from_min_size(
            rect.min + Vec2::new(x * unit, 9.0 * unit),
            Vec2::new(w * unit, 14.0 * unit),
        );
        painter.rect_filled(bar, CornerRadius::same(1), p.accent);
    }
}

// ------------------------------------------------------------------------------------ widgets

fn card<R>(ui: &mut egui::Ui, p: Palette, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn step_title(ui: &mut egui::Ui, p: Palette, number: u8, title: &str, active: bool) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
        let (fill, text) = if active {
            (p.accent, Color32::WHITE)
        } else {
            (p.surface2, p.muted)
        };
        ui.painter().circle_filled(rect.center(), 11.0, fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            egui::FontId::proportional(12.0),
            text,
        );
        ui.label(RichText::new(title).size(16.0).strong().color(if active {
            p.text
        } else {
            p.muted
        }));
    });
}

#[derive(Clone, Copy)]
enum Tone {
    Ok,
    Info,
    Warn,
    Danger,
}

fn callout(ui: &mut egui::Ui, p: Palette, tone: Tone, title: &str, body: Option<&str>) {
    let color = match tone {
        Tone::Ok => p.ok,
        Tone::Info => p.info,
        Tone::Warn => p.warn,
        Tone::Danger => p.danger,
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).color(p.text).strong());
            if let Some(body) = body {
                ui.label(RichText::new(body).color(p.muted));
            }
        });
}

fn primary_button(ui: &mut egui::Ui, p: Palette, text: &str, enabled: bool) -> egui::Response {
    let (fill, ink, stroke) = if enabled {
        (p.accent, Color32::WHITE, Stroke::NONE)
    } else {
        (p.surface2, p.muted, Stroke::new(1.0, p.border))
    };
    let button = egui::Button::new(RichText::new(text).color(ink).strong().size(15.0))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(160.0, 38.0));
    ui.add_enabled(enabled, button)
}

fn muted(ui: &mut egui::Ui, p: Palette, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(p.muted));
}

// ------------------------------------------------------------------------------------ state

enum Msg {
    Release(Result<Release, Failure>),
    Probed(Result<DeviceInfo, Failure>),
    Job(Event),
    JobDone(Result<Outcome, Failure>),
}

enum ReleaseState {
    Loading,
    Ready(Release),
    Offline(Failure),
}

enum DeviceState {
    None,
    Probing,
    Ready(DeviceInfo),
    Failed(Failure),
}

enum Work {
    Idle,
    Running { step: String, progress: f32 },
    Done(Outcome),
    Failed(Failure),
}

struct LocalFile {
    path: PathBuf,
    data: Vec<u8>,
}

struct App {
    ctx: egui::Context,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    base_url: String,
    release: ReleaseState,
    ports: Vec<PortEntry>,
    ports_checked: Option<Instant>,
    port: Option<String>,
    port_missing_since: Option<Instant>,
    device: DeviceState,
    product: Option<Product>,
    local: Option<LocalFile>,
    local_error: Option<String>,
    erase: bool,
    allow_unsigned: bool,
    work: Work,
    log: Vec<String>,
}

pub fn run() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("IR Blaster Recovery")
            .with_app_id("irblaster-recovery")
            .with_inner_size([600.0, 760.0])
            .with_min_inner_size([480.0, 560.0])
            .with_icon(Arc::new(icon())),
        ..Default::default()
    };
    // A missing system library (e.g. libxkbcommon-x11 on a minimal Linux) panics inside winit:
    // catch it so the user gets a message instead of a silent exit.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "IR Blaster Recovery",
            options,
            Box::new(|cc| Ok(Box::new(App::new(cc)))),
        )
    }));
    let error = match result {
        Ok(Ok(())) => return ExitCode::SUCCESS,
        Ok(Err(err)) => err.to_string(),
        Err(panic) => panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown error".into()),
    };
    let text = format!(
        "The window could not open ({error}).\n\nThe command-line mode still works: run irblaster-recovery --help in a terminal."
    );
    eprintln!("{text}");
    let _ = rfd::MessageDialog::new()
        .set_title("IR Blaster Recovery")
        .set_description(text)
        .set_level(rfd::MessageLevel::Error)
        .show();
    ExitCode::FAILURE
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        // follows the system; IRBLASTER_RECOVERY_THEME=light|dark overrides it
        match std::env::var("IRBLASTER_RECOVERY_THEME").as_deref() {
            Ok("light") => cc.egui_ctx.set_theme(egui::Theme::Light),
            Ok("dark") => cc.egui_ctx.set_theme(egui::Theme::Dark),
            _ => {}
        }
        let (tx, rx) = channel();
        let base_url =
            std::env::var("IRBLASTER_RECOVERY_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            tx,
            rx,
            base_url,
            release: ReleaseState::Loading,
            ports: Vec::new(),
            ports_checked: None,
            port: None,
            port_missing_since: None,
            device: DeviceState::None,
            product: None,
            local: None,
            local_error: None,
            erase: false,
            allow_unsigned: false,
            work: Work::Idle,
            log: Vec::new(),
        };
        app.fetch_release();
        app
    }

    fn spawn(&self, job: impl FnOnce(Sender<Msg>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            job(tx);
            ctx.request_repaint();
        });
    }

    fn fetch_release(&mut self) {
        self.release = ReleaseState::Loading;
        let base = self.base_url.clone();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Release(Release::fetch(&base)));
        });
    }

    fn boot_index(&self) -> BootIndex {
        match &self.release {
            ReleaseState::Ready(release) => release.boot.clone(),
            _ => BootIndex::embedded(),
        }
    }

    fn release(&self) -> Option<&Release> {
        match &self.release {
            ReleaseState::Ready(release) => Some(release),
            _ => None,
        }
    }

    fn busy(&self) -> bool {
        matches!(self.device, DeviceState::Probing) || matches!(self.work, Work::Running { .. })
    }

    fn probe(&mut self) {
        let Some(port) = self.port.clone() else {
            return;
        };
        self.device = DeviceState::Probing;
        self.work = Work::Idle;
        let boot = self.boot_index();
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Probed(device::probe(&port, false, &boot)));
        });
    }

    fn start(&mut self) {
        let DeviceState::Ready(info) = &self.device else {
            return;
        };
        let Some(product) = self.product else { return };
        let firmware = match (&self.local, self.release()) {
            (Some(local), _) => Firmware::File {
                path: local.path.clone(),
                data: local.data.clone(),
            },
            (None, Some(release)) => match release.manifest.build(product, &info.chip) {
                Some(build) => Firmware::Published(build.clone()),
                None => return,
            },
            (None, None) => return,
        };
        let job = Job {
            port: info.port.clone(),
            chip: info.chip.clone(),
            product,
            firmware,
            erase: self.erase || self.erase_forced(),
            allow_unsigned: self.allow_unsigned,
            no_reset: false,
            release: self.release().cloned(),
        };
        self.log.clear();
        self.work = Work::Running {
            step: "Starting…".into(),
            progress: 0.0,
        };
        self.spawn(move |tx| {
            let events = tx.clone();
            let result = recover::run(&job, &mut |event| {
                let _ = events.send(Msg::Job(event));
            });
            let _ = tx.send(Msg::JobDone(result));
        });
    }

    fn erase_forced(&self) -> bool {
        let (DeviceState::Ready(info), Some(product)) = (&self.device, self.product) else {
            return false;
        };
        let boot = self.boot_index();
        match boot.target(product, &info.chip) {
            Some(target) => erase_required(
                info.table_sha256.as_deref(),
                target,
                info.installed == Installed::Blank,
            ),
            None => false,
        }
    }

    /// Products this board can be recovered as.
    fn products(&self, chip: &str) -> Vec<Product> {
        match self.release() {
            Some(release) if self.local.is_none() => release.products_for(chip),
            _ => {
                let boot = self.boot_index();
                Product::ALL
                    .into_iter()
                    .filter(|p| boot.target(*p, chip).is_some())
                    .collect()
            }
        }
    }

    fn set_local_file(&mut self, path: PathBuf, data: Vec<u8>) {
        self.local_error = None;
        if image_chip(&data).is_none() {
            self.local_error = Some(format!("{} isn't ESP32 firmware.", path.display()));
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.contains("hub") {
            self.product = Some(Product::Hub);
        } else if name.starts_with("irblaster-") {
            self.product = Some(Product::Node);
        }
        self.allow_unsigned = false;
        self.local = Some(LocalFile { path, data });
    }

    fn pick_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Firmware", &["bin"])
            .set_title("Choose a firmware file")
            .pick_file()
        {
            match std::fs::read(&path) {
                Ok(data) => self.set_local_file(path, data),
                Err(e) => self.local_error = Some(format!("Can't read {}: {e}", path.display())),
            }
        }
    }

    fn reset(&mut self) {
        self.device = DeviceState::None;
        self.work = Work::Idle;
        self.erase = false;
        self.log.clear();
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Release(Ok(release)) => self.release = ReleaseState::Ready(release),
                Msg::Release(Err(failure)) => self.release = ReleaseState::Offline(failure),
                Msg::Probed(Ok(info)) => {
                    let products = self.products(&info.chip);
                    self.product = match info.installed {
                        Installed::Product(p) if products.contains(&p) => Some(p),
                        _ => self
                            .product
                            .filter(|p| products.contains(p))
                            .or_else(|| products.first().copied()),
                    };
                    self.device = DeviceState::Ready(info);
                }
                Msg::Probed(Err(failure)) => self.device = DeviceState::Failed(failure),
                Msg::Job(Event::Step(text)) => {
                    if let Work::Running { step, .. } = &mut self.work {
                        *step = text;
                    }
                }
                Msg::Job(Event::Progress(value)) => {
                    if let Work::Running { progress, .. } = &mut self.work {
                        *progress = value;
                    }
                }
                Msg::Job(Event::Log(line)) => self.log.push(line),
                Msg::JobDone(Ok(outcome)) => self.work = Work::Done(outcome),
                Msg::JobDone(Err(failure)) => {
                    self.log.push(format!("Failed: {failure}"));
                    self.work = Work::Failed(failure);
                }
            }
        }
        let due = self
            .ports_checked
            .is_none_or(|t| t.elapsed() > Duration::from_millis(1200));
        if due && !self.busy() {
            self.ports = device::list_ports();
            self.ports_checked = Some(Instant::now());
            let present = self
                .port
                .as_ref()
                .is_some_and(|p| self.ports.iter().any(|e| &e.name == p));
            if present {
                self.port_missing_since = None;
            } else if self.port.is_some() {
                // Boards with USB built into the chip (ESP32-C3/S3) drop off the bus for a moment
                // whenever they restart, including after Connect: only forget the board after a while.
                let since = *self.port_missing_since.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_secs(5) {
                    self.port = None;
                    self.port_missing_since = None;
                    if !matches!(self.work, Work::Done(_)) {
                        self.device = DeviceState::None;
                    }
                }
            }
            if self.port.is_none() {
                let recognised: Vec<_> = self.ports.iter().filter(|p| p.recognised).collect();
                self.port = match recognised.as_slice() {
                    [only] => Some(only.name.clone()),
                    _ if self.ports.len() == 1 => Some(self.ports[0].name.clone()),
                    _ => None,
                };
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if self.busy() {
                break;
            }
            let path = file.path().to_path_buf();
            match file.bytes() {
                Ok(data) => self.set_local_file(path, data),
                Err(e) => self.local_error = Some(format!("Can't read {}: {e}", path.display())),
            }
        }
    }
}

// ------------------------------------------------------------------------------------ drawing

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.handle_drops(&ctx);
        let p = palette(ui);

        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(20, 12))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| self.header(ui, p));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.connect_card(ui, p);
                        ui.add_space(12.0);
                        self.firmware_card(ui, p);
                        ui.add_space(12.0);
                        self.recover_card(ui, p);
                        ui.add_space(12.0);
                        self.footer(ui, p);
                    });
            });

        let delay = if self.busy() {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(1200)
        };
        ctx.request_repaint_after(delay);
    }
}

impl App {
    fn header(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal(|ui| {
            brand_mark(ui, p);
            ui.label(
                RichText::new("IR Blaster Recovery")
                    .size(17.0)
                    .strong()
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (dot, text) = match &self.release {
                    ReleaseState::Loading => (p.warn, "Checking for firmware…".to_owned()),
                    ReleaseState::Ready(release) => {
                        let latest = release
                            .manifest
                            .nodes
                            .values()
                            .chain(release.manifest.hubs.values())
                            .map(|b| b.version.clone())
                            .max_by(|a, b| version_key(a).cmp(&version_key(b)));
                        (
                            p.ok,
                            latest
                                .map(|v| format!("Latest {v}"))
                                .unwrap_or_else(|| "No firmware published".into()),
                        )
                    }
                    ReleaseState::Offline(_) => (p.danger, "Offline".to_owned()),
                };
                if matches!(self.release, ReleaseState::Offline(_))
                    && ui.small_button("Retry").clicked()
                {
                    self.fetch_release();
                }
                ui.label(RichText::new(text).color(p.muted).size(13.0));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
            });
        });
    }

    fn connect_card(&mut self, ui: &mut egui::Ui, p: Palette) {
        let busy = self.busy();
        card(ui, p, |ui| {
            step_title(ui, p, 1, "Connect the board", true);
            muted(
                ui,
                p,
                "Plug the node or hub into this computer with a USB data cable.",
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let selected = self
                    .port
                    .as_ref()
                    .map(|name| {
                        let desc = self
                            .ports
                            .iter()
                            .find(|e| &e.name == name)
                            .map(|e| e.description.clone())
                            .unwrap_or_default();
                        if desc.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}  ·  {desc}")
                        }
                    })
                    .unwrap_or_else(|| {
                        if self.ports.is_empty() {
                            "No board found".into()
                        } else {
                            "Choose a port".into()
                        }
                    });
                ui.add_enabled_ui(!busy && !self.ports.is_empty(), |ui| {
                    egui::ComboBox::from_id_salt("port")
                        .width(300.0)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for entry in &self.ports {
                                let label = format!("{}  ·  {}", entry.name, entry.description);
                                if ui
                                    .selectable_label(
                                        self.port.as_deref() == Some(entry.name.as_str()),
                                        label,
                                    )
                                    .clicked()
                                {
                                    self.port = Some(entry.name.clone());
                                    self.device = DeviceState::None;
                                    self.work = Work::Idle;
                                }
                            }
                        });
                });
                let label = if matches!(self.device, DeviceState::Ready(_)) {
                    "Check again"
                } else {
                    "Connect"
                };
                if ui
                    .add_enabled(!busy && self.port.is_some(), egui::Button::new(label))
                    .clicked()
                {
                    self.probe();
                }
            });

            match &self.device {
                DeviceState::None if self.ports.is_empty() => {
                    ui.add_space(4.0);
                    callout(
                        ui,
                        p,
                        Tone::Info,
                        "Waiting for a board…",
                        Some(
                            "Some cables only charge: use one that carries data. On Windows, boards with a CP210x or CH340 \
                             USB chip may need its driver; Windows Update usually installs it within a minute.",
                        ),
                    );
                }
                DeviceState::None => {}
                DeviceState::Probing => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        muted(ui, p, "Connecting… the board restarts while we look at it.");
                    });
                }
                DeviceState::Failed(failure) => {
                    ui.add_space(4.0);
                    callout(
                        ui,
                        p,
                        Tone::Danger,
                        &failure.message,
                        failure.hint.as_deref(),
                    );
                }
                DeviceState::Ready(info) => {
                    ui.add_space(4.0);
                    egui::Grid::new("facts")
                        .num_columns(2)
                        .spacing([24.0, 6.0])
                        .show(ui, |ui| {
                            let fact = |ui: &mut egui::Ui, key: &str, value: String| {
                                ui.label(RichText::new(key).color(p.muted));
                                ui.label(RichText::new(value).color(p.text));
                                ui.end_row();
                            };
                            fact(
                                ui,
                                "Chip",
                                format!(
                                    "{} {}",
                                    info.chip_label(),
                                    info.revision.clone().unwrap_or_default()
                                ),
                            );
                            fact(
                                ui,
                                "MAC address",
                                info.mac.clone().unwrap_or_else(|| "unknown".into()),
                            );
                            fact(ui, "Flash", info.flash_size.clone());
                            let installed = match info.installed {
                                Installed::Product(p) => p.label().to_owned(),
                                Installed::Blank => "Nothing (blank)".to_owned(),
                                Installed::Unknown => {
                                    "Other firmware or an older layout".to_owned()
                                }
                            };
                            fact(ui, "Installed", installed);
                        });
                }
            }
        });
    }

    fn firmware_card(&mut self, ui: &mut egui::Ui, p: Palette) {
        let ready = matches!(self.device, DeviceState::Ready(_))
            && !self.busy()
            && !matches!(self.work, Work::Done(_));
        let chip = match &self.device {
            DeviceState::Ready(info) => Some(info.chip.clone()),
            _ => None,
        };
        card(ui, p, |ui| {
            step_title(ui, p, 2, "Choose the firmware", ready);
            let Some(chip) = chip else {
                muted(ui, p, "Connect the board first.");
                return;
            };
            ui.add_enabled_ui(ready, |ui| {
                let products = self.products(&chip);
                if products.is_empty() {
                    callout(ui, p, Tone::Warn, &format!("No IR Blaster firmware is published for {}.", chip_label(&chip)), None);
                    return;
                }
                for product in products {
                    let version = match (&self.local, self.release()) {
                        (None, Some(release)) => release.manifest.build(product, &chip).map(|b| b.version.clone()),
                        _ => None,
                    };
                    let text = match version {
                        Some(v) => format!("{}   {v}", product.label()),
                        None => product.label().to_owned(),
                    };
                    ui.radio_value(&mut self.product, Some(product), text);
                }
                ui.add_space(4.0);
                let online = self.release().is_some();
                match &self.local {
                    Some(local) => {
                        let name = local.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let text = format!("File: {name} ({})", format_size(local.data.len()));
                        let mut use_published = false;
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(text).color(p.text));
                            use_published = online && ui.link("Use the published firmware instead").clicked();
                        });
                        if use_published {
                            self.local = None;
                        }
                    }
                    None => {
                        if let ReleaseState::Offline(failure) = &self.release {
                            callout(
                                ui,
                                p,
                                Tone::Warn,
                                "Offline: the published firmware can't be downloaded.",
                                Some(&format!("{}\nChoose a firmware file you downloaded earlier, or retry when online.", failure.message)),
                            );
                        }
                    }
                }
                if let Some(local) = &self.local
                    && !signature::trusted_keys().is_empty()
                    && !signature::is_signed(&local.data)
                {
                    ui.checkbox(&mut self.allow_unsigned, "This file isn't signed. I built it myself and want to flash it anyway.");
                }
                if let Some(error) = &self.local_error {
                    callout(ui, p, Tone::Danger, error, None);
                }
                ui.horizontal(|ui| {
                    if ui.button("Use a file on this computer…").clicked() {
                        self.pick_file();
                    }
                    muted(ui, p, "or drop a .bin file on this window");
                });
            });
        });
    }

    fn recover_card(&mut self, ui: &mut egui::Ui, p: Palette) {
        let device_ready = matches!(self.device, DeviceState::Ready(_));
        let firmware_ready = self.product.is_some()
            && (self.local.as_ref().is_some_and(|local| {
                signature::trusted_keys().is_empty()
                    || signature::is_signed(&local.data)
                    || self.allow_unsigned
            }) || (self.local.is_none() && self.release().is_some()));
        let active = device_ready && firmware_ready;
        let forced = self.erase_forced();
        card(ui, p, |ui| {
            step_title(
                ui,
                p,
                3,
                "Recover",
                active || !matches!(self.work, Work::Idle),
            );
            match &self.work {
                Work::Idle => {
                    ui.add_enabled_ui(active, |ui| {
                        if forced {
                            let mut on = true;
                            ui.add_enabled(false, egui::Checkbox::new(&mut on, "Erase everything first"));
                            muted(ui, p, "This board's storage layout differs from this firmware's, so its settings, WiFi details and learned buttons are erased.");
                        } else {
                            ui.checkbox(&mut self.erase, "Erase everything first");
                            muted(ui, p, "Removes settings, WiFi details and learned buttons. Use it if the board still doesn't start after a normal recovery.");
                        }
                    });
                    ui.add_space(6.0);
                    if primary_button(ui, p, "Recover", active).clicked() {
                        self.start();
                    }
                }
                Work::Running { step, progress } => {
                    ui.label(RichText::new(step.as_str()).color(p.text));
                    ui.add(
                        egui::ProgressBar::new(*progress)
                            .show_percentage()
                            .fill(p.accent),
                    );
                    muted(ui, p, "Keep the board plugged in until this finishes.");
                }
                Work::Done(outcome) => {
                    let version = outcome
                        .version
                        .as_deref()
                        .map(|v| format!(" {v}"))
                        .unwrap_or_default();
                    let body = if outcome.erased {
                        "It's restarting now. Its flash was erased, so set it up again as a new device."
                    } else {
                        "It's restarting now, with its settings kept."
                    };
                    callout(
                        ui,
                        p,
                        Tone::Ok,
                        &format!("{}{version} installed.", outcome.product.label()),
                        Some(body),
                    );
                    ui.add_space(4.0);
                    if ui.button("Recover another board").clicked() {
                        self.reset();
                    }
                }
                Work::Failed(failure) => {
                    callout(
                        ui,
                        p,
                        Tone::Danger,
                        &failure.message,
                        failure.hint.as_deref(),
                    );
                    ui.add_space(4.0);
                    if ui.button("Try again").clicked() {
                        self.work = Work::Idle;
                    }
                }
            }
            if !self.log.is_empty() {
                egui::CollapsingHeader::new(RichText::new("Details").color(p.muted)).show(
                    ui,
                    |ui| {
                        for line in &self.log {
                            ui.label(RichText::new(line).monospace().size(12.0).color(p.muted));
                        }
                    },
                );
            }
        });
    }

    fn footer(&self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .color(p.muted)
                    .size(12.0),
            );
            ui.label(RichText::new("·").color(p.muted).size(12.0));
            ui.hyperlink_to(
                RichText::new("Firmware downloads").size(12.0),
                &self.base_url,
            );
        });
    }
}

fn version_key(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}
