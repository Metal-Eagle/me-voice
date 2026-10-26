//! Screen context read once when a dictation starts: the foreground app and window
//! title, the text near the cursor (via UI Automation) and optionally the clipboard.
//! It feeds Whisper's initial prompt (rare names and terms) and the AI clean-up prompt.
//! Context is only held in memory for the dictation and is never stored or logged.

use std::collections::HashMap;

use crate::settings::ContextLevel;

/// Most text taken from the focused field (the end of it, nearest the cursor).
const MAX_TEXT_CHARS: usize = 4000;
/// Most clipboard text taken as context.
const MAX_CLIPBOARD_CHARS: usize = 2000;

/// Common English and Dutch words that are capitalised but are not names or terms.
const COMMON_WORDS: &[&str] = &[
    // English
    "the", "this", "that", "these", "those", "and", "but", "for", "with", "from", "into",
    "hello", "hi", "hey", "thanks", "thank", "please", "yes", "okay", "sure", "dear", "best",
    "regards", "kind", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday",
    "sunday", "january", "february", "march", "april", "june", "july", "august", "september",
    "october", "november", "december", "not", "you", "your", "our", "they", "their", "she",
    "her", "his", "him", "its", "what", "when", "where", "why", "how", "who", "which", "also",
    "just", "can", "could", "would", "should", "will", "may", "might", "let", "get", "good",
    "now", "here", "there", "then", "than", "some", "all", "any", "each", "every", "other",
    "about", "after", "before", "over", "under", "again", "are", "was", "were", "have", "has",
    // Dutch
    "de", "het", "een", "en", "of", "dat", "die", "dit", "deze", "ik", "je", "jij", "jullie",
    "wij", "zij", "hij", "ze", "er", "ook", "maar", "want", "dus", "ja", "nee", "hallo",
    "dank", "bedankt", "graag", "alstublieft", "goed", "goedemorgen", "doei", "met", "voor",
    "van", "naar", "uit", "bij", "aan", "als", "nog", "wel", "niet", "geen", "hoe", "wat",
    "waar", "wanneer", "waarom", "wie", "welke", "maandag", "dinsdag", "woensdag", "donderdag",
    "vrijdag", "zaterdag", "zondag", "januari", "februari", "maart", "mei", "juni", "juli",
    "augustus", "oktober", "november", "mijn", "onze", "hun", "zijn", "haar", "ons", "jouw",
    "uw", "heb", "hebben", "is", "zijn", "kan", "kun", "moet", "wordt", "worden",
];

/// What was read from the screen when the dictation started.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Program name of the foreground app, e.g. `ms-teams.exe`.
    pub app: Option<String>,
    /// Title of the foreground window.
    pub title: String,
    /// Selected text, or the text of the focused field (not password fields).
    pub text: String,
    /// Clipboard text, when the settings allow it.
    pub clipboard: String,
}

/// Reads the context the settings allow. Cheap; call it on a background thread at the start of a dictation.
pub fn capture(level: ContextLevel, clipboard: bool) -> Context {
    let mut context = Context::default();
    if level != ContextLevel::Off {
        context.app = crate::text::foreground::app_name();
        context.title = window_title();
    }
    if matches!(level, ContextLevel::Text) {
        context.text = focused_text();
    }
    if clipboard {
        context.clipboard = clipboard_text();
    }
    context
}

