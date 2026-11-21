//! Per-user startup registration. Changing it never stops the running app.

use anyhow::Result;

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(windows)]
const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The current user's Run key, read and written through the registry API. Running
/// `reg.exe` instead flashes a console window, since this app has none of its own.
#[cfg(windows)]
mod run_key {
    use anyhow::{Result, bail};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_ANY, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn check(code: WIN32_ERROR, what: &str) -> Result<()> {
        if code != ERROR_SUCCESS {
            bail!("{what}: {}", std::io::Error::from_raw_os_error(code as i32));
        }
        Ok(())
    }

    pub fn exists(name: &str) -> Result<bool> {
        let (key, name) = (wide(super::KEY), wide(name));
        let code = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_ANY, null_mut(), null_mut(), null_mut())
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        check(code, "reading startup registration")?;
        Ok(true)
    }

    pub fn set(name: &str, value: &str) -> Result<()> {
        let (key, name, data) = (wide(super::KEY), wide(name), wide(value));
        let code = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ,
                data.as_ptr().cast(), (data.len() * 2) as u32,
            )
        };
        check(code, "updating startup registration")
    }

    /// Removes the value; one that is already gone counts as removed.
    pub fn delete(name: &str) -> Result<()> {
        let (key, name) = (wide(super::KEY), wide(name));
        let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(code, "updating startup registration")
    }
}

/// Windows: removes the "me-voice-dev" start-at-login entry older versions let a
/// development build add. It launched that build, console window included, at every login.
#[cfg(windows)]
pub fn remove_dev_entry() {
    if run_key::exists("me-voice-dev").unwrap_or(false) && run_key::delete("me-voice-dev").is_ok() {
        crate::app::log::write("removed the development build from start at login");
    }
}

/// Windows: if the old "mc-voice" start-at-login entry exists, replaces it with the
/// me-voice one. Returns whether it did.
#[cfg(windows)]
pub fn replace_old_entry() -> bool {
    if !run_key::exists("mc-voice").unwrap_or(false) {
        return false;
    }
    if let Err(e) = set_enabled(true) {
        crate::app::log::write(&format!("could not migrate start at login: {e:#}"));
        return false;
    }
    if let Err(e) = run_key::delete("mc-voice") {
        crate::app::log::write(&format!("could not remove the old mc-voice start-at-login entry: {e:#}"));
        return false;
    }
    true
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<()> {
    use anyhow::{Context, bail};

    let name = if crate::settings::is_dev() { "me-voice-dev" } else { "me-voice" };
    if !enabled {
        return run_key::delete(name);
    }
    // A development build opens a console window and lives in a checkout that may be
    // a stale worktree, so it never starts itself at login.
    if crate::settings::is_dev() {
        bail!("development builds don't start at login; use the installed me-voice");
    }
    let exe = std::env::current_exe().context("finding the startup executable")?;
    run_key::set(name, &format!("\"{}\"", exe.display()))
}

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> Result<()> {
    use anyhow::Context;
    use std::fs;

    let home = directories::BaseDirs::new().context("finding the home folder for login startup")?;
    let folder = home.home_dir().join("Library").join("LaunchAgents");
    let dev = crate::settings::is_dev();
    let path = folder.join(format!("{}.plist", label(dev)));
    if enabled {
        let body = launch_agent(&std::env::current_exe()?, dev)?;
        fs::create_dir_all(&folder).context("creating the LaunchAgents folder")?;
        fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    } else if let Err(e) = fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e).with_context(|| format!("removing {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_enabled(_enabled: bool) -> Result<()> {
    anyhow::bail!("start at login is not supported on this platform")
}

#[cfg(any(target_os = "macos", test))]
fn label(dev: bool) -> &'static str {
    if dev { "io.github.metal-eagle.me-voice-dev" } else { "io.github.metal-eagle.me-voice" }
}

#[cfg(any(target_os = "macos", test))]
fn launch_agent(exe: &std::path::Path, dev: bool) -> Result<String> {
    use anyhow::Context;
    let executable = exe.to_str().context("the startup executable path is not valid UTF-8")?
        .replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;");
    let args = if dev { "\n        <string>--dev</string>" } else { "" };
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
             <key>Label</key><string>{}</string>\n\
             <key>ProgramArguments</key>\n    <array>\n        <string>{executable}</string>{args}\n    </array>\n\
             <key>RunAtLoad</key><true/>\n\
         </dict>\n</plist>\n", label(dev)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_login_quotes_paths_as_xml_not_shell_arguments() {
        let body = launch_agent(std::path::Path::new("/Applications/Voice & <Talk> \"test\".app/me-voice"), false).unwrap();
        assert!(body.contains("<string>/Applications/Voice &amp; &lt;Talk&gt; &quot;test&quot;.app/me-voice</string>"));
        assert!(body.contains("<key>RunAtLoad</key><true/>"));
        assert!(!body.contains("--dev"));
    }

    #[test]
    fn mac_dev_login_has_a_separate_identity_and_argument() {
        let body = launch_agent(std::path::Path::new("/Applications/me-voice.app/me-voice"), true).unwrap();
        assert!(body.contains(label(true)));
        assert!(body.contains("<string>--dev</string>"));
        assert_ne!(label(true), label(false));
    }
}
