//! The tray app: waits for the shortcut, records, transcribes and pastes.
//!
//! Everything is event driven. Between dictations the event loop sleeps
//! (`ControlFlow::Wait`), the microphone is closed and, after the idle
//! timeout, the model is unloaded from memory.

pub mod apps;
pub mod instance;
pub mod ipc;
pub mod log;
pub mod power;
pub mod tray;
pub mod update;

mod dictation;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::MenuEvent;

use crate::settings::{AiProvider, ContextLevel, HotkeyMode, Settings, ShortcutAction};
use crate::speech::audio::{self, Recording};
use crate::speech::models;
use crate::speech::sound::{self, Beep};
use crate::speech::transcriber::{Request, Transcriber, SAMPLE_RATE};
use crate::text::cleanup;
use crate::text::context::Context;
use crate::text::insert;
use crate::text::profiles;
use crate::ui::overlay::{self, Overlay, Phase};

pub use dictation::transcribe;
use dictation::*;
use ipc::Command;
use tray::{Status, Tray};

/// Clips shorter than this are treated as an accidental press.
const MIN_CLIP: Duration = Duration::from_millis(300);
/// Sound-wave refresh rate while listening (the only time the app redraws anything).
const FRAME: Duration = Duration::from_millis(33);
/// Re-run live transcription once this much new audio has come in.
const PARTIAL_EVERY_SECONDS: f32 = 0.3;
/// The live preview only re-reads this many recent seconds.
const PARTIAL_WINDOW_SECONDS: usize = 10;
/// How long the final text stays in the pill after pasting.
const SHOW_RESULT: Duration = Duration::from_millis(1800);
/// First automatic update check after start, then every `UPDATE_EVERY`.
const UPDATE_FIRST: Duration = Duration::from_secs(60);
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

enum UserEvent {
    HotKey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    Tray(tray_icon::TrayIconEvent),
    Ipc(Command),
    Transcribed(Result<Dictation>),
    /// The text went to the AI; the pill shows it.
    Polishing,
    /// Live (partial) transcription while still recording.
    Partial(String),
    Update(UpdateResult),
}

/// Result of an update check from the worker thread.
enum UpdateResult {
    UpToDate,
    /// Downloaded and ready to install.
    Ready { version: String, installer: std::path::PathBuf },
    /// Newer, but it can't be installed by itself (no installer, or not Windows).
    Available { version: String },
    Failed(String),
}

enum State {
    Idle,
    /// `latched` = started with a tap, so the next press stops it.
    Recording { rec: Recording, pressed_at: Instant, latched: bool },
    Transcribing,
}

struct App {
    settings: Settings,
    hotkeys: GlobalHotKeyManager,
    dictation: Option<HotKey>,
    cancel: Option<HotKey>,
    hotkey_paused: bool,
    state: State,
    tray: Tray,
    overlay: Option<Overlay>,
    /// Words recognised so far, shown in the pill.
    live_text: String,
    partial_running: bool,
    /// Live preview runs in this dictation (logged, to see that it works).
    partial_count: u32,
    /// Tells a running live preview to stop, so the final transcription can start at once.
    partial_abort: Arc<std::sync::atomic::AtomicBool>,
    partial_seconds: f32,
    quit_requested: bool,
    /// Length of the last recording, for the history.
    clip_seconds: f32,
    /// After pasting, the pill shows the result until this moment.
    show_result_until: Option<Instant>,
    transcriber: Arc<Mutex<Transcriber>>,
    proxy: EventLoopProxy<UserEvent>,
    /// Program in front when the current dictation started (for profiles and the dashboard).
    target_app: Option<String>,
    /// Profile name forced by an extra shortcut for the next dictation; `None` = pick by app.
    forced_profile: Option<String>,
    /// Settings for the current dictation: `settings` with the matching profile applied.
    active: Settings,
    /// Name of the profile in use, `None` when no profile matched (shown as a badge).
    active_profile: Option<String>,
    /// Extra shortcuts that are registered, by hotkey id.
    extra: HashMap<u32, (HotKey, ShortcutAction)>,
    /// Hotkey id of the shortcut that started the current dictation.
    started_by: Option<u32>,
    /// Last dictation, kept in memory only (for paste again, copy and fix).
    last_text: String,
    /// Seconds left before an automatic stop, shown in the pill (`None` = no countdown).
    countdown: Option<f32>,
    /// The current dictation is a Command Mode instruction for the selected text.
    command_mode: bool,
    /// Screen context read when the dictation started, arriving from a background thread.
    context: Option<std::sync::mpsc::Receiver<Context>>,
    /// The AI provider, started with the dictation so it is ready when the text is.
    warm: Option<crate::text::polish::Warm>,
    /// The AI is working on the text (shown in the pill).
    polishing: bool,
    /// An update check or download is running.
    update_running: bool,
    /// The user asked for this check from the menu, so "up to date" is shown too.
    update_manual: bool,
    next_update_check: Instant,
    /// Downloaded installer, run as soon as no dictation is in progress.
    pending_install: Option<std::path::PathBuf>,
}

