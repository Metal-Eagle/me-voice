//! Microphone capture. The stream only exists between `start` and `stop`,
//! so the microphone is closed (and costs nothing) the rest of the time.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{InputCallbackInfo, SampleFormat, Stream};

use crate::speech::transcriber::SAMPLE_RATE;

pub struct Recording {
    /// `None` when a test recording is played instead of the microphone.
    stream: Option<Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    stop_test: Arc<std::sync::atomic::AtomicBool>,
}

/// Names of the microphones the system currently offers.
pub fn microphones() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else { return Vec::new() };
    let mut names: Vec<String> = devices.filter_map(|d| device_name(&d)).collect();
    names.sort();
    names.dedup();
    names
}

fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

/// Opens the chosen microphone (or the system default) and starts recording.
/// Falls back to the default when the chosen one isn't connected.
pub fn start(microphone: Option<&str>) -> Result<Recording> {
    if let Ok(path) = std::env::var("ME_VOICE_TEST_WAV") {
        return start_test_recording(&path);
    }
    let host = cpal::default_host();
    let chosen = microphone.and_then(|wanted| {
        host.input_devices().ok()?.find(|d| device_name(d).as_deref() == Some(wanted))
    });
    let device = chosen
        .or_else(|| host.default_input_device())
        .ok_or_else(|| anyhow!("no microphone found"))?;
    let supported = device.default_input_config()?;
    let rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let config = supported.config();
    let buffer = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));
    let on_error = |e: cpal::Error| eprintln!("audio error: {e}");

    let stream = match supported.sample_format() {
        SampleFormat::F32 => {
            let buf = buffer.clone();
            device.build_input_stream(
                config,
                move |data: &[f32], _: &InputCallbackInfo| push_mono(&buf, data, channels, |s| s),
                on_error,
                None,
            )?
        }
        SampleFormat::I16 => {
            let buf = buffer.clone();
            device.build_input_stream(
                config,
                move |data: &[i16], _: &InputCallbackInfo| {
                    push_mono(&buf, data, channels, |s| s as f32 / i16::MAX as f32)
                },
                on_error,
                None,
            )?
        }
        SampleFormat::I32 => {
            let buf = buffer.clone();
            device.build_input_stream(
                config,
                move |data: &[i32], _: &InputCallbackInfo| {
                    push_mono(&buf, data, channels, |s| s as f32 / i32::MAX as f32)
                },
                on_error,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported microphone sample format {other:?}")),
    };
    stream.play()?;
    Ok(Recording { stream: Some(stream), buffer, rate, stop_test: Arc::new(std::sync::atomic::AtomicBool::new(false)) })
}

fn push_mono<T: Copy>(buf: &Mutex<Vec<f32>>, data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) {
    let mut buf = buf.lock().unwrap();
    for frame in data.chunks(channels.max(1)) {
        let sum: f32 = frame.iter().map(|&s| to_f32(s)).sum();
        buf.push(sum / frame.len() as f32);
    }
}

impl Recording {
    /// True when the last ~2 seconds are louder than `level`.
    pub fn has_sound(&self, level: f32) -> bool {
        has_sound_at(&self.buffer.lock().unwrap(), self.rate, level)
    }

    /// How long it has been quiet at the end of the recording. `None` until any speech.
    pub fn silent_for(&self, level: f32) -> Option<Duration> {
        silent_for(&self.buffer.lock().unwrap(), self.rate, level)
    }

    pub fn seconds(&self) -> f32 {
        self.buffer.lock().unwrap().len() as f32 / self.rate as f32
    }

    /// Copy of everything recorded so far as 16 kHz mono (for live transcription).
    pub fn snapshot(&self) -> Vec<f32> {
        let samples = self.buffer.lock().unwrap().clone();
        resample(&samples, self.rate, SAMPLE_RATE as u32)
    }

    /// Loudness of the last ~2 seconds as `n` values from 0.0 to 1.0 (for the sound wave).
    pub fn levels(&self, n: usize) -> Vec<f32> {
        levels_of(&self.buffer.lock().unwrap(), self.rate, n)
    }

    /// Stops recording, closes the microphone and returns 16 kHz mono audio.
    pub fn stop(self) -> Vec<f32> {
        self.stop_test.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(stream) = self.stream {
            let _ = stream.pause();
            drop(stream);
        }
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap());
        resample(&samples, self.rate, SAMPLE_RATE as u32)
    }
}

