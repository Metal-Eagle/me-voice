//! User settings, stored as `settings.json` in the OS config folder.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InsertMode {
    Paste,
    Type,
    /// Sends the text to the T3 Code thread opened last, as its next message.
    T3,
}

/// How the dictation shortcut starts and stops listening.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyMode {
    /// Tap to latch on, or hold and release to stop.
    #[default]
    HoldOrTap,
    /// Hold to talk; releasing always stops.
    HoldOnly,
    /// Press to start, press again to stop; releasing does nothing.
    TapOnly,
}

/// Where AI clean-up and Command Mode send the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProvider {
    #[default]
    Off,
    /// An OpenAI-compatible server such as Ollama or LM Studio (localhost unless allowed).
    Local,
    /// The Claude CLI (`claude -p`): the text goes to Anthropic.
    ClaudeCli,
}

/// How much of the screen is read as context for a dictation. Never stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextLevel {
    #[default]
    Off,
    /// The program name and the window title.
    Window,
    /// Also the selected text, or the text of the field you type in.
    Text,
}

/// What an extra shortcut does when pressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShortcutAction {
    /// Dictate using the named profile, like the dictation shortcut.
    Dictate { profile: String },
    /// Dictate and send the text to T3 Code, whatever program is in front.
    DictateToT3,
    PasteLast,
    FixLast,
    /// Opens the paragraph window; pressed again it locks in the line and starts the next.
    Paragraph,
    /// Copies the selected text, listens for an instruction and replaces the selection
    /// with the AI's answer.
    Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraShortcut {
    pub hotkey: String,
    pub action: ShortcutAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub hotkey_mode: HotkeyMode,
    /// Shortcuts for other actions (profiles, paste last, fix last).
    pub extra_shortcuts: Vec<ExtraShortcut>,
    pub cancel_key: String,
    pub hold_threshold_ms: u64,
    pub model_id: String,
    /// Model for the live preview while you talk; empty = same as `model_id`.
    pub preview_model_id: String,
    pub language: String,
    /// Microphone name; `None` = system default.
    pub microphone: Option<String>,
    pub insert_mode: InsertMode,
    /// Minutes of idle time before the model is unloaded. 0 = right away, negative = never.
    pub unload_after_minutes: i64,
    pub sounds: bool,
    /// Save the text of each dictation in the History page (numbers are always kept).
    pub keep_history: bool,
    /// Your typing speed, used for "time saved" on the dashboard.
    pub typing_wpm: u32,
    pub start_at_login: bool,
    /// Download and install new releases from GitHub by itself.
    pub auto_update: bool,
    pub remove_fillers: bool,
    pub spoken_formatting: bool,
    /// Voice commands at the end of a dictation: stop words stop listening, send words
    /// also press Enter after pasting. Comma-separated.
    pub voice_commands: bool,
    pub stop_words: String,
    pub send_words: String,
    /// Press Enter after pasting, for every dictation (the send word does it once).
    pub press_enter_after_paste: bool,
    /// Remove the full stop at the end of a dictation ("..." is kept).
    pub drop_final_period: bool,
    /// Per-app profiles: each changes only the settings it lists.
    pub profiles: Vec<Profile>,
    /// Cut quiet parts from the start and end, and shorten long pauses, before transcribing.
    pub trim_silence: bool,
    /// Stop by itself after this many seconds of silence (only after a tap to start). 0 = off.
    pub auto_stop_seconds: u32,
    /// Quieter than this counts as silence (0.0 to 1.0, same scale as the sound wave).
    pub silence_level: f32,
    /// Paragraph window: start a new line after this many seconds of silence. 0 = off.
    pub paragraph_pause_seconds: f32,
    pub ai_provider: AiProvider,
    /// Polish every dictation with the AI (when a provider is set).
    pub ai_polish: bool,
    /// What the AI should do with a dictation, e.g. "fix grammar, keep my words".
    pub ai_style: String,
    /// Base URL of the local server, e.g. `http://localhost:11434/v1`.
    pub ai_url: String,
    pub ai_model: String,
    /// Allow an `ai_url` that is not on this computer.
    pub ai_allow_remote: bool,
    /// Model alias passed to `claude --model`, e.g. `haiku`.
    pub claude_model: String,
    /// Give up on the AI after this many seconds and paste the text without it.
    pub ai_timeout_seconds: u32,
    pub context: ContextLevel,
    /// Also use the clipboard text as context.
    pub context_clipboard: bool,
    /// Send the context to Claude too (local servers always get it).
    pub context_to_claude: bool,
}

/// A named set of setting changes for some apps (see `profiles::effective`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    /// Lower-case program names, e.g. `ms-teams.exe`.
    pub apps: Vec<String>,
    pub overrides: ProfileOverrides,
}

