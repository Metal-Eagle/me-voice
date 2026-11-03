//! Watches the T3 Code threads the user sent a message to by voice, and reports what they are
//! doing, so the app can speak it: started, still working, waiting for an OK, a question,
//! finished or failed.
//!
//! T3's state database is read-only and polled every 2 s, but only while something is watched.
//! With nothing watched the watcher thread sleeps on a condition variable, so it costs no CPU.

use std::sync::{Condvar, Mutex, MutexGuard, Once, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::app::apps;

/// How often the database is read while something is watched.
const POLL: Duration = Duration::from_secs(2);
/// A thread that hasn't started a run this long after it was sent to is forgotten.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// A watched thread, as spoken: its id and its short spoken name.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadInfo {
    pub id: String,
    pub name: String,
}

/// Something that happened to a watched thread.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum WatchEvent {
    Working(ThreadInfo),
    StillWorking(Vec<ThreadInfo>),
    NeedsOk(ThreadInfo),
    Question(ThreadInfo, String),
    Finished(ThreadInfo),
    Failed(ThreadInfo),
}

/// One thread (or one project's next thread) being followed.
struct Watch {
    /// `None` until the new thread in `project` has appeared.
    id: Option<String>,
    project: Option<String>,
    /// Empty until the thread is found in T3's database.
    name: String,
    /// Runs requested at or after this (Unix ms) belong to this watch; so does a run already running.
    since_ms: i64,
    since: Instant,
    /// Open requests already announced (or already open when the watch began).
    announced: Vec<String>,
    baselined: bool,
    /// Ordinal of the run being followed.
    run: Option<i64>,
    said_working: bool,
    running_since: Option<Instant>,
    /// Still-working lines already spoken.
    reminders: u32,
    /// The state from the last poll: "working", "waiting for your OK", "has a question", or "".
    label: &'static str,
}

impl Watch {
    fn new(id: Option<String>, project: Option<String>) -> Watch {
        Watch {
            id,
            project,
            name: String::new(),
            since_ms: now_ms(),
            since: Instant::now(),
            announced: Vec::new(),
            baselined: false,
            run: None,
            said_working: false,
            running_since: None,
            reminders: 0,
            label: "",
        }
    }
}

struct Inner {
    watches: Vec<Watch>,
    status: Vec<(ThreadInfo, &'static str)>,
}

static STATE: Mutex<Inner> = Mutex::new(Inner { watches: Vec::new(), status: Vec::new() });
static WAKE: Condvar = Condvar::new();

fn state() -> MutexGuard<'static, Inner> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Start watching this thread (the user just sent to it by voice).
#[allow(dead_code)]
pub fn watch_thread(id: &str) {
    let mut s = state();
    if s.watches.iter().any(|w| w.id.as_deref() == Some(id)) {
        return;
    }
    s.watches.push(Watch::new(Some(id.to_string()), None));
    WAKE.notify_all();
}

/// A new thread is being started in this project: watch the newest thread created in it after now.
#[allow(dead_code)]
pub fn watch_new_thread_in(project_id: &str) {
    let mut s = state();
    s.watches.push(Watch::new(None, Some(project_id.to_string())));
    WAKE.notify_all();
}

/// Starts the background watcher once (later calls do nothing). `on_event` is called from the
/// watcher thread. `still_every` returns the current `say_still_working_minutes` (0 = never).
#[allow(dead_code)]
pub fn start(on_event: impl Fn(WatchEvent) + Send + 'static, still_every: impl Fn() -> u32 + Send + 'static) {
    static ONCE: Once = Once::new();
    ONCE.call_once(move || {
        let spawned = std::thread::Builder::new().name("t3-watch".into()).spawn(move || run(on_event, still_every));
        if let Err(e) = spawned {
            crate::app::log::write(&format!("T3 watch: can't start the watcher thread: {e}"));
        }
    });
}

/// Watched threads that aren't idle, with a short state: "working", "waiting for your OK", "has a question".
#[allow(dead_code)]
pub fn status() -> Vec<(ThreadInfo, &'static str)> {
    state().status.clone()
}

/// Newest assistant message of the thread (raw markdown), if any.
#[allow(dead_code)]
pub fn last_message(thread_id: &str) -> Option<String> {
    let db = Db::open()?;
    match db.last_message(thread_id) {
        Ok(text) => text,
        Err(e) => {
            crate::app::log::write(&format!("T3 watch: last message: {e:#}"));
            None
        }
    }
}

/// Short spoken name: project title + first 4 words of the thread title, e.g. "me-voice, plan text to speech".
/// Generic titles ("New thread", empty) use just the project.
#[allow(dead_code)]
pub fn spoken_name(project_title: &str, thread_title: &str) -> String {
    let project = speakable(project_title, usize::MAX);
    let title = thread_title.trim();
    let generic = title.is_empty() || title.eq_ignore_ascii_case("new thread") || title.eq_ignore_ascii_case("untitled thread");
    let thread = if generic { String::new() } else { speakable(title, 4) };
    match (project.is_empty(), thread.is_empty()) {
        (false, false) => format!("{project}, {thread}"),
        (false, true) => project,
        (true, false) => thread,
        (true, true) => "T3 thread".to_string(),
    }
}

/// The first `words` words of `text`, with punctuation that sounds bad when spoken removed.
fn speakable(text: &str, words: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}'))
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.trim_matches('-'))
        .filter(|w| !w.is_empty())
        .take(words)
        .collect::<Vec<_>>()
        .join(" ")
}