impl Context {
    /// Rare words and names for Whisper's prompt, most useful first, at most `max_chars` characters, comma-separated.
    pub fn keywords(&self, max_chars: usize) -> String {
        // (first spelling seen, count); the position in the vec is the first appearance.
        let mut entries: Vec<(String, usize)> = Vec::new();
        let mut position: HashMap<String, usize> = HashMap::new();

        for source in [&self.title, &self.text, &self.clipboard] {
            for (word, sentence_start) in words(source) {
                let qualifies = is_term(&word) || (!sentence_start && is_capitalised(&word));
                if !qualifies || is_common(&word) {
                    continue;
                }
                let key = word.to_lowercase();
                match position.get(&key) {
                    Some(&i) => entries[i].1 += 1,
                    None => {
                        position.insert(key, entries.len());
                        entries.push((word, 1));
                    }
                }
            }
        }

        // Stable sort: equal counts keep their first-appearance order.
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        let mut out = String::new();
        for (word, _) in entries {
            let separator = if out.is_empty() { 0 } else { 2 };
            if out.chars().count() + separator + word.chars().count() > max_chars {
                break;
            }
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&word);
        }
        out
    }

    /// Text for the AI prompt (app, title, text, clipboard sections), cut to `max_chars`. Empty when there is nothing.
    pub fn for_ai(&self, max_chars: usize) -> String {
        let mut out = String::new();
        let sections = [
            ("App", self.app.clone().unwrap_or_default()),
            ("Window", self.title.clone()),
            ("Text near the cursor", self.text.clone()),
            ("Clipboard", self.clipboard.clone()),
        ];
        for (label, value) in sections {
            if value.trim().is_empty() {
                continue;
            }
            out.push_str(label);
            out.push_str(": ");
            out.push_str(value.trim());
            out.push('\n');
        }
        cut_start(out.trim_end(), max_chars)
    }
}

/// Words of `text` with a flag for whether each one starts a sentence (or a line).
/// Emails, addresses, URLs and file names are dropped.
fn words(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut sentence_start = true;
        for raw in line.split_whitespace() {
            let start = sentence_start;
            sentence_start = raw.ends_with(['.', '!', '?']);
            if raw.contains(['@', '/', '\\', ':']) {
                continue;
            }
            let mut word = raw.trim_matches(|c: char| !c.is_alphanumeric());
            if let Some(stripped) = word.strip_suffix("'s").or_else(|| word.strip_suffix("\u{2019}s")) {
                word = stripped;
            }
            let valid = !word.is_empty()
                && word.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if valid {
                out.push((word.to_string(), start));
            }
        }
    }
    out
}

/// CamelCase, ALLCAPS of two or more letters, or a mix of letters and digits.
fn is_term(word: &str) -> bool {
    if word.chars().count() < 3 {
        return false;
    }
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    let has_digit = word.chars().any(|c| c.is_ascii_digit());
    let camel = word.chars().skip(1).any(|c| c.is_uppercase()) && word.chars().any(|c| c.is_lowercase());
    let caps = letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase());
    (has_digit && !letters.is_empty()) || camel || caps
}

/// Starts with a capital letter (the caller checks it is not at a sentence start).
fn is_capitalised(word: &str) -> bool {
    word.chars().next().is_some_and(|c| c.is_uppercase()) && word.chars().count() >= 3
}

fn is_common(word: &str) -> bool {
    let lower = word.to_lowercase();
    word.chars().count() < 3 || COMMON_WORDS.contains(&lower.as_str())
}

/// Keeps the first `max` characters, on a character boundary.
fn cut_start(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Keeps the last `max` characters, on a character boundary.
fn cut_end(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        text.chars().skip(count - max).collect()
    }
}

fn clipboard_text() -> String {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(text) => cut_start(&text, MAX_CLIPBOARD_CHARS),
        Err(_) => String::new(),
    }
}

#[cfg(windows)]
fn window_title() -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(window, buf.as_mut_ptr(), buf.len() as i32);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

#[cfg(not(windows))]
fn window_title() -> String {
    String::new()
}

/// Text of the focused element, or empty if it cannot be read. Never panics.
#[cfg(windows)]
fn focused_text() -> String {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    unsafe {
        // Only uninitialise if this call did the initialising (S_OK or S_FALSE both count as success).
        let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let text = std::panic::catch_unwind(|| read_focused()).ok().flatten().unwrap_or_default();
        if initialised {
            CoUninitialize();
        }
        text
    }
}

#[cfg(not(windows))]
fn focused_text() -> String {
    String::new()
}

