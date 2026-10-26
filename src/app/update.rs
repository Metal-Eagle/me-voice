//! Updates from the GitHub releases of Metal-Eagle/me-voice.
//!
//! The repository is public, so the requests need no GitHub login.
//!
//! On Windows the release's `me-voice-setup-<version>.exe` is downloaded and run silently;
//! the installer closes this app and starts the new version. Elsewhere it only tells you.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const REPO: &str = "Metal-Eagle/me-voice";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Release {
    pub version: String,
    /// File name and download URL of the Windows installer, when the release has one.
    installer: Option<(String, String)>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// The latest release when it is newer than this build, `None` when up to date.
pub fn check() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut response = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "me-voice")
        .call()
        .context("checking for updates")?;
    let release: ApiRelease = serde_json::from_reader(response.body_mut().as_reader()).context("reading the release")?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    if !is_newer(&version, CURRENT) {
        return Ok(None);
    }
    let installer = release
        .assets
        .into_iter()
        .find(|a| a.name.starts_with("me-voice-setup-") && a.name.ends_with(".exe"))
        .map(|a| (a.name, a.browser_download_url));
    Ok(Some(Release { version, installer }))
}

/// Downloads the installer to the temp folder and returns its path.
pub fn download(release: &Release) -> Result<PathBuf> {
    let (name, url) = release.installer.as_ref().ok_or_else(|| anyhow!("release {} has no Windows installer", release.version))?;
    let dir = std::env::temp_dir().join("me-voice-update");
    fs::create_dir_all(&dir)?;
    let dest = dir.join(name);
    let partial = dest.with_extension("part");
    let mut response = agent().get(url).header("User-Agent", "me-voice").call().context("downloading the update")?;
    let mut reader = response.body_mut().as_reader();
    let mut out = File::create(&partial)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
    }
    out.flush()?;
    drop(out);
    fs::rename(&partial, &dest)?;
    Ok(dest)
}

/// Starts the installer silently. It closes this app and starts the new version when done.
#[cfg(windows)]
pub fn install(installer: &std::path::Path) -> Result<()> {
    Command::new(installer)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .with_context(|| format!("starting {}", installer.display()))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn install(_installer: &std::path::Path) -> Result<()> {
    bail!("automatic install is only available on Windows")
}

/// Whether this platform can install a release by itself.
pub fn can_install(release: &Release) -> bool {
    cfg!(windows) && release.installer.is_some()
}

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(300))).build();
    ureq::Agent::new_with_config(config)
}

/// Compares dotted version numbers ("0.10.0" is newer than "0.9.1").
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| v.split(['.', '-']).take(3).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_as_numbers() {
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(is_newer("0.9.2", "0.9.1"));
        assert!(!is_newer("0.9.1", "0.9.1"));
        assert!(!is_newer("0.9.0", "0.9.1"));
        assert!(is_newer("1.0.0", "0.99.99"));
    }

}