/// What a shortcut event does to the dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Stop,
    /// Keep recording and remember that it was latched by a tap.
    Latch,
    Nothing,
}

/// The dictation as the shortcut sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    /// `latched`: started with a tap; `held_for`: how long the key has been down.
    Recording { latched: bool, held_for: Duration },
    /// Transcribing: presses do nothing.
    Busy,
}

/// Decides what a shortcut press or release does, depending on the shortcut mode.
fn decide(mode: HotkeyMode, stage: Stage, edge: HotKeyState, threshold: Duration) -> Step {
    match (stage, edge) {
        (Stage::Idle, HotKeyState::Pressed) => Step::Start,
        (Stage::Recording { latched: true, .. }, HotKeyState::Pressed) => Step::Stop,
        (Stage::Recording { latched: false, held_for }, HotKeyState::Released) => match mode {
            HotkeyMode::HoldOrTap if held_for < threshold => Step::Latch,
            HotkeyMode::TapOnly => Step::Nothing,
            HotkeyMode::HoldOrTap | HotkeyMode::HoldOnly => Step::Stop,
        },
        _ => Step::Nothing,
    }
}

pub fn run() -> Result<()> {
    crate::settings::migrate_from_mc_voice();
    // Only one tray app at a time: a second start just opens the settings window.
    let Some(_instance) = instance::acquire("app") else {
        let _ = ipc::send(Command::ShowSettings);
        return Ok(());
    };

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    GlobalHotKeyEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::HotKey(e));
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));
    let proxy = event_loop.create_proxy();
    tray_icon::TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Tray(e));
    }));
    let proxy = Mutex::new(event_loop.create_proxy());
    ipc::listen(move |cmd| {
        let _ = proxy.lock().unwrap().send_event(UserEvent::Ipc(cmd));
    })?;

    let mut app = App {
        settings: Settings::load(),
        active: Settings::load(),
        hotkeys: GlobalHotKeyManager::new().map_err(|e| anyhow!("shortcuts: {e}"))?,
        dictation: None,
        cancel: None,
        hotkey_paused: false,
        state: State::Idle,
        tray: Tray::new()?,
        overlay: None,
        live_text: String::new(),
        partial_running: false,
        partial_count: 0,
        partial_abort: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        partial_seconds: 0.0,
        show_result_until: None,
        clip_seconds: 0.0,
        quit_requested: false,
        transcriber: Arc::new(Mutex::new(Transcriber::new())),
        proxy: event_loop.create_proxy(),
        target_app: None,
        forced_profile: None,
        active_profile: None,
        extra: HashMap::new(),
        started_by: None,
        last_text: String::new(),
        countdown: None,
        command_mode: false,
        context: None,
        polishing: false,
        warm: None,
        update_running: false,
        update_manual: false,
        next_update_check: Instant::now() + UPDATE_FIRST,
        pending_install: None,
    };
    app.register_hotkey();

    // First run: open settings so the model can be downloaded.
    let model_missing = models::find(&app.settings.model_id).map_or(true, |m| !m.is_downloaded());
    if model_missing {
        open_settings();
    }

    event_loop.run(move |event, target, control_flow| {
        match event {
            Event::UserEvent(UserEvent::HotKey(e)) => app.on_hotkey(e),
            Event::UserEvent(UserEvent::Menu(e)) => {
                if e.id == *app.tray.settings.id() {
                    open_settings();
                } else if e.id == *app.tray.paste_last.id() {
                    app.paste_last();
                } else if e.id == *app.tray.fix_last.id() {
                    app.fix_last();
                } else if e.id == *app.tray.copy_last.id() {
                    app.copy_last();
                } else if e.id == *app.tray.check_update.id() {
                    app.check_for_update(true);
                } else if e.id == *app.tray.quit.id() {
                    app.quit_requested = true;
                }
            }
            // Left-click on the tray icon opens settings.
            Event::UserEvent(UserEvent::Tray(tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            })) => open_settings(),
            Event::UserEvent(UserEvent::Ipc(cmd)) => app.on_ipc(cmd),
            Event::UserEvent(UserEvent::Transcribed(result)) => app.on_transcribed(result),
            Event::UserEvent(UserEvent::Polishing) => app.polishing = true,
            Event::UserEvent(UserEvent::Update(result)) => app.on_update(result),
            Event::UserEvent(UserEvent::Partial(text)) => {
                app.partial_running = false;
                if matches!(app.state, State::Recording { .. }) {
                    // "…stop" / "…send" at the end: stop listening right away.
                    // "scratch that" is only a whole-dictation command, so it doesn't stop early.
                    let command = cleanup::voice_command(&text, &app.settings).filter(|(c, _)| *c != cleanup::VoiceCommand::Scratch);
                    if let Some((command, rest)) = command {
                        log::write(&format!("voice command: {command:?}"));
                        app.live_text = rest;
                        app.stop_recording();
                    } else if !text.trim().is_empty() {
                        app.live_text = text;
                    }
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => app.unload_if_idle(),
            Event::RedrawRequested(id) => {
                if let Some(o) = app.overlay.as_mut().filter(|o| o.window_id() == id) {
                    o.paint();
                }
                return;
            }
            _ => {}
        }
        if app.settings.auto_update && Instant::now() >= app.next_update_check {
            app.check_for_update(false);
        }
        if app.pending_install.is_some() && matches!(app.state, State::Idle) {
            app.install_update();
        }
        if app.quit_requested {
            // Quit means everything: close an open settings window too.
            if *control_flow != ControlFlow::Exit {
                let _ = ipc::send_to(&ipc::settings_socket(), Command::Quit);
                log::write("quit");
            }
            app.overlay = None;
            *control_flow = ControlFlow::Exit;
            return;
        }
        app.check_auto_stop();
        app.update_overlay(target);
        *control_flow = app.next_wake();
    });
}