/// How long after the last reminder (or after running began) the next still-working line is due.
/// First after `every_minutes`, then 5 minutes later, then every 10 minutes.
fn still_working_gap(every_minutes: u32, reminders: u32) -> Duration {
    let first = Duration::from_secs(u64::from(every_minutes) * 60);
    match reminders {
        0 => first,
        1 => first + Duration::from_secs(5 * 60),
        n => first + Duration::from_secs(5 * 60) + Duration::from_secs(10 * 60) * (n - 1),
    }
}

fn run(on_event: impl Fn(WatchEvent), still_every: impl Fn() -> u32) {
    loop {
        {
            let mut s = state();
            while s.watches.is_empty() {
                s = WAKE.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        }
        std::thread::sleep(POLL);
        let every = still_every();
        let events = {
            let mut s = state();
            poll(&mut s, every, Instant::now())
        };
        for event in events {
            on_event(event);
        }
    }
}

/// One poll over every watch. Runs under the state lock; the events are delivered after it is released.
fn poll(s: &mut Inner, still_every: u32, now: Instant) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    let Some(db) = Db::open() else {
        // No database (T3 not installed or unreadable): give up on watches that never started.
        s.watches.retain(|w| w.run.is_some() || now.duration_since(w.since) <= START_TIMEOUT);
        return events;
    };

    let mut kept: Vec<Watch> = Vec::new();
    let mut statuses = Vec::new();
    for mut w in std::mem::take(&mut s.watches) {
        match check(&db, &mut w, now, &mut events) {
            Ok(Step::Gone) => {}
            Ok(Step::Keep(label)) => {
                w.label = label;
                if let (Some(id), false) = (&w.id, label.is_empty()) {
                    statuses.push((ThreadInfo { id: id.clone(), name: w.name.clone() }, label));
                }
                kept.push(w);
            }
            Err(e) => {
                crate::app::log::write(&format!("T3 watch: {e:#}"));
                kept.push(w);
            }
        }
    }
    s.status = statuses;

    // Still-working lines: every thread due in this poll goes into one event. The shown thread is
    // skipped (and stays due until it's not shown), so shown_thread() is only asked when something is due.
    let due: Vec<usize> = if still_every == 0 {
        Vec::new()
    } else {
        kept.iter()
            .enumerate()
            .filter(|(_, w)| {
                w.label == "working"
                    && w.id.is_some()
                    && w.running_since.is_some_and(|since| now.duration_since(since) >= still_working_gap(still_every, w.reminders))
            })
            .map(|(i, _)| i)
            .collect()
    };
    if !due.is_empty() {
        let shown = apps::shown_thread();
        let mut threads = Vec::new();
        for i in due {
            let w = &mut kept[i];
            let Some(id) = w.id.clone() else { continue };
            if shown.as_deref() == Some(id.as_str()) {
                continue;
            }
            w.reminders += 1;
            threads.push(ThreadInfo { id, name: w.name.clone() });
        }
        if !threads.is_empty() {
            events.push(WatchEvent::StillWorking(threads));
        }
    }

    s.watches = kept;
    events
}

