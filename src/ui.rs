//! The tuner window, styled after a vintage gold-on-charcoal VU meter.
//!
//! Layout: top bar (device, tuning selector, A4), a glowing meter window with
//! the cents needle, input level bars on both sides, and string keys below.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Mesh, Pos2, Rect,
    Response, Sense, Shape, Stroke, StrokeKind, Vec2,
};

use crate::audio::{self, Engine, Reading};
use crate::notes::{self, Note, TUNINGS};

/// Within this many cents counts as "in tune".
const IN_TUNE_CENTS: f32 = 3.0;
/// Keep showing the last note this long after the signal stops.
const HOLD: Duration = Duration::from_millis(1500);
/// Half the sweep of the meter scale, in degrees.
const SWEEP_DEG: f32 = 50.0;

// Palette.
const BACKDROP: Color32 = Color32::from_rgb(0x08, 0x08, 0x09);
const CHASSIS: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1f);
const CHASSIS_EDGE: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2f);
const RECESS: Color32 = Color32::from_rgb(0x11, 0x11, 0x12);
const SHADOW: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
const LABEL: Color32 = Color32::from_rgb(0x6e, 0x6e, 0x72);
const GOLD: Color32 = Color32::from_rgb(0xd8, 0x9a, 0x3a);
const GOLD_BRIGHT: Color32 = Color32::from_rgb(0xff, 0xc4, 0x55);
const GOLD_DIM: Color32 = Color32::from_rgb(0x7a, 0x58, 0x24);
const LCD: Color32 = Color32::from_rgb(0xec, 0xe0, 0xc4);
const ERROR: Color32 = Color32::from_rgb(0xf0, 0x6a, 0x5a);

fn oswald(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("oswald".into()))
}

fn lcd(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("lcd".into()))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "oswald".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Oswald-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "dseg14".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/DSEG14Classic-Regular.ttf"
        ))),
    );
    // Fall back to the default fonts for any glyph ours lack.
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    for (family, primary) in [("oswald", "oswald"), ("lcd", "dseg14")] {
        let mut chain = vec![primary.to_string()];
        chain.extend(fallback.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain);
    }
    // Use Oswald for regular widgets too (menus, tooltips).
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "oswald".into());
    ctx.set_fonts(fonts);
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Snap to the closest string of the selected tuning.
    Auto,
    /// Always tune toward one chosen string.
    String(usize),
    /// Show the nearest chromatic note.
    Chromatic,
}

pub struct TunerApp {
    reading: Arc<Mutex<Reading>>,
    engine: Option<Engine>,
    error: Option<String>,
    devices: Vec<String>,
    device: Option<String>,
    tuning: usize,
    a4: f32,
    mode: Mode,
    /// Needle position actually drawn (eased toward the measured value).
    needle: f32,
    /// Level bar position actually drawn (fast attack, slow release).
    level: f32,
}

