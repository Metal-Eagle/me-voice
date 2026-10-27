//! Settings window (design B: sidebar menu, resizable, light/dark). Runs as its own short-lived process
//! (`me-voice settings`) so it uses nothing once closed.

mod about;
mod ai;
mod behaviour;
mod profiles;
mod shortcuts;
mod speech;
mod words;

use about::*;
use speech::*;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::app::ipc::{self, Command};
use crate::speech::models::{self, Engine, Model};
use crate::settings::{config_dir, models_dir, AiProvider, ContextLevel, ExtraShortcut, HotkeyMode, InsertMode, Profile, Settings, ShortcutAction};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    History,
    Shortcut,
    Speech,
    Behaviour,
    Words,
    Profiles,
    Ai,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    Dictation,
    Cancel,
    /// Extra shortcut at this index.
    Extra(usize),
}

#[derive(Default)]
struct Download {
    /// The model being downloaded; only its card shows the progress.
    model_id: String,
    done: u64,
    total: Option<u64>,
    finished: bool,
    error: Option<String>,
}

struct SettingsApp {
    saved: Settings,
    edit: Settings,
    page: Page,
    models: Vec<Model>,
    microphones: Vec<String>,
    capturing: Option<Capture>,
    shortcut_error: Option<String>,
    download: Option<Arc<Mutex<Download>>>,
    /// A model that was clicked before it was downloaded: it is selected once it is ready.
    select_when_ready: Option<String>,
    /// Last saved window size and position.
    window: Option<crate::ui::window_state::WindowState>,
    history: crate::ui::pages::HistoryView,
    /// Snippets and dictionary words as edited (saved together with the settings).
    snippets: Vec<(String, String)>,
    saved_snippets: Vec<(String, String)>,
    dictionary: String,
    saved_dictionary: String,
    /// Index into `edit.profiles` of the profile shown in the Profiles editor.
    selected_profile: usize,
    /// App name typed into the Profiles editor, before "Add".
    new_app: String,
    /// User voice commands (`commands.json`) as edited: spoken phrase and inserted text.
    commands: Vec<(String, String)>,
    saved_commands: Vec<(String, String)>,
    /// Test switch `ME_VOICE_SCREENSHOT`: save the window to this .bmp file once it has settled, then close.
    screenshot: Option<std::path::PathBuf>,
    frames: u32,
    /// Result of the AI page's Test button, filled in by a background thread.
    ai_test: Arc<Mutex<Option<String>>>,
    /// The About page's update check, filled in by a background thread.
    update: Arc<Mutex<UpdateState>>,
}

/// Where a voice command works, shown as a coloured tag on the Words page.
#[derive(Clone, Copy)]
enum Scope {
    Anywhere,
    Whole,
    End,
}

impl Scope {
    fn tag(self) -> (&'static str, egui::Color32) {
        match self {
            Scope::Anywhere => ("ANYWHERE", ACCENT),
            Scope::Whole => ("WHOLE DICTATION", ORANGE),
            Scope::End => ("AT THE END", OK),
        }
    }
}

use crate::ui::theme::{self, group, Icon, ACCENT, GREEN as OK, ORANGE, RED as ERROR};

/// Below this content width, setting rows stack the control under the label.
const NARROW: f32 = 460.0;
/// Below this width the Profiles list sits above its editor instead of beside it.
const WIDE: f32 = 560.0;
/// Model cards on the Speech page: narrowest width before a column is dropped, height, gap.
const CARD_MIN_WIDTH: f32 = 230.0;
const CARD_HEIGHT: f32 = 148.0;
const CARD_GAP: f32 = 12.0;

/// The settings window title; the tray app finds an open window by it.
pub fn window_title() -> &'static str {
    if crate::settings::is_dev() { "me-voice settings (dev)" } else { "me-voice settings" }
}

