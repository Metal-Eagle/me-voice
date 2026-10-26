//! Dictation history (`history.jsonl` in the local data folder), one JSON line per
//! dictation. Feeds the History and Dashboard pages. Stays on this computer.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time in seconds.
    pub time: i64,
    /// Length of the recording.
    pub seconds: f32,
    pub words: u32,
    /// Empty when "Keep history text" is off; the numbers still count for the dashboard.
    #[serde(default)]
    pub text: String,
    /// Program the text went into (e.g. `ms-teams.exe`), never the window title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

fn path() -> PathBuf {
    crate::settings::data_dir().join("history.jsonl")
}

pub fn append(entry: &Entry) {
    let path = path();
    let _ = fs::create_dir_all(path.parent().unwrap());
    if let (Ok(mut f), Ok(line)) = (OpenOptions::new().create(true).append(true).open(&path), serde_json::to_string(entry)) {
        let _ = writeln!(f, "{line}");
    }
}

/// All entries, oldest first. Broken lines are skipped.
pub fn load() -> Vec<Entry> {
    let mut entries: Vec<Entry> = fs::read_to_string(path())
        .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.time);
    entries
}

pub fn clear() {
    let _ = fs::remove_file(path());
}

pub fn count_words(text: &str) -> u32 {
    text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count() as u32
}

/// Numbers for the dashboard.
pub struct Stats {
    pub words: u64,
    pub sessions: u64,
    pub speaking_seconds: f64,
    pub typing_seconds: f64,
    pub words_per_minute: f64,
    pub words_per_session: f64,
    /// Words per day for the last 30 days, oldest first (index 29 = today).
    pub last_30_days: [u64; 30],
    /// Words per program over the last 30 days, highest first: top 5, then "Other".
    /// Empty when no dictation in that period has a program.
    pub per_app: Vec<(String, u64)>,
}

impl Stats {
    pub fn from(entries: &[Entry], typing_wpm: u32) -> Self {
        let words: u64 = entries.iter().map(|e| e.words as u64).sum();
        let sessions = entries.len() as u64;
        let speaking_seconds: f64 = entries.iter().map(|e| e.seconds as f64).sum();
        let typing_seconds = words as f64 / typing_wpm.max(1) as f64 * 60.0;

        let today = chrono::Local::now().date_naive();
        let mut last_30_days = [0u64; 30];
        let mut apps: Vec<(String, u64)> = Vec::new();
        let mut no_app = 0u64;
        let mut any_app = false;
        for e in entries {
            let Some(day) = chrono::DateTime::from_timestamp(e.time, 0).map(|t| t.with_timezone(&chrono::Local).date_naive()) else {
                continue;
            };
            let ago = (today - day).num_days();
            if (0..30).contains(&ago) {
                let w = e.words as u64;
                last_30_days[29 - ago as usize] += w;
                match &e.app {
                    Some(exe) => {
                        any_app = true;
                        add_words(&mut apps, app_label(exe), w);
                    }
                    None => no_app += w,
                }
            }
        }
        let per_app = if any_app { rank_apps(apps, no_app) } else { Vec::new() };

        Self {
            words,
            sessions,
            speaking_seconds,
            typing_seconds,
            words_per_minute: if speaking_seconds > 0.0 { words as f64 / (speaking_seconds / 60.0) } else { 0.0 },
            words_per_session: if sessions > 0 { words as f64 / sessions as f64 } else { 0.0 },
            last_30_days,
            per_app,
        }
    }

    /// How many times faster than typing.
    pub fn speedup(&self) -> f64 {
        if self.speaking_seconds > 0.0 { self.typing_seconds / self.speaking_seconds } else { 0.0 }
    }

    pub fn time_saved_seconds(&self) -> f64 {
        (self.typing_seconds - self.speaking_seconds).max(0.0)
    }
}

/// Friendly name for a program's exe name: "ms-teams.exe" becomes "Teams".
/// Other programs get their exe name without ".exe", capitalised.
pub fn app_label(exe: &str) -> String {
    let exe = exe.trim().to_lowercase();
    let known = match exe.as_str() {
        "ms-teams.exe" | "teams.exe" => "Teams",
        "outlook.exe" | "olk.exe" => "Outlook",
        "code.exe" => "VS Code",
        "msedge.exe" => "Edge",
        "chrome.exe" => "Chrome",
        "firefox.exe" => "Firefox",
        "slack.exe" => "Slack",
        "windowsterminal.exe" => "Terminal",
        "winword.exe" => "Word",
        "excel.exe" => "Excel",
        "notepad.exe" => "Notepad",
        "explorer.exe" => "File Explorer",
        "claude.exe" => "Claude",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let base = exe.strip_suffix(".exe").unwrap_or(&exe);
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Unknown".to_string(),
    }
}

