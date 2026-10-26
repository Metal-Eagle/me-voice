//! Speech engines behind one wrapper: loads the model on first use and unloads it again
//! when idle. Whisper (whisper.cpp) and Parakeet (ONNX) are both supported.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Result};
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
    Whisper(WhisperContext),
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

    fn load(&mut self, model_path: &Path, engine: Engine) -> Result<&mut Loaded> {
        if let Some(i) = self.loaded.iter().position(|(p, _)| p == model_path) {
            return Ok(&mut self.loaded[i].1);
        }
        if self.loaded.len() >= MAX_LOADED {
            self.loaded.remove(0);
        }
        let loaded = match engine {
            Engine::Whisper => Loaded::Whisper(load_whisper(model_path)?),
            Engine::Parakeet => {
                let model = ParakeetModel::load(model_path, &Quantization::Int8)
                    .map_err(|e| anyhow!("loading model {}: {e}", model_path.display()))?;
                Loaded::Parakeet(Box::new(model))
            }
        };
        self.loaded.push((model_path.to_path_buf(), loaded));
        Ok(&mut self.loaded.last_mut().unwrap().1)
    }

    /// Transcribes 16 kHz mono samples.
    pub fn transcribe(&mut self, samples: &[f32], req: &Request) -> Result<String> {
        self.last_used = Instant::now();
        let loaded = self.load(req.model_path, req.engine)?;
        let text = match loaded {
            Loaded::Whisper(ctx) => whisper_text(ctx, samples, req)?,
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

fn whisper_text(ctx: &WhisperContext, samples: &[f32], req: &Request) -> Result<String> {
    let mut state = ctx.create_state().map_err(|e| anyhow!("creating state: {e}"))?;

    // Whisper needs at least one second of audio; pad short clips with silence.
    let mut audio = samples.to_vec();
    if audio.len() < SAMPLE_RATE {
        audio.resize(SAMPLE_RATE + SAMPLE_RATE / 10, 0.0);
    }

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(thread_count());
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
/// CPUs with slow efficiency cores more threads can be much slower. `ME_VOICE_THREADS`
/// overrides the default (for testing).
fn thread_count() -> i32 {
    if let Some(n) = std::env::var("ME_VOICE_THREADS").ok().and_then(|v| v.parse::<i32>().ok()) {
        return n.max(1);
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    cores.clamp(1, 8) as i32
}