impl App {
    fn register_hotkey(&mut self) {
        self.unregister_hotkeys();
        match self.settings.hotkey.parse::<HotKey>() {
            Ok(hk) => match self.hotkeys.register(hk) {
                Ok(()) => {
                    log::write(&format!("shortcut {} registered", self.settings.hotkey));
                    self.dictation = Some(hk);
                }
                Err(e) => log::write(&format!("could not register shortcut {}: {e}", self.settings.hotkey)),
            },
            Err(e) => log::write(&format!("invalid shortcut {}: {e}", self.settings.hotkey)),
        }
        for extra in self.settings.extra_shortcuts.clone() {
            match extra.hotkey.parse::<HotKey>() {
                Ok(hk) => match self.hotkeys.register(hk) {
                    Ok(()) => {
                        log::write(&format!("shortcut {} registered", extra.hotkey));
                        self.extra.insert(hk.id(), (hk, extra.action));
                    }
                    Err(e) => log::write(&format!("could not register shortcut {}: {e}", extra.hotkey)),
                },
                Err(e) => log::write(&format!("invalid shortcut {}: {e}", extra.hotkey)),
            }
        }
    }

    /// Releases the dictation and extra shortcuts (the cancel key is handled separately).
    fn unregister_hotkeys(&mut self) {
        if let Some(old) = self.dictation.take() {
            let _ = self.hotkeys.unregister(old);
        }
        for (_, (hk, _)) in self.extra.drain() {
            let _ = self.hotkeys.unregister(hk);
        }
    }

    /// The cancel key is only registered while recording, so it never steals Esc otherwise.
    fn set_cancel_key(&mut self, active: bool) {
        if let Some(old) = self.cancel.take() {
            let _ = self.hotkeys.unregister(old);
        }
        if active {
            if let Ok(hk) = self.settings.cancel_key.parse::<HotKey>() {
                if self.hotkeys.register(hk).is_ok() {
                    self.cancel = Some(hk);
                }
            }
        }
    }

