//! One dictation from recording to text, on the worker thread: transcribe with the
//! screen context, split off a voice command and let the AI polish the result.

use super::*;

/// A finished dictation from the worker thread.
pub(super) struct Dictation {
    pub(super) text: String,
    /// Voice command said at the end, already cut from `text`.
    pub(super) command: Option<cleanup::VoiceCommand>,
}

pub fn transcribe(
    settings: &Settings,
    transcriber: &Mutex<Transcriber>,
    samples: &[f32],
    light: bool,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<String> {
    transcribe_with(settings, transcriber, samples, light, abort, "")
}

/// The model for a run: the small preview model for live text (when it's downloaded and
/// knows the language), the main (accurate) model otherwise.
pub(super) fn pick_model(settings: &Settings, light: bool) -> Result<models::Model> {
    let main = models::find(&settings.model_id).ok_or_else(|| anyhow!("unknown model {}", settings.model_id))?;
    if !light {
        return Ok(main);
    }
    // Only when it knows the chosen language: an English-only preview would show
    // English nonsense while you speak Dutch.
    let speaks = |m: &models::Model| settings.language == "auto" && m.languages.iter().any(|l| l == "auto")
        || m.languages.iter().any(|l| *l == settings.language);
    Ok(models::find(&settings.preview_model_id).filter(|m| m.is_downloaded() && speaks(m)).unwrap_or(main))
}

/// Like `transcribe`, with `hints` (names from the screen) put before the dictionary
/// words in the model's prompt.
pub(super) fn transcribe_with(
    settings: &Settings,
    transcriber: &Mutex<Transcriber>,
    samples: &[f32],
    light: bool,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
    hints: &str,
) -> Result<String> {
    let raw = raw_text(settings, transcriber, samples, light, abort, hints)?;
    Ok(clean_text(&raw, settings))
}

/// Applies the cleanup (fillers, formatting, snippets, commands) to raw model output.
pub(super) fn clean_text(raw: &str, settings: &Settings) -> String {
    // Small files, read fresh each time so edits apply without a restart.
    cleanup::clean(raw, settings, &cleanup::load_dictionary(), &cleanup::load_snippets(), &cleanup::load_commands())
}

/// The model's text before cleanup. `hints` go before the dictionary words in the prompt.
pub(super) fn raw_text(
    settings: &Settings,
    transcriber: &Mutex<Transcriber>,
    samples: &[f32],
    light: bool,
    abort: Option<Arc<std::sync::atomic::AtomicBool>>,
    hints: &str,
) -> Result<String> {
    let model = pick_model(settings, light)?;
    let words = cleanup::load_dictionary().prompt();
    let prompt = join_prompt(&[hints, &words]);
    // English-only models always transcribe as English, whatever the language setting says.
    let language = if model.languages.iter().all(|l| l.as_str() == "en") { "en" } else { settings.language.as_str() };
    let waiting = Instant::now();
    let mut t = transcriber.lock().unwrap();
    let waited = waiting.elapsed();
    if !light && waited > Duration::from_millis(50) {
        log::write(&format!("waited {:.2} s for the speech engine", waited.as_secs_f32()));
    }
    t.transcribe(
        samples,
        &Request {
            engine: model.engine,
            model_path: &model.path(),
            language,
            acft: model.acft,
            prompt: &prompt,
            light,
            abort,
            threads: settings.threads,
        },
    )
}

/// Joins the non-empty prompt parts with ", ".
pub(super) fn join_prompt(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(", ")
}

/// The last `n` characters of `text`.
pub(super) fn tail_chars(text: &str, n: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(n)).collect()
}

/// Most characters of screen context: in Whisper's prompt (it only reads about 224 tokens),
/// and in the AI prompt.
pub(super) const HINT_CHARS: usize = 200;
pub(super) const AI_CONTEXT_CHARS: usize = 3000;
/// Shortest dictation worth sending to the AI.
pub(super) const MIN_POLISH_WORDS: u32 = 4;

