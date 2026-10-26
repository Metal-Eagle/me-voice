//! Rule-based clean-up between transcription and pasting: personal dictionary,
//! snippets, filler-word removal and spoken formatting. Runs fully offline.

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::settings::{config_dir, Settings};

/// `dictionary.json`: words to spell exactly like this, plus free-form replacements.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Dictionary {
    pub words: Vec<String>,
    pub replacements: BTreeMap<String, String>,
}

/// `snippets.json`: spoken trigger phrase -> text to insert.
pub type Snippets = BTreeMap<String, String>;

/// `commands.json`: spoken phrase -> text to insert wherever the phrase is said.
pub type Commands = BTreeMap<String, String>;

fn load_json<T: Default + for<'de> Deserialize<'de> + Serialize>(name: &str) -> T {
    let path = config_dir().join(name);
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => {
            // Create an empty file so it's easy to find and edit.
            let value = T::default();
            let _ = fs::create_dir_all(config_dir());
            let _ = fs::write(&path, serde_json::to_string_pretty(&value).unwrap_or_default());
            value
        }
    }
}

pub fn load_dictionary() -> Dictionary {
    load_json("dictionary.json")
}

/// Writes dictionary.json (used by the fix window and the settings page).
pub fn save_dictionary(dict: &Dictionary) -> anyhow::Result<()> {
    fs::create_dir_all(config_dir())?;
    fs::write(config_dir().join("dictionary.json"), serde_json::to_string_pretty(dict)?)?;
    Ok(())
}

pub fn load_snippets() -> Snippets {
    load_json("snippets.json")
}

pub fn load_commands() -> Commands {
    load_json("commands.json")
}

impl Dictionary {
    /// Hint for Whisper so names and jargon are recognised.
    pub fn prompt(&self) -> String {
        self.words.join(", ")
    }
}

pub fn clean(text: &str, settings: &Settings, dict: &Dictionary, snippets: &Snippets, commands: &Commands) -> String {
    let lang = settings.language.as_str();
    let mut out = remove_sound_labels(text).trim().to_string();

    // A whole dictation that matches a snippet trigger is replaced by the snippet.
    let bare = normalise(&out);
    if let Some((_, expansion)) = snippets.iter().find(|(k, _)| normalise(k) == bare) {
        return expansion.clone();
    }

    if settings.remove_fillers {
        out = remove_fillers(&out, lang);
    }
    if settings.spoken_formatting {
        out = spoken_formatting(&out, lang);
    }
    for (phrase, text) in commands {
        out = replace_words(&out, phrase, text);
    }
    for (from, to) in &dict.replacements {
        out = replace_words(&out, from, to);
    }
    for word in &dict.words {
        out = replace_words(&out, word, word);
    }
    for (trigger, expansion) in snippets {
        out = replace_words(&out, trigger, expansion);
    }
    tidy(&out)
}

/// What a voice command at the end of a dictation asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCommand {
    /// Stop listening and paste.
    Stop,
    /// Stop listening, paste and press Enter.
    Send,
    /// "Scratch that" as the whole dictation: undo the previous paste, paste nothing.
    Scratch,
}

/// The phrase that undoes the previous paste, in the dictation language.
pub fn scratch_phrase(lang: &str) -> &'static str {
    match lang {
        "nl" => "weg ermee",
        _ => "scratch that",
    }
}