    fn on_hotkey(&mut self, e: GlobalHotKeyEvent) {
        log::write(&format!("shortcut {:?}", e.state));
        if self.cancel.is_some_and(|c| c.id() == e.id) {
            if e.state == HotKeyState::Pressed {
                self.cancel_recording();
            }
            return;
        }
        if self.hotkey_paused {
            return;
        }
        let is_main = self.dictation.is_some_and(|d| d.id() == e.id);
        // Profile for an extra dictation shortcut; `None` for the main shortcut.
        let mut profile = None;
        let mut command = false;
        if !is_main {
            let action = match self.extra.get(&e.id) {
                Some((_, action)) => action.clone(),
                None => return,
            };
            match action {
                ShortcutAction::Dictate { profile: name } => profile = Some(name),
                ShortcutAction::PasteLast if e.state == HotKeyState::Pressed => self.paste_last(),
                ShortcutAction::FixLast if e.state == HotKeyState::Pressed => self.fix_last(),
                ShortcutAction::Paragraph if e.state == HotKeyState::Pressed => crate::ui::paragraph_window::open_or_new_line(),
                ShortcutAction::Command => command = true,
                _ => {}
            }
            if profile.is_none() && !command {
                return;
            }
        }
        // While recording, only the shortcut that started it, or the main one, can stop it.
        if !is_main && !matches!(self.state, State::Idle) && self.started_by != Some(e.id) {
            return;
        }
        let stage = match &self.state {
            State::Idle => Stage::Idle,
            State::Recording { pressed_at, latched, .. } => Stage::Recording { latched: *latched, held_for: pressed_at.elapsed() },
            State::Transcribing => Stage::Busy,
        };
        let threshold = Duration::from_millis(self.settings.hold_threshold_ms);
        match decide(self.settings.hotkey_mode, stage, e.state, threshold) {
            Step::Start => {
                self.forced_profile = profile.filter(|p| !p.trim().is_empty());
                self.started_by = Some(e.id);
                self.command_mode = command;
                self.start_recording(self.settings.hotkey_mode == HotkeyMode::TapOnly);
            }
            Step::Stop => self.stop_recording(),
            Step::Latch => {
                if let State::Recording { latched, .. } = &mut self.state {
                    *latched = true;
                }
            }
            Step::Nothing => {}
        }
    }

    fn on_ipc(&mut self, cmd: Command) {
        match cmd {
            Command::NewLine => {}
            Command::Toggle => match self.state {
                State::Idle => {
                    self.forced_profile = None;
                    self.command_mode = false;
                    self.start_recording(true);
                }
                State::Recording { .. } => self.stop_recording(),
                State::Transcribing => {}
            },
            Command::Reload => {
                let old_model = self.settings.model_id.clone();
                self.settings = Settings::load();
                self.active = self.settings.clone();
                self.register_hotkey();
                if self.settings.model_id != old_model {
                    self.transcriber.lock().unwrap().unload();
                }
            }
            // Fully release the shortcut while the settings window captures a new one,
            // otherwise Windows keeps swallowing that key combination.
            Command::PauseHotkey => {
                self.hotkey_paused = true;
                self.unregister_hotkeys();
            }
            Command::ResumeHotkey => {
                self.hotkey_paused = false;
                self.settings = Settings::load();
                self.register_hotkey();
            }
            Command::ShowSettings => open_settings(),
            Command::Quit => self.quit_requested = true,
        }
    }

    fn start_recording(&mut self, latched: bool) {
        self.target_app = crate::text::foreground::app_name();
        let forced = self.forced_profile.take();
        self.active = profiles::effective(&self.settings, self.target_app.as_deref(), forced.as_deref());
        self.active_profile = profiles::matching(&self.settings, self.target_app.as_deref(), forced.as_deref()).map(|p| p.name.clone());
        let model_ready = models::find(&self.active.model_id).is_some_and(|m| m.is_downloaded());
        if !model_ready {
            log::write(&format!("model {} is not downloaded; opening settings", self.active.model_id));
            open_settings();
            return;
        }
        match audio::start(self.settings.microphone.as_deref()) {
            Ok(rec) => {
                self.state = State::Recording { rec, pressed_at: Instant::now(), latched };
                self.live_text.clear();
                self.partial_seconds = 0.0;
                self.partial_count = 0;
                self.show_result_until = None;
                self.polishing = false;
                self.context = start_context(&self.active);
                self.warm = Some(crate::text::polish::warm_up(&self.active, self.command_mode, &self.active.ai_style));
                self.tray.set_status(Status::Recording);
                self.set_cancel_key(true);
                if self.settings.sounds {
                    sound::play(Beep::Start);
                }
                log::write("listening");
            }
            Err(e) => log::write(&format!("microphone: {e}")),
        }
    }