impl TunerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKDROP;
        visuals.window_fill = CHASSIS;
        visuals.window_stroke = Stroke::new(1.0, CHASSIS_EDGE);
        visuals.selection.bg_fill = GOLD_DIM;
        visuals.selection.stroke = Stroke::new(1.0, GOLD_BRIGHT);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x26, 0x1e);
        cc.egui_ctx.set_visuals(visuals);

        let mut app = Self {
            reading: Arc::default(),
            engine: None,
            error: None,
            devices: audio::input_device_names(),
            device: audio::default_input_name(),
            tuning: 0,
            a4: 440.0,
            mode: Mode::Auto,
            needle: 0.0,
            level: 0.0,
        };
        app.restart_audio(&cc.egui_ctx);
        app
    }

    fn restart_audio(&mut self, ctx: &egui::Context) {
        self.engine = None; // stop the old stream first
        *self.reading.lock().unwrap() = Reading::default();
        let ctx = ctx.clone();
        match Engine::start(self.device.as_deref(), self.reading.clone(), move || {
            ctx.request_repaint()
        }) {
            Ok(engine) => {
                self.device = Some(engine.device_name.clone());
                self.engine = Some(engine);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn set_tuning(&mut self, i: usize) {
        if i != self.tuning {
            self.tuning = i;
            if let Mode::String(_) = self.mode {
                self.mode = Mode::Auto;
            }
        }
    }

    /// Top bar: logo, device menu, tuning selector, A4, rescan.
    fn top_bar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter_at(rect);
        let mid = rect.center().y;
        let mut restart = false;

        // Logo: two offset arcs.
        let logo = Pos2::new(rect.left() + 30.0, mid);
        arc_line(&p, logo, 13.0, 200f32, 470f32, Stroke::new(3.0, LABEL));
        arc_line(
            &p,
            logo + Vec2::new(3.0, 2.0),
            8.0,
            20f32,
            250f32,
            Stroke::new(2.5, LABEL),
        );

        // Input device button and its menu.
        let dev_rect =
            Rect::from_center_size(Pos2::new(rect.left() + 82.0, mid), Vec2::splat(34.0));
        let dev = icon_button(ui, dev_rect, "device", Icon::Mic).on_hover_text(format!(
            "Input: {}",
            self.device.as_deref().unwrap_or("none")
        ));
        egui::Popup::menu(&dev).width(260.0).show(|ui| {
            ui.label(egui::RichText::new("INPUT DEVICE").color(LABEL).size(12.0));
            for name in &self.devices {
                if ui
                    .selectable_label(self.device.as_ref() == Some(name), name)
                    .clicked()
                {
                    self.device = Some(name.clone());
                    restart = true;
                }
            }
            if self.devices.is_empty() {
                ui.label("No input devices found");
            }
        });

        // Rescan button.
        let scan_rect =
            Rect::from_center_size(Pos2::new(rect.right() - 30.0, mid), Vec2::splat(34.0));
        if icon_button(ui, scan_rect, "rescan", Icon::Rescan)
            .on_hover_text("Rescan input devices and restart audio")
            .clicked()
        {
            self.devices = audio::input_device_names();
            restart = true;
        }

        // A4 reference: drag or scroll to change, double-click to reset.
        let a4_rect =
            Rect::from_center_size(Pos2::new(rect.right() - 108.0, mid), Vec2::new(98.0, 34.0));
        let a4 = ui
            .interact(a4_rect, ui.scope_id().with("a4"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text("A4 reference: drag or scroll to adjust, double-click for 440 Hz");
        let mut delta = -a4.drag_delta().y * 0.1;
        if a4.hovered() {
            delta += ui.input(|i| i.smooth_scroll_delta.y) * 0.02;
        }
        self.a4 = (self.a4 + delta).clamp(415.0, 466.0);
        if a4.double_clicked() {
            self.a4 = 440.0;
        }
        recess(&p, a4_rect, 4.0);
        p.text(
            a4_rect.left_center() + Vec2::new(10.0, 0.0),
            Align2::LEFT_CENTER,
            "A4",
            oswald(13.0),
            GOLD_DIM,
        );
        lcd_text(
            &p,
            a4_rect.right_center() - Vec2::new(10.0, 0.0),
            Align2::RIGHT_CENTER,
            &format!("{:.1}", self.a4),
            lcd(15.0),
            if a4.hovered() || a4.dragged() {
                GOLD_BRIGHT
            } else {
                GOLD
            },
        );

        // Tuning selector ("preset menu").
        let left = dev_rect.right() + 16.0;
        let right = a4_rect.left() - 16.0;
        let menu = Rect::from_min_max(Pos2::new(left, mid - 19.0), Pos2::new(right, mid + 19.0));
        recess(&p, menu, 4.0);
        let n = TUNINGS.len();
        let prev = Rect::from_min_size(menu.min, Vec2::new(34.0, menu.height()));
        let next = Rect::from_min_size(
            Pos2::new(menu.right() - 34.0, menu.top()),
            Vec2::new(34.0, menu.height()),
        );
        for (r, dir) in [(prev, -1.0f32), (next, 1.0)] {
            let resp = ui.interact(
                r,
                ui.scope_id().with(("tuning-step", dir as i32)),
                Sense::click(),
            );
            let c = if resp.hovered() { GOLD_BRIGHT } else { GOLD };
            let tip = r.center() + Vec2::new(5.0 * dir, 0.0);
            let back = r.center() - Vec2::new(4.0 * dir, 0.0);
            p.add(Shape::convex_polygon(
                vec![tip, back + Vec2::new(0.0, -6.0), back + Vec2::new(0.0, 6.0)],
                c,
                Stroke::NONE,
            ));
            let sep_x = if dir < 0.0 { r.right() } else { r.left() };
            p.line_segment(
                [
                    Pos2::new(sep_x, r.top() + 9.0),
                    Pos2::new(sep_x, r.bottom() - 9.0),
                ],
                Stroke::new(2.0, GOLD_DIM),
            );
            if resp.clicked() {
                self.set_tuning(
                    (self.tuning as isize + dir as isize).rem_euclid(n as isize) as usize
                );
            }
        }
        let name_rect = Rect::from_min_max(
            Pos2::new(prev.right(), menu.top()),
            Pos2::new(next.left(), menu.bottom()),
        );
        let name = ui
            .interact(name_rect, ui.scope_id().with("tuning-name"), Sense::click())
            .on_hover_text("Choose tuning");
        p.text(
            name_rect.center(),
            Align2::CENTER_CENTER,
            TUNINGS[self.tuning].name.to_uppercase(),
            oswald(17.0),
            if name.hovered() { GOLD_BRIGHT } else { GOLD },
        );
        egui::Popup::menu(&name).show(|ui| {
            for (i, t) in TUNINGS.iter().enumerate() {
                if ui.selectable_label(i == self.tuning, t.name).clicked() {
                    self.set_tuning(i);
                }
            }
        });

        if restart {
            self.restart_audio(ui.ctx());
        }
    }

    /// Row of string keys with AUTO and CHROM at the ends.
    fn string_keys(&mut self, ui: &mut egui::Ui, rect: Rect, active: Option<usize>, in_tune: bool) {
        let p = ui.painter_at(rect.expand(12.0));
        let strings = TUNINGS[self.tuning].strings;
        let count = strings.len() + 2;
        let gap = 10.0;
        let key_w = ((rect.width() - gap * (count as f32 - 1.0)) / count as f32).min(96.0);
        let total = key_w * count as f32 + gap * (count as f32 - 1.0);
        let mut x = rect.center().x - total / 2.0;

        let mut key = |ui: &mut egui::Ui, id: &str, label: String, selected: bool, lit: bool| {
            let r = Rect::from_min_size(
                Pos2::new(x, rect.top() + 14.0),
                Vec2::new(key_w, rect.height() - 14.0),
            );
            x += key_w + gap;
            let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());

            // LED above the key.
            let led = Pos2::new(r.center().x, rect.top() + 5.0);
            if lit {
                glow(&p, led, 9.0, GOLD_BRIGHT, if in_tune { 0.5 } else { 0.25 });
                p.circle_filled(led, 3.0, GOLD_BRIGHT);
            } else {
                p.circle_filled(led, 3.0, RECESS);
            }

            // Key body: soft vertical gradient with a top highlight.
            let top = if resp.is_pointer_button_down_on() {
                0x18
            } else if resp.hovered() {
                0x2c
            } else {
                0x26
            };
            p.rect_filled(r.translate(Vec2::new(0.0, 3.0)), 8.0, SHADOW);
            p.add(vgradient(
                r,
                Color32::from_gray(top),
                Color32::from_gray(0x17),
                8.0,
            ));
            p.rect_stroke(
                r,
                8.0,
                Stroke::new(1.0, Color32::from_gray(0x0c)),
                StrokeKind::Outside,
            );
            p.line_segment(
                [
                    r.left_top() + Vec2::new(8.0, 1.0),
                    r.right_top() + Vec2::new(-8.0, 1.0),
                ],
                Stroke::new(1.0, Color32::from_gray(0x36)),
            );
            if selected {
                let bar = Rect::from_center_size(
                    Pos2::new(r.center().x, r.bottom() - 7.0),
                    Vec2::new(key_w * 0.4, 3.0),
                );
                p.rect_filled(bar, 1.5, GOLD);
            }
            let color = if lit {
                GOLD_BRIGHT
            } else if selected {
                GOLD
            } else {
                LABEL
            };
            p.text(
                r.center() - Vec2::new(0.0, 2.0),
                Align2::CENTER_CENTER,
                label,
                oswald(20.0),
                color,
            );
            resp
        };

        if key(ui, "auto", "AUTO".into(), self.mode == Mode::Auto, false)
            .on_hover_text("Snap to the closest string")
            .clicked()
        {
            self.mode = Mode::Auto;
        }
        for (i, n) in strings.iter().enumerate() {
            let selected = self.mode == Mode::String(i);
            let label = format!("{}{}", n.name(), n.octave());
            if key(
                ui,
                &format!("string{i}"),
                label,
                selected,
                active == Some(i),
            )
            .on_hover_text("Lock to this string")
            .clicked()
            {
                self.mode = if selected {
                    Mode::Auto
                } else {
                    Mode::String(i)
                };
            }
        }
        if key(
            ui,
            "chrom",
            "CHROM".into(),
            self.mode == Mode::Chromatic,
            false,
        )
        .on_hover_text("Chromatic: show the nearest note")
        .clicked()
        {
            self.mode = Mode::Chromatic;
        }
    }
}

impl eframe::App for TunerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let reading = self.reading.lock().unwrap().clone();
        let fresh = reading
            .updated
            .is_some_and(|t| t.elapsed() < HOLD)
            .then_some(reading.freq)
            .flatten();

        // Work out the target note and offset for the current mode.
        let tuning = &TUNINGS[self.tuning];
        let target: Option<(Note, f32, Option<usize>)> = fresh.map(|f| match self.mode {
            Mode::Chromatic => {
                let (note, c) = notes::nearest_note(f, self.a4);
                (note, c, None)
            }
            Mode::Auto | Mode::String(_) => {
                let i = match self.mode {
                    Mode::String(i) => i.min(tuning.strings.len() - 1),
                    _ => tuning.closest_string(f, self.a4),
                };
                let note = tuning.strings[i];
                (note, notes::cents(f, note.freq(self.a4)), Some(i))
            }
        });
        let in_tune = target.is_some_and(|(_, c, _)| c.abs() <= IN_TUNE_CENTS);

        // Ease the needle toward the measured value (frame-rate independent),
        // resting at the far left like a real meter when nothing is playing.
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let goal = target.map_or(-50.0, |(_, c, _)| c.clamp(-50.0, 50.0));
        let speed = if target.is_some() { 14.0 } else { 4.0 };
        self.needle += (goal - self.needle) * (1.0 - (-dt * speed).exp());

        // Level bars: jump up, fall back slowly.
        let level_goal = ((reading.level_db + 70.0) / 70.0).clamp(0.0, 1.0);
        self.level = if level_goal > self.level {
            level_goal
        } else {
            self.level + (level_goal - self.level) * (1.0 - (-dt * 6.0).exp())
        };

        let animating =
            (goal - self.needle).abs() > 0.05 || (level_goal - self.level).abs() > 0.002;
        if animating || reading.updated.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKDROP))
            .show(ui, |ui| {
                let full = ui.max_rect().shrink(14.0);
                let p = ui.painter().clone();

                // Chassis: top bar plus main body.
                let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), 58.0));
                let body = Rect::from_min_max(Pos2::new(full.left(), bar.bottom() + 6.0), full.max);
                for r in [bar, body] {
                    p.rect_filled(r.translate(Vec2::new(0.0, 4.0)), 6.0, SHADOW);
                    p.add(vgradient(r, Color32::from_gray(0x22), CHASSIS, 6.0));
                    p.rect_stroke(r, 6.0, Stroke::new(1.0, CHASSIS_EDGE), StrokeKind::Inside);
                }
                self.top_bar(ui, bar);

                // Side columns with dot grilles and level bars.
                let side_w = (body.width() * 0.08).clamp(44.0, 70.0);
                let inner = body.shrink2(Vec2::new(side_w, 0.0));
                for (col, label) in [
                    (
                        Rect::from_min_max(body.min, Pos2::new(inner.left(), body.bottom())),
                        "INPUT",
                    ),
                    (
                        Rect::from_min_max(Pos2::new(inner.right(), body.top()), body.max),
                        "LEVEL",
                    ),
                ] {
                    dots_row(&p, Pos2::new(col.center().x, col.top() + 14.0), 6);
                    dots_row(&p, Pos2::new(col.center().x, col.bottom() - 12.0), 6);
                    let bars = Rect::from_min_max(
                        Pos2::new(col.center().x - 8.0, col.top() + 34.0),
                        Pos2::new(col.center().x + 8.0, col.bottom() - 52.0),
                    );
                    level_bars(&p, bars, self.level, reading.level_db > -3.0);
                    p.text(
                        Pos2::new(col.center().x, col.bottom() - 34.0),
                        Align2::CENTER_CENTER,
                        label,
                        oswald(13.0),
                        LABEL,
                    );
                }

                // Meter window, divider and string keys.
                let keys_h = 74.0;
                let window = Rect::from_min_max(
                    Pos2::new(inner.left(), body.top() + 22.0),
                    Pos2::new(inner.right(), body.bottom() - keys_h - 40.0),
                );
                let divider_y = window.bottom() + 12.0;
                p.line_segment(
                    [
                        Pos2::new(window.left(), divider_y),
                        Pos2::new(window.right(), divider_y),
                    ],
                    Stroke::new(2.0, SHADOW),
                );
                p.line_segment(
                    [
                        Pos2::new(window.left(), divider_y + 2.0),
                        Pos2::new(window.right(), divider_y + 2.0),
                    ],
                    Stroke::new(1.0, CHASSIS_EDGE),
                );
                meter(
                    ui,
                    window,
                    self.needle,
                    target.map(|(n, c, _)| (n, c)),
                    fresh,
                    self.error.as_deref(),
                );

                let keys = Rect::from_min_max(
                    Pos2::new(inner.left() + 8.0, divider_y + 16.0),
                    Pos2::new(inner.right() - 8.0, divider_y + 16.0 + keys_h - 10.0),
                );
                self.string_keys(ui, keys, target.and_then(|t| t.2), in_tune);
            });
    }
}

