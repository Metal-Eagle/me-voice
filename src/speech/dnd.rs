//! Do not disturb: while on, me-voice doesn't talk (no T3 updates, no spoken replies).
//! Dictation keeps working. The state is a file in the config folder, so a shortcut, the
//! command line (`me-voice dnd on|off|toggle`) or an automation can switch it.

use std::path::PathBuf;

fn flag() -> PathBuf {
    crate::settings::config_dir().join("do-not-disturb")
}

pub fn is_on() -> bool {
    flag().exists()
}

pub fn set(on: bool) -> std::io::Result<()> {
    let path = flag();
    if on {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, b"on")
    } else {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Switches it and returns the new state.
pub fn toggle() -> std::io::Result<bool> {
    let on = !is_on();
    set(on)?;
    Ok(on)
}