    fn cancel_recording(&mut self) {
        self.warm = None;
        self.partial_abort.store(true, std::sync::atomic::Ordering::Relaxed);
        if let State::Recording { rec, .. } = std::mem::replace(&mut self.state, State::Idle) {
            drop(rec.stop());
        }
        self.set_cancel_key(false);
        self.tray.set_status(Status::Idle);
    }

    fn stop_recording(&mut self) {
        let State::Recording { rec, .. } = std::mem::replace(&mut self.state, State::Transcribing) else {
            return;
        };
        self.set_cancel_key(false);
        self.partial_abort.store(true, std::sync::atomic::Ordering::Relaxed);
        log::write(&format!("live preview updated {} times", self.partial_count));
        let samples = rec.stop();
        self.clip_seconds = samples.len() as f32 / SAMPLE_RATE as f32;
        // Quiet parts are cut before transcribing: faster, and no made-up words from silence.
        let samples = if self.settings.trim_silence {
            audio::trim_silence(&samples, self.settings.silence_level)
        } else {
            samples
        };
        if self.settings.sounds {
            sound::play(Beep::Stop);
        }
        if samples.len() < SAMPLE_RATE * MIN_CLIP.as_millis() as usize / 1000 {
            self.warm = None;
            self.state = State::Idle;
            self.tray.set_status(Status::Idle);
            return;
        }
        self.tray.set_status(Status::Transcribing);

        let settings = self.active.clone();
        let transcriber = self.transcriber.clone();
        let proxy = self.proxy.clone();
        let command_mode = self.command_mode;
        let context = self.context.take();
        let warm = self.warm.take().unwrap_or_else(crate::text::polish::Warm::none);
        std::thread::spawn(move || {
            let started = Instant::now();
            power::set_priority(power::Priority::High);
            let result = finish(&settings, &transcriber, &samples, command_mode, context, warm, &proxy);
            power::set_priority(power::Priority::Normal);
            log::write(&format!(
                "transcribed {:.1} s of audio in {:.2} s",
                samples.len() as f32 / SAMPLE_RATE as f32,
                started.elapsed().as_secs_f32()
            ));
            let _ = proxy.send_event(UserEvent::Transcribed(result));
        });
    }

    fn on_transcribed(&mut self, result: Result<Dictation>) {
        self.state = State::Idle;
        self.polishing = false;
        self.tray.set_status(Status::Idle);
        match result {
            Ok(Dictation { text, command }) => {
                // A stop/send word at the end is a command, not part of the text.
                let (text, send) = match command {
                    Some(cleanup::VoiceCommand::Scratch) => {
                        // "scratch that": undo the previous paste; nothing is pasted (text is empty).
                        log::write("voice command: scratch");
                        std::thread::spawn(|| {
                            if let Err(e) = insert::undo() {
                                log::write(&e.to_string());
                            }
                        });
                        (String::new(), false)
                    }
                    Some(command) => (text, command == cleanup::VoiceCommand::Send),
                    None => (text, false),
                };
                let text = if self.active.drop_final_period && !self.command_mode { drop_final_period(&text) } else { text };
                let press_enter = send || (self.active.press_enter_after_paste && !text.is_empty());
                let words = crate::text::history::count_words(&text);
                if words > 0 {
                    crate::text::history::append(&crate::text::history::Entry {
                        time: chrono::Local::now().timestamp(),
                        seconds: self.clip_seconds,
                        words,
                        text: if self.active.keep_history { text.clone() } else { String::new() },
                        app: self.target_app.clone(),
                    });
                    self.last_text = text.clone();
                    self.tray.set_last(&text);
                }
                self.live_text = text.clone();
                self.show_result_until = Some(Instant::now() + SHOW_RESULT);
                log::write(&format!("transcribed {} characters", text.chars().count()));
                // On its own thread: pasting waits for keys to be released and for the
                // target app, and the event loop (the card) must keep running meanwhile.
                // ME_VOICE_NO_INSERT=1: don't paste (stress tests must not type into other windows).
                let skip_insert = std::env::var("ME_VOICE_NO_INSERT").as_deref() == Ok("1");
                let mode = self.active.insert_mode;
                let paste_text = text.clone();
                std::thread::spawn(move || {
                    if skip_insert {
                        return;
                    }
                    if let Err(e) = insert::insert(&paste_text, mode, press_enter) {
                        log::write(&e.to_string());
                    }
                });
            }
            Err(e) => log::write(&e.to_string()),
        }
        if self.settings.unload_after_minutes == 0 {
            self.transcriber.lock().unwrap().unload();
        }
    }

