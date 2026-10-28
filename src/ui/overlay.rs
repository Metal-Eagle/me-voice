//! The listening card at the top of the screen (design: "two-line card").
//! Top line: status dot, label and a compact live sound wave. Below: the words as
//! they are recognised, then the final text for a moment after it is pasted.
//!
//! The window only exists while dictating; it never takes focus (so pasting
//! still goes to your app) and clicks pass through it.
//!
//! When dictating to T3 Code, a list of recent threads hangs under it: the number keys
//! pick the one the text goes to. 0 swaps it for a list of projects to start a new thread in.

use std::num::NonZeroU32;
use std::rc::Rc;

use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use softbuffer::{Context, Surface};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};

const WIDTH: f64 = 840.0;
const HEIGHT: f64 = 156.0;
const TOP_MARGIN: f64 = 18.0;
const PAD_X: f64 = 26.0;
const LABEL_SIZE: f64 = 13.0;
const TEXT_SIZE: f64 = 25.0;
const HINT_SIZE: f64 = 17.0;
const WAVE_WIDTH: f64 = 260.0;
const WAVE_HEIGHT: f64 = 40.0;
/// Centre of the status line, and the line under it.
const HEAD_Y: f64 = 30.0;
const RULE_Y: f64 = 60.0;
/// Centres of the two lines of words.
const LINE_OLD_Y: f64 = 94.0;
const LINE_NEW_Y: f64 = 128.0;
/// The stop countdown starts this many seconds before the automatic stop.
pub const COUNTDOWN_SECONDS: f32 = 2.0;
pub const BARS: usize = 44;
/// The T3 thread list under the words: its heading, then one row per thread.
const LIST_HEAD: f64 = 34.0;
const ROW_HEIGHT: f64 = 34.0;
const LIST_PAD: f64 = 12.0;
const ROW_SIZE: f64 = 17.0;

const BACKGROUND: [u8; 3] = [0x18, 0x1A, 0x1E];
const WAVE: [u8; 3] = [0x1E, 0x9B, 0xFF];
const WAVE_IDLE: [u8; 3] = [0x55, 0x5D, 0x66];
const TEXT: [u8; 3] = [0xF2, 0xF4, 0xF6];
const TEXT_OLD: [u8; 3] = [0x78, 0x7C, 0x82];
const RULE: [u8; 3] = [0x2C, 0x2F, 0x34];
const BACKGROUND_PILL: [u8; 3] = [0x3A, 0x3E, 0x44];
const LABEL: [u8; 3] = [0x9A, 0xA1, 0xA8];
const HINT: [u8; 3] = [0x7A, 0x82, 0x8C];
const RED: [u8; 3] = [0xFF, 0x3B, 0x30];
const YELLOW: [u8; 3] = [0xFF, 0xD6, 0x0A];
const GREEN: [u8; 3] = [0x34, 0xC7, 0x59];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Listening,
    Transcribing,
    /// The AI is cleaning up the text; `true` when it goes to Claude (the cloud).
    Polishing(bool),
    /// The text is done and waits for a T3 thread to be picked; seconds left before it is dropped.
    Choosing(u32),
    Done,
}

pub struct Overlay {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    font: FontArc,
    label_font: FontArc,
    frame: (Phase, Vec<f32>, String),
    /// Seconds left before the automatic stop, `None` when not counting down.
    countdown: Option<f32>,
    opened: std::time::Instant,
    /// Profile name shown after the status label.
    badge: Option<String>,
    /// T3 thread (or project) titles to pick from, and the picked one.
    threads: Vec<String>,
    chosen: Option<usize>,
    /// The rows are projects to start a new thread in.
    new_thread: bool,
}

impl Overlay {
    pub fn open<T>(target: &EventLoopWindowTarget<T>) -> Option<Self> {
        #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
        let mut builder = WindowBuilder::new()
            .with_title("me-voice listening")
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(true)
            .with_focused(false)
            .with_focusable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(WIDTH, height(0)));
        #[cfg(target_os = "windows")]
        {
            use tao::platform::windows::WindowBuilderExtWindows;
            builder = builder.with_skip_taskbar(true);
        }
        let window = Rc::new(builder.build(target).map_err(|e| crate::app::log::write(&format!("pill window: {e}"))).ok()?);
        let _ = window.set_ignore_cursor_events(true);
        prepare_window(&window);

