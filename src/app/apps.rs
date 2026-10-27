//! T3 Code: whether it's installed, and sending dictation to it through its CLI.

/// Whether a program name is T3 Code: `t3 code (beta).exe`, `t3 code.exe` or `t3code.exe`,
/// in any case. Not its uninstaller, and not other programs that merely start with "t3code".
pub fn is_t3_code_exe(name: &str) -> bool {
    let name = name.to_lowercase().replace(' ', "");
    name.strip_prefix("t3code").is_some_and(|rest| rest.ends_with(".exe") && (rest.starts_with('.') || rest.starts_with('(')))
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

/// Project ids are a UUID with a prefix, e.g. `mcp:1c5fb07e-…`.
#[cfg(any(windows, test))]
fn is_project_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
}

/// Spoken at the start of a dictation, these start a new T3 chat with the rest as its first
/// message. A new chat in T3 doesn't exist until its first message, so there's nothing to send to.
const NEW_CHAT_WORDS: [&str; 2] = ["new chat", "nieuwe chat"];

/// The first message when the dictation starts with "new chat", without those words.
pub fn new_chat_message(text: &str) -> Option<&str> {
    let start = text.trim_start();
    NEW_CHAT_WORDS.iter().find_map(|words| {
        let head = start.get(..words.len())?;
        let rest = &start[words.len()..];
        // A whole word only: "new chats are…" is an ordinary dictation.
        let whole = rest.chars().next().is_none_or(|c| !c.is_alphanumeric());
        (head.eq_ignore_ascii_case(words) && whole).then(|| rest.trim_start_matches(|c: char| c.is_whitespace() || ",.:;!?".contains(c)))
    })
}

/// The thread the user opened last, from T3's state database. Read-only, not immutable,
/// so visits still sitting in the WAL file are seen too.
#[cfg(windows)]
fn last_opened_thread(home: &std::path::Path) -> anyhow::Result<(String, String)> {
    use rusqlite::{Connection, OpenFlags};
    use std::time::Duration;

    let db = home.join("userdata").join("statev2.sqlite");
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    last_opened_thread_in(&conn)
}

/// T3 also logs a visit when a thread that is open somewhere gets new agent activity, so the
/// newest visit can be a busy background thread. `opens.openedAt` is set only when the user
/// opens a thread, and those visits copy it, so the newest `openedAt` is the one they opened.
/// Returns that thread's id and its project's id.
#[cfg(windows)]
fn last_opened_thread_in(conn: &rusqlite::Connection) -> anyhow::Result<(String, String)> {
    let (id, project): (String, String) = conn.query_row(
        "SELECT stream_id, json_extract(payload_json, '$.projectId') FROM orchestration_events
         WHERE event_type = 'thread.visited' AND json_extract(payload_json, '$.opens.openedAt') IS NOT NULL
         ORDER BY json_extract(payload_json, '$.opens.openedAt') DESC, sequence DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    anyhow::ensure!(is_thread_id(&id), "T3 Code thread id looks wrong");
    anyhow::ensure!(is_project_id(&project), "T3 Code project id looks wrong");
    Ok((id, project))
}

/// Windows flag: start the child process without a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Sends the text to the T3 Code thread opened last, through T3's own CLI. The CLI submits it
/// as that thread's next message. Starting with "new chat" starts a new thread in the same
/// project instead.
#[cfg(windows)]
pub fn t3_send(text: &str) -> anyhow::Result<()> {
    use anyhow::{anyhow, bail};
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    let home = t3_home().ok_or_else(|| anyhow!("T3 Code folder not found"))?;
    let (thread_id, project_id) = last_opened_thread(&home)?;
    let cli = home.join("bin").join("t3.cmd");
    if !cli.is_file() {
        bail!("T3 CLI not found at {}", cli.display());
    }
    let (args, text) = match new_chat_message(text) {
        Some("") => bail!("\"new chat\" without a message"),
        Some(first) => (["thread", "start", project_id.as_str(), "-"], first),
        None => (["thread", "send", thread_id.as_str(), "-"], text),
    };
    let mut child = Command::new(&cli)
        .args(args)
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
        assert!(is_t3_code_exe("t3code.exe"));
        assert!(is_t3_code_exe("T3Code.EXE"));
        assert!(!is_t3_code_exe("uninstall t3 code (beta).exe"));
        assert!(!is_t3_code_exe("t3codex.exe"));
        assert!(!is_t3_code_exe("t3code-server.exe"));
        assert!(!is_t3_code_exe("ms-teams.exe"));
        assert!(!is_t3_code_exe("t3 code (beta)"));
    }

    /// Reproduces the wrong-thread bug: background activity on B after the user opened A.
    #[cfg(windows)]
    #[test]
    fn last_opened_thread_ignores_activity_visits() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE orchestration_events (sequence INTEGER, stream_id TEXT, event_type TEXT, payload_json TEXT);
             INSERT INTO orchestration_events VALUES
               (1, 'bbbb', 'thread.visited', '{\"projectId\":\"mcp:p1\",\"opens\":{\"openedAt\":1000}}'),
               (2, 'aaaa', 'thread.visited', '{\"projectId\":\"mcp:p2\",\"opens\":{\"openedAt\":2000}}'),
               (3, 'bbbb', 'thread.visited', '{\"projectId\":\"mcp:p1\",\"opens\":{\"openedAt\":1000}}'),
               (4, 'cccc', 'thread.visited', '{\"opens\":null}'),
               (5, 'aaaa', 'run.updated', '{}');",
        )
        .unwrap();
        assert_eq!(last_opened_thread_in(&conn).unwrap(), ("aaaa".to_string(), "mcp:p2".to_string()));
    }

    #[test]
    fn new_chat_words_start_a_new_chat() {
        assert_eq!(new_chat_message("New chat, fix the build."), Some("fix the build."));
        assert_eq!(new_chat_message("  new chat: kijk naar de tests"), Some("kijk naar de tests"));
        assert_eq!(new_chat_message("Nieuwe chat. Maak een plan"), Some("Maak een plan"));
        assert_eq!(new_chat_message("New chat."), Some(""));
        assert_eq!(new_chat_message("New chats are fine"), None);
        assert_eq!(new_chat_message("Open a new chat please"), None);
        assert_eq!(new_chat_message("new"), None);
    }

    #[test]
    fn project_id_accepts_t3_ids_only() {
        assert!(is_project_id("mcp:1c5fb07e-5ea9-4d6e-b0fa-d0351d948890"));
        assert!(!is_project_id(""));
        assert!(!is_project_id("mcp:x & del"));
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