/// The glowing meter window: cents scale, needle, note name and readouts.
fn meter(
    ui: &mut egui::Ui,
    rect: Rect,
    needle: f32,
    target: Option<(Note, f32)>,
    freq: Option<f32>,
    error: Option<&str>,
) {
    // Bezel and warm gradient background.
    let outer = ui.painter();
    outer.rect_filled(rect.expand(3.0), 12.0, SHADOW);
    outer.add(vgradient(
        rect,
        Color32::from_rgb(0x2e, 0x24, 0x18),
        Color32::from_rgb(0x07, 0x06, 0x05),
        10.0,
    ));
    let p = ui.painter_at(rect.shrink(1.0));

    let in_tune = target.is_some_and(|(_, c)| c.abs() <= IN_TUNE_CENTS);
    let live = target.is_some();

    // Pivot sits just below the window; the scale arcs above it.
    let radius = (rect.width() * 0.42).min(rect.height() * 0.78).max(80.0);
    let pivot = Pos2::new(rect.center().x, rect.bottom() + radius * 0.12);
    let scale_r = radius;
    let point = |cents: f32, r: f32| {
        let a = (cents / 50.0) * SWEEP_DEG.to_radians();
        pivot + Vec2::new(a.sin(), -a.cos()) * r
    };

    // Soft glow at the bottom, brighter when in tune.
    glow(
        &p,
        Pos2::new(rect.center().x, rect.bottom()),
        rect.width() * 0.35,
        GOLD,
        if in_tune { 0.22 } else { 0.10 },
    );

    let scale_col = if live { GOLD } else { GOLD_DIM };
    let thin = Stroke::new(1.5, scale_col);
    let arc = |from: f32, to: f32, r: f32, stroke: Stroke| {
        let steps = 64;
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| point(from + (to - from) * i as f32 / steps as f32, r))
            .collect();
        p.add(Shape::line(pts, stroke));
    };
    arc(-50.0, -IN_TUNE_CENTS, scale_r, thin);
    arc(IN_TUNE_CENTS, 50.0, scale_r, thin);
    // Thick band marks the in-tune zone.
    let band = if in_tune { GOLD_BRIGHT } else { scale_col };
    if in_tune {
        for w in [18.0, 12.0] {
            arc(
                -IN_TUNE_CENTS - 1.0,
                IN_TUNE_CENTS + 1.0,
                scale_r - 3.0,
                Stroke::new(w, GOLD_BRIGHT.gamma_multiply(0.12)),
            );
        }
    }
    arc(
        -IN_TUNE_CENTS,
        IN_TUNE_CENTS,
        scale_r - 3.0,
        Stroke::new(7.0, band),
    );
    // Inner fine scale, one tick per cent.
    for c in -50..=50 {
        let c = c as f32;
        let len = if c as i32 % 5 == 0 { 7.0 } else { 3.5 };
        p.line_segment(
            [point(c, scale_r - 16.0), point(c, scale_r - 16.0 - len)],
            Stroke::new(1.0, scale_col.gamma_multiply(0.45)),
        );
    }

    // Ticks and numbers every 10 cents (5 for minor ticks).
    let num_size = (radius * 0.075).clamp(13.0, 22.0);
    for c in (-50..=50).step_by(5) {
        let major = c % 10 == 0;
        let c = c as f32;
        let len = if c == 0.0 {
            18.0
        } else if major {
            13.0
        } else {
            7.0
        };
        p.line_segment(
            [point(c, scale_r), point(c, scale_r + len)],
            Stroke::new(if major { 2.0 } else { 1.2 }, scale_col),
        );
        if major {
            let label = if c == 0.0 {
                "0".to_string()
            } else {
                format!("{}", c.abs() as i32)
            };
            p.text(
                point(c, scale_r + len + num_size * 0.9),
                Align2::CENTER_CENTER,
                label,
                oswald(num_size),
                scale_col,
            );
        }
    }
    // End caps like "VU": flat side and sharp side.
    for (c, s) in [(-58.0, "FLAT"), (58.0, "SHARP")] {
        p.text(
            point(c, scale_r + 8.0),
            Align2::CENTER_CENTER,
            s,
            oswald(num_size * 0.9),
            scale_col,
        );
    }
    p.text(
        point(0.0, scale_r - 38.0),
        Align2::CENTER_CENTER,
        "CENTS",
        oswald(12.0),
        scale_col.gamma_multiply(0.8),
    );

    // Note name and status in the middle of the dial.
    let note_pos = point(0.0, scale_r * 0.52);
    let note_size = (radius * 0.30).clamp(36.0, 110.0);
    match (error, target) {
        (Some(err), _) => {
            p.text(
                note_pos,
                Align2::CENTER_CENTER,
                format!("⚠ {err}"),
                oswald(17.0),
                ERROR,
            );
        }
        (None, Some((note, cents))) => {
            let col = if in_tune { GOLD_BRIGHT } else { GOLD };
            if in_tune {
                glow(&p, note_pos, note_size * 0.9, GOLD_BRIGHT, 0.18);
            }
            p.text(
                note_pos,
                Align2::CENTER_CENTER,
                note.name(),
                oswald(note_size),
                col,
            );
            let w = note.name().len() as f32 * note_size * 0.22;
            p.text(
                note_pos + Vec2::new(w + 4.0, note_size * 0.2),
                Align2::LEFT_CENTER,
                note.octave().to_string(),
                oswald(note_size * 0.38),
                col.gamma_multiply(0.75),
            );
            let status = if in_tune {
                "IN  TUNE"
            } else if cents < 0.0 {
                "TUNE  UP"
            } else {
                "TUNE  DOWN"
            };
            p.text(
                note_pos + Vec2::new(0.0, note_size * 0.72),
                Align2::CENTER_CENTER,
                status,
                oswald(15.0),
                col,
            );
        }
        (None, None) => {
            p.text(
                note_pos + Vec2::new(0.0, note_size * 0.3),
                Align2::CENTER_CENTER,
                "PLAY  A  STRING",
                oswald(16.0),
                GOLD_DIM,
            );
        }
    }

    // Needle with a dark dome over the pivot.
    let needle_col = if live { GOLD_BRIGHT } else { GOLD };
    let tip = point(needle, scale_r + 22.0);
    p.line_segment(
        [pivot + Vec2::new(2.0, 0.0), tip + Vec2::new(2.0, 2.0)],
        Stroke::new(2.5, Color32::from_black_alpha(120)),
    );
    p.line_segment([pivot, tip], Stroke::new(2.0, needle_col));
    let dome_r = Vec2::new(rect.width() * 0.13, radius * 0.22);
    let dome_c = Pos2::new(pivot.x, rect.bottom() + dome_r.y * 0.35);
    p.add(Shape::ellipse_filled(
        dome_c,
        dome_r,
        Color32::from_rgb(0x16, 0x14, 0x12),
    ));
    p.add(Shape::ellipse_stroke(
        dome_c,
        dome_r,
        Stroke::new(1.0, Color32::from_rgb(0x3a, 0x30, 0x24)),
    ));

    // LCD readouts in the bottom corners.
    let pad = 14.0;
    let lcd_size = (rect.height() * 0.06).clamp(15.0, 24.0);
    let cents_text = target.map_or("--.-".into(), |(_, c)| format!("{c:+.1}"));
    let freq_text = freq.map_or("--.--".into(), |f| format!("{f:.2}"));
    let base_y = rect.bottom() - pad;
    p.text(
        Pos2::new(rect.left() + pad, base_y - lcd_size - 8.0),
        Align2::LEFT_BOTTOM,
        "CENTS",
        lcd(10.0),
        LCD.gamma_multiply(0.55),
    );
    lcd_text(
        &p,
        Pos2::new(rect.left() + pad, base_y),
        Align2::LEFT_BOTTOM,
        &cents_text,
        lcd(lcd_size),
        LCD,
    );
    p.text(
        Pos2::new(rect.right() - pad, base_y - lcd_size - 8.0),
        Align2::RIGHT_BOTTOM,
        "FREQ  HZ",
        lcd(10.0),
        LCD.gamma_multiply(0.55),
    );
    lcd_text(
        &p,
        Pos2::new(rect.right() - pad, base_y),
        Align2::RIGHT_BOTTOM,
        &freq_text,
        lcd(lcd_size),
        LCD,
    );

    // Glass edge.
    outer.rect_stroke(
        rect,
        10.0,
        Stroke::new(1.5, Color32::from_rgb(0x3a, 0x2e, 0x20)),
        StrokeKind::Inside,
    );
}

