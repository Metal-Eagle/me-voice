//! T3 Code: whether it's installed, and sending dictation to it through its CLI.

/// Whether a program name is T3 Code, e.g. `t3 code (beta).exe` (not its uninstaller).
pub fn is_t3_code_exe(name: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with("t3 code") && name.ends_with(".exe")
}

/// Whether T3 Code is installed: its beta build lives in `%LOCALAPPDATA%\Programs\t3code`.
#[cfg(windows)]
pub fn t3_code_installed() -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return false };
    let dir = std::path::PathBuf::from(local).join("Programs").join("t3code");
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|entry| is_t3_code_exe(&entry.file_name().to_string_lossy()))
}

#[cfg(not(windows))]
pub fn t3_code_installed() -> bool {
    false
}

/// T3 Code's data folder: `T3CODE_HOME` when set, else `%USERPROFILE%\.t3`.
#[cfg(windows)]
fn t3_home() -> Option<std::path::PathBuf> {
    match std::env::var_os("T3CODE_HOME") {
        Some(home) if !home.is_empty() => Some(home.into()),
        _ => std::env::var_os("USERPROFILE").map(|home| std::path::PathBuf::from(home).join(".t3")),
    }
}

/// A thread id ends up on a command line, so only accept what a UUID is made of.
#[cfg(any(windows, test))]
fn is_thread_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The thread the user opened last, from T3's state database. Read-only, not immutable,
/// so visits still sitting in the WAL file are seen too.
#[cfg(windows)]
fn last_visited_thread(home: &std::path::Path) -> anyhow::Result<String> {
    use rusqlite::{Connection, OpenFlags};
    use std::time::Duration;

    let db = home.join("userdata").join("statev2.sqlite");
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    let id: String = conn.query_row(
        "SELECT stream_id FROM orchestration_events WHERE event_type = 'thread.visited' ORDER BY sequence DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(is_thread_id(&id), "T3 Code thread id looks wrong");
    Ok(id)
}

/// Windows flag: start the child process without a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Sends the text to the T3 Code thread opened last, through T3's own CLI. The CLI submits it
/// as that thread's next message.
#[cfg(windows)]
pub fn t3_send(text: &str) -> anyhow::Result<()> {
    use anyhow::{anyhow, bail};
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    let home = t3_home().ok_or_else(|| anyhow!("T3 Code folder not found"))?;
    let thread_id = last_visited_thread(&home)?;
    let cli = home.join("bin").join("t3.cmd");
    if !cli.is_file() {
        bail!("T3 CLI not found at {}", cli.display());
    }
    let mut child = Command::new(&cli)
        .args(["thread", "send", &thread_id, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;

    // The message goes in on its own thread, so a full pipe can't block the wait below.
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("T3 CLI: no stdin"))?;
    let message = text.to_string();
    thread::spawn(move || {
        let _ = stdin.write_all(message.as_bytes());
    });
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("T3 CLI: no stderr"))?;
    let errors = thread::spawn(move || {
        let mut out = String::new();
        let _ = stderr.read_to_string(&mut out);
        out
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            // Kill the whole tree: the .cmd runs node, which could otherwise still send the
            // message after we've given up and typed it instead.
            let _ = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
            let _ = child.kill();
            let _ = child.wait();
            bail!("T3 CLI did not finish within 15 seconds");
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        let errors = errors.join().unwrap_or_default();
        let first = errors.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("no error output");
        bail!("T3 CLI failed ({status}): {first}");
    }
    Ok(())
}

/// T3 Code only exists on Windows here.
#[cfg(not(windows))]
pub fn t3_send(_text: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("T3 Code is only supported on Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t3_code_exe_matches_t3_only() {
        assert!(is_t3_code_exe("t3 code (beta).exe"));
        assert!(is_t3_code_exe("T3 Code (Beta).exe"));
        assert!(is_t3_code_exe("t3 code.exe"));
        assert!(!is_t3_code_exe("uninstall t3 code (beta).exe"));
        assert!(!is_t3_code_exe("ms-teams.exe"));
        assert!(!is_t3_code_exe("t3 code (beta)"));
    }

    #[test]
    fn thread_id_accepts_uuid_and_rejects_shell_text() {
        assert!(is_thread_id("37f31e6f-0151-40e9-8bc0-e62d96c6549d"));
        assert!(is_thread_id(&"a".repeat(64)));
        assert!(!is_thread_id(""));
        assert!(!is_thread_id("a b"));
        assert!(!is_thread_id("x;del"));
        assert!(!is_thread_id("../x"));
        assert!(!is_thread_id(&"a".repeat(65)));
    }
}