/// Testing: `ME_VOICE_TEST_WAV=<16 kHz wav>` plays that file in real time as if it came
/// from the microphone (end-to-end tests without a person talking or sound playing).
fn start_test_recording(path: &str) -> Result<Recording> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let full_scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i32>().filter_map(Result::ok).map(|v| v as f32 / full_scale).collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
    };
    let rate = spec.sample_rate;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (buf, flag) = (buffer.clone(), stop.clone());
    std::thread::spawn(move || {
        let chunk = rate as usize / 50; // 20 ms
        for part in samples.chunks(chunk).chain(std::iter::repeat_n(&[0.0f32; 320][..], 500)) {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            buf.lock().unwrap().extend_from_slice(part);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    Ok(Recording { stream: None, buffer, rate, stop_test: stop })
}

/// Loudness of the last ~2 seconds of `samples` as `n` values from 0.0 to 1.0.
pub fn levels_of(samples: &[f32], rate: u32, n: usize) -> Vec<f32> {
    let window = (rate as usize * 2 / n).max(1);
    let start = samples.len().saturating_sub(window * n);
    let recent = &samples[start..];
    let missing = n - recent.len() / window;
    let mut out = vec![0.0; missing];
    out.extend(recent.chunks(window).take(n - missing).map(|c| {
        let rms = (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt();
        // Square root makes quiet speech visible without loud speech clipping.
        (rms * 6.0).sqrt().min(1.0)
    }));
    out
}

/// Default for the silence level setting: louder than this counts as speech.
/// The bar is deliberately low: quiet microphones must still get a live preview.
pub const DEFAULT_SILENCE_LEVEL: f32 = 0.04;

/// True when the last ~2 seconds hold anything worth recognising (not near-silence).
pub fn has_sound(samples: &[f32], rate: u32) -> bool {
    has_sound_at(samples, rate, DEFAULT_SILENCE_LEVEL)
}

fn has_sound_at(samples: &[f32], rate: u32, level: f32) -> bool {
    levels_of(samples, rate, 8).iter().copied().fold(0.0, f32::max) >= level
}

/// Length of one analysis frame for silence detection (20 ms).
fn frame_len(rate: u32) -> usize {
    (rate as usize * 20 / 1000).max(1)
}

/// Loudness of one frame on the same 0.0 to 1.0 scale as `levels_of`.
fn frame_level(frame: &[f32]) -> f32 {
    let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt();
    (rms * 6.0).sqrt().min(1.0)
}

/// How long the audio has been quiet at its end; `None` when no frame is louder than `level`.
fn silent_for(samples: &[f32], rate: u32, level: f32) -> Option<Duration> {
    let mut quiet = 0;
    for frame in samples.rchunks(frame_len(rate)) {
        if frame_level(frame) >= level {
            return Some(Duration::from_secs_f64(quiet as f64 / rate as f64));
        }
        quiet += frame.len();
    }
    None
}

/// Silence kept before and after speech (200 ms, in 20 ms frames).
const SPEECH_PAD_FRAMES: usize = 10;
/// Pauses longer than this (1 s) are shortened.
const LONG_PAUSE_FRAMES: usize = 50;
/// A shortened pause keeps this much silence (300 ms).
const SHORT_PAUSE_FRAMES: usize = 15;

/// Removes silence before and after speech and shortens long pauses inside it.
/// Works on 16 kHz audio. Returns nothing when there is no speech at all.
pub fn trim_silence(samples: &[f32], level: f32) -> Vec<f32> {
    let size = frame_len(SAMPLE_RATE as u32);
    let speech: Vec<bool> = samples.chunks(size).map(|f| frame_level(f) >= level).collect();
    let runs = speech_runs(&speech);
    let (Some(first), Some(last)) = (runs.first(), runs.last()) else { return Vec::new() };
    // Frame ranges to copy, in order. In a long pause only its first and last part stay.
    let mut ranges = Vec::new();
    let mut cursor = first.0.saturating_sub(SPEECH_PAD_FRAMES);
    for pair in runs.windows(2) {
        let (prev_end, next_start) = (pair[0].1, pair[1].0);
        if next_start - prev_end > LONG_PAUSE_FRAMES {
            let head = SHORT_PAUSE_FRAMES / 2;
            ranges.push((cursor, prev_end + head));
            cursor = next_start - (SHORT_PAUSE_FRAMES - head);
        }
    }
    ranges.push((cursor, (last.1 + SPEECH_PAD_FRAMES).min(speech.len())));
    ranges
        .iter()
        .flat_map(|&(from, to)| samples[from * size..(to * size).min(samples.len())].iter().copied())
        .collect()
}

/// Runs of `true` as `(start, end)` frame indices, end exclusive.
fn speech_runs(speech: &[bool]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (i, &on) in speech.iter().enumerate() {
        if !on {
            continue;
        }
        if let Some(run) = runs.last_mut().filter(|r| r.1 == i) {
            run.1 = i + 1;
        } else {
            runs.push((i, i + 1));
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = SAMPLE_RATE;

    /// Silence or a 440 Hz tone, in seconds.
    fn part(seconds: f32, tone: bool) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| if tone { 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / RATE as f32).sin() } else { 0.0 })
            .collect()
    }

    fn join(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    #[test]
    fn silence_trims_to_nothing() {
        assert!(trim_silence(&part(2.0, false), DEFAULT_SILENCE_LEVEL).is_empty());
    }

    #[test]
    fn leading_and_trailing_silence_is_cut_but_speech_padded() {
        let input = join(&[part(1.0, false), part(1.0, true), part(1.0, false)]);
        let out = trim_silence(&input, DEFAULT_SILENCE_LEVEL).len() as f32 / RATE as f32;
        assert!((out - 1.4).abs() < 0.05, "got {out} s");
    }

    #[test]
    fn long_pause_is_shortened_to_300_ms() {
        let input = join(&[part(0.5, false), part(0.5, true), part(3.0, false), part(0.5, true), part(0.5, false)]);
        let out = trim_silence(&input, DEFAULT_SILENCE_LEVEL).len() as f32 / RATE as f32;
        // Speech 1.0 s, pause 0.3 s, plus 200 ms padding on both ends.
        assert!((out - 1.7).abs() < 0.05, "got {out} s");
    }

    #[test]
    fn short_pause_is_kept() {
        let input = join(&[part(0.5, true), part(0.5, false), part(0.5, true)]);
        let out = trim_silence(&input, DEFAULT_SILENCE_LEVEL).len() as f32 / RATE as f32;
        assert!((out - 1.5).abs() < 0.05, "got {out} s");
    }

    #[test]
    fn silent_for_counts_quiet_end_only() {
        let input = join(&[part(0.5, true), part(1.0, false)]);
        let quiet = silent_for(&input, RATE as u32, DEFAULT_SILENCE_LEVEL).unwrap().as_secs_f32();
        assert!((quiet - 1.0).abs() < 0.03, "got {quiet} s");
        assert!(silent_for(&part(1.0, false), RATE as u32, DEFAULT_SILENCE_LEVEL).is_none());
    }
}

/// Linear resampling; good enough for speech going into Whisper.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = (input.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}
