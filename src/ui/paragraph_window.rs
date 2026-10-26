//! "Dictate paragraph" window: talk in lines and see all of it while you speak.
//!
//! Each line you finish ("new line", a pause, the button or the shortcut again) is
//! transcribed with the main model and added under the previous ones; the words you are
//! saying right now show live underneath. Paste puts the whole paragraph where your cursor
//! was. Runs as its own short-lived process (`me-voice paragraph`), like the fix window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::speech::audio::{self, Recording};
use crate::text::cleanup::{self, VoiceCommand};
use crate::app::ipc::{self, Command};
use crate::speech::models;
use crate::settings::{is_dev, Settings};
use crate::ui::theme;
use crate::speech::transcriber::{Transcriber, SAMPLE_RATE};

/// Lines shorter than this are an accidental sound.
const MIN_CLIP_SAMPLES: usize = SAMPLE_RATE * 3 / 10;
/// Update the live line once this much new audio has come in.
const PREVIEW_EVERY_SECONDS: f32 = 0.3;
/// The live line only re-reads this many recent seconds.
const PREVIEW_WINDOW: usize = SAMPLE_RATE * 10;
const BARS: usize = 32;

/// Shortcut pressed: lock in the line if the window is open, otherwise open it.
pub fn open_or_new_line() {
    if ipc::send_to(&ipc::paragraph_socket(), Command::NewLine).is_ok() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("paragraph");
        if is_dev() {
            cmd.arg("--dev");
        }
        let _ = cmd.spawn();
    }
}

/// What the window leaves behind for after it has closed.
#[derive(Default)]
struct Outcome {
    /// Text to paste and whether to press Enter afterwards; `None` = cancelled.
    paste: Option<(String, bool)>,
    /// The "new line after pause" value, saved when it changed.
    pause: Option<f32>,
}

enum Event {
    /// Live text for the line in progress (generation, text).
    Preview(u32, String),
    /// A finished line, transcribed with the main model.
    Final(Result<String>),
}

pub fn run() -> Result<()> {
    // One paragraph window at a time; a second start just means "new line" (see above).
    let Some(_instance) = crate::app::instance::acquire("paragraph") else {
        let _ = ipc::send_to(&ipc::paragraph_socket(), Command::NewLine);
        return Ok(());
    };
    let settings = Settings::load();
    if !models::find(&settings.model_id).is_some_and(|m| m.is_downloaded()) {
        return Err(anyhow!("paragraph: the speech model is not downloaded yet (Settings → Speech)"));
    }
    let (cmd_tx, cmd_rx) = mpsc::channel();
    ipc::listen_on(&ipc::paragraph_socket(), move |cmd| {
        let _ = cmd_tx.send(cmd);
    })?;
    let rec = audio::start(settings.microphone.as_deref())?;
    crate::app::log::write("paragraph window: listening");

    let transcriber = Arc::new(Mutex::new(Transcriber::new()));
    let (event_tx, event_rx) = mpsc::channel();
    let (job_tx, job_rx) = mpsc::channel::<Vec<f32>>();
    {
        // Lines are transcribed one after the other, so they come back in order.
        let (settings, transcriber, event_tx) = (settings.clone(), transcriber.clone(), event_tx.clone());
        std::thread::spawn(move || {
            for samples in job_rx {
                let result = crate::app::transcribe(&settings, &transcriber, &samples, false, None);
                let _ = event_tx.send(Event::Final(result));
            }
        });
    }

    let outcome = Arc::new(Mutex::new(Outcome::default()));
    let mut preview_settings = settings.clone();
    // "new line" must stay words in the live text, so the window can see it was said.
    preview_settings.spoken_formatting = false;
    let app = ParagraphApp {
        pause: settings.paragraph_pause_seconds,
        loaded_pause: settings.paragraph_pause_seconds,
        settings,
        preview_settings,
        rec: Some(rec),
        transcriber,
        text: String::new(),
        live: String::new(),
        offset: 0,
        generation: 0,
        pending: 0,
        preview_running: false,
        preview_seconds: 0.0,
        preview_abort: Arc::new(AtomicBool::new(false)),
        finishing: None,
        jobs: job_tx,
        events_tx: event_tx,
        events: event_rx,
        commands: cmd_rx,
        outcome: outcome.clone(),
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title(if is_dev() { "Dictate paragraph (dev)" } else { "Dictate paragraph" })
        .with_inner_size([560.0, 520.0])
        .with_min_inner_size([420.0, 400.0])
        .with_always_on_top();
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "me-voice paragraph",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow!("paragraph window: {e}"))?;

    let outcome = std::mem::take(&mut *outcome.lock().unwrap());
    if let Some(pause) = outcome.pause {
        let mut settings = Settings::load();
        settings.paragraph_pause_seconds = pause;
        if let Err(e) = settings.save().and_then(|_| ipc::send(Command::Reload)) {
            crate::app::log::write(&format!("paragraph: could not save the pause setting: {e}"));
        }
    }
    // Only after the window has closed, and after a moment, so focus is back in the app
    // that was in front.
    if let Some((text, press_enter)) = outcome.paste {
        std::thread::sleep(Duration::from_millis(300));
        crate::text::insert::insert(&text, Settings::load().insert_mode, press_enter)?;
    }
    Ok(())
}

