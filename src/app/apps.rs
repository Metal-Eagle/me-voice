//! Apps that have their own profile preset, found on this computer.

/// The program name that profiles list for T3 Code (lower case, as `text::foreground` reads it).
pub const T3_CODE_EXE: &str = "t3 code (beta).exe";

/// Whether T3 Code is installed: its beta build lives in `%LOCALAPPDATA%\Programs\t3code`.
#[cfg(windows)]
pub fn t3_code_installed() -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return false };
    let dir = std::path::PathBuf::from(local).join("Programs").join("t3code");
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        name.starts_with("t3 code") && name.ends_with(".exe") && !name.starts_with("uninstall")
    })
}

#[cfg(not(windows))]
pub fn t3_code_installed() -> bool {
    false
}
