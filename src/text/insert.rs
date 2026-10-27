//! Puts the text where the cursor is: paste via the clipboard (default) or type it out.

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use enigo::Direction::{Click, Press, Release};
use enigo::{Enigo, Key, Keyboard, Settings as EnigoSettings};

use crate::settings::InsertMode;

/// How long the target app gets to read the clipboard before it's restored.
/// Busy computers can be slow to handle Ctrl+V; restoring too early pastes the old text.
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(900);

/// Inserts the text; with `press_enter`, presses Enter afterwards (sends chat messages).
pub fn insert(text: &str, mode: InsertMode, press_enter: bool) -> Result<()> {
    if text.is_empty() && !press_enter {
        return Ok(());
    }
    // With a shortcut like Ctrl+Shift+Alt+Space, a still-held Shift or Alt would turn
    // our Ctrl+V into Ctrl+Shift+Alt+V, which apps ignore. Wait until they're released.
    wait_for_keys_released(Duration::from_millis(1500));
    let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| anyhow!("keyboard: {e}"))?;
    if !text.is_empty() {
        match mode {
            InsertMode::Type => type_text(&mut enigo, text)?,
            InsertMode::Paste => paste(&mut enigo, text, press_enter)?,
            InsertMode::T3 => {
                // The T3 CLI submits the message itself, so Enter is never pressed for it below.
                if let Err(e) = crate::app::apps::t3_send(text) {
                    crate::app::log::write(&format!("T3 native insert: {e}"));
                    type_text(&mut enigo, text)?;
                }
            }
        }
    }
    if press_enter && (mode == InsertMode::Type || text.is_empty()) && mode != InsertMode::T3 {
        sleep(Duration::from_millis(60));
        enigo.key(Key::Return, Click).map_err(|e| anyhow!("enter: {e}"))?;
    }
    Ok(())
}

/// Types the text. Line breaks go in as Shift+Enter: a plain Enter sends the message in chat
/// apps and prompt boxes, and editors take Shift+Enter as a line break too.
fn type_text(enigo: &mut Enigo, text: &str) -> Result<()> {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            let line_break = (|| {
                enigo.key(Key::Shift, Press)?;
                enigo.key(Key::Return, Click)?;
                enigo.key(Key::Shift, Release)
            })();
            line_break.map_err(|e| anyhow!("line break: {e}"))?;
        }
        let line = line.trim_end_matches('\r');
        if !line.is_empty() {
            enigo.text(line).map_err(|e| anyhow!("typing: {e}"))?;
        }
    }
    Ok(())
}

/// Presses Undo (Ctrl+Z, Cmd+Z on macOS) once in the focused app: undoes the last paste.
pub fn undo() -> Result<()> {
    wait_for_keys_released(Duration::from_millis(1500));
    let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| anyhow!("keyboard: {e}"))?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    let undo_keys = (|| {
        enigo.key(modifier, Press)?;
        enigo.key(Key::Unicode('z'), Click)?;
        enigo.key(modifier, Release)
    })();
    undo_keys.map_err(|e| anyhow!("undo: {e}"))
}

/// Copies the selected text of the focused app (Ctrl+C) and puts the old clipboard back.
/// Empty when nothing is selected.
pub fn copy_selection() -> Result<String> {
    wait_for_keys_released(Duration::from_millis(1500));
    let mut clipboard = Clipboard::new()?;
    let previous = clipboard.get_text().ok();
    let _ = clipboard.clear();
    let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| anyhow!("keyboard: {e}"))?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    let copy_keys = (|| {
        enigo.key(modifier, Press)?;
        enigo.key(Key::Unicode('c'), Click)?;
        enigo.key(modifier, Release)
    })();
    copy_keys.map_err(|e| anyhow!("copy: {e}"))?;
    // Apps put the selection on the clipboard a little later; wait for it briefly.
    let start = Instant::now();
    let mut selection = String::new();
    while start.elapsed() < Duration::from_millis(500) {
        sleep(Duration::from_millis(30));
        if let Ok(text) = clipboard.get_text() {
            selection = text;
            break;
        }
    }
    if let Some(previous) = previous {
        let _ = clipboard.set_text(previous);
    }
    Ok(selection)
}

fn paste(enigo: &mut Enigo, text: &str, press_enter: bool) -> Result<()> {
    let mut clipboard = Clipboard::new()?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text)?;
    // Let the clipboard change settle before the app is asked to read it.
    sleep(Duration::from_millis(40));

    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    let paste_keys = (|| {
        enigo.key(modifier, Press)?;
        enigo.key(Key::Unicode('v'), Click)?;
        enigo.key(modifier, Release)
    })();
    paste_keys.map_err(|e| anyhow!("pasting: {e}"))?;
    if press_enter {
        // Give the app a moment to insert the text before sending it.
        sleep(Duration::from_millis(150));
        enigo.key(Key::Return, Click).map_err(|e| anyhow!("enter: {e}"))?;
    }

    sleep(CLIPBOARD_RESTORE_DELAY);
    if let Some(previous) = previous {
        // Only restore if nobody else changed the clipboard in the meantime.
        if clipboard.get_text().ok().as_deref() == Some(text) {
            let _ = clipboard.set_text(previous);
        }
    }
    Ok(())
}

/// Waits (up to `max`) until Shift, Ctrl, Alt and the Windows keys are all up.
fn wait_for_keys_released(max: Duration) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        const KEYS: [i32; 5] = [0x10, 0x11, 0x12, 0x5B, 0x5C]; // Shift, Ctrl, Alt, left/right Windows
        let start = Instant::now();
        while start.elapsed() < max {
            let held = KEYS.iter().any(|&k| unsafe { GetAsyncKeyState(k) } as u16 & 0x8000 != 0);
            if !held {
                return;
            }
            sleep(Duration::from_millis(15));
        }
        crate::app::log::write("paste: modifier keys still held, pasting anyway");
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (max, Instant::now());
}
