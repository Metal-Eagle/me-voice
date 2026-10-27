//! Per-user startup registration. Changing it never stops the running app.

use anyhow::Result;

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<()> {
    use anyhow::{Context, bail};
    use std::process::Command;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let name = if crate::settings::is_dev() { "me-voice-dev" } else { "me-voice" };
    let mut cmd = Command::new("reg");
    if enabled {
        let exe = std::env::current_exe().context("finding the startup executable")?;
        let mut value = format!("\"{}\"", exe.display());
        if crate::settings::is_dev() {
            value.push_str(" --dev");
        }
        cmd.args(["add", KEY, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"]);
    } else {
        let exists = Command::new("reg").args(["query", KEY, "/v", name]).output()?;
        if !exists.status.success() {
            // A missing value is already disabled; a missing/inaccessible key is not.
            let key = Command::new("reg").args(["query", KEY]).output()?;
            if !key.status.success() {
                bail!("reading startup registration: {}", String::from_utf8_lossy(&key.stderr).trim());
            }
            return Ok(());
        }
        cmd.args(["delete", KEY, "/v", name, "/f"]);
    }
    let output = cmd.output().context("updating startup registration")?;
    if !output.status.success() {
        bail!("updating startup registration: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
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
