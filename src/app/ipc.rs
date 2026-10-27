//! Small local-socket channel between the running app, the settings window and
//! the `me-voice toggle` command. One line of text per message.

use std::io::{BufRead, BufReader, Write};

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericNamespaced, Listener, ListenerOptions, Name, Stream};

fn socket_name(base: &str) -> String {
    #[cfg(windows)]
    let scope = String::new();
    #[cfg(unix)]
    let scope = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(crate::settings::data_dir().as_os_str().as_encoded_bytes());
        format!("-{}", hash[..8].iter().map(|b| format!("{b:02x}")).collect::<String>())
    };
    format!("{base}{scope}{}.sock", crate::settings::instance_suffix())
}

/// The tray app listens here.
fn app_socket() -> String {
    socket_name("me-voice")
}

/// The settings window listens here (to come to the front, or close when the app quits).
pub fn settings_socket() -> String {
    socket_name("me-voice-settings")
}

/// The paragraph window listens here (the paragraph shortcut locks in a line).
pub fn paragraph_socket() -> String {
    socket_name("me-voice-paragraph")
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
    let listener = bind_listener(name)?;
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

fn bind_listener(name: Name<'_>) -> std::io::Result<Listener> {
    let result = ListenerOptions::new().name(name.borrow()).create_sync();
    #[cfg(unix)]
    if result.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::AddrInUse) {
        // A process exit does not drop the listener on its background thread.
        // Never displace a live listener, even if its instance lock is elsewhere.
        match Stream::connect(name.borrow()) {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                return ListenerOptions::new().name(name).try_overwrite(true).create_sync();
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use interprocess::local_socket::GenericFilePath;
    use std::os::unix::net::UnixListener;

    fn socket_path() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "me-voice-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn recovers_socket_left_by_an_exited_process() {
        let path = socket_path();
        drop(UnixListener::bind(&path).unwrap());
        let listener = bind_listener(path.as_path().to_fs_name::<GenericFilePath>().unwrap()).unwrap();
        assert!(path.exists());
        drop(listener);
        assert!(!path.exists());
    }

    #[test]
    fn does_not_replace_a_live_listener() {
        let path = socket_path();
        let listener = bind_listener(path.as_path().to_fs_name::<GenericFilePath>().unwrap()).unwrap();
        let error = bind_listener(path.as_path().to_fs_name::<GenericFilePath>().unwrap()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        assert!(Stream::connect(path.as_path().to_fs_name::<GenericFilePath>().unwrap()).is_ok());
        drop(listener);
        assert!(!path.exists());
    }
}