/// Reads the focused element through UI Automation. Must run on a COM-initialised thread.
#[cfg(windows)]
unsafe fn read_focused() -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern,
        UIA_TextPatternId, UIA_ValuePatternId,
    };

    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None::<&windows::core::IUnknown>, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        if element.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }

        let mut text = String::new();
        if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
            // Selected text first.
            if let Ok(selection) = pattern.GetSelection() {
                if selection.Length().unwrap_or(0) > 0 {
                    if let Ok(range) = selection.GetElement(0) {
                        if let Ok(selected) = range.GetText(-1) {
                            text = selected.to_string();
                        }
                    }
                }
            }
            // Otherwise the whole document, keeping its end (nearest the cursor).
            if text.trim().is_empty() {
                if let Ok(range) = pattern.DocumentRange() {
                    if let Ok(all) = range.GetText(-1) {
                        text = cut_end(&all.to_string(), MAX_TEXT_CHARS);
                    }
                }
            }
        }
        // Fields without a text pattern (or an empty one) may still expose their value.
        if text.trim().is_empty() {
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                if let Ok(value) = pattern.CurrentValue() {
                    text = cut_end(&value.to_string(), MAX_TEXT_CHARS);
                }
            }
        }
        Some(cut_end(&text, MAX_TEXT_CHARS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(title: &str, text: &str, clipboard: &str) -> Context {
        Context {
            app: None,
            title: title.into(),
            text: text.into(),
            clipboard: clipboard.into(),
        }
    }

    #[test]
    fn keywords_pick_names_not_sentence_starts() {
        // "Hello" and "Monday" start sentences, so they do not count; the rest are mid-sentence.
        let c = ctx("", "Hello Jeroen. We met Sanne Bakker today. Monday works.", "");
        assert_eq!(c.keywords(200), "Jeroen, Sanne, Bakker");
    }

    #[test]
    fn keywords_include_camel_acronym_and_digits() {
        let c = ctx("", "We use GitHub and the API, plus GPT4 and the iPhone.", "");
        let k = c.keywords(200);
        assert!(k.contains("GitHub"));
        assert!(k.contains("API"));
        assert!(k.contains("GPT4"));
        assert!(k.contains("iPhone"));
    }

    #[test]
    fn keywords_skip_common_short_and_addresses() {
        let c = ctx("", "Mail jan@example.com or see www.Example.org. De Het Een Dank Hallo Ok.", "");
        let k = c.keywords(200);
        assert!(!k.contains("jan"));
        assert!(!k.contains("example"));
        assert!(!k.contains("De"));
        assert!(!k.contains("Het"));
        assert!(!k.contains("Ok"));
    }

    #[test]
    fn keywords_dedupe_case_insensitively_and_rank_by_count() {
        // Sanne appears twice, Bram once: Sanne ranks first, and "sanne" is not capitalised.
        let c = ctx("", "Ask Sanne. Then tell Sanne again, and Bram too. sanne? Bram.", "");
        assert_eq!(c.keywords(200), "Sanne, Bram");
    }

    #[test]
    fn keywords_respect_max_chars() {
        let c = ctx("", "see Alpha Bravo Charlie Delta Echo Foxtrot Golf", "");
        let k = c.keywords(12);
        assert!(k.chars().count() <= 12);
        assert_eq!(k, "Alpha, Bravo");
        assert_eq!(c.keywords(0), "");
    }

    #[test]
    fn keywords_cut_on_char_boundaries() {
        let c = ctx("", "Zoë Ångström Ünal", "");
        let k = c.keywords(9);
        assert!(k.chars().count() <= 9);
        let _ = c.keywords(1);
    }

    #[test]
    fn for_ai_empty_when_nothing() {
        assert_eq!(Context::default().for_ai(500), "");
    }

    #[test]
    fn for_ai_sections_and_limit() {
        let c = Context {
            app: Some("notepad.exe".into()),
            title: "Notes".into(),
            text: "Some text".into(),
            clipboard: "clip".into(),
        };
        let all = c.for_ai(500);
        assert!(all.starts_with("App: notepad.exe\nWindow: Notes\n"));
        assert!(all.contains("Text near the cursor: Some text"));
        assert!(all.ends_with("Clipboard: clip"));
        assert_eq!(c.for_ai(10).chars().count(), 10);
    }

    #[test]
    fn cut_end_keeps_tail() {
        assert_eq!(cut_end("abcdef", 3), "def");
        assert_eq!(cut_end("ééé", 2), "éé");
    }
}
