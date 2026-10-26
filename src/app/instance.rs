//! Single-instance guard: an exclusive lock on a file in the local data folder.
//! The OS releases the lock when the process exits, even after a crash.

use std::fs::{self, File};

/// Holds the lock for as long as the returned value lives.
pub struct Guard(#[allow(dead_code)] File);

/// Returns `None` when another process already holds the lock called `name`.
pub fn acquire(name: &str) -> Option<Guard> {
    let dir = crate::settings::data_dir();
    let _ = fs::create_dir_all(&dir);
    let file = File::options().create(true).write(true).truncate(false).open(dir.join(format!("{name}.lock"))).ok()?;
    file.try_lock().ok()?;
    Some(Guard(file))
}
