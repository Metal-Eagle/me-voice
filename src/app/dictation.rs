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
    let main = models::find(&settings.model_id).ok_or_else(|| anyhow!("unknown model {}", settings.model_id))?;
    // The live preview uses the small fast model when it's downloaded; the final text
    // always comes from the main (accurate) model.
    let model = if light {
        // Only when it knows the chosen language: an English-only preview would show
        // English nonsense while you speak Dutch.
        let speaks = |m: &models::Model| settings.language == "auto" && m.languages.iter().any(|l| l == "auto")
            || m.languages.iter().any(|l| *l == settings.language);
        models::find(&settings.preview_model_id).filter(|m| m.is_downloaded() && speaks(m)).unwrap_or(main)
    } else {
        main
    };
    // Small files, read fresh each time so edits apply without a restart.
    let dict = cleanup::load_dictionary();
    let snippets = cleanup::load_snippets();
    let commands = cleanup::load_commands();
    let words = dict.prompt();
    let prompt = [hints, words.as_str()].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(", ");
    // English-only models always transcribe as English, whatever the language setting says.
    let language = if model.languages.iter().all(|l| l.as_str() == "en") { "en" } else { settings.language.as_str() };
    let raw = transcriber.lock().unwrap().transcribe(
        samples,
        &Request { engine: model.engine, model_path: &model.path(), language, acft: model.acft, prompt: &prompt, light, abort },
    )?;
    Ok(cleanup::clean(&raw, settings, &dict, &snippets, &commands))
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
    samples: &[f32],
    command_mode: bool,
    context: Option<std::sync::mpsc::Receiver<Context>>,
    warm: crate::text::polish::Warm,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<Dictation> {
    // Copied now, after the shortcut is let go, so the Ctrl+C isn't mixed with held keys.
    let selection = if command_mode { insert::copy_selection()? } else { String::new() };
    let context = context.and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok()).unwrap_or_default();
    let text = transcribe_with(settings, transcriber, samples, false, None, &context.keywords(HINT_CHARS))?;
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
