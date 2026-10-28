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

/// A thread id ends up on a command line, so only accept what a UUID is made of, with the
/// `mcp:` prefix T3 gives threads started through its tools.
#[cfg(any(windows, test))]
fn is_thread_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
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

/// A T3 Code thread the dictation can go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Thread {
    pub id: String,
    pub project: String,
    /// The name of the project, as in T3's project list.
    pub project_title: String,
    pub title: String,
}

impl T3Thread {
    /// The row on the card: the project's name, then the thread's title.
    pub fn label(&self) -> String {
        match (self.project_title.trim(), self.title.trim()) {
            ("", title) => title.to_string(),
            (project, "") => format!("{project} · Untitled thread"),
            (project, title) => format!("{project} · {title}"),
        }
    }
}

/// A T3 Code project a new thread can start in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Project {
    pub id: String,
    pub title: String,
}

/// Where a dictation to T3 goes: an existing thread, or a new thread in a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum T3Target {
    Thread(T3Thread),
    NewThread(T3Project),
}

/// How many recent threads (and projects) the card offers, picked with the number keys 1 to this.
pub const RECENT_THREADS: usize = 6;

/// T3's state database, read-only, not immutable, so visits still sitting in the WAL file are seen too.
#[cfg(windows)]
fn open_state(home: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {
    use rusqlite::{Connection, OpenFlags};
    let db = home.join("userdata").join("statev2.sqlite");
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(conn)
}

/// The threads and the projects the user opened most recently, newest first: the first thread
/// is where a dictation goes by default. Empty when T3 Code or its database isn't there.
#[cfg(windows)]
pub fn recent() -> (Vec<T3Thread>, Vec<T3Project>) {
    let result = t3_home().ok_or_else(|| anyhow::anyhow!("T3 Code folder not found")).and_then(|home| {
        let conn = open_state(&home)?;
        Ok((recent_threads_in(&conn, RECENT_THREADS)?, recent_projects_in(&conn, RECENT_THREADS)?))
    });
    result.unwrap_or_else(|e: anyhow::Error| {
        crate::app::log::write(&format!("T3 recent threads: {e:#}"));
        (Vec::new(), Vec::new())
    })
}

#[cfg(not(windows))]
pub fn recent() -> (Vec<T3Thread>, Vec<T3Project>) {
    (Vec::new(), Vec::new())
}

/// Projects ordered by when the user last opened one of their threads; deleted ones are left out.
#[cfg(windows)]
fn recent_projects_in(conn: &rusqlite::Connection, limit: usize) -> anyhow::Result<Vec<T3Project>> {
    let mut query = conn.prepare(
        "SELECT t.project_id, COALESCE(p.title, ''), MAX(json_extract(e.payload_json, '$.opens.openedAt')) AS opened
         FROM orchestration_events e JOIN orchestration_v2_projection_threads t ON t.thread_id = e.stream_id
         JOIN projection_projects p ON p.project_id = t.project_id
         WHERE e.event_type = 'thread.visited' AND p.deleted_at IS NULL
         GROUP BY t.project_id HAVING opened IS NOT NULL ORDER BY opened DESC LIMIT ?1",
    )?;
    let rows = query.query_map([limit as i64], |row| Ok(T3Project { id: row.get(0)?, title: row.get(1)? }))?;
    Ok(rows.flatten().filter(|p| is_project_id(&p.id)).collect())
}

/// Like `last_opened_thread_in`, ordered by when the user opened each thread, with its title;
/// archived and deleted threads, and threads of deleted projects, are left out.
#[cfg(windows)]
fn recent_threads_in(conn: &rusqlite::Connection, limit: usize) -> anyhow::Result<Vec<T3Thread>> {
    let mut query = conn.prepare(
        "SELECT e.stream_id, t.project_id, COALESCE(p.title, ''), COALESCE(t.title, ''), MAX(json_extract(e.payload_json, '$.opens.openedAt')) AS opened
         FROM orchestration_events e JOIN orchestration_v2_projection_threads t ON t.thread_id = e.stream_id
         JOIN projection_projects p ON p.project_id = t.project_id
         WHERE e.event_type = 'thread.visited' AND t.archived_at IS NULL AND t.deleted_at IS NULL AND p.deleted_at IS NULL
         GROUP BY e.stream_id HAVING opened IS NOT NULL ORDER BY opened DESC LIMIT ?1",
    )?;
    let rows = query.query_map([limit as i64], |row| Ok(T3Thread { id: row.get(0)?, project: row.get(1)?, project_title: row.get(2)?, title: row.get(3)? }))?;
    Ok(rows.flatten().filter(|t| is_thread_id(&t.id) && is_project_id(&t.project)).collect())
}

/// The thread T3 Code's window in front is showing, read from its address
/// (`t3code://app/#/<environment>/<thread>`) through UI Automation. `None` when the window in
/// front isn't T3 Code, it shows no thread, or Windows won't tell.
#[cfg(windows)]
pub fn shown_thread() -> Option<String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        // Only uninitialise if this call did the initialising (S_OK or S_FALSE both count as success).
        let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        // Chromium builds its accessibility tree only once asked, so the first read can be empty.
        let read = || std::panic::catch_unwind(|| read_shown_url()).ok().flatten();
        let url = read().or_else(|| {
            std::thread::sleep(std::time::Duration::from_millis(150));
            read()
        });
        if initialised {
            CoUninitialize();
        }
        let id = thread_in_url(&url?)?;
        is_thread_id(&id).then_some(id)
    }
}

