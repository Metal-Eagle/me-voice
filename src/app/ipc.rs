//! Small local-socket channel between the running app, the settings window and
//! the `me-voice toggle` command. One line of text per message.

use std::io::{BufRead, BufReader, Write};

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};

/// The tray app listens here.
fn app_socket() -> String {
    format!("me-voice{}.sock", crate::settings::instance_suffix())
}

/// The settings window listens here (to come to the front, or close when the app quits).
pub fn settings_socket() -> String {
    format!("me-voice-settings{}.sock", crate::settings::instance_suffix())
}

/// The paragraph window listens here (the paragraph shortcut locks in a line).
pub fn paragraph_socket() -> String {
    format!("me-voice-paragraph{}.sock", crate::settings::instance_suffix())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Paragraph window: lock in the line being said and start the next one.
    NewLine,
    /// Start or stop dictation (for desktops where global shortcuts don't work, e.g. Wayland).
    Toggle,
    /// Settings changed on disk: reload them.
    Reload,
    /// The settings window is capturing a shortcut: don't react to ours.
    PauseHotkey,
    ResumeHotkey,
    /// Another instance was started: open the settings window.
    ShowSettings,
    /// The tray app is quitting: close the settings window too.
    Quit,
}

impl Command {
    fn as_str(self) -> &'static str {
        match self {
            Command::NewLine => "new-line",
            Command::Toggle => "toggle",
            Command::Reload => "reload",
            Command::PauseHotkey => "pause-hotkey",
            Command::ResumeHotkey => "resume-hotkey",
            Command::ShowSettings => "show-settings",
            Command::Quit => "quit",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "new-line" => Command::NewLine,
            "toggle" => Command::Toggle,
            "reload" => Command::Reload,
            "pause-hotkey" => Command::PauseHotkey,
            "resume-hotkey" => Command::ResumeHotkey,
            "show-settings" => Command::ShowSettings,
            "quit" => Command::Quit,
            _ => return None,
        })
    }
}

/// Sends a command to the running app. Fails if it isn't running.
pub fn send(cmd: Command) -> Result<()> {
    send_to(&app_socket(), cmd)
}

pub fn send_to(socket: &str, cmd: Command) -> Result<()> {
    let name = socket.to_ns_name::<GenericNamespaced>()?;
    let mut conn = Stream::connect(name)?;
    conn.write_all(format!("{}\n", cmd.as_str()).as_bytes())?;
    Ok(())
}

/// Starts listening on a background thread. The thread blocks on the socket,
/// so it uses no CPU while nothing is sent.
pub fn listen(on_command: impl FnMut(Command) + Send + 'static) -> Result<()> {
    listen_on(&app_socket(), on_command)
}

pub fn listen_on(socket: &str, mut on_command: impl FnMut(Command) + Send + 'static) -> Result<()> {
    let name = socket.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    std::thread::spawn(move || {
        for conn in listener.incoming().filter_map(Result::ok) {
            let mut line = String::new();
            if BufReader::new(conn).read_line(&mut line).is_ok() {
                if let Some(cmd) = Command::parse(&line) {
                    on_command(cmd);
                }
            }
        }
    });
    Ok(())
}