/// Settings a profile changes. `None` means "same as Default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_mode: Option<InsertMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub press_enter_after_paste: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_final_period: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_fillers: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spoken_formatting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_polish: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_style: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "F5".into(),
            hotkey_mode: HotkeyMode::default(),
            extra_shortcuts: Vec::new(),
            cancel_key: "Escape".into(),
            hold_threshold_ms: 300,
            model_id: "futo-en-244".into(),
            preview_model_id: "futo-en-39".into(),
            language: "en".into(),
            microphone: None,
            insert_mode: InsertMode::Paste,
            unload_after_minutes: 5,
            sounds: true,
            keep_history: true,
            typing_wpm: 40,
            start_at_login: false,
            auto_update: true,
            remove_fillers: true,
            spoken_formatting: true,
            voice_commands: true,
            stop_words: "stop".into(),
            send_words: "enter, send".into(),
            press_enter_after_paste: false,
            drop_final_period: false,
            profiles: Vec::new(),
            trim_silence: true,
            auto_stop_seconds: 2,
            silence_level: crate::speech::audio::DEFAULT_SILENCE_LEVEL,
            paragraph_pause_seconds: 1.5,
            ai_provider: AiProvider::Off,
            ai_polish: true,
            ai_style: "Fix grammar, punctuation and capitalisation. Keep my words and meaning.".into(),
            ai_url: "http://localhost:11434/v1".into(),
            ai_model: "qwen2.5:3b".into(),
            ai_allow_remote: false,
            claude_model: "haiku".into(),
            ai_timeout_seconds: 10,
            context: ContextLevel::Off,
            context_clipboard: false,
            context_to_claude: false,
        }
    }
}

fn dirs() -> ProjectDirs {
    ProjectDirs::from("", "", "me-voice").expect("no home directory found")
}

/// The app used to be called mc-voice. Moves its settings, history and models folders
/// to the me-voice names (once, when the new ones don't exist yet) and swaps the old
/// start-at-login entry for the new one. A move fails harmlessly while the old app is
/// still running; it is tried again on the next start.
pub fn migrate_from_mc_voice() {
    if home_override().is_some() {
        return;
    }
    let Some(old) = ProjectDirs::from("", "", "mc-voice") else { return };
    let new = dirs();
    // The app's own folder, e.g. %APPDATA%\mc-voice, which also holds the -dev variants.
    let root = |p: &Path, name: &str| p.ancestors().find(|a| a.file_name().is_some_and(|n| n == name)).map(Path::to_path_buf);
    let mut notes = Vec::new();
    for (from, to) in [(old.config_dir(), new.config_dir()), (old.data_local_dir(), new.data_local_dir())] {
        let (Some(from), Some(to)) = (root(from, "mc-voice"), root(to, "me-voice")) else { continue };
        if from.exists() && !to.exists() {
            match fs::rename(&from, &to) {
                Ok(()) => notes.push(format!("moved {} to {}", from.display(), to.display())),
                Err(e) => notes.push(format!("could not move {}: {e}", from.display())),
            }
        }
    }
    if crate::ui::settings::replace_old_login_entry() {
        notes.push("replaced the mc-voice start-at-login entry".into());
    }
    // Log only after the moves: writing the log first would create the new folder.
    notes.iter().for_each(|n| crate::app::log::write(n));
}

/// Optional override: `ME_VOICE_HOME` puts settings, models, history and the log in one
/// folder (portable install, or testing without touching your real settings).
fn home_override() -> Option<PathBuf> {
    std::env::var_os("ME_VOICE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Development mode: a separate instance with its own settings, history, log, lock and
/// sockets, so it can run next to the normal app. On for debug builds and with `--dev`;
/// `ME_VOICE_DEV=0` turns it off, `ME_VOICE_DEV=1` turns it on.
pub fn is_dev() -> bool {
    static DEV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEV.get_or_init(|| match std::env::var("ME_VOICE_DEV").as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        _ => cfg!(debug_assertions) || std::env::args().any(|a| a == "--dev"),
    })
}

/// Added to folder, lock and socket names in development mode.
pub fn instance_suffix() -> &'static str {
    if is_dev() { "-dev" } else { "" }
}

fn with_suffix(dir: PathBuf) -> PathBuf {
    let name = format!("{}{}", dir.file_name().unwrap_or_default().to_string_lossy(), instance_suffix());
    dir.with_file_name(name)
}

pub fn config_dir() -> PathBuf {
    with_suffix(home_override().map(|h| h.join("config")).unwrap_or_else(|| dirs().config_dir().to_path_buf()))
}

/// History, log and lock files.
pub fn data_dir() -> PathBuf {
    with_suffix(base_data_dir())
}

fn base_data_dir() -> PathBuf {
    home_override().map(|h| h.join("data")).unwrap_or_else(|| dirs().data_local_dir().to_path_buf())
}

/// Shared by the normal and the development instance, so models download only once.
pub fn models_dir() -> PathBuf {
    base_data_dir().join("models")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    /// Loads settings, falling back to defaults when the file is missing or broken.
    pub fn load() -> Self {
        fs::read_to_string(settings_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path();
        fs::create_dir_all(path.parent().unwrap())?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_mode_t3_round_trips_as_t3() {
        assert_eq!(serde_json::to_string(&InsertMode::T3).unwrap(), "\"t3\"");
        assert_eq!(serde_json::from_str::<InsertMode>("\"t3\"").unwrap(), InsertMode::T3);
    }

    #[test]
    fn dictate_to_t3_action_round_trips() {
        let json = serde_json::to_string(&ShortcutAction::DictateToT3).unwrap();
        assert_eq!(json, "{\"type\":\"dictateToT3\"}");
        assert_eq!(serde_json::from_str::<ShortcutAction>(&json).unwrap(), ShortcutAction::DictateToT3);
    }
}
