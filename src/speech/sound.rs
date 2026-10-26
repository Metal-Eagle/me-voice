//! Short start/stop sounds, generated in code: a bright rising chime to start, a single
//! low falling note to stop, so they can't be mistaken for each other. The audio output is only opened
//! for the length of the beep.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, SampleFormat};

#[derive(Clone, Copy)]
pub enum Beep {
    /// Bright two-note chime going up: listening.
    Start,
    /// One soft low note sliding down: stopped.
    Stop,
}

/// Plays the beep on a background thread, so it never delays recording.
pub fn play(beep: Beep) {
    std::thread::spawn(move || {
        if let Err(e) = play_blocking(beep) {
            crate::app::log::write(&format!("sound: {e}"));
        }
    });
}

/// Silence before the beep: USB/dock/Bluetooth outputs often drop the first ~100 ms
/// while they wake up, which swallowed the whole beep.
const LEAD_IN: f32 = 0.15;
const VOLUME: f32 = 0.30;

/// One note: pitch at the start and end (Hz), length (s), and how much of the 2nd
/// harmonic to add (a bell-like chime has some, a soft low note none).
struct Note {
    from: f32,
    to: f32,
    length: f32,
    bell: f32,
}

fn notes(beep: Beep) -> &'static [Note] {
    match beep {
        Beep::Start => &[
            Note { from: 784.0, to: 784.0, length: 0.09, bell: 0.35 },
            Note { from: 1175.0, to: 1175.0, length: 0.16, bell: 0.35 },
        ],
        Beep::Stop => &[Note { from: 392.0, to: 262.0, length: 0.2, bell: 0.0 }],
    }
}

fn play_blocking(beep: Beep) -> anyhow::Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no audio output"))?;
    let supported = device.default_output_config()?;
    let rate = supported.sample_rate() as f32;
    let channels = supported.channels() as usize;
    let lead = (LEAD_IN * rate) as usize;
    let list = notes(beep);
    let total = lead + list.iter().map(|n| (n.length * rate) as usize).sum::<usize>();
    let pos = Arc::new(AtomicUsize::new(0));

    let sample = move |i: usize| -> f32 {
        if i < lead || i >= total {
            return 0.0;
        }
        // Find the note this sample belongs to and the time inside it.
        let mut t = (i - lead) as f32 / rate;
        for n in list {
            if t < n.length {
                let phase = TAU * (n.from * t + (n.to - n.from) * t * t / (2.0 * n.length));
                let tone = phase.sin() + n.bell * (2.0 * phase).sin();
                // Quick attack, then a gentle decay like a struck note; short fade-out against clicks.
                let envelope = (t / 0.006).min(1.0) * (-3.0 * t / n.length).exp() * ((n.length - t) / 0.008).min(1.0);
                return tone * VOLUME * envelope;
            }
            t -= n.length;
        }
        0.0
    };

    let config = supported.config();
    let on_error = |e: cpal::Error| crate::app::log::write(&format!("sound: {e}"));
    macro_rules! stream {
        ($t:ty, $convert:expr) => {{
            let p = pos.clone();
            device.build_output_stream(
                config,
                move |data: &mut [$t], _: &OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        let s = sample(p.fetch_add(1, Ordering::Relaxed));
                        frame.fill($convert(s));
                    }
                },
                on_error,
                None,
            )?
        }};
    }
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream!(f32, |s: f32| s),
        SampleFormat::I16 => stream!(i16, |s: f32| (s * i16::MAX as f32) as i16),
        SampleFormat::I32 => stream!(i32, |s: f32| (s as f64 * i32::MAX as f64) as i32),
        SampleFormat::U16 => stream!(u16, |s: f32| ((s * 0.5 + 0.5) * u16::MAX as f32) as u16),
        other => anyhow::bail!("unsupported output format {other:?}"),
    };
    stream.play()?;
    // Keep the stream alive until the beep has fully played out (plus device latency).
    std::thread::sleep(Duration::from_secs_f32(total as f32 / rate) + Duration::from_millis(350));
    Ok(())
}