/// If the text ends with a stop or send word (as whole words, any punctuation around it),
/// returns the command and the text without it.
pub fn voice_command(text: &str, settings: &Settings) -> Option<(VoiceCommand, String)> {
    if !settings.voice_commands {
        return None;
    }
    if normalise(text) == scratch_phrase(&settings.language) {
        return Some((VoiceCommand::Scratch, String::new()));
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let candidates = [(VoiceCommand::Send, &settings.send_words), (VoiceCommand::Stop, &settings.stop_words)];
    for (command, list) in candidates {
        for phrase in list.split(',').map(normalise).filter(|p| !p.is_empty()) {
            let n = phrase.split(' ').count();
            if words.len() >= n && normalise(&words[words.len() - n..].join(" ")) == phrase {
                let rest = words[..words.len() - n].join(" ");
                let rest = rest.trim_end_matches([',', ';', ':', ' ']).to_string();
                return Some((command, rest));
            }
        }
    }
    None
}

/// If the spoken text ends with "new line" / "new paragraph" (or the Dutch ones), returns
/// the text before it. Used by the paragraph window to lock in a line as you say it.
pub fn strip_line_break(text: &str, lang: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (phrase, _) in spoken_commands(lang).iter().filter(|(_, out)| out.starts_with('\n') && !out.contains('-')) {
        let n = phrase.split(' ').count();
        if words.len() >= n && normalise(&words[words.len() - n..].join(" ")) == *phrase {
            let rest = words[..words.len() - n].join(" ");
            return Some(rest.trim_end_matches([',', ';', ':', ' ']).to_string());
        }
    }
    None
}

/// Adds `line` under `text` on a new line. Empty lines change nothing.
pub fn append_line(text: &str, line: &str) -> String {
    let line = line.trim();
    if line.is_empty() {
        return text.to_string();
    }
    if text.trim().is_empty() {
        return line.to_string();
    }
    format!("{}\n{line}", text.trim_end())
}

/// Whisper labels noises instead of words: "[Click]", "[BLANK_AUDIO]", "(music)", "*sighs*".
/// Square brackets and asterisks are always labels; round brackets only for known noise words.
fn remove_sound_labels(text: &str) -> String {
    const NOISES: &[&str] = &[
        "music", "silence", "applause", "laughter", "laughs", "laughing", "noise", "click", "clicking",
        "beep", "inaudible", "coughs", "coughing", "sighs", "breathing", "background noise", "blank_audio",
    ];
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let close = match c {
            '[' => Some(']'),
            '*' => Some('*'),
            '(' => Some(')'),
            _ => None,
        };
        if let Some(close) = close {
            if let Some(end) = text[i + 1..].find(close) {
                let inner = text[i + 1..i + 1 + end].trim().to_lowercase();
                if c != '(' || NOISES.contains(&inner.as_str()) {
                    // Skip the label, including its closing character.
                    let stop = i + 1 + end;
                    while chars.peek().is_some_and(|&(j, _)| j <= stop) {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lowercase, without punctuation, for comparing whole phrases.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fillers(lang: &str) -> &'static [&'static str] {
    match lang {
        "nl" => &["eh", "ehm", "uh", "uhm", "euh"],
        _ => &["um", "uh", "uhm", "umm", "er", "erm", "ah", "hmm", "mm"],
    }
}

fn remove_fillers(text: &str, lang: &str) -> String {
    let fillers = fillers(lang);
    let mut kept: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let core = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if fillers.contains(&core.as_str()) {
            if let Some(prev) = kept.last_mut() {
                // "Thanks, uh, for" -> "Thanks for", but keep sentence punctuation
                // that was attached to the filler ("fine, um." -> "fine.").
                let trimmed = prev.trim_end_matches(',').to_string();
                match word.chars().last().filter(|c| ".?!".contains(*c)) {
                    Some(end) if !trimmed.ends_with(['.', '?', '!']) => *prev = format!("{trimmed}{end}"),
                    _ => *prev = trimmed,
                }
            }
            continue;
        }
        // Drop immediate repeats like "the the".
        if let Some(prev) = kept.last() {
            if prev.to_lowercase() == word.to_lowercase() && word.chars().all(char::is_alphabetic) {
                continue;
            }
        }
        kept.push(word.to_string());
    }
    kept.join(" ")
}

fn spoken_commands(lang: &str) -> &'static [(&'static str, &'static str)] {
    match lang {
        // "dubbele punt" must come before "punt", or it would become "dubbele .".
        "nl" => &[
            ("nieuwe alinea", "\n\n"),
            ("nieuwe regel", "\n"),
            ("opsommingsteken", "\n-"),
            ("dubbele punt", ":"),
            ("punt", "."),
            ("komma", ","),
            ("vraagteken", "?"),
            ("uitroepteken", "!"),
        ],
        _ => &[
            ("new paragraph", "\n\n"),
            ("new line", "\n"),
            ("bullet point", "\n-"),
            ("full stop", "."),
            ("period", "."),
            ("comma", ","),
            ("question mark", "?"),
            ("exclamation mark", "!"),
            ("exclamation point", "!"),
            ("colon", ":"),
            ("semicolon", ";"),
        ],
    }
}

/// Phrases that wrap the next text in quotes; they don't touch the word before them.
fn spoken_quotes(lang: &str) -> &'static [(&'static str, &'static str)] {
    match lang {
        "nl" => &[("aanhalingsteken openen", "“"), ("aanhalingsteken sluiten", "”")],
        _ => &[("open quote", "“"), ("close quote", "”")],
    }
}

/// Phrase that capitalises the next word.
fn caps_phrase(lang: &str) -> &'static str {
    match lang {
        "nl" => "hoofdletters",
        _ => "all caps",
    }
}

fn spoken_formatting(text: &str, lang: &str) -> String {
    let mut out = text.to_string();
    for (phrase, symbol) in spoken_commands(lang) {
        out = replace_phrase_with_symbol(&out, phrase, symbol);
    }
    for (phrase, symbol) in spoken_quotes(lang) {
        out = replace_phrase_plain(&out, phrase, symbol);
    }
    upper_next_word(&out, caps_phrase(lang))
}

/// Replaces a spoken phrase (with any punctuation Whisper put around it) by a symbol.
fn replace_phrase_with_symbol(text: &str, phrase: &str, symbol: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let n = phrase.split(' ').count();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if i + n <= words.len() && normalise(&words[i..i + n].join(" ")) == phrase {
            let prev = out.pop().unwrap_or_default();
            // A line break keeps the sentence's own punctuation; a spoken symbol replaces it.
            let prev = if symbol.starts_with('\n') {
                prev.trim_end_matches(',').to_string()
            } else {
                prev.trim_end_matches([',', '.', ';', ':']).to_string()
            };
            out.push(format!("{prev}{symbol}"));
            i += n;
            continue;
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

/// Replaces a spoken phrase by a symbol, leaving the words around it as they are.
/// `tidy` attaches the symbol to the neighbouring word.
fn replace_phrase_plain(text: &str, phrase: &str, symbol: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let n = phrase.split(' ').count();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if i + n <= words.len() && normalise(&words[i..i + n].join(" ")) == phrase {
            out.push(symbol.to_string());
            i += n;
            continue;
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

/// "all caps word" -> "WORD": the phrase is dropped and the next word is upper-cased.
fn upper_next_word(text: &str, phrase: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let n = phrase.split(' ').count();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if i + n < words.len() && normalise(&words[i..i + n].join(" ")) == phrase {
            out.push(words[i + n].to_uppercase());
            i += n + 1;
            continue;
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

/// Case-insensitive whole-word replacement.
fn replace_words(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = from.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut start = 0;
    while let Some(pos) = lower[start..].find(&needle) {
        let begin = start + pos;
        let end = begin + needle.len();
        let before_ok = lower[..begin].chars().last().map_or(true, |c| !c.is_alphanumeric());
        let after_ok = lower[end..].chars().next().map_or(true, |c| !c.is_alphanumeric());
        if before_ok && after_ok && text.is_char_boundary(begin) && text.is_char_boundary(end) {
            out.push_str(&text[last..begin]);
            out.push_str(to);
            last = end;
        }
        start = end;
    }
    out.push_str(&text[last..]);
    out
}

/// Fixes spacing around punctuation and line breaks, and capitalises sentences.
fn tidy(text: &str) -> String {
    let mut s = text.to_string();
    for p in [" .", " ,", " ?", " !", " :", " ;"] {
        s = s.replace(p, &p[1..]);
    }
    s = s.replace(" \n", "\n").replace("\n ", "\n");
    // Quotes hug the words inside them.
    s = s.replace("“ ", "“").replace(" ”", "”");
    let mut out = String::with_capacity(s.len());
    let mut capitalise = true;
    for c in s.chars() {
        if capitalise && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capitalise = false;
        } else {
            out.push(c);
        }
        if ".?!\n".contains(c) {
            capitalise = true;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn removes_fillers_and_repeats() {
        let out = clean("Um, I think the the plan is, uh, fine.", &settings(), &Dictionary::default(), &Snippets::new(), &Commands::new());
        assert_eq!(out, "I think the plan is fine.");
    }

    #[test]
    fn spoken_formatting_works() {
        let out = clean("Hello there, new line. How are you question mark", &settings(), &Dictionary::default(), &Snippets::new(), &Commands::new());
        assert_eq!(out, "Hello there\nHow are you?");
    }

    #[test]
    fn strips_a_spoken_line_break_at_the_end() {
        assert_eq!(strip_line_break("Hello there, new line", "en").as_deref(), Some("Hello there"));
        assert_eq!(strip_line_break("Hello there new line.", "en").as_deref(), Some("Hello there"));
        assert_eq!(strip_line_break("Hallo daar nieuwe regel", "nl").as_deref(), Some("Hallo daar"));
        assert_eq!(strip_line_break("new line in the middle of it", "en"), None);
        assert_eq!(strip_line_break("bullet point", "en"), None);
    }

    #[test]
    fn appends_lines_under_each_other() {
        assert_eq!(append_line("", " Hello. "), "Hello.");
        assert_eq!(append_line("Hello.\n", "How are you?"), "Hello.\nHow are you?");
        assert_eq!(append_line("Hello.", "  "), "Hello.");
    }

    #[test]
    fn dictionary_fixes_spelling() {
        let dict = Dictionary { words: vec!["Kubernetes".into()], replacements: BTreeMap::new() };
        let out = clean("open kubernetes please", &settings(), &dict, &Snippets::new(), &Commands::new());
        assert_eq!(out, "Open Kubernetes please");
    }

    #[test]
    fn removes_sound_labels() {
        let out = clean("[Click] Hello (music) there (yes) *sighs* [BLANK_AUDIO]", &settings(), &Dictionary::default(), &Snippets::new(), &Commands::new());
        assert_eq!(out, "Hello there (yes)");
    }

    #[test]
    fn voice_commands_at_the_end_only() {
        let s = settings();
        assert_eq!(voice_command("See you tomorrow, send.", &s), Some((VoiceCommand::Send, "See you tomorrow".into())));
        assert_eq!(voice_command("That is all. Stop!", &s), Some((VoiceCommand::Stop, "That is all.".into())));
        assert_eq!(voice_command("Please send the report", &s), None);
        assert_eq!(voice_command("Enter", &s), Some((VoiceCommand::Send, String::new())));
    }

    #[test]
    fn whole_snippet_expands() {
        let mut snippets = Snippets::new();
        snippets.insert("my sign off".into(), "Kind regards,\nAlex".into());
        let out = clean("My sign off.", &settings(), &Dictionary::default(), &snippets, &Commands::new());
        assert_eq!(out, "Kind regards,\nAlex");
    }

    fn dutch() -> Settings {
        Settings { language: "nl".into(), ..Settings::default() }
    }

    fn run(text: &str, s: &Settings) -> String {
        clean(text, s, &Dictionary::default(), &Snippets::new(), &Commands::new())
    }

    #[test]
    fn bullet_points() {
        assert_eq!(run("Groceries bullet point milk bullet point eggs", &settings()), "Groceries\n- Milk\n- Eggs");
        assert_eq!(run("Boodschappen opsommingsteken melk opsommingsteken eieren", &dutch()), "Boodschappen\n- Melk\n- Eieren");
    }

    #[test]
    fn quotes_hug_their_words() {
        assert_eq!(run("He said open quote hello close quote today", &settings()), "He said “hello” today");
        assert_eq!(run("Hij zei aanhalingsteken openen hoi aanhalingsteken sluiten", &dutch()), "Hij zei “hoi”");
    }

    #[test]
    fn all_caps_next_word() {
        assert_eq!(run("The name is all caps kubernetes", &settings()), "The name is KUBERNETES");
        assert_eq!(run("Dit is hoofdletters nl", &dutch()), "Dit is NL");
    }

    #[test]
    fn dutch_punctuation_commands() {
        assert_eq!(run("Let op dubbele punt dit is belangrijk", &dutch()), "Let op: dit is belangrijk");
        assert_eq!(run("Hallo komma wereld vraagteken", &dutch()), "Hallo, wereld?");
        assert_eq!(run("Hallo nieuwe regel dag", &dutch()), "Hallo\nDag");
    }

    #[test]
    fn scratch_that_is_a_whole_dictation_command() {
        let s = settings();
        assert_eq!(voice_command("Scratch that.", &s), Some((VoiceCommand::Scratch, String::new())));
        assert_eq!(voice_command("Weg ermee!", &dutch()), Some((VoiceCommand::Scratch, String::new())));
        assert_eq!(voice_command("Hello, scratch that", &s), None);
        let off = Settings { voice_commands: false, ..Settings::default() };
        assert_eq!(voice_command("Scratch that", &off), None);
    }

    #[test]
    fn user_commands_apply_anywhere() {
        let mut commands = Commands::new();
        commands.insert("my signature".into(), "Best,\nAlex".into());
        let out = clean("Thanks, My Signature", &settings(), &Dictionary::default(), &Snippets::new(), &commands);
        assert_eq!(out, "Thanks, Best,\nAlex");
    }

    #[test]
    fn commands_json_allows_newlines() {
        let commands: Commands = serde_json::from_str(r#"{"my address": "Main Street 1\nAmsterdam"}"#).unwrap();
        assert_eq!(commands["my address"], "Main Street 1\nAmsterdam");
    }
}