/// Adds words to a program's total, merging rows with the same label.
fn add_words(list: &mut Vec<(String, u64)>, label: String, words: u64) {
    match list.iter_mut().find(|(l, _)| *l == label) {
        Some((_, total)) => *total += words,
        None => list.push((label, words)),
    }
}

/// Sorts programs by words, keeps the top 5 and adds "Other" for the rest and for
/// dictations without a program (the row is left out when that adds up to 0).
fn rank_apps(mut apps: Vec<(String, u64)>, no_app: u64) -> Vec<(String, u64)> {
    apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let rest: u64 = apps.iter().skip(5).map(|a| a.1).sum::<u64>() + no_app;
    apps.truncate(5);
    if rest > 0 {
        apps.push(("Other".to_string(), rest));
    }
    apps
}

/// "2h 48m 35s", "4m 12s", "38s".
pub fn format_duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m {s}s"),
        _ => format!("{h}h {m}m {s}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(time: i64, words: u32, app: Option<&str>) -> Entry {
        Entry { time, seconds: 30.0, words, text: String::new(), app: app.map(str::to_string) }
    }

    #[test]
    fn app_labels() {
        assert_eq!(app_label("ms-teams.exe"), "Teams");
        assert_eq!(app_label("teams.exe"), "Teams");
        assert_eq!(app_label("code.exe"), "VS Code");
        assert_eq!(app_label("WindowsTerminal.exe"), "Terminal");
        assert_eq!(app_label("foobar.exe"), "Foobar");
        assert_eq!(app_label("tool"), "Tool");
    }

    #[test]
    fn apps_merge_by_label() {
        let now = chrono::Local::now().timestamp();
        let entries = vec![
            entry(now, 100, Some("ms-teams.exe")),
            entry(now, 50, Some("teams.exe")),
            entry(now, 30, Some("outlook.exe")),
        ];
        let s = Stats::from(&entries, 40);
        assert_eq!(s.per_app, vec![("Teams".to_string(), 150), ("Outlook".to_string(), 30)]);
    }

    #[test]
    fn apps_top_five_and_other() {
        let now = chrono::Local::now().timestamp();
        let mut entries: Vec<Entry> = (1..=7).map(|i| entry(now, 10 * (8 - i), Some(&format!("a{i}.exe")))).collect();
        entries.push(entry(now, 5, None));
        let s = Stats::from(&entries, 40);
        assert_eq!(s.per_app.len(), 6);
        assert_eq!(s.per_app[0], ("A1".to_string(), 70));
        assert_eq!(s.per_app[4], ("A5".to_string(), 30));
        assert_eq!(s.per_app[5], ("Other".to_string(), 35));
    }

    #[test]
    fn apps_empty_without_program() {
        let now = chrono::Local::now().timestamp();
        let entries = vec![entry(now, 20, None), entry(now - 86_400 * 40, 20, Some("code.exe"))];
        let s = Stats::from(&entries, 40);
        assert!(s.per_app.is_empty());
        assert_eq!(s.words, 40);
    }

    #[test]
    fn stats_add_up() {
        let now = chrono::Local::now().timestamp();
        let entries = vec![entry(now, 75, None), entry(now - 86_400, 75, None)];
        let s = Stats::from(&entries, 40);
        assert_eq!(s.words, 150);
        assert_eq!(s.sessions, 2);
        assert_eq!(s.words_per_minute, 150.0);
        assert_eq!(s.typing_seconds, 225.0);
        assert_eq!(s.speedup(), 3.75);
        assert_eq!(s.last_30_days[29], 75);
        assert_eq!(s.last_30_days[28], 75);
    }

    #[test]
    fn durations_format() {
        assert_eq!(format_duration(10115.0), "2h 48m 35s");
        assert_eq!(format_duration(252.0), "4m 12s");
        assert_eq!(format_duration(38.0), "38s");
    }
}