/// LCD text with faint unlit segments behind it.
fn lcd_text(p: &egui::Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    // In DSEG, "~" lights every segment; keep "." so the widths match.
    let ghost: String = text
        .chars()
        .map(|c| if c == '.' { '.' } else { '~' })
        .collect();
    p.text(pos, align, ghost, font.clone(), color.gamma_multiply(0.07));
    p.text(pos, align, text, font, color);
}

/// Two thin vertical gold bars showing input level.
fn level_bars(p: &egui::Painter, rect: Rect, level: f32, clipping: bool) {
    for x in [rect.left() + 2.0, rect.right() - 2.0] {
        let track =
            Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::new(5.0, rect.height()));
        p.rect_filled(track, 2.5, SHADOW);
        p.rect_stroke(
            track,
            2.5,
            Stroke::new(1.0, Color32::from_gray(0x2a)),
            StrokeKind::Outside,
        );
        let h = (track.height() - 4.0) * level;
        if h > 1.0 {
            let fill = Rect::from_min_max(
                Pos2::new(track.left() + 1.0, track.bottom() - 2.0 - h),
                Pos2::new(track.right() - 1.0, track.bottom() - 2.0),
            );
            let top = if clipping { GOLD_BRIGHT } else { GOLD };
            p.add(vgradient(fill, top, GOLD_DIM, 1.5));
        }
    }
}