    /// Paste the last dictation again, with the current insert mode.
    fn paste_last(&mut self) {
        if self.last_text.is_empty() {
            return;
        }
        let text = self.last_text.clone();
        let mode = self.settings.insert_mode;
        std::thread::spawn(move || {
            if let Err(e) = insert::insert(&text, mode, false) {
                log::write(&e.to_string());
            }
        });
    }

    /// Copy the last dictation to the clipboard.
    fn copy_last(&mut self) {
        if self.last_text.is_empty() {
            return;
        }
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(self.last_text.clone())) {
            Ok(()) => log::write("copied last dictation"),
            Err(e) => log::write(&format!("copy: {e}")),
        }
    }

    /// Open the "Fix last dictation" window with the last dictation.
    fn fix_last(&mut self) {
        if !self.last_text.is_empty() {
            crate::ui::fix_window::spawn(&self.last_text);
        }
    }

    fn unload_deadline(&self) -> Option<Instant> {
        let minutes = self.settings.unload_after_minutes;
        if minutes <= 0 || !matches!(self.state, State::Idle) {
            return None;
        }
        let t = self.transcriber.lock().unwrap();
        t.is_loaded().then(|| t.last_used + Duration::from_secs(minutes as u64 * 60))
    }

    fn unload_if_idle(&mut self) {
        if self.unload_deadline().is_some_and(|d| Instant::now() >= d) {
            self.transcriber.lock().unwrap().unload();
        }
    }

    /// Sleep until the next event, or until the model should be unloaded.
    /// The pill exists only while dictating and briefly afterwards.
    fn update_overlay(&mut self, target: &tao::event_loop::EventLoopWindowTarget<UserEvent>) {
        let showing_result = self.show_result_until.is_some_and(|t| Instant::now() < t);
        let (phase, levels) = match &self.state {
            State::Recording { rec, .. } => (Phase::Listening, rec.levels(overlay::BARS)),
            State::Transcribing if self.polishing => {
                (Phase::Polishing(self.active.ai_provider == AiProvider::ClaudeCli), vec![0.0; overlay::BARS])
            }
            State::Transcribing => (Phase::Transcribing, vec![0.0; overlay::BARS]),
            State::Idle if showing_result => (Phase::Done, vec![0.0; overlay::BARS]),
            State::Idle => {
                self.overlay = None;
                self.show_result_until = None;
                return;
            }
        };
        if self.overlay.is_none() {
            self.overlay = Overlay::open(target);
        }
        let badge = self.active_profile.clone();
        if let Some(o) = self.overlay.as_mut() {
            o.set_badge(badge);
            o.set_countdown(self.countdown);
            o.update(phase, levels, &self.live_text);
        }
        if phase == Phase::Listening {
            self.maybe_start_partial();
        }
    }

    /// Stops a tap-started dictation after the chosen seconds of silence, and sets the
    /// countdown shown in the pill during the last seconds. Runs on the frame tick.
    fn check_auto_stop(&mut self) {
        let limit = self.settings.auto_stop_seconds as f32;
        let level = self.settings.silence_level;
        let State::Recording { rec, latched: true, .. } = &self.state else {
            self.countdown = None;
            return;
        };
        let (left, quiet) = match rec.silent_for(level) {
            Some(quiet) if limit > 0.0 => (limit - quiet.as_secs_f32(), quiet.as_secs_f32()),
            _ => (f32::MAX, 0.0),
        };
        if left <= 0.0 {
            log::write("stopped after silence");
            self.stop_recording();
            return;
        }
        // Only once you have actually gone quiet for a moment, so the words stay visible
        // while you talk (also when the limit is no longer than the countdown).
        self.countdown = (left <= overlay::COUNTDOWN_SECONDS && quiet >= 0.5).then_some(left);
    }

    /// Live transcription: re-transcribe what was said so far, one run at a time.
    fn maybe_start_partial(&mut self) {
        let State::Recording { rec, .. } = &self.state else { return };
        let seconds = rec.seconds();
        if self.partial_running || seconds - self.partial_seconds < PARTIAL_EVERY_SECONDS {
            return;
        }
        // Nothing new to recognise while you're silent: skip the work.
        if !rec.has_sound(self.settings.silence_level) {
            return;
        }
        self.partial_running = true;
        self.partial_count += 1;
        self.partial_seconds = seconds;
        // Only the last few seconds: enough for the newest words, and fast even when you talk long.
        let mut samples = rec.snapshot();
        let keep = SAMPLE_RATE * PARTIAL_WINDOW_SECONDS;
        if samples.len() > keep {
            samples.drain(..samples.len() - keep);
        }
        let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.partial_abort = abort.clone();
        let settings = self.active.clone();
        let transcriber = self.transcriber.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            power::set_priority(power::Priority::AboveNormal);
            let text = match transcribe(&settings, &transcriber, &samples, true, Some(abort.clone())) {
                Ok(text) => text,
                Err(e) => {
                    log::write(&format!("live preview: {e}"));
                    String::new()
                }
            };
            if !abort.load(std::sync::atomic::Ordering::Relaxed) {
                power::set_priority(power::Priority::Normal);
            }
            let _ = proxy.send_event(UserEvent::Partial(text));
        });
    }

    fn next_wake(&self) -> ControlFlow {
        if matches!(self.state, State::Recording { .. } | State::Transcribing) {
            return ControlFlow::WaitUntil(Instant::now() + FRAME);
        }
        if let Some(until) = self.show_result_until {
            return ControlFlow::WaitUntil(until.max(Instant::now()));
        }
        let update = self.settings.auto_update.then_some(self.next_update_check);
        match self.unload_deadline().into_iter().chain(update).min() {
            Some(deadline) => ControlFlow::WaitUntil(deadline.max(Instant::now())),
            None => ControlFlow::Wait,
        }
    }

    /// Checks GitHub for a newer release and downloads it, on a worker thread.
    fn check_for_update(&mut self, manual: bool) {
        self.next_update_check = Instant::now() + UPDATE_EVERY;
        if self.update_running || self.pending_install.is_some() {
            return;
        }
        self.update_running = true;
        self.update_manual = manual;
        self.tray.set_updating(true);
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = match update::check() {
                Ok(None) => UpdateResult::UpToDate,
                Ok(Some(release)) if update::can_install(&release) => match update::download(&release) {
                    Ok(installer) => UpdateResult::Ready { version: release.version, installer },
                    Err(e) => UpdateResult::Failed(format!("{e:#}")),
                },
                Ok(Some(release)) => UpdateResult::Available { version: release.version },
                Err(e) => UpdateResult::Failed(format!("{e:#}")),
            };
            let _ = proxy.send_event(UserEvent::Update(result));
        });
    }

    fn on_update(&mut self, result: UpdateResult) {
        self.update_running = false;
        self.tray.set_updating(false);
        let current = update::CURRENT;
        match result {
            UpdateResult::UpToDate => {
                log::write("update: up to date");
                if self.update_manual {
                    notify("You're up to date", &format!("me-voice {current} is the latest version."));
                }
            }
            UpdateResult::Ready { version, installer } => {
                log::write(&format!("update: {version} downloaded"));
                notify(&format!("Updating to me-voice {version}"), "me-voice restarts in a moment. Your settings are kept.");
                self.pending_install = Some(installer);
            }
            UpdateResult::Available { version } => {
                log::write(&format!("update: {version} available"));
                notify(&format!("me-voice {version} is available"), &format!("Download it from {}", update::releases_page()));
            }
            UpdateResult::Failed(e) => {
                log::write(&format!("update check failed: {e}"));
                if self.update_manual {
                    notify("Couldn't check for updates", &e);
                }
            }
        }
    }

    /// Runs the downloaded installer, which closes this app and starts the new version.
    fn install_update(&mut self) {
        let Some(installer) = self.pending_install.take() else { return };
        match update::install(&installer) {
            Ok(()) => {
                log::write("update: installing");
                self.quit_requested = true;
            }
            Err(e) => {
                log::write(&format!("update install failed: {e:#}"));
                notify("Update failed", &format!("{e:#}"));
            }
        }
    }
}