        // Top centre of the screen the window landed on.
        if let Some(monitor) = window.current_monitor().or_else(|| target.primary_monitor()) {
            let scale = monitor.scale_factor();
            let size = window.outer_size();
            let x = monitor.position().x + (monitor.size().width as i32 - size.width as i32) / 2;
            let y = monitor.position().y + (TOP_MARGIN * scale) as i32;
            window.set_outer_position(PhysicalPosition::new(x, y));
        }

        let context = Context::new(window.clone()).map_err(|e| crate::app::log::write(&format!("pill drawing: {e}"))).ok()?;
        let surface = Surface::new(&context, window.clone()).map_err(|e| crate::app::log::write(&format!("pill surface: {e}"))).ok()?;
        let font = load_font("segoeui.ttf")?;
        let label_font = load_font("seguisb.ttf").unwrap_or_else(|| font.clone());
        window.set_visible(true);
        Some(Self {
            window,
            surface,
            font,
            label_font,
            frame: (Phase::Listening, Vec::new(), String::new()),
            countdown: None,
            opened: std::time::Instant::now(),
            badge: None,
            threads: Vec::new(),
            chosen: None,
            new_thread: false,
        })
    }

    pub fn window_id(&self) -> tao::window::WindowId {
        self.window.id()
    }

    /// Stores what to show and asks the OS for a repaint (drawn in `paint`).
    /// `levels` are 0.0–1.0, oldest first; `text` is what has been recognised so far.
    pub fn update(&mut self, phase: Phase, levels: Vec<f32>, text: &str) {
        self.frame = (phase, levels, text.to_string());
        self.window.request_redraw();
    }

    /// Shows the profile name after the status ("LISTENING · CHAT"); `None` hides it.
    pub fn set_badge(&mut self, badge: Option<String>) {
        self.badge = badge;
    }

    /// Shows the T3 threads under the card, `chosen` (if any) highlighted; the card grows to fit them.
    /// With `new_thread` the rows are projects to start a new thread in.
    pub fn set_threads(&mut self, threads: Vec<String>, chosen: Option<usize>, new_thread: bool) {
        if threads.len() != self.threads.len() {
            self.window.set_inner_size(LogicalSize::new(WIDTH, height(threads.len())));
        }
        self.threads = threads;
        self.chosen = chosen;
        self.new_thread = new_thread;
    }

    /// Shows "stopping in n s" with a ring while an automatic stop is near (`None` hides it).
    pub fn set_countdown(&mut self, seconds_left: Option<f32>) {
        self.countdown = seconds_left;
    }

    /// Draws the latest frame; call on the window's `RedrawRequested` event.
    pub fn paint(&mut self) {
        let (phase, levels, text) = std::mem::replace(&mut self.frame, (Phase::Listening, Vec::new(), String::new()));
        self.draw(phase, &levels, &text);
        self.frame = (phase, levels, text);
    }

    fn draw(&mut self, phase: Phase, levels: &[f32], text: &str) {
        let size = self.window.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            crate::app::log::write(&format!("pill: zero size {size:?}"));
            return;
        };
        if let Err(e) = self.surface.resize(nw, nh) {
            crate::app::log::write(&format!("pill resize: {e}"));
            return;
        }
        let (w, h) = (size.width as usize, size.height as usize);
        let scale = self.window.scale_factor();
        let mut canvas = Canvas { pixels: vec![pack(BACKGROUND); w * h], w, h };

        let x0 = PAD_X * scale;
        let x1 = w as f64 - PAD_X * scale;
        let head_y = HEAD_Y * scale;

        // Status line: dot with a soft glow (blinking while listening), label, profile pill.
        let seconds = self.countdown.map(|left| left.max(0.0).ceil() as u32);
        let (dot, label) = match (phase, seconds) {
            (Phase::Listening, Some(n)) => (RED, format!("LISTENING · STOPPING IN {n} S")),
            (Phase::Listening, None) => (RED, "LISTENING".to_string()),
            (Phase::Transcribing, _) => (YELLOW, "TRANSCRIBING".to_string()),
            (Phase::Polishing(false), _) => (YELLOW, "POLISHING".to_string()),
            (Phase::Polishing(true), _) => (YELLOW, "POLISHING · CLAUDE".to_string()),
            (Phase::Choosing(left), _) => (YELLOW, format!("PICK A THREAD · DROPPED IN {left} S")),
            (Phase::Done, _) => (GREEN, "PASTED".to_string()),
        };
        let blink = if phase == Phase::Listening {
            0.65 + 0.35 * (self.opened.elapsed().as_secs_f32() * 5.0).cos()
        } else {
            1.0
        };
        canvas.circle_alpha(x0 + 6.0 * scale, head_y, 10.0 * scale, dot, 0.2 * blink);
        canvas.circle_alpha(x0 + 6.0 * scale, head_y, 6.0 * scale, dot, blink);

        // Sound wave on the right of the status line.
        let wave_left = x1 - WAVE_WIDTH * scale;
        let mut label_end = canvas.text(&self.label_font, &label, x0 + 22.0 * scale, wave_left - 12.0 * scale, head_y, LABEL_SIZE * scale, 2.0 * scale, LABEL);
        if let Some(badge) = &self.badge {
            let name = badge.to_string();
            let pill_px = 12.0 * scale;
            let text_w = canvas.measure(&self.label_font, &name, pill_px, 0.0);
            let (pw, ph) = (text_w + 20.0 * scale, 22.0 * scale);
            let px = label_end + 14.0 * scale;
            if px + pw < wave_left - 8.0 * scale {
                canvas.round_rect(px, head_y - ph / 2.0, pw, ph, ph / 2.0, BACKGROUND_PILL);
                canvas.text(&self.label_font, &name, px + 10.0 * scale, px + pw - 6.0, head_y, pill_px, 0.0, TEXT);
                label_end = px + pw;
            }
        }
        if let Some(left) = self.countdown {
            // The ring empties as the stop comes closer.
            canvas.ring(label_end + 16.0 * scale, head_y, 8.0 * scale, 2.6 * scale, (left / COUNTDOWN_SECONDS).clamp(0.0, 1.0) as f64, dot);
        }
        let slot = WAVE_WIDTH * scale / BARS as f64;
        let bar_w = (slot * 0.56).max(2.0);
        let max_half = WAVE_HEIGHT * scale / 2.0;
        let colour = if phase == Phase::Listening { WAVE } else { WAVE_IDLE };
        for (i, &level) in levels.iter().take(BARS).enumerate() {
            let half = (level.clamp(0.0, 1.0) as f64 * max_half).max(2.0 * scale);
            canvas.rounded_bar(wave_left + i as f64 * slot, head_y - half, bar_w, half * 2.0, colour);
        }

        // Thin line under the status line.
        canvas.round_rect(x0, RULE_Y * scale, x1 - x0, scale.max(1.0), 0.0, RULE);

        // The words on two lines: the newest always visible, the line above dimmed.
        let dots_room = 46.0 * scale;
        let right = if phase == Phase::Done { x1 } else { x1 - dots_room };
        let text_px = TEXT_SIZE * scale;
        let shown = text.replace('\n', " ");
        let (old_line, new_line) = canvas.last_two_lines(&self.font, &shown, text_px, right - x0);
        if !old_line.is_empty() {
            canvas.text(&self.font, &old_line, x0, right, LINE_OLD_Y * scale, text_px, 0.0, TEXT_OLD);
        } else if self.countdown.is_some() {
            // Not much said yet: room for a hint.
            canvas.text(&self.font, "Keep talking to continue · Esc to cancel", x0, x1, LINE_OLD_Y * scale, HINT_SIZE * scale, 0.0, HINT);
        }
        let mut end = x0;
        if !new_line.is_empty() {
            end = canvas.text(&self.font, &new_line, x0, right, LINE_NEW_Y * scale, text_px, 0.0, TEXT) + 8.0 * scale;
        } else if phase == Phase::Done {
            canvas.text(&self.font, "Nothing recognised", x0, x1, LINE_NEW_Y * scale, text_px, 0.0, HINT);
        }
        if phase != Phase::Done {
            // Each dot rises and swells with your voice (or a steady wave while transcribing).
            let t = self.opened.elapsed().as_secs_f32();
            let voice = levels.last().copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let energy = if phase == Phase::Listening { 0.25 + 0.75 * voice } else { 0.5 };
            let colour = if phase == Phase::Listening { WAVE } else { HINT };
            for i in 0..3 {
                let wave = ((t * 7.0 - i as f32 * 0.9).sin() * 0.5 + 0.5) * energy;
                let r = (3.4 + 2.2 * wave as f64) * scale;
                let lift = 6.0 * wave as f64 * scale;
                let alpha = 0.45 + 0.55 * wave;
                canvas.circle_alpha(end + 8.0 * scale + i as f64 * 14.0 * scale, LINE_NEW_Y * scale + 4.0 * scale - lift, r, colour, alpha);
            }
        }

        if !self.threads.is_empty() {
            self.draw_threads(&mut canvas, phase, scale, x0, x1);
        }

        // Countdown bar along the bottom edge: drains towards the left.
        if let Some(left) = self.countdown {
            let fraction = (left / COUNTDOWN_SECONDS).clamp(0.0, 1.0) as f64;
            canvas.round_rect(0.0, h as f64 - 4.0 * scale, w as f64 * fraction, 4.0 * scale, 0.0, dot);
        }

        let mut buffer = match self.surface.buffer_mut() {
            Ok(b) => b,
            Err(e) => {
                crate::app::log::write(&format!("pill buffer: {e}"));
                return;
            }
        };
        buffer.copy_from_slice(&canvas.pixels);
        if let Err(e) = buffer.present() {
            crate::app::log::write(&format!("pill present: {e}"));
        }
    }
}