struct ParagraphApp {
    settings: Settings,
    preview_settings: Settings,
    /// The microphone; `None` once the window is closing.
    rec: Option<Recording>,
    transcriber: Arc<Mutex<Transcriber>>,
    /// The finished lines, editable.
    text: String,
    /// What is being said right now (not locked in yet).
    live: String,
    /// How much of the recording (16 kHz samples) is already in finished lines.
    offset: usize,
    /// Goes up with every finished line, so late live updates for it are ignored.
    generation: u32,
    /// Finished lines still being transcribed.
    pending: usize,
    preview_running: bool,
    preview_seconds: f32,
    preview_abort: Arc<AtomicBool>,
    /// Set once Paste is chosen: the window closes when the last line has come back.
    /// The flag says whether to press Enter after pasting.
    finishing: Option<bool>,
    jobs: Sender<Vec<f32>>,
    events_tx: Sender<Event>,
    events: Receiver<Event>,
    commands: Receiver<Command>,
    outcome: Arc<Mutex<Outcome>>,
    pause: f32,
    loaded_pause: f32,
}

impl ParagraphApp {
    /// Locks in what has been said since the last line and starts the next one.
    fn new_line(&mut self) {
        let Some(rec) = &self.rec else { return };
        let all = rec.snapshot();
        let line = all.get(self.offset..).unwrap_or_default().to_vec();
        self.offset = all.len();
        self.generation += 1;
        self.preview_abort.store(true, Ordering::Relaxed);
        self.live.clear();
        let line = if self.settings.trim_silence { audio::trim_silence(&line, self.settings.silence_level) } else { line };
        if line.len() < MIN_CLIP_SAMPLES {
            return;
        }
        self.pending += 1;
        let _ = self.jobs.send(line);
    }

    fn on_final(&mut self, result: Result<String>) {
        self.pending = self.pending.saturating_sub(1);
        let text = match result {
            Ok(text) => text,
            Err(e) => {
                crate::app::log::write(&format!("paragraph: {e}"));
                return;
            }
        };
        let mut line = text.trim().to_string();
        // A spoken "new line" at the end only ended the line.
        if let Some(rest) = cleanup::strip_line_break(&line, &self.settings.language) {
            line = rest;
        }
        match cleanup::voice_command(&line, &self.settings) {
            // "scratch that" on its own takes the last line away again.
            Some((VoiceCommand::Scratch, _)) => {
                self.text = match self.text.trim_end().rfind('\n') {
                    Some(i) => self.text[..i].to_string(),
                    None => String::new(),
                };
            }
            // "stop" / "send" at the end finishes the paragraph, like Paste.
            Some((command, rest)) => {
                self.text = cleanup::append_line(&self.text, &rest);
                self.finishing = Some(command == VoiceCommand::Send);
            }
            None => self.text = cleanup::append_line(&self.text, &line),
        }
    }

    fn on_preview(&mut self, generation: u32, text: String) {
        self.preview_running = false;
        if generation != self.generation {
            return;
        }
        let text = text.trim().to_string();
        if let Some(rest) = cleanup::strip_line_break(&text, &self.settings.language) {
            // You said "new line": show the words up to it and lock them in.
            self.live = rest;
            self.new_line();
        } else if !text.is_empty() {
            self.live = text;
        }
    }

