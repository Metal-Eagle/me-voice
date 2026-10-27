// No console window for the tray app on Windows (release builds).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod settings;
mod speech;
mod text;
mod ui;

use app::{ipc, log, power};
use speech::{audio, models, transcriber};
use text::cleanup;
use ui::{fix_window, icon, paragraph_window, settings as settings_ui};
use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    let result = run();
    if let Err(e) = &result {
        log::write(&format!("startup/command failed: {e:#}"));
    }
    result
}

fn run() -> Result<()> {
    // `--dev` (development instance) can appear anywhere; see settings::is_dev.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--dev").collect();
    power::disable_throttling();
    // Release builds abort on a panic without a trace; write it to the log first.
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        log::write(&format!("CRASH in thread '{thread}' at {place}: {message}"));
    }));
    match args.first().map(String::as_str) {
        None => app::run(),
        Some("settings") => settings_ui::run(),
        Some("fix-last") => fix_window::run(args.get(1).ok_or_else(|| anyhow!("usage: me-voice fix-last <file>"))?),
        Some("paragraph") => paragraph_window::run(),
        Some("quit") => ipc::send(ipc::Command::Quit).map_err(|_| anyhow!("me-voice is not running")),
        Some("toggle") => ipc::send(ipc::Command::Toggle).map_err(|_| anyhow!("me-voice is not running")),
        Some("check-update") => {
            match app::update::check()? {
                Some(release) => println!("me-voice {} is available (this is {})", release.version, app::update::CURRENT),
                None => println!("me-voice {} is up to date", app::update::CURRENT),
            }
            Ok(())
        }
        Some("microphones") => {
            audio::microphones().iter().for_each(|m| println!("{m}"));
            Ok(())
        }
        Some("write-icon") => Ok(icon::write_ico(std::path::Path::new(args.get(1).map_or("me-voice.ico", String::as_str)))?),
        Some("live-test") => live_test(args.get(1).ok_or_else(|| anyhow!("usage: me-voice live-test <file.wav>"))?),
        Some("transcribe") => transcribe_file(args.get(1).ok_or_else(|| anyhow!("usage: me-voice transcribe <file.wav>"))?),
        Some(other) => Err(anyhow!("unknown command {other}. Use: settings | toggle | quit | microphones | transcribe <file.wav> | live-test <file.wav>")),
    }
}

/// Test helper: plays a 16 kHz WAV through the same live-preview logic as the tray app
/// (an update every half second over the last 10 seconds, voice commands included) and
/// prints what the card would show, with timings.
fn live_test(path: &str) -> Result<()> {
    use std::time::Instant;

    let settings = settings::Settings::load();
    let samples = read_wav_16k_mono(path)?;
    let rate = transcriber::SAMPLE_RATE;
    let transcriber = std::sync::Mutex::new(transcriber::Transcriber::new());
    power::set_priority(power::Priority::AboveNormal);
    let step = rate / 2;
    let mut end = step;
    while end <= samples.len() {
        let heard = &samples[..end];
        let at = end as f32 / rate as f32;
        if !audio::has_sound(heard, rate as u32) {
            println!("{at:5.1} s  (silence, skipped)");
            end += step;
            continue;
        }
        let window = &heard[heard.len().saturating_sub(rate * 10)..];
        let start = Instant::now();
        let text = app::transcribe(&settings, &transcriber, window, true, None)?;
        let took = start.elapsed().as_millis();
        println!("{at:5.1} s  [{took:4} ms]  {text}");
        if let Some((command, rest)) = cleanup::voice_command(&text, &settings) {
            println!("        -> voice command {command:?}: stop listening, paste \"{rest}\"");
            return Ok(());
        }
        // Like the app: the next update starts once this one is done.
        end += step.max((took as usize * rate) / 1000 / step * step);
    }
    println!("(end of file, no voice command)");
    Ok(())
}

/// Test helper: transcribe a WAV file with the current settings and print the result.
fn transcribe_file(path: &str) -> Result<()> {
    use std::time::Instant;

    let settings = settings::Settings::load();
    // ME_VOICE_MODEL=<id> tries another model (speed testing).
    let model_id = std::env::var("ME_VOICE_MODEL").unwrap_or_else(|_| settings.model_id.clone());
    let model = models::find(&model_id).ok_or_else(|| anyhow!("unknown model {model_id}"))?;
    let samples = read_wav_16k_mono(path)?;
    let dict = cleanup::load_dictionary();
    let snippets = cleanup::load_snippets();
    let commands = cleanup::load_commands();
    let prompt = dict.prompt();

    let mut t = transcriber::Transcriber::new();
    let request = transcriber::Request {
        engine: model.engine,
        model_path: &model.path(),
        language: &settings.language,
        acft: model.acft,
        prompt: &prompt,
        light: false,
        abort: None,
    };
    let start = Instant::now();
    let raw = t.transcribe(&samples, &request)?;
    let first = start.elapsed();
    let start = Instant::now();
    let _ = t.transcribe(&samples, &request)?;
    let second = start.elapsed();

    let seconds = samples.len() as f32 / transcriber::SAMPLE_RATE as f32;
    println!("audio:     {seconds:.1} s, audio_ctx {}", transcriber::audio_ctx_for(samples.len()));
    println!("raw:       {raw}");
    println!("cleaned:   {}", cleanup::clean(&raw, &settings, &dict, &snippets, &commands));
    println!("cpu:       {}", whisper_rs::print_system_info());
    println!("time:      {first:.2?} (incl. model load), {second:.2?} (model loaded)");
    Ok(())
}

fn read_wav_16k_mono(path: &str) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(anyhow!("WAV must be 16 kHz (got {} Hz)", spec.sample_rate));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32))
            .collect::<Result<_, _>>()?,
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
    };
    let ch = spec.channels as usize;
    Ok(samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect())
}
