//! Speech engines behind one wrapper: loads the model on first use and unloads it again
//! when idle. Whisper (whisper.cpp) and Parakeet (ONNX) are both supported.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Result};
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::speech::models::Engine;

pub const SAMPLE_RATE: usize = 16_000;

/// whisper.cpp encodes 30 s of audio into 1500 frames, i.e. 50 frames per second.
const FRAMES_PER_SECOND: f32 = 50.0;
const FULL_AUDIO_CTX: i32 = 1500;
/// Extra frames on top of the clip length, so words at the very end aren't cut off.
const AUDIO_CTX_MARGIN: i32 = 64;

/// Main model plus (optionally) a small, fast one for the live preview.
const MAX_LOADED: usize = 2;

/// A loaded speech model of either engine.
enum Loaded {
    /// The state (work buffers) is kept and reused, so each run skips allocating it.
    Whisper(WhisperContext, Option<WhisperState>),
    Parakeet(Box<ParakeetModel>),
}

pub struct Transcriber {
    loaded: Vec<(PathBuf, Loaded)>,
    pub last_used: Instant,
}

pub struct Request<'a> {
    pub engine: Engine,
    pub model_path: &'a Path,
    pub language: &'a str,
    /// ACFT models can safely use a shortened audio window.
    pub acft: bool,
    /// Hint words for names and jargon (personal dictionary).
    pub prompt: &'a str,
    /// Live preview while still recording: one segment, fewer CPU threads.
    pub light: bool,
    /// Set to true to stop this transcription early (live preview when you stop talking).
    pub abort: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// CPU threads for Whisper; 0 = pick automatically.
    pub threads: u32,
}

impl Transcriber {
    pub fn new() -> Self {
        whisper_rs::install_logging_hooks();
        Self { loaded: Vec::new(), last_used: Instant::now() }
    }

    pub fn is_loaded(&self) -> bool {
        !self.loaded.is_empty()
    }

    pub fn unload(&mut self) {
        self.loaded.clear();
    }

    /// Loads a model ahead of time (when you start talking), so the first
    /// transcription doesn't have to wait for it.
    pub fn preload(&mut self, model_path: &Path, engine: Engine) -> Result<()> {
        self.load(model_path, engine)?;
        self.last_used = Instant::now();
        Ok(())
    }

    fn load(&mut self, model_path: &Path, engine: Engine) -> Result<&mut Loaded> {
        if let Some(i) = self.loaded.iter().position(|(p, _)| p == model_path) {
            return Ok(&mut self.loaded[i].1);
        }
        let started = Instant::now();
        if self.loaded.len() >= MAX_LOADED {
            self.loaded.remove(0);
        }
        let loaded = match engine {
            Engine::Whisper => Loaded::Whisper(load_whisper(model_path)?, None),
            Engine::Parakeet => {
                let model = ParakeetModel::load(model_path, &Quantization::Int8)
                    .map_err(|e| anyhow!("loading model {}: {e}", model_path.display()))?;
                Loaded::Parakeet(Box::new(model))
            }
        };
        crate::app::log::write(&format!(
            "loaded {} in {:.2} s",
            model_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            started.elapsed().as_secs_f32()
        ));
        self.loaded.push((model_path.to_path_buf(), loaded));
        Ok(&mut self.loaded.last_mut().unwrap().1)
    }

    /// Transcribes 16 kHz mono samples.
    pub fn transcribe(&mut self, samples: &[f32], req: &Request) -> Result<String> {
        self.last_used = Instant::now();
        // A live preview that was overtaken (you stopped talking) skips its turn,
        // so the final transcription never waits behind it.
        if req.abort.as_ref().is_some_and(|a| a.load(std::sync::atomic::Ordering::Relaxed)) {
            return Ok(String::new());
        }
        let loaded = self.load(req.model_path, req.engine)?;
        let text = match loaded {
            Loaded::Whisper(ctx, state) => {
                if state.is_none() {
                    *state = Some(ctx.create_state().map_err(|e| anyhow!("creating state: {e}"))?);
                }
                whisper_text(state.as_mut().unwrap(), samples, req)?
            }
            Loaded::Parakeet(model) => parakeet_text(model, samples)?,
        };
        self.last_used = Instant::now();
        Ok(text.trim().to_string())
    }
}