impl Overlay {
    /// The thread list under the words: "SEND TO" and a row per thread with its number key.
    fn draw_threads(&self, canvas: &mut Canvas, phase: Phase, scale: f64, x0: f64, x1: f64) {
        let top = HEIGHT * scale;
        canvas.round_rect(x0, top, x1 - x0, scale.max(1.0), 0.0, RULE);
        let n = self.threads.len();
        let head = match (phase, self.new_thread) {
            (Phase::Listening | Phase::Choosing(_), false) => format!("SEND TO · 1–{n} THREAD · 0 NEW THREAD"),
            (Phase::Listening | Phase::Choosing(_), true) => format!("NEW THREAD IN · 1–{n} PROJECT · 0 BACK"),
            (Phase::Done, false) => "SENT TO".to_string(),
            (Phase::Done, true) => "STARTED A NEW THREAD IN".to_string(),
            (_, false) => "SENDING TO".to_string(),
            (_, true) => "STARTING A NEW THREAD IN".to_string(),
        };
        canvas.text(&self.label_font, &head, x0, x1, top + LIST_HEAD / 2.0 * scale + 2.0 * scale, LABEL_SIZE * scale, 2.0 * scale, LABEL);
        for (i, title) in self.threads.iter().enumerate() {
            let cy = top + (LIST_HEAD + ROW_HEIGHT * (i as f64 + 0.5)) * scale;
            let chosen = Some(i) == self.chosen;
            if chosen {
                let h = (ROW_HEIGHT - 4.0) * scale;
                canvas.round_rect(x0 - 10.0 * scale, cy - h / 2.0, x1 - x0 + 20.0 * scale, h, 8.0 * scale, BACKGROUND_PILL);
            }
            // The number key in a small square, blue when it's the chosen thread.
            let k = 22.0 * scale;
            canvas.round_rect(x0, cy - k / 2.0, k, k, 5.0 * scale, if chosen { WAVE } else { RULE });
            let number = (i + 1).to_string();
            let nx = x0 + (k - canvas.measure(&self.label_font, &number, LABEL_SIZE * scale, 0.0)) / 2.0;
            canvas.text(&self.label_font, &number, nx, x0 + k, cy, LABEL_SIZE * scale, 0.0, TEXT);
            let title = match title.trim() {
                "" if self.new_thread => "Untitled project",
                "" => "Untitled thread",
                _ => title.as_str(),
            };
            let font = if chosen { &self.label_font } else { &self.font };
            canvas.text(font, title, x0 + k + 14.0 * scale, x1, cy, ROW_SIZE * scale, 0.0, if chosen { TEXT } else { TEXT_OLD });
        }
    }
}

