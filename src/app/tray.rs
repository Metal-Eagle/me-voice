//! Tray icon with three states: the app icon with the dot coloured by status.

use anyhow::Result;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Recording,
    Transcribing,
}

pub struct Tray {
    icon: TrayIcon,
    last_info: MenuItem,
    pub paste_last: MenuItem,
    pub fix_last: MenuItem,
    pub copy_last: MenuItem,
    pub settings: MenuItem,
    pub check_update: MenuItem,
    pub quit: MenuItem,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let last_info = MenuItem::new("No dictation yet", false, None);
        let paste_last = MenuItem::new("Paste last again", false, None);
        let fix_last = MenuItem::new("Fix last dictation…", false, None);
        let copy_last = MenuItem::new("Copy last dictation", false, None);
        let settings = MenuItem::new("Settings…", true, None);
        let check_update = MenuItem::new("Check for updates…", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append(&last_info)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&paste_last)?;
        menu.append(&fix_last)?;
        menu.append(&copy_last)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&settings)?;
        menu.append(&check_update)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip(Status::Idle))
            .with_icon(draw(Status::Idle))
            .build()?;
        Ok(Self { icon, last_info, paste_last, fix_last, copy_last, settings, check_update, quit })
    }

    pub fn set_status(&self, status: Status) {
        let _ = self.icon.set_icon(Some(draw(status)));
        let _ = self.icon.set_tooltip(Some(tooltip(status)));
    }

    /// Greys out "Check for updates…" while a check or download runs.
    pub fn set_updating(&self, busy: bool) {
        self.check_update.set_text(if busy { "Checking for updates…" } else { "Check for updates…" });
        self.check_update.set_enabled(!busy);
    }

    /// Shows the last dictation in the menu; the actions below need one to work on.
    pub fn set_last(&self, text: &str) {
        let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let has_text = !words.is_empty();
        let label = if has_text { format!("Last: \"{}\"", preview(&words, 40)) } else { "No dictation yet".to_string() };
        self.last_info.set_text(label);
        for item in [&self.paste_last, &self.fix_last, &self.copy_last] {
            item.set_enabled(has_text);
        }
    }
}

/// The first `max` characters, with "…" when cut short.
fn preview(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max).collect::<String>())
}

fn tooltip(status: Status) -> &'static str {
    match status {
        Status::Idle if crate::settings::is_dev() => "me-voice (dev): ready",
        Status::Idle => "me-voice: ready",
        Status::Recording => "me-voice: listening…",
        Status::Transcribing => "me-voice: transcribing…",
    }
}

/// The app icon, with the dot showing the status.
fn draw(status: Status) -> Icon {
    const SIZE: u32 = 32;
    let dot = match status {
        Status::Idle => crate::ui::icon::ORANGE,
        Status::Recording => [0xFF, 0x3B, 0x30],
        Status::Transcribing => [0xFF, 0xD6, 0x00],
    };
    Icon::from_rgba(crate::ui::icon::rgba(SIZE, dot), SIZE, SIZE).expect("valid icon size")
}