pub fn run() -> Result<()> {
    crate::settings::migrate_from_mc_voice();
    // One settings window at a time: a second open brings the existing one to the front.
    let Some(_instance) = crate::app::instance::acquire("settings") else {
        let _ = ipc::send_to(&ipc::settings_socket(), Command::ShowSettings);
        return Ok(());
    };

    // Reopen with the size and position it had last time.
    let window = crate::ui::window_state::WindowState::load();
    let mut viewport = egui::ViewportBuilder::default()
            .with_title(window_title())
            .with_inner_size([840.0, 560.0])
            .with_min_inner_size([520.0, 420.0])
            .with_resizable(true)
            .with_icon(egui::IconData {
                rgba: crate::ui::icon::rgba(64, crate::ui::icon::ORANGE),
                width: 64,
                height: 64,
            });
    if let Some(w) = window {
        viewport = w.apply(viewport);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let saved = Settings::load();
    let page = match std::env::var("ME_VOICE_PAGE").as_deref() {
        Ok("speech") => Page::Speech,
        Ok("behaviour") => Page::Behaviour,
        Ok("words") => Page::Words,
        Ok("profiles") => Page::Profiles,
        Ok("ai") => Page::Ai,
        Ok("about") => Page::About,
        Ok("shortcut") => Page::Shortcut,
        Ok("history") => Page::History,
        _ => Page::Dashboard,
    };
    let snippets: Vec<(String, String)> = crate::text::cleanup::load_snippets().into_iter().collect();
    let commands: Vec<(String, String)> = crate::text::cleanup::load_commands().into_iter().collect();
    let dictionary = crate::text::cleanup::load_dictionary().words.join("
");
    let app = SettingsApp {
        edit: saved.clone(),
        saved,
        page,
        models: models::list(),
        microphones: crate::speech::audio::microphones(),
        capturing: None,
        shortcut_error: None,
        download: None,
        select_when_ready: None,
        window,
        history: crate::ui::pages::HistoryView { entries: crate::text::history::load(), ..Default::default() },
        saved_snippets: snippets.clone(),
        snippets,
        saved_commands: commands.clone(),
        commands,
        saved_dictionary: dictionary.clone(),
        dictionary,
        selected_profile: 0,
        new_app: String::new(),
        screenshot: std::env::var_os("ME_VOICE_SCREENSHOT").map(Into::into),
        frames: 0,
        ai_test: Arc::new(Mutex::new(None)),
        update: Arc::new(Mutex::new(UpdateState::NotChecked)),
    };
    let result = eframe::run_native(
        "me-voice settings",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            // Come to the front when someone opens settings again.
            let ctx = cc.egui_ctx.clone();
            let _ = ipc::listen_on(&ipc::settings_socket(), move |cmd| {
                match cmd {
                    Command::ShowSettings => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    _ => {}
                }
                ctx.request_repaint();
            });
            Ok(Box::new(app))
        }),
    );
    // Never leave the main app's shortcut paused.
    let _ = ipc::send(Command::ResumeHotkey);
    result.map_err(|e| anyhow!("settings window: {e}"))
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::ui::text_menu::show_pending(ui.ctx(), frame);
        if self.capturing.is_some() {
            self.read_captured_keys(ui.ctx());
        }
        self.remember_window(ui.ctx());
        self.take_screenshot(ui.ctx());
        let changed = self.edit != self.saved
            || self.snippets != self.saved_snippets
            || self.dictionary != self.saved_dictionary
            || self.commands != self.saved_commands;
        if changed && ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }

        let sidebar = egui::Frame::new().fill(theme::sidebar_fill(ui)).inner_margin(egui::Margin::symmetric(12, 0));
        egui::Panel::left("menu").exact_size(200.0).resizable(false).frame(sidebar).show(ui, |ui| {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                // App logo: the app icon itself, rendered once at 2x for sharp edges.
                let id = egui::Id::unique("app-logo");
                let logo = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                    let image = egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::ui::icon::rgba(64, crate::ui::icon::ORANGE));
                    let texture = ui.ctx().load_texture("app-logo", image, egui::TextureOptions::LINEAR);
                    ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
                    texture
                });
                ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(28.0, 28.0)));
                ui.add_space(4.0);
                ui.label(theme::title("me-voice", 18.0));
                if crate::settings::is_dev() {
                    ui.label(egui::RichText::new("DEV").size(10.5).color(theme::ORANGE).strong());
                }
            });
            ui.add_space(22.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            for (page, icon, label) in [
                (Page::Dashboard, Icon::Grid, "Dashboard"),
                (Page::History, Icon::Clock, "History"),
                (Page::Shortcut, Icon::Keyboard, "Shortcut"),
                (Page::Speech, Icon::Mic, "Speech"),
                (Page::Behaviour, Icon::Sliders, "Behaviour"),
                (Page::Words, Icon::Text, "Words"),
                (Page::Profiles, Icon::Layers, "Profiles"),
                (Page::Ai, Icon::Sparkle, "AI"),
                (Page::About, Icon::Info, "About"),
            ] {
                if theme::nav_item(ui, icon, label, self.page == page) {
                    if page == Page::History || page == Page::Dashboard {
                        self.history.entries = crate::text::history::load();
                    }
                    self.page = page;
                }
            }
        });

        let footer = egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(20, 12));
        egui::Panel::bottom("footer").frame(footer).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(err) = &self.shortcut_error {
                    ui.colored_label(ERROR, err);
                } else if changed {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, ACCENT);
                    ui.label(egui::RichText::new("Unsaved changes").color(ACCENT));
                } else {
                    ui.weak("All changes saved");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = theme::primary_button("Save").min_size(egui::vec2(96.0, 34.0));
                    if ui.add_enabled(changed, save).on_hover_text("Ctrl+S").clicked() {
                        self.save();
                    }
                    let close = egui::Button::new(if changed { "Cancel" } else { "Close" }).min_size(egui::vec2(96.0, 34.0));
                    if ui.add(close).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                // Keep lines readable on wide or maximised windows: a centred column of at most 760 px.
                let width = (ui.available_width() - 64.0).min(760.0);
                let side = (ui.available_width() - width) / 2.0;
                ui.add_space(26.0);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        match self.page {
                            Page::Dashboard => {
                                let stats = crate::text::history::Stats::from(&self.history.entries, self.edit.typing_wpm);
                                crate::ui::pages::dashboard(ui, &stats, &mut self.edit.typing_wpm);
                            }
                            Page::History => crate::ui::pages::history(ui, &mut self.history, &mut self.edit.keep_history),
                            Page::Shortcut => self.shortcut_page(ui),
                            Page::Speech => self.speech_page(ui),
                            Page::Behaviour => self.behaviour_page(ui),
                            Page::Words => self.words_page(ui),
                            Page::Profiles => self.profiles_page(ui),
                            Page::Ai => self.ai_page(ui),
                            Page::About => self.about_page(ui),
                        }
                    });
                });
                ui.add_space(18.0);
            });
        });
    }
}