#[cfg(not(windows))]
pub fn shown_thread() -> Option<String> {
    None
}

/// The address of the page in the window in front: the value of its document element.
/// Must run on a COM-initialised thread.
#[cfg(windows)]
unsafe fn read_shown_url() -> Option<String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants, UIA_ControlTypePropertyId,
        UIA_DocumentControlTypeId, UIA_ValuePatternId,
    };
    unsafe {
        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None::<&windows::core::IUnknown>, CLSCTX_INPROC_SERVER).ok()?;
        let window = automation.ElementFromHandle(HWND(hwnd)).ok()?;
        let condition = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &windows::Win32::System::Variant::VARIANT::from(UIA_DocumentControlTypeId.0))
            .ok()?;
        let document = window.FindFirst(TreeScope_Descendants, &condition).ok()?;
        let pattern = document.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok()?;
        Some(pattern.CurrentValue().ok()?.to_string())
    }
}

/// The thread id in a T3 Code address: the last part of the `#/…` route, `%3A` decoded.
#[cfg(any(windows, test))]
fn thread_in_url(url: &str) -> Option<String> {
    let route = url.split_once("#/")?.1;
    let route = route.split(['?', '#']).next()?;
    let mut parts = route.split('/').filter(|p| !p.is_empty());
    let (_environment, thread) = (parts.next()?, parts.next()?);
    Some(thread.replace("%3A", ":").replace("%3a", ":"))
}

/// One thread, by id, as on the card; `None` when it is unknown, archived or deleted.
#[cfg(windows)]
pub fn thread(id: &str) -> Option<T3Thread> {
    let home = t3_home()?;
    let conn = open_state(&home).ok()?;
    conn.query_row(
        "SELECT t.thread_id, t.project_id, COALESCE(p.title, ''), COALESCE(t.title, '')
         FROM orchestration_v2_projection_threads t JOIN projection_projects p ON p.project_id = t.project_id
         WHERE t.thread_id = ?1 AND t.archived_at IS NULL AND t.deleted_at IS NULL AND p.deleted_at IS NULL",
        [id],
        |row| Ok(T3Thread { id: row.get(0)?, project: row.get(1)?, project_title: row.get(2)?, title: row.get(3)? }),
    )
    .ok()
    .filter(|t| is_thread_id(&t.id) && is_project_id(&t.project))
}