/// A desktop notification; failures only go to the log.
fn notify(title: &str, body: &str) {
    if let Err(e) = notify_rust::Notification::new().summary(title).body(body).appname("me-voice").show() {
        log::write(&format!("notification failed: {e}"));
    }
}

/// The settings window is a separate short-lived process, so it costs nothing once closed.
/// If it's already open, the settings process brings its window to the front instead.
/// An open window is brought to the front directly, and clicks while a new window is
/// still starting are ignored, so a few quick clicks on the tray icon open one window.
fn open_settings() {
    static LAST_START: Mutex<Option<Instant>> = Mutex::new(None);
    if focus_settings_window() {
        return;
    }
    let mut last = LAST_START.lock().unwrap();
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
        return;
    }
    *last = Some(Instant::now());
    // The tray click gives this process the right to take the foreground; pass it on
    // so the new window opens in front instead of behind other windows.
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
            windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("settings");
        if crate::settings::is_dev() {
            cmd.arg("--dev");
        }
        let _ = cmd.spawn();
    }
}

/// Restores and focuses the settings window if it is open. Returns whether it was found.
#[cfg(windows)]
fn focus_settings_window() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
    let title: Vec<u16> = crate::ui::settings::window_title().encode_utf16().chain([0]).collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return false;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    true
}