/// Window height with `threads` rows of T3 threads under the card.
fn height(threads: usize) -> f64 {
    if threads == 0 {
        HEIGHT
    } else {
        HEIGHT + LIST_HEAD + ROW_HEIGHT * threads as f64 + LIST_PAD
    }
}

struct Canvas {
    pixels: Vec<u32>,
    w: usize,
    h: usize,
}

impl Canvas {
    fn blend(&mut self, x: usize, y: usize, colour: [u8; 3], alpha: f32) {
        if x >= self.w || y >= self.h || alpha <= 0.0 {
            return;
        }
        let i = y * self.w + x;
        let bg = unpack(self.pixels[i]);
        let a = alpha.min(1.0);
        let mix = |c: u8, b: u8| (c as f32 * a + b as f32 * (1.0 - a)) as u8;
        self.pixels[i] = pack([mix(colour[0], bg[0]), mix(colour[1], bg[1]), mix(colour[2], bg[2])]);
    }

    fn circle_alpha(&mut self, cx: f64, cy: f64, r: f64, colour: [u8; 3], alpha: f32) {
        for y in (cy - r - 1.0).max(0.0) as usize..=(cy + r + 1.0) as usize {
            for x in (cx - r - 1.0).max(0.0) as usize..=(cx + r + 1.0) as usize {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, colour, (r - d + 0.5).clamp(0.0, 1.0) as f32 * alpha);
            }
        }
    }

    #[allow(dead_code)]
    fn circle(&mut self, cx: f64, cy: f64, r: f64, colour: [u8; 3]) {
        for y in (cy - r - 1.0).max(0.0) as usize..=(cy + r + 1.0) as usize {
            for x in (cx - r - 1.0).max(0.0) as usize..=(cx + r + 1.0) as usize {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, colour, (r - d + 0.5).clamp(0.0, 1.0) as f32);
            }
        }
    }

    /// A ring of `width` around (cx, cy), filled clockwise from the top up to `fraction` (0.0 to 1.0).
    fn ring(&mut self, cx: f64, cy: f64, r: f64, width: f64, fraction: f64, colour: [u8; 3]) {
        for y in (cy - r - 1.0).max(0.0) as usize..=(cy + r + 1.0) as usize {
            for x in (cx - r - 1.0).max(0.0) as usize..=(cx + r + 1.0) as usize {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt();
                let edge = (r - d + 0.5).clamp(0.0, 1.0) * (d - (r - width) + 0.5).clamp(0.0, 1.0);
                // Clockwise angle from the top, 0 to 2π.
                let angle = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU);
                if angle <= fraction * std::f64::consts::TAU {
                    self.blend(x, y, colour, edge as f32);
                }
            }
        }
    }

    /// A filled rectangle with rounded corners of radius `r`.
    fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64, colour: [u8; 3]) {
        let r = r.min(w / 2.0).min(h / 2.0);
        for py in y.max(0.0) as usize..=(y + h).ceil() as usize {
            for px in x.max(0.0) as usize..=(x + w).ceil() as usize {
                let (fx, fy) = (px as f64 + 0.5, py as f64 + 0.5);
                if fx < x || fy < y || fx > x + w || fy > y + h {
                    continue;
                }
                let cx = fx.clamp(x + r, x + w - r);
                let cy = fy.clamp(y + r, y + h - r);
                let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
                self.blend(px, py, colour, (r - d + 0.5).clamp(0.0, 1.0) as f32);
            }
        }
    }

    /// Width of `text` in pixels.
    fn measure(&self, font: &FontArc, text: &str, px: f64, spacing: f64) -> f64 {
        let scaled = font.as_scaled(PxScale::from(px as f32));
        let mut x = 0.0f32;
        let mut prev = None;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            prev = Some(id);
            x += scaled.h_advance(id) + spacing as f32;
        }
        x as f64
    }

    /// Wraps `text` into lines of at most `max` pixels and returns the last two
    /// (the one above and the newest), so the newest words are always on screen.
    fn last_two_lines(&self, font: &FontArc, text: &str, px: f64, max: f64) -> (String, String) {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if current.is_empty() || self.measure(font, &candidate, px, 0.0) <= max {
                current = candidate;
            } else {
                lines.push(std::mem::replace(&mut current, word.to_string()));
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        let new = lines.pop().unwrap_or_default();
        let old = lines.pop().unwrap_or_default();
        (old, new)
    }

    /// A vertical bar with round ends.
    fn rounded_bar(&mut self, x: f64, y: f64, w: f64, h: f64, colour: [u8; 3]) {
        let r = w / 2.0;
        let cx = x + r;
        for py in (y - 1.0).max(0.0) as usize..=(y + h + 1.0) as usize {
            for px in x.max(0.0) as usize..=(x + w + 1.0) as usize {
                let fy = (py as f64 + 0.5).clamp(y + r, (y + h - r).max(y + r));
                let d = ((px as f64 + 0.5 - cx).powi(2) + (py as f64 + 0.5 - fy).powi(2)).sqrt();
                self.blend(px, py, colour, (r - d + 0.5).clamp(0.0, 1.0) as f32);
            }
        }
    }

    /// One line of text between `left` and `right`, vertically centred on `cy`, with extra
    /// letter `spacing`. If it's too long, the start is cut off and fades out on the left,
    /// so the newest words stay visible.
    #[allow(clippy::too_many_arguments)]
    /// Returns where the drawn text ends.
    fn text(&mut self, font: &FontArc, text: &str, left: f64, right: f64, cy: f64, px: f64, spacing: f64, colour: [u8; 3]) -> f64 {
        let scaled = font.as_scaled(PxScale::from(px as f32));
        let spacing = spacing as f32;
        // Position of each character from the start of the string.
        let mut glyphs = Vec::with_capacity(text.len());
        let mut x = 0.0f32;
        let mut prev = None;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            prev = Some(id);
            glyphs.push((id, x));
            x += scaled.h_advance(id) + spacing;
        }
        let total = x - spacing;
        let max = (right - left) as f32;
        let overflow = (total - max).max(0.0);
        let fade = if overflow > 0.0 { 28.0 * (px as f32 / 16.0) } else { 0.0 };
        let baseline = cy as f32 + (scaled.ascent() + scaled.descent()) / 2.0;
        for (id, gx) in glyphs {
            let x = left as f32 + gx - overflow;
            if x + scaled.h_advance(id) < left as f32 {
                continue;
            }
            let glyph = id.with_scale_and_position(px as f32, point(x, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|ox, oy, cov| {
                    let (gx, gy) = (b.min.x as i32 + ox as i32, b.min.y as i32 + oy as i32);
                    if gx < left as i32 || gy < 0 || gx as f64 >= right {
                        return;
                    }
                    let fade_alpha = if fade > 0.0 { ((gx as f32 - left as f32) / fade).clamp(0.0, 1.0) } else { 1.0 };
                    self.blend(gx as usize, gy as usize, colour, cov * fade_alpha);
                });
            }
        }
        left + (total.min(max)) as f64
    }
}

/// Segoe UI from the Windows fonts folder, or the bundled font elsewhere.
fn load_font(file: &str) -> Option<FontArc> {
    let system = std::env::var_os("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts").join(file))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| FontArc::try_from_vec(bytes).ok());
    system.or_else(|| FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).ok())
}

fn pack(c: [u8; 3]) -> u32 {
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}

fn unpack(p: u32) -> [u8; 3] {
    [(p >> 16) as u8, (p >> 8) as u8, p as u8]
}

/// Windows: click-through makes this a "layered" window, which shows nothing until
/// it gets an opacity; also ask Windows 11 for rounded corners (a pill-like look).
fn prepare_window(window: &Window) {
    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowExtWindows;
        use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetLayeredWindowAttributes, LWA_ALPHA};
        let pref = DWMWCP_ROUND;
        unsafe {
            // Slightly see-through so it doesn't fully hide what's underneath.
            SetLayeredWindowAttributes(window.hwnd() as _, 0, 242, LWA_ALPHA);
            DwmSetWindowAttribute(
                window.hwnd() as _,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &pref as *const _ as *const _,
                std::mem::size_of_val(&pref) as u32,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}