/// Page title with a one-line explanation.
/// The trimmed setting, or `fallback` when the setting is empty.
fn non_empty(setting: &str, fallback: &str) -> String {
    match setting.trim() {
        "" => fallback.to_string(),
        s => s.to_string(),
    }
}

/// The SAY cell of a built-in command: the phrase in bold, its Dutch alias weak.
fn say_cell(ui: &mut egui::Ui, say: &str, alias: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(say).strong());
        ui.weak(format!("· {alias}"));
    });
}

/// A small coloured label naming where a voice command works.
fn scope_tag(ui: &mut egui::Ui, scope: Scope) {
    let (text, colour) = scope.tag();
    ui.label(egui::RichText::new(text).size(11.0).strong().color(colour));
}

fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.heading(title);
    ui.weak(subtitle);
    ui.add_space(16.0);
}

/// One setting: title and description on the left, control on the right.
/// On narrow windows the control moves under the description.
fn setting_row(ui: &mut egui::Ui, title: &str, description: &str, control: impl FnOnce(&mut egui::Ui)) {
    theme::row_divider(ui);
    ui.add_space(10.0);
    let labels = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new(title).family(theme::semibold()));
        if !description.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(description).weak().size(13.0)).wrap());
        }
    };
    if ui.available_width() >= NARROW {
        ui.horizontal(|ui| {
            let label_width = ui.available_width() * 0.48;
            ui.allocate_ui_with_layout(egui::vec2(label_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(label_width);
                ui.spacing_mut().item_spacing.y = 2.0;
                labels(ui);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
        });
    } else {
        ui.spacing_mut().item_spacing.y = 2.0;
        labels(ui);
        ui.add_space(6.0);
        ui.horizontal(control);
    }
    ui.add_space(10.0);
}

impl SettingsApp {
    /// Saves size and position whenever they change (not while the mouse is still dragging).
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.screenshot else { return };
        self.frames += 1;
        if self.frames == 10 {
            let (path, done) = (path.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
                if let Err(e) = save_bmp(&path, &image) {
                    eprintln!("screenshot: {e}");
                }
                done.send_viewport_cmd(egui::ViewportCommand::Close);
                done.request_repaint();
            })));
        }
        ctx.request_repaint();
    }

    fn remember_window(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let current = crate::ui::window_state::WindowState::current(ctx, self.window);
        if current.is_some() && current != self.window {
            self.window = current;
            if let Some(w) = current {
                w.save();
            }
        }
    }

    fn save(&mut self) {
        // Rows left without a key are dropped rather than saved.
        self.edit.extra_shortcuts.retain(|e| !e.hotkey.is_empty());
        if self.snippets != self.saved_snippets || self.dictionary != self.saved_dictionary || self.commands != self.saved_commands {
            if let Err(e) = self.save_words() {
                self.shortcut_error = Some(format!("Could not save words: {e}"));
                return;
            }
        }
        if let Err(e) = self.edit.save() {
            self.shortcut_error = Some(format!("Could not save: {e}"));
            return;
        }
        if self.edit.start_at_login != self.saved.start_at_login {
            set_start_at_login(self.edit.start_at_login);
        }
        self.saved = self.edit.clone();
        let _ = ipc::send(Command::Reload);
    }

    /// Writes the words and snippets in dictionary.json (keeping its replacements).
    fn save_words(&mut self) -> Result<()> {
        let snippets: crate::text::cleanup::Snippets = self
            .snippets
            .iter()
            .filter(|(p, t)| !p.trim().is_empty() && !t.is_empty())
            .map(|(p, t)| (p.trim().to_string(), t.clone()))
            .collect();
        let mut dict = crate::text::cleanup::load_dictionary();
        dict.words = self.dictionary.lines().map(str::trim).filter(|w| !w.is_empty()).map(String::from).collect();
        dict.snippets = snippets;
        crate::text::cleanup::save_dictionary(&dict)?;

        let commands: crate::text::cleanup::Commands = self
            .commands
            .iter()
            .filter(|(p, t)| !p.trim().is_empty() && !t.is_empty())
            .map(|(p, t)| (p.trim().to_string(), t.clone()))
            .collect();
        std::fs::write(config_dir().join("commands.json"), serde_json::to_string_pretty(&commands)?)?;

        self.saved_snippets = self.snippets.clone();
        self.saved_dictionary = self.dictionary.clone();
        self.saved_commands = self.commands.clone();
        Ok(())
    }
}