fn load_whisper(model_path: &Path) -> Result<WhisperContext> {
    let mut ctx_params = WhisperContextParameters::default();
    // Flash attention: measured slower on CPU (1.53 s vs 1.27 s for 20 s of speech), so off.
    // ME_VOICE_FLASH=1 turns it on (testing; may help on GPU builds).
    ctx_params.flash_attn(env_flag("ME_VOICE_FLASH", false));
    WhisperContext::new_with_params(model_path, ctx_params).map_err(|e| anyhow!("loading model {}: {e}", model_path.display()))
}

fn whisper_text(state: &mut WhisperState, samples: &[f32], req: &Request) -> Result<String> {
    // Whisper needs at least one second of audio; pad short clips with silence.
    let mut audio = samples.to_vec();
    if audio.len() < SAMPLE_RATE {
        audio.resize(SAMPLE_RATE + SAMPLE_RATE / 10, 0.0);
    }

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(thread_count(req.threads));
    // The preview only needs the running text, not segment boundaries.
    params.set_single_segment(req.light);
    params.set_language(if req.language == "auto" { None } else { Some(req.language) });
    params.set_no_context(true);
    params.set_no_timestamps(true);
    params.set_suppress_blank(true);
    // No temperature fallback: by default Whisper decodes the same audio up to five more
    // times when it's unsure. That costs seconds and causes repeated/stumbling words.
    // ME_VOICE_FALLBACK=1 turns it back on (testing).
    if !env_flag("ME_VOICE_FALLBACK", false) {
        params.set_temperature_inc(0.0);
    }
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    if !req.prompt.is_empty() {
        params.set_initial_prompt(req.prompt);
    }
    // Not using whisper's abort callback: in whisper-rs 0.16 it fired at random and made
    // most live-preview runs fail (error -6). Preview runs are short enough (<0.1 s with
    // the small model) that the final transcription can simply wait for them.
    let _ = &req.abort;
    if req.acft {
        params.set_audio_ctx(audio_ctx_for(audio.len()));
    }

    state.full(params, &audio).map_err(|e| anyhow!("transcribing: {e}"))?;

    let mut text = String::new();
    for segment in state.as_iter() {
        text.push_str(&segment.to_str_lossy().map_err(|e| anyhow!("{e}"))?);
    }
    Ok(text)
}

/// Parakeet detects the language itself, so no language is passed.
fn parakeet_text(model: &mut ParakeetModel, samples: &[f32]) -> Result<String> {
    let params = ParakeetParams { language: None, timestamp_granularity: None };
    let result = model.transcribe_with(samples, &params).map_err(|e| anyhow!("transcribing: {e}"))?;
    Ok(result.text)
}

/// Number of encoder frames for a clip of `n_samples`, capped at the full 30 s window.
pub fn audio_ctx_for(n_samples: usize) -> i32 {
    let seconds = n_samples as f32 / SAMPLE_RATE as f32;
    ((seconds * FRAMES_PER_SECOND).ceil() as i32 + AUDIO_CTX_MARGIN).min(FULL_AUDIO_CTX)
}

/// Reads an on/off switch from the environment (for speed testing); `default` otherwise.
fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name).as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        _ => default,
    }
}

/// Whisper splits work evenly over its threads and waits for the slowest one, so on
/// CPUs with hyperthreading or slow efficiency cores more threads can be much slower.
/// `setting` (0 = auto) comes from the settings; `ME_VOICE_THREADS` overrides both (testing).
pub fn thread_count(setting: u32) -> i32 {
    if let Some(n) = std::env::var("ME_VOICE_THREADS").ok().and_then(|v| v.parse::<i32>().ok()) {
        return n.max(1);
    }
    if setting > 0 {
        return setting as i32;
    }
    let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    auto_threads(logical) as i32
}

/// About the number of physical cores: half the logical ones (hyperthreading), at least
/// two where there are two, at most eight.
fn auto_threads(logical: usize) -> usize {
    (logical / 2).max(logical.min(2)).min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_threads_follow_physical_cores() {
        assert_eq!(auto_threads(1), 1);
        assert_eq!(auto_threads(2), 2);
        assert_eq!(auto_threads(4), 2);
        assert_eq!(auto_threads(8), 4);
        assert_eq!(auto_threads(12), 6);
        assert_eq!(auto_threads(32), 8);
    }
}