/// Starts reading the screen context on its own thread, when the settings ask for it.
pub(super) fn start_context(settings: &Settings) -> Option<std::sync::mpsc::Receiver<Context>> {
    if settings.context == ContextLevel::Off && !settings.context_clipboard {
        return None;
    }
    let (level, clipboard) = (settings.context, settings.context_clipboard);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(crate::text::context::capture(level, clipboard));
    });
    Some(rx)
}

/// The final pass of a dictation, on the worker thread: transcribe with the screen
/// context, split off a voice command and let the AI polish the text. In Command Mode
/// the spoken instruction is applied to the selected text instead.
pub(super) fn finish(
    settings: &Settings,
    transcriber: &Mutex<Transcriber>,
    pieces: &[String],
    tail: &[f32],
    command_mode: bool,
    context: Option<std::sync::mpsc::Receiver<Context>>,
    warm: crate::text::polish::Warm,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<Dictation> {
    // Copied now, after the shortcut is let go, so the Ctrl+C isn't mixed with held keys.
    let selection = if command_mode { insert::copy_selection()? } else { String::new() };
    let waiting = Instant::now();
    let context = context.and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok()).unwrap_or_default();
    if waiting.elapsed() > Duration::from_millis(50) {
        log::write(&format!("waited {:.2} s for the screen context", waiting.elapsed().as_secs_f32()));
    }
    // The pieces done while talking, then the rest (with the text before it as a prompt).
    let mut raw: Vec<String> = pieces.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    if !tail.is_empty() {
        let before = tail_chars(&raw.join(" "), CHUNK_PROMPT_CHARS);
        let prompt = join_prompt(&[&context.keywords(HINT_CHARS), &before]);
        let started = Instant::now();
        let last = raw_text(settings, transcriber, tail, false, None, &prompt)?;
        log::write(&format!(
            "transcribed the last {:.1} s in {:.2} s",
            tail.len() as f32 / SAMPLE_RATE as f32,
            started.elapsed().as_secs_f32()
        ));
        raw.push(last.trim().to_string());
    }
    let text = clean_text(&raw.join(" "), settings);
    let to_claude = settings.ai_provider == AiProvider::ClaudeCli;
    let ai_context = if to_claude && !settings.context_to_claude { String::new() } else { context.for_ai(AI_CONTEXT_CHARS) };

    if command_mode {
        if text.trim().is_empty() {
            return Ok(Dictation { text, command: None });
        }
        let _ = proxy.send_event(UserEvent::Polishing);
        let answer = crate::text::polish::command(settings, &selection, &text, &ai_context, warm)?;
        return Ok(Dictation { text: answer, command: None });
    }

    let (command, mut text) = match cleanup::voice_command(&text, settings) {
        Some((command, rest)) => (Some(command), rest),
        None => (None, text),
    };
    if wants_polish(settings, &text, command) {
        let _ = proxy.send_event(UserEvent::Polishing);
        let started = Instant::now();
        match crate::text::polish::polish(settings, &text, &settings.ai_style, &ai_context, warm) {
            Ok(polished) => {
                log::write(&format!("polished in {:.2} s", started.elapsed().as_secs_f32()));
                text = polished;
            }
            // Never lose a dictation: paste it as it is.
            Err(e) => log::write(&format!("polish: {e}")),
        }
    }
    Ok(Dictation { text, command })
}

/// Whether this dictation goes to the AI: long enough, not "scratch that" and not a snippet.
pub(super) fn wants_polish(settings: &Settings, text: &str, command: Option<cleanup::VoiceCommand>) -> bool {
    settings.ai_provider != AiProvider::Off
        && settings.ai_polish
        // Low-power mode: a local AI model is the slowest step on a weak computer.
        && !settings.low_power
        && command != Some(cleanup::VoiceCommand::Scratch)
        && crate::text::history::count_words(text) >= MIN_POLISH_WORDS
        && !cleanup::load_snippets().values().any(|v| v == text)
}

/// Drops one full stop at the end ("done." becomes "done"); an ellipsis ("...") stays.
pub(super) fn drop_final_period(text: &str) -> String {
    match text.trim_end().strip_suffix('.') {
        Some(rest) if !rest.ends_with('.') => rest.to_string(),
        _ => text.to_string(),
    }
}