/// On/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(44.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let off = ui.visuals().widgets.inactive.bg_fill;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    let fill = egui::Color32::from_rgb(mix(off.r(), ACCENT.r()), mix(off.g(), ACCENT.g()), mix(off.b(), ACCENT.b()));
    let radius = rect.height() / 2.0;
    ui.painter().rect_filled(rect, radius, fill);
    let x = rect.left() + radius + (rect.width() - 2.0 * radius) * t;
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), radius - 3.0, egui::Color32::WHITE);
    response
}

/// Writes an image as an uncompressed 32-bit BMP (for the screenshot test switch).
fn save_bmp(path: &Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let data_len = (w * h * 4) as u32;
    let mut out = Vec::with_capacity(54 + data_len as usize);
    out.extend_from_slice(b"BM");
    for v in [54 + data_len, 0, 54, 40] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes()); // negative: rows top to bottom
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for v in [0u32, data_len, 2835, 2835, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for c in &image.pixels {
        out.extend_from_slice(&[c.b(), c.g(), c.r(), 255]);
    }
    std::fs::write(path, out)
}

fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Windows only: if the old "mc-voice" start-at-login entry exists, removes it and adds
/// the me-voice one instead. Returns whether it did.
pub fn replace_old_login_entry() -> bool {
    #[cfg(target_os = "windows")]
    {
        let reg = |args: &[&str]| std::process::Command::new("reg").args(args).output().is_ok_and(|o| o.status.success());
        if reg(&["query", RUN_KEY, "/v", "mc-voice"]) {
            reg(&["delete", RUN_KEY, "/v", "mc-voice", "/f"]);
            set_start_at_login(true);
            return true;
        }
    }
    false
}

/// Windows only for now: adds or removes me-voice in the per-user Run registry key.
fn set_start_at_login(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        const KEY: &str = RUN_KEY;
        let mut cmd = std::process::Command::new("reg");
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return };
            cmd.args(["add", KEY, "/v", "me-voice", "/t", "REG_SZ", "/d"]).arg(exe).arg("/f");
        } else {
            cmd.args(["delete", KEY, "/v", "me-voice", "/f"]);
        }
        let _ = cmd.output();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
}