    /// Pause, shortcut and live-text events, then the automatic new line and the live update.
    fn tick(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            if cmd == Command::NewLine {
                self.new_line();
            }
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Preview(generation, text) => self.on_preview(generation, text),
                Event::Final(result) => self.on_final(result),
            }
        }
        let Some(rec) = &self.rec else { return };
        let level = self.settings.silence_level;
        let quiet_long_enough = self.pause > 0.0
            && !self.live.trim().is_empty()
            && rec.silent_for(level).is_some_and(|q| q.as_secs_f32() >= self.pause);
        if quiet_long_enough && self.finishing.is_none() {
            self.new_line();
            return;
        }
        if self.finishing.is_none() {
            self.maybe_start_preview();
        }
    }

    /// Live transcription of the line in progress, one run at a time.
    fn maybe_start_preview(&mut self) {
        let Some(rec) = &self.rec else { return };
        let seconds = rec.seconds();
        if self.preview_running || seconds - self.preview_seconds < PREVIEW_EVERY_SECONDS {
            return;
        }
        self.preview_seconds = seconds;
        let all = rec.snapshot();
        let mut line = all.get(self.offset..).unwrap_or_default().to_vec();
        if line.len() > PREVIEW_WINDOW {
            line.drain(..line.len() - PREVIEW_WINDOW);
        }
        // Nothing worth recognising yet: skip the work (and made-up words from silence).
        if audio::trim_silence(&line, self.settings.silence_level).is_empty() {
            return;
        }
        self.preview_running = true;
        let abort = Arc::new(AtomicBool::new(false));
        self.preview_abort = abort.clone();
        let (settings, transcriber, tx, generation) =
            (self.preview_settings.clone(), self.transcriber.clone(), self.events_tx.clone(), self.generation);
        std::thread::spawn(move || {
            let text = crate::app::transcribe(&settings, &transcriber, &line, true, Some(abort)).unwrap_or_default();
            let _ = tx.send(Event::Preview(generation, text));
        });
    }

    /// Paste chosen (button, Ctrl+Enter or "stop"): lock in the last line, then close.
    fn start_finishing(&mut self, press_enter: bool) {
        if self.finishing.is_none() {
            self.new_line();
            self.finishing = Some(press_enter);
        }
    }

    fn close(&mut self, ctx: &egui::Context, paste: bool) {
        if let Some(rec) = self.rec.take() {
            drop(rec.stop());
        }
        let mut outcome = self.outcome.lock().unwrap();
        if paste {
            let text = self.text.trim().to_string();
            if !text.is_empty() {
                outcome.paste = Some((text, self.finishing == Some(true)));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

fn wave(ui: &mut egui::Ui, levels: &[f32], colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 22.0), egui::Sense::hover());
    let step = rect.width() / levels.len().max(1) as f32;
    for (i, level) in levels.iter().enumerate() {
        let height = (level * rect.height()).max(2.0);
        let centre = egui::pos2(rect.left() + step * (i as f32 + 0.5), rect.center().y);
        ui.painter().rect_filled(egui::Rect::from_center_size(centre, egui::vec2(step * 0.6, height)), 1.0, colour);
    }
}

impl eframe::App for ParagraphApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::ui::text_menu::show_pending(&ctx, frame);
        ctx.request_repaint_after(Duration::from_millis(33));
        self.tick();

        // Pasting waits for the last line to come back from the model.
        if self.finishing.is_some() && self.pending == 0 {
            self.close(&ctx, true);
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close(&ctx, false);
            return;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            self.start_finishing(false);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Dictate paragraph");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let levels = self.rec.as_ref().map(|r| r.levels(BARS)).unwrap_or_else(|| vec![0.0; BARS]);
                if self.finishing.is_some() {
                    theme::tag(ui, "FINISHING", theme::ORANGE);
                } else if self.pending > 0 {
                    theme::tag(ui, &format!("TRANSCRIBING {}", self.pending), theme::ORANGE);
                } else {
                    theme::tag(ui, "LISTENING", theme::RED);
                }
                wave(ui, &levels, theme::ACCENT);
            });
        });
        ui.weak("Say a sentence, then “new line” (or pause, or press the shortcut again) to lock it in.");
        ui.add_space(6.0);

        theme::section_label(ui, "YOUR PARAGRAPH");
        egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            crate::ui::text_menu::add(
                ui,
                egui::TextEdit::multiline(&mut self.text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .hint_text("Finished lines show up here. You can edit them."),
            );
        });

        theme::section_label(ui, "SAYING NOW");
        theme::card(ui, None, |ui| {
            ui.set_width(ui.available_width());
            if self.live.trim().is_empty() {
                ui.weak("…");
            } else {
                ui.label(egui::RichText::new(&self.live).italics());
            }
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("New line after a pause of");
            let slider = egui::Slider::new(&mut self.pause, 0.0..=4.0).step_by(0.1).custom_formatter(|v, _| {
                if v < 0.05 { "off".to_string() } else { format!("{v:.1} s") }
            });
            if ui.add(slider).changed() {
                self.outcome.lock().unwrap().pause = (self.pause != self.loaded_pause).then_some(self.pause);
            }
        });

        ui.add_space(8.0);
        let words = crate::text::history::count_words(&self.text) + crate::text::history::count_words(&self.live);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(theme::primary_button("Paste")).on_hover_text("Ctrl+Enter").clicked() {
                self.start_finishing(false);
            }
            if ui.button("Copy").clicked() {
                ctx.copy_text(self.text.trim().to_string());
            }
            if ui.button("Clear").clicked() {
                self.text.clear();
            }
            if ui.button("Cancel").on_hover_text("Esc").clicked() {
                self.close(&ctx, false);
            }
            if ui.button("New line").on_hover_text("Lock in what you just said").clicked() {
                self.new_line();
            }
            ui.weak(format!("{words} words"));
        });
    }
}