#[cfg(not(windows))]
pub fn thread(_id: &str) -> Option<T3Thread> {
    None
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

/// Sends the text to `target`, or else the T3 Code thread opened last, through T3's own CLI.
/// The CLI submits it as that thread's next message. Starting with "new chat" starts a new
/// thread in the same project instead.
#[cfg(windows)]
pub fn t3_send(text: &str, target: Option<&T3Target>) -> anyhow::Result<()> {
    use anyhow::{anyhow, bail};
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    let home = t3_home().ok_or_else(|| anyhow!("T3 Code folder not found"))?;
    let (thread_id, project_id) = match target {
        Some(T3Target::Thread(t)) if is_thread_id(&t.id) && is_project_id(&t.project) => (t.id.clone(), t.project.clone()),
        Some(T3Target::NewThread(p)) if is_project_id(&p.id) => (String::new(), p.id.clone()),
        _ => last_opened_thread_in(&open_state(&home)?)?,
    };
    // A new thread picked on the card: the whole dictation is its first message.
    let new_thread = matches!(target, Some(T3Target::NewThread(_)));
    let cli = home.join("bin").join("t3.cmd");
    if !cli.is_file() {
        bail!("T3 CLI not found at {}", cli.display());
    }
    // Saying "new chat" as well is fine: those words are dropped either way.
    let (args, text) = match new_chat_message(text) {
        _ if new_thread => (["thread", "start", project_id.as_str(), "-"], new_chat_message(text).unwrap_or(text)),
        Some("") => bail!("\"new chat\" without a message"),
        Some(first) => (["thread", "start", project_id.as_str(), "-"], first),
        None => (["thread", "send", thread_id.as_str(), "-"], text),
    };
    if text.trim().is_empty() {
        bail!("no message to send");
    }
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
pub fn t3_send(_text: &str, _target: Option<&T3Target>) -> anyhow::Result<()> {
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

    #[test]
    fn thread_in_url_reads_the_route() {
        assert_eq!(
            thread_in_url("t3code://app/#/1f1e7a65-23a6/f27ddbec-791a").as_deref(),
            Some("f27ddbec-791a")
        );
        assert_eq!(thread_in_url("t3code://app/#/env/mcp%3Ac022-b7").as_deref(), Some("mcp:c022-b7"));
        assert_eq!(thread_in_url("t3code://app/#/env/abc?tab=diff").as_deref(), Some("abc"));
        assert_eq!(thread_in_url("t3code://app/#/env"), None);
        assert_eq!(thread_in_url("t3code://app/"), None);
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

    #[cfg(windows)]
    #[test]
    fn recent_threads_newest_opened_first_without_archived() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE orchestration_events (sequence INTEGER, stream_id TEXT, event_type TEXT, payload_json TEXT);
             CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT, project_id TEXT, title TEXT, archived_at TEXT, deleted_at TEXT);
             CREATE TABLE projection_projects (project_id TEXT, title TEXT, deleted_at TEXT);
             INSERT INTO projection_projects VALUES ('mcp:p1', 'One', NULL), ('mcp:p2', 'Two', NULL), ('mcp:p3', 'Gone', '2026');
             INSERT INTO orchestration_v2_projection_threads VALUES
               ('aaaa', 'mcp:p1', 'First', NULL, NULL), ('mcp:bbbb', 'mcp:p2', 'Second', NULL, NULL),
               ('cccc', 'mcp:p1', 'Old', '2026', NULL), ('dddd', 'mcp:p1', 'Never opened', NULL, NULL),
               ('eeee', 'mcp:p3', 'Deleted project', NULL, NULL);
             INSERT INTO orchestration_events VALUES
               (1, 'aaaa', 'thread.visited', '{\"opens\":{\"openedAt\":1000}}'),
               (2, 'mcp:bbbb', 'thread.visited', '{\"opens\":{\"openedAt\":2000}}'),
               (3, 'aaaa', 'thread.visited', '{\"opens\":{\"openedAt\":3000}}'),
               (7, 'eeee', 'thread.visited', '{\"opens\":{\"openedAt\":5000}}'),
               (4, 'mcp:bbbb', 'thread.visited', '{\"opens\":{\"openedAt\":2000}}'),
               (5, 'cccc', 'thread.visited', '{\"opens\":{\"openedAt\":4000}}'),
               (6, 'dddd', 'thread.visited', '{\"opens\":null}');",
        )
        .unwrap();
        let titles: Vec<_> = recent_threads_in(&conn, 6).unwrap().into_iter().map(|t| (t.project_title, t.title)).collect();
        assert_eq!(titles, [("One".into(), "First".into()), ("Two".to_string(), "Second".to_string())]);
        assert_eq!(recent_threads_in(&conn, 1).unwrap().len(), 1);
        let projects: Vec<_> = recent_projects_in(&conn, 6).unwrap().into_iter().map(|p| p.title).collect();
        assert_eq!(projects, ["One", "Two"]);
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
        assert!(is_thread_id(&"a".repeat(80)));
        assert!(!is_thread_id(""));
        assert!(!is_thread_id("a b"));
        assert!(!is_thread_id("x;del"));
        assert!(!is_thread_id("../x"));
        assert!(is_thread_id("mcp:b623482d-c4e0-40ed-a9bf-9497ad681124"));
        assert!(!is_thread_id(&"a".repeat(81)));
    }
}