/// A horizontal row of small recessed dots, centred on `center`.
fn dots_row(p: &egui::Painter, center: Pos2, n: usize) {
    let step = 6.0;
    let x0 = center.x - step * (n as f32 - 1.0) / 2.0;
    for i in 0..n {
        let c = Pos2::new(x0 + step * i as f32, center.y);
        p.circle_filled(c, 2.0, SHADOW);
        p.circle_filled(c + Vec2::new(0.0, 0.8), 0.8, Color32::from_gray(0x2c));
    }
}

/// A recessed dark panel.
fn recess(p: &egui::Painter, rect: Rect, radius: f32) {
    p.rect_filled(rect, radius, RECESS);
    p.rect_stroke(rect, radius, Stroke::new(1.0, SHADOW), StrokeKind::Inside);
    p.line_segment(
        [
            rect.left_bottom() + Vec2::new(radius, 0.5),
            rect.right_bottom() + Vec2::new(-radius, 0.5),
        ],
        Stroke::new(1.0, Color32::from_gray(0x30)),
    );
}

#[derive(Clone, Copy)]
enum Icon {
    Mic,
    Rescan,
}

/// Square gold-tinted button with a painted icon.
fn icon_button(ui: &mut egui::Ui, rect: Rect, id: &str, icon: Icon) -> Response {
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let p = ui.painter_at(rect.expand(4.0));
    let (top, bottom) = if resp.is_pointer_button_down_on() {
        (
            Color32::from_rgb(0x2a, 0x22, 0x12),
            Color32::from_rgb(0x3e, 0x31, 0x18),
        )
    } else if resp.hovered() {
        (
            Color32::from_rgb(0x5a, 0x46, 0x22),
            Color32::from_rgb(0x30, 0x26, 0x14),
        )
    } else {
        (
            Color32::from_rgb(0x4a, 0x3a, 0x1c),
            Color32::from_rgb(0x26, 0x1e, 0x10),
        )
    };
    p.rect_filled(rect.translate(Vec2::new(0.0, 2.0)), 4.0, SHADOW);
    p.add(vgradient(rect, top, bottom, 4.0));
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(0x12, 0x0e, 0x08)),
        StrokeKind::Outside,
    );

    let c = rect.center();
    let s = Stroke::new(2.0, Color32::from_rgb(0xf4, 0xe6, 0xc8));
    match icon {
        Icon::Mic => {
            let capsule = Rect::from_center_size(c - Vec2::new(0.0, 3.0), Vec2::new(8.0, 13.0));
            p.rect_stroke(capsule, 4.0, s, StrokeKind::Middle);
            arc_line(&p, c - Vec2::new(0.0, 1.0), 7.5, 90.0, 270.0, s);
            p.line_segment([c + Vec2::new(0.0, 6.5), c + Vec2::new(0.0, 10.0)], s);
        }
        Icon::Rescan => {
            arc_line(&p, c, 8.0, -30.0, 250.0, s);
            // Arrow head at the end of the arc.
            let a = (-30f32).to_radians();
            let end = c + Vec2::new(a.cos(), a.sin()) * 8.0;
            p.add(Shape::convex_polygon(
                vec![
                    end + Vec2::new(-4.0, -3.0),
                    end + Vec2::new(4.0, -2.0),
                    end + Vec2::new(1.0, 4.0),
                ],
                s.color,
                Stroke::NONE,
            ));
        }
    }
    resp
}