#[cfg(not(windows))]
fn focus_settings_window() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAP: Duration = Duration::from_millis(100);
    const HOLD: Duration = Duration::from_millis(500);
    const THRESHOLD: Duration = Duration::from_millis(300);

    fn recording(latched: bool, held_for: Duration) -> Stage {
        Stage::Recording { latched, held_for }
    }

    #[test]
    fn hold_or_tap_tap_latches_and_next_press_stops() {
        let mode = HotkeyMode::HoldOrTap;
        assert_eq!(decide(mode, Stage::Idle, HotKeyState::Pressed, THRESHOLD), Step::Start);
        assert_eq!(decide(mode, recording(false, TAP), HotKeyState::Released, THRESHOLD), Step::Latch);
        assert_eq!(decide(mode, recording(true, TAP), HotKeyState::Pressed, THRESHOLD), Step::Stop);
        assert_eq!(decide(mode, recording(true, TAP), HotKeyState::Released, THRESHOLD), Step::Nothing);
    }

    #[test]
    fn hold_or_tap_holding_then_releasing_stops() {
        assert_eq!(decide(HotkeyMode::HoldOrTap, recording(false, HOLD), HotKeyState::Released, THRESHOLD), Step::Stop);
    }

    #[test]
    fn hold_only_release_always_stops_and_never_latches() {
        let mode = HotkeyMode::HoldOnly;
        assert_eq!(decide(mode, Stage::Idle, HotKeyState::Pressed, THRESHOLD), Step::Start);
        assert_eq!(decide(mode, recording(false, TAP), HotKeyState::Released, THRESHOLD), Step::Stop);
        assert_eq!(decide(mode, recording(false, HOLD), HotKeyState::Released, THRESHOLD), Step::Stop);
    }

    #[test]
    fn tap_only_press_toggles_and_release_is_ignored() {
        let mode = HotkeyMode::TapOnly;
        assert_eq!(decide(mode, Stage::Idle, HotKeyState::Pressed, THRESHOLD), Step::Start);
        assert_eq!(decide(mode, recording(true, HOLD), HotKeyState::Pressed, THRESHOLD), Step::Stop);
        assert_eq!(decide(mode, recording(true, HOLD), HotKeyState::Released, THRESHOLD), Step::Nothing);
        assert_eq!(decide(mode, recording(false, HOLD), HotKeyState::Released, THRESHOLD), Step::Nothing);
    }

    #[test]
    fn presses_while_busy_do_nothing() {
        for mode in [HotkeyMode::HoldOrTap, HotkeyMode::HoldOnly, HotkeyMode::TapOnly] {
            assert_eq!(decide(mode, Stage::Busy, HotKeyState::Pressed, THRESHOLD), Step::Nothing);
            assert_eq!(decide(mode, Stage::Busy, HotKeyState::Released, THRESHOLD), Step::Nothing);
        }
    }
}