enum Step {
    /// The watch is over: finished, failed, cancelled, or it never started.
    Gone,
    /// Still watching, in this state ("" = not shown on the status list yet).
    Keep(&'static str),
}

/// Looks at one watch, pushes the events it has for this poll and says what state it is in.
fn check(db: &Db, w: &mut Watch, now: Instant, events: &mut Vec<WatchEvent>) -> anyhow::Result<Step> {
    if w.id.is_none() {
        let Some(project) = w.project.clone() else { return Ok(Step::Gone) };
        match db.newest_thread_in(&project, w.since_ms)? {
            Some(id) => w.id = Some(id),
            None if now.duration_since(w.since) > START_TIMEOUT => {
                crate::app::log::write(&format!("T3 watch: no new thread appeared in project {project}"));
                return Ok(Step::Gone);
            }
            None => return Ok(Step::Keep("")),
        }
    }
    let Some(id) = w.id.clone() else { return Ok(Step::Gone) };
    if w.name.is_empty() {
        match apps::thread(&id) {
            Some(t) => w.name = spoken_name(&t.project_title, &t.title),
            None => {
                crate::app::log::write(&format!("T3 watch: thread {id} not found, not watching it"));
                return Ok(Step::Gone);
            }
        }
    }
    let info = ThreadInfo { id: id.clone(), name: w.name.clone() };

    let open = db.open_requests(&id)?;
    if !w.baselined {
        // Whatever was already waiting when the watch began is not news.
        w.baselined = true;
        w.announced = open.iter().map(|(request, _)| request.clone()).collect();
    }

    if let Some(run) = db.latest_run(&id)? {
        let ours = w.run == Some(run.ordinal) || run.requested_ms >= w.since_ms || run.status == "running";
        if ours {
            w.run = Some(run.ordinal);
            match run.status.as_str() {
                "running" => {
                    if !w.said_working {
                        w.said_working = true;
                        events.push(WatchEvent::Working(info.clone()));
                    }
                    w.running_since.get_or_insert(now);
                }
                "completed" => {
                    events.push(WatchEvent::Finished(info));
                    return Ok(Step::Gone);
                }
                "failed" => {
                    events.push(WatchEvent::Failed(info));
                    return Ok(Step::Gone);
                }
                "interrupted" | "cancelled" => return Ok(Step::Gone),
                _ => {}
            }
        }
    }
    if w.run.is_none() && now.duration_since(w.since) > START_TIMEOUT {
        crate::app::log::write(&format!("T3 watch: thread {id} never started, forgotten"));
        return Ok(Step::Gone);
    }

    let mut has_question = false;
    let mut has_approval = false;
    for (request, kind) in &open {
        let is_question = kind.as_str() == "user_input";
        if is_question {
            has_question = true;
        } else {
            has_approval = true;
        }
        if w.announced.iter().any(|a| a == request) {
            continue;
        }
        w.announced.push(request.clone());
        let event = if is_question {
            let text = db.question_text(request).unwrap_or_else(|e| {
                crate::app::log::write(&format!("T3 watch: question text: {e:#}"));
                String::new()
            });
            WatchEvent::Question(info.clone(), text)
        } else {
            WatchEvent::NeedsOk(info.clone())
        };
        events.push(event);
    }
    Ok(Step::Keep(if has_question {
        "has a question"
    } else if has_approval {
        "waiting for your OK"
    } else {
        "working"
    }))
}

/// A run of a thread, as T3 records it.
struct Run {
    ordinal: i64,
    status: String,
    requested_ms: i64,
}

/// T3's state database, opened read-only for one poll.
#[cfg(windows)]
struct Db(rusqlite::Connection);

#[cfg(windows)]
impl Db {
    fn open() -> Option<Db> {
        let home = apps::t3_home()?;
        match apps::open_state(&home) {
            Ok(conn) => Some(Db(conn)),
            Err(e) => {
                crate::app::log::write(&format!("T3 watch: {e:#}"));
                None
            }
        }
    }