/// Circular arc from `from` to `to` degrees (0 = right, clockwise, screen coordinates).
fn arc_line(p: &egui::Painter, center: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let steps = 32;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
            center + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

/// Rounded-ish rectangle with a vertical gradient. Corners are approximated by
/// insetting the top and bottom rows, which reads as rounded at these sizes.
fn vgradient(rect: Rect, top: Color32, bottom: Color32, radius: f32) -> Shape {
    let mut mesh = Mesh::default();
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    // Rows: corner curve on top, straight middle, corner curve on bottom.
    let mut rows: Vec<(f32, f32)> = Vec::new(); // (y, inset)
    let n = 6;
    for i in 0..=n {
        let t = i as f32 / n as f32; // 0 at edge → 1 at end of curve
        let y = rect.top() + r * t;
        let inset = r - (r * r - (r - r * t).powi(2)).max(0.0).sqrt();
        rows.push((y, inset));
    }
    for i in (0..=n).rev() {
        let t = i as f32 / n as f32;
        let y = rect.bottom() - r * t;
        let inset = r - (r * r - (r - r * t).powi(2)).max(0.0).sqrt();
        rows.push((y, inset));
    }
    for (y, inset) in &rows {
        let t = ((y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let col = lerp_color(top, bottom, t);
        mesh.colored_vertex(Pos2::new(rect.left() + inset, *y), col);
        mesh.colored_vertex(Pos2::new(rect.right() - inset, *y), col);
    }
    for i in 0..rows.len() as u32 - 1 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    Shape::mesh(mesh)
}

/// Radial glow: bright centre fading to transparent at `radius`.
fn glow(p: &egui::Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    let mut mesh = Mesh::default();
    let inner = color.gamma_multiply(strength);
    mesh.colored_vertex(center, inner);
    let steps = 48;
    for i in 0..=steps {
        let a = std::f32::consts::TAU * i as f32 / steps as f32;
        mesh.colored_vertex(
            center + Vec2::new(a.cos(), a.sin()) * radius,
            Color32::TRANSPARENT,
        );
    }
    for i in 1..=steps as u32 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}
