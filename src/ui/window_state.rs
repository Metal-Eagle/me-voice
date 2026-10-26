//! Remembers where the settings window was and how big it was (`window.json` in the
//! settings folder), so it opens the same way next time.

use std::fs;

use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

fn path() -> std::path::PathBuf {
    crate::settings::config_dir().join("window.json")
}

impl WindowState {
    /// The saved state, if it still fits on the screens that are connected now.
    pub fn load() -> Option<Self> {
        let state: Self = serde_json::from_str(&fs::read_to_string(path()).ok()?).ok()?;
        (state.width >= 200.0 && state.height >= 200.0).then_some(state)
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all(crate::settings::config_dir());
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path(), json);
        }
    }

    /// Applies size, position (only when it's on a connected screen) and maximised state.
    pub fn apply(&self, viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
        let mut viewport = viewport.with_inner_size([self.width, self.height]).with_maximized(self.maximized);
        if on_screen(self.x, self.y, self.width) {
            viewport = viewport.with_position([self.x, self.y]);
        }
        viewport
    }

    /// Reads the current window from egui; `None` while the window isn't known yet.
    /// When maximised, the normal size and position are kept from `previous`.
    pub fn current(ctx: &egui::Context, previous: Option<Self>) -> Option<Self> {
        ctx.input(|i| {
            let vp = i.viewport();
            let maximized = vp.maximized.unwrap_or(false);
            if vp.minimized == Some(true) {
                return previous;
            }
            if maximized {
                return previous.map(|p| Self { maximized: true, ..p });
            }
            let outer = vp.outer_rect?;
            let inner = vp.inner_rect?;
            Some(Self { x: outer.min.x, y: outer.min.y, width: inner.width(), height: inner.height(), maximized: false })
        })
    }
}

/// Is the window's title bar on one of the connected screens? (Avoids reopening
/// off-screen after a monitor was unplugged.)
fn on_screen(x: f32, y: f32, width: f32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        };
        let (left, top, w, h) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN) as f32,
                GetSystemMetrics(SM_YVIRTUALSCREEN) as f32,
                GetSystemMetrics(SM_CXVIRTUALSCREEN) as f32,
                GetSystemMetrics(SM_CYVIRTUALSCREEN) as f32,
            )
        };
        // At least 100 px of the title bar must be visible.
        x + width - 100.0 > left && x + 100.0 < left + w && y >= top - 10.0 && y + 40.0 < top + h
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (x, y, width);
        true
    }
}
