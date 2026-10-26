//! Tiny log file (`me-voice.log` in the local data folder), because the release
//! build has no console. Kept short: it's trimmed when it grows past 256 KB.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SIZE: u64 = 256 * 1024;

pub fn path() -> PathBuf {
    crate::settings::data_dir().join("me-voice.log")
}

pub fn write(message: &str) {
    eprintln!("{message}");
    let path = path();
    let _ = fs::create_dir_all(path.parent().unwrap());
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_SIZE) {
        let _ = fs::remove_file(&path);
    }
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{h:02}:{m:02}:{s:02} UTC  {message}");
    }
}
