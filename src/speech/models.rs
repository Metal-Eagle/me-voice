//! The model list (`models.json`) plus download and checksum verification.
//!
//! The built-in list is compiled into the app. A `models.json` in the config folder
//! replaces it, so models can be added without rebuilding.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::settings::{config_dir, models_dir};

const BUILT_IN: &str = include_str!("../../models.json");

/// Which speech engine runs the model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Whisper,
    Parakeet,
}

/// One file of a model that is stored as a folder (for example Parakeet's several ONNX files).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub url: String,
    pub file: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    /// Download URL of a single-file model (whisper `.bin`).
    #[serde(default)]
    pub url: String,
    /// File name of a single-file model, or folder name of a model with `files`.
    pub file: String,
    pub sha256: Option<String>,
    pub languages: Vec<String>,
    pub acft: bool,
    #[serde(default)]
    pub engine: Engine,
    /// Approximate download size in MB, shown on the Speech page.
    #[serde(default)]
    pub size_mb: Option<u64>,
    /// Files of a folder model. Empty for a single-file model.
    #[serde(default)]
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize)]
struct ModelList {
    models: Vec<Model>,
}

pub fn list() -> Vec<Model> {
    let custom = fs::read_to_string(config_dir().join("models.json")).ok();
    let parsed = custom
        .as_deref()
        .and_then(|s| serde_json::from_str::<ModelList>(s).ok())
        .or_else(|| serde_json::from_str::<ModelList>(BUILT_IN).ok());
    parsed.map(|l| l.models).unwrap_or_default()
}

pub fn find(id: &str) -> Option<Model> {
    list().into_iter().find(|m| m.id == id)
}

impl Model {
    /// The model file, or the model folder when it has `files`.
    pub fn path(&self) -> PathBuf {
        models_dir().join(&self.file)
    }

    pub fn is_downloaded(&self) -> bool {
        if self.files.is_empty() {
            self.path().is_file()
        } else {
            self.files.iter().all(|f| self.path().join(&f.file).is_file())
        }
    }

    /// Downloads the model, reporting progress as (bytes done, total bytes) per file.
    pub fn download(&self, mut progress: impl FnMut(u64, Option<u64>)) -> Result<()> {
        if self.files.is_empty() {
            return fetch(&self.url, &self.path(), self.sha256.as_deref(), &self.name, &mut progress);
        }
        for f in &self.files {
            fetch(&f.url, &self.path().join(&f.file), f.sha256.as_deref(), &f.file, &mut progress)?;
        }
        Ok(())
    }
}

/// Downloads one file to `dest` via a `.part` file, checking its SHA-256 when one is given.
fn fetch(url: &str, dest: &Path, expected: Option<&str>, label: &str, progress: &mut impl FnMut(u64, Option<u64>)) -> Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = dest.with_extension("part");
    let mut response = ureq::get(url).call().with_context(|| format!("downloading {url}"))?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();
    let mut out = File::create(&partial)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);

    let hash = hex(&hasher.finalize());
    if let Some(expected) = expected {
        if !expected.eq_ignore_ascii_case(&hash) {
            let _ = fs::remove_file(&partial);
            bail!("checksum mismatch for {label} (got {hash})");
        }
    }
    fs::rename(&partial, dest).map_err(|e| anyhow!("saving model: {e}"))?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}