    /// The newest thread of the project created at or after `since_ms`.
    fn newest_thread_in(&self, project: &str, since_ms: i64) -> anyhow::Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT thread_id FROM orchestration_v2_projection_threads
                 WHERE project_id = ?1 AND archived_at IS NULL AND deleted_at IS NULL
                   AND CAST((julianday(created_at) - 2440587.5) * 86400000 AS INTEGER) >= ?2
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![project, since_ms],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    fn latest_run(&self, thread: &str) -> anyhow::Result<Option<Run>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT ordinal, status,
                        CAST((julianday(requested_at) - 2440587.5) * 86400000 AS INTEGER)
                 FROM orchestration_v2_projection_runs WHERE thread_id = ?1
                 ORDER BY ordinal DESC LIMIT 1",
                [thread],
                |r| {
                    Ok(Run {
                        ordinal: r.get(0)?,
                        status: r.get(1)?,
                        requested_ms: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    })
                },
            )
            .optional()?)
    }

    /// (request id, kind) of the requests still waiting on the user.
    fn open_requests(&self, thread: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut stmt = self.0.prepare(
            "SELECT runtime_request_id, kind FROM orchestration_v2_projection_runtime_requests
             WHERE thread_id = ?1 AND status NOT IN ('resolved', 'cancelled') ORDER BY created_at",
        )?;
        let rows = stmt.query_map([thread], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The question text of a `user_input` request: it's in the turn item, not the request.
    fn question_text(&self, request: &str) -> anyhow::Result<String> {
        use rusqlite::OptionalExtension;
        let text = self
            .0
            .query_row(
                "SELECT json_extract(payload_json, '$.questions[0].question') FROM orchestration_v2_projection_turn_items
                 WHERE type = 'user_input_request' AND json_extract(payload_json, '$.requestId') = ?1 LIMIT 1",
                [request],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(text.unwrap_or_default())
    }

    fn last_message(&self, thread: &str) -> anyhow::Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT json_extract(payload_json, '$.text') FROM orchestration_v2_projection_messages
                 WHERE thread_id = ?1 AND role = 'assistant' AND trim(json_extract(payload_json, '$.text')) <> ''
                 ORDER BY created_at DESC LIMIT 1",
                [thread],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }
}

/// Non-Windows builds have no T3 database: `open` never succeeds, so the other methods are never reached.
#[cfg(not(windows))]
struct Db;

#[cfg(not(windows))]
impl Db {
    fn open() -> Option<Db> {
        None
    }

    fn newest_thread_in(&self, _project: &str, _since_ms: i64) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    fn latest_run(&self, _thread: &str) -> anyhow::Result<Option<Run>> {
        Ok(None)
    }

    fn open_requests(&self, _thread: &str) -> anyhow::Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }

    fn question_text(&self, _request: &str) -> anyhow::Result<String> {
        Ok(String::new())
    }

    fn last_message(&self, _thread: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_name_is_project_and_first_four_title_words() {
        assert_eq!(spoken_name("me-voice", "Plan text to speech for the app"), "me-voice, Plan text to speech");
        assert_eq!(spoken_name("me-voice", "Plan"), "me-voice, Plan");
    }

    #[test]
    fn spoken_name_drops_generic_and_empty_titles() {
        assert_eq!(spoken_name("me-voice", "New thread"), "me-voice");
        assert_eq!(spoken_name("me-voice", "  new THREAD "), "me-voice");
        assert_eq!(spoken_name("me-voice", ""), "me-voice");
        assert_eq!(spoken_name("", "Fix it"), "Fix it");
        assert_eq!(spoken_name("", ""), "T3 thread");
    }

    #[test]
    fn spoken_name_strips_punctuation() {
        assert_eq!(spoken_name("My App: v2", "What's next? (really) - ok!"), "My App v2, Whats next really ok");
        assert_eq!(spoken_name("a_b", "x/y.z"), "a b, x y z");
    }

    #[test]
    fn still_working_schedule_backs_off() {
        let min = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(still_working_gap(2, 0), min(2));
        assert_eq!(still_working_gap(2, 1), min(7));
        assert_eq!(still_working_gap(2, 2), min(17));
        assert_eq!(still_working_gap(2, 3), min(27));
        assert_eq!(still_working_gap(1, 0), min(1));
        assert_eq!(still_working_gap(1, 1), min(6));
    }
}
