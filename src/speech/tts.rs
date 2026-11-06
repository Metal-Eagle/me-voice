//! Text to speech with Kokoro-82M (fp32 ONNX), fully on this PC.
//!
//! Text → phonemes with misaki's English dictionaries (`us_gold.json`, `us_silver.json`), phonemes →
//! token ids with Kokoro's vocabulary, then the model turns ids + a voice style (Heart, `af_heart`)
//! into 24 kHz audio. Files live in `%LOCALAPPDATA%\me-voice\kokoro`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ort::session::Session;
use ort::value::Tensor;

pub const SAMPLE_RATE: u32 = 24_000;
pub const DEFAULT_VOICE: &str = "af_heart";
const STYLE_DIM: usize = 256;
/// The model takes at most 512 tokens, two of them the start/end pad.
const MAX_PHONEMES: usize = 510;

/// The Kokoro folder: `%LOCALAPPDATA%\me-voice\kokoro`.
pub fn dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("me-voice").join("kokoro")
}

/// Whether the model, voice and dictionaries are all there.
pub fn installed() -> bool {
    let d = dir();
    ["model.onnx", "us_gold.json", "us_silver.json"].iter().all(|f| d.join(f).exists())
        && d.join(format!("{DEFAULT_VOICE}.bin")).exists()
}

pub struct Kokoro {
    session: Session,
    /// Style vectors, one per phoneme count: `[510][256]`.
    voice: Vec<f32>,
    lexicon: Lexicon,
}

impl Kokoro {
    pub fn load(dir: &Path, voice: &str) -> Result<Self> {
        let session = Session::builder()?
            .with_intra_threads(4)
            .map_err(|e| anyhow!("{e}"))?
            .commit_from_file(dir.join("model.onnx"))
            .context("loading Kokoro model")?;
        let bytes = std::fs::read(dir.join(format!("{voice}.bin"))).with_context(|| format!("voice {voice}"))?;
        let voice = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        Ok(Self { session, voice, lexicon: Lexicon::load(dir)? })
    }

    /// Speaks `text` and returns 24 kHz mono samples. Long text is cut into sentences.
    pub fn synthesize(&mut self, text: &str, speed: f32) -> Result<Vec<f32>> {
        let mut out = Vec::new();
        for sentence in split_sentences(text) {
            let phonemes = self.lexicon.phonemize(&sentence);
            if phonemes.trim().is_empty() {
                continue;
            }
            out.extend(self.synthesize_phonemes(&phonemes, speed)?);
        }
        Ok(out)
    }

    fn synthesize_phonemes(&mut self, phonemes: &str, speed: f32) -> Result<Vec<f32>> {
        let mut ids: Vec<i64> = phonemes.chars().filter_map(token_id).collect();
        ids.truncate(MAX_PHONEMES);
        let count = ids.len();
        let rows = self.voice.len() / STYLE_DIM;
        let row = count.min(rows - 1);
        let style = self.voice[row * STYLE_DIM..(row + 1) * STYLE_DIM].to_vec();

        let mut tokens = Vec::with_capacity(count + 2);
        tokens.push(0);
        tokens.extend(ids);
        tokens.push(0);
        let len = tokens.len();
        let outputs = self.session.run(ort::inputs![
            "input_ids" => Tensor::from_array((vec![1i64, len as i64], tokens))?,
            "style" => Tensor::from_array((vec![1i64, STYLE_DIM as i64], style))?,
            "speed" => Tensor::from_array((vec![1i64], vec![speed]))?,
        ])?;
        let (_, audio) = outputs[0].try_extract_tensor::<f32>()?;
        Ok(audio.to_vec())
    }
}

/// Splits on sentence ends so each model call stays short (and later: speak while the next renders).
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '!' | '?' | '\n') && current.trim().len() > 1 {
            out.push(std::mem::take(&mut current).trim().to_string());
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Kokoro's vocabulary (from the model's `tokenizer.json`).
fn token_id(c: char) -> Option<i64> {
    const VOCAB: &[(char, i64)] = &[
        (';', 1), (':', 2), (',', 3), ('.', 4), ('!', 5), ('?', 6), ('—', 9), ('…', 10), ('"', 11), ('(', 12),
        (')', 13), ('“', 14), ('”', 15), (' ', 16), ('\u{303}', 17), ('ʣ', 18), ('ʥ', 19), ('ʦ', 20), ('ʨ', 21),
        ('ᵝ', 22), ('ꭧ', 23), ('A', 24), ('I', 25), ('O', 31), ('Q', 33), ('S', 35), ('T', 36), ('W', 39),
        ('Y', 41), ('ᵊ', 42), ('a', 43), ('b', 44), ('c', 45), ('d', 46), ('e', 47), ('f', 48), ('h', 50),
        ('i', 51), ('j', 52), ('k', 53), ('l', 54), ('m', 55), ('n', 56), ('o', 57), ('p', 58), ('q', 59),
        ('r', 60), ('s', 61), ('t', 62), ('u', 63), ('v', 64), ('w', 65), ('x', 66), ('y', 67), ('z', 68),
        ('ɑ', 69), ('ɐ', 70), ('ɒ', 71), ('æ', 72), ('β', 75), ('ɔ', 76), ('ɕ', 77), ('ç', 78), ('ɖ', 80),
        ('ð', 81), ('ʤ', 82), ('ə', 83), ('ɚ', 85), ('ɛ', 86), ('ɜ', 87), ('ɟ', 90), ('ɡ', 92), ('ɥ', 99),
        ('ɨ', 101), ('ɪ', 102), ('ʝ', 103), ('ɯ', 110), ('ɰ', 111), ('ŋ', 112), ('ɳ', 113), ('ɲ', 114),
        ('ɴ', 115), ('ø', 116), ('ɸ', 118), ('θ', 119), ('œ', 120), ('ɹ', 123), ('ɾ', 125), ('ɻ', 126),
        ('ʁ', 128), ('ɽ', 129), ('ʂ', 130), ('ʃ', 131), ('ʈ', 132), ('ʧ', 133), ('ʊ', 135), ('ʋ', 136),
        ('ʌ', 138), ('ɣ', 139), ('ɤ', 140), ('χ', 142), ('ʎ', 143), ('ʒ', 147), ('ʔ', 148), ('ˈ', 156),
        ('ˌ', 157), ('ː', 158), ('ʰ', 162), ('ʲ', 164), ('↓', 169), ('→', 171), ('↗', 172), ('↘', 173),
        ('ᵻ', 177),
    ];
    VOCAB.iter().find(|(v, _)| *v == c).map(|(_, id)| *id)
}

/// Word → phonemes, from misaki's gold dictionary first, then silver.
struct Lexicon {
    words: HashMap<String, String>,
}

impl Lexicon {
    fn load(dir: &Path) -> Result<Self> {
        let mut words = HashMap::new();
        // Silver first, so gold entries overwrite it.
        for file in ["us_silver.json", "us_gold.json"] {
            let raw = std::fs::read_to_string(dir.join(file)).with_context(|| format!("reading {file}"))?;
            let map: HashMap<String, serde_json::Value> = serde_json::from_str(&raw).with_context(|| format!("parsing {file}"))?;
            for (word, value) in map {
                let phonemes = match value {
                    serde_json::Value::String(s) => Some(s),
                    serde_json::Value::Object(o) => o.get("DEFAULT").and_then(|v| v.as_str()).map(str::to_string),
                    _ => None,
                };
                if let Some(p) = phonemes {
                    words.insert(word, p);
                }
            }
        }
        if words.is_empty() {
            return Err(anyhow!("Kokoro dictionaries are empty"));
        }
        Ok(Self { words })
    }

    fn lookup(&self, word: &str) -> Option<String> {
        let lower = word.to_lowercase();
        let mut capital = lower.clone();
        if let Some(first) = capital.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        [word, lower.as_str(), capital.as_str()].iter().find_map(|w| self.words.get(*w).cloned())
    }

    fn word(&self, word: &str) -> String {
        if let Some(p) = self.lookup(word) {
            return p;
        }
        if word.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = word.parse::<u64>() {
                return number_words(n).split(' ').map(|w| self.word(w)).collect::<Vec<_>>().join(" ");
            }
        }
        if let Some(p) = self.inflected(word) {
            return p;
        }
        // Acronyms and letters mixed with digits (T3, GPT): say them one by one.
        let letters = word.chars().filter(|c| c.is_alphabetic()).count();
        let acronym = word.chars().any(|c| c.is_ascii_digit())
            || (letters <= 5 && letters > 1 && word.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()))
            || letters == 1;
        if acronym {
            return self.spell(word);
        }
        if let Some(p) = self.compound(&word.to_lowercase()) {
            return p;
        }
        // Anything else is a word: sound it out instead of spelling it.
        sound_out(&word.to_lowercase())
    }

    fn spell(&self, word: &str) -> String {
        word.chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| {
                let s = c.to_string();
                if c.is_ascii_digit() {
                    self.word(&s)
                } else {
                    self.lookup(&s.to_uppercase()).unwrap_or_default()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn known(&self, w: &str) -> Option<String> {
        if w.len() < 2 {
            return None;
        }
        self.lookup(w).or_else(|| self.inflected(w))
    }

    /// Two known words glued together ("codebase", "unrendered" = un + rendered).
    fn compound(&self, lower: &str) -> Option<String> {
        if !lower.is_ascii() {
            return None;
        }
        for i in (2..lower.len().saturating_sub(1)).rev() {
            let (a, b) = lower.split_at(i);
            if let (Some(pa), Some(pb)) = (self.known(a), self.known(b)) {
                return Some(format!("{pa}{}", pb.replace('ˈ', "ˌ")));
            }
        }
        None
    }

    /// A known word plus -s, -'s, -ed or -ing ("delivered" from "deliver"), so words missing
    /// from the dictionary aren't spelled out letter by letter.
    fn inflected(&self, word: &str) -> Option<String> {
        let lower = word.to_lowercase();
        // Stems to try: as is, with a final e put back (used -> use), and with a doubled
        // consonant undone (stopped -> stop).
        let stems = |rest: &str| -> Vec<String> {
            let mut v = vec![rest.to_string(), format!("{rest}e")];
            // copied -> copy
            if let Some(r) = rest.strip_suffix('i') {
                v.push(format!("{r}y"));
            }
            let b = rest.as_bytes();
            if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] {
                v.push(rest[..rest.len() - 1].to_string());
            }
            v
        };
        let last = |p: &str| p.chars().rev().find(|c| !matches!(c, 'ˈ' | 'ˌ' | 'ː')).unwrap_or(' ');
        let find = |rest: &str| stems(rest).into_iter().find_map(|s| self.lookup(&s));
        if let Some(rest) = lower.strip_suffix("ed").or_else(|| lower.strip_suffix('d')) {
            if let Some(p) = find(rest) {
                let end = match last(&p) {
                    't' | 'd' => "ᵻd",
                    'p' | 'k' | 'f' | 's' | 'ʃ' | 'ʧ' | 'θ' => "t",
                    _ => "d",
                };
                return Some(format!("{p}{end}"));
            }
        }
        if let Some(rest) = lower.strip_suffix("ing") {
            if let Some(p) = find(rest) {
                return Some(format!("{p}ɪŋ"));
            }
        }
        for suffix in ["'s", "es", "s"] {
            if let Some(rest) = lower.strip_suffix(suffix) {
                if let Some(p) = self.lookup(rest) {
                    let end = match last(&p) {
                        's' | 'z' | 'ʃ' | 'ʒ' | 'ʧ' | 'ʤ' => "ᵻz",
                        'p' | 't' | 'k' | 'f' | 'θ' => "s",
                        _ => "z",
                    };
                    return Some(format!("{p}{end}"));
                }
            }
        }
        None
    }

    fn phonemize(&self, text: &str) -> String {
        let mut out = String::new();
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            if !word.is_empty() {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(&self.word(word));
                word.clear();
            }
        };
        for c in text.chars() {
            let c = if c == '’' { '\'' } else { c };
            if c.is_alphanumeric() || (c == '\'' && !word.is_empty()) {
                word.push(c);
            } else {
                flush(&mut word, &mut out);
                match c {
                    '.' | ',' | '!' | '?' | ';' | ':' | '—' | '…' => out.push(c),
                    '-' => out.push(' '),
                    _ => {
                        if !out.ends_with(' ') && !out.is_empty() {
                            out.push(' ');
                        }
                    }
                }
            }
        }
        flush(&mut word, &mut out);
        out
    }
}

/// Rough English letter-to-sound rules for words the dictionary doesn't know.
fn sound_out(word: &str) -> String {
    const RULES: &[(&str, &str)] = &[
        ("tion", "ʃən"), ("sion", "ʒən"), ("ture", "ʧɚ"), ("ough", "ʌf"), ("igh", "aɪ"), ("tch", "ʧ"),
        ("sch", "sk"), ("ch", "ʧ"), ("sh", "ʃ"), ("th", "θ"), ("ph", "f"), ("wh", "w"), ("ck", "k"),
        ("ng", "ŋ"), ("qu", "kw"), ("gh", ""), ("kn", "n"), ("wr", "ɹ"), ("ee", "i"), ("ea", "i"),
        ("oo", "u"), ("ou", "aʊ"), ("ow", "oʊ"), ("oi", "ɔɪ"), ("oy", "ɔɪ"), ("ai", "eɪ"), ("ay", "eɪ"),
        ("au", "ɔ"), ("aw", "ɔ"), ("ie", "i"), ("ei", "eɪ"), ("ey", "i"), ("oa", "oʊ"), ("er", "ɚ"),
        ("ir", "ɜɹ"), ("ur", "ɜɹ"), ("ar", "ɑɹ"), ("or", "ɔɹ"), ("ew", "ju"),
    ];
    let b: Vec<char> = word.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let s: String = b.iter().collect();
    let vowel = |c: char| "aeiouy".contains(c);
    let mut out = String::new();
    let mut stressed = false;
    let mut i = 0;
    while i < b.len() {
        let rest = &s[i..];
        if let Some((g, p)) = RULES.iter().find(|(g, _)| rest.starts_with(g)) {
            if !stressed && p.chars().any(|c| "iuaeɪɔɚɜɑoʊ".contains(c)) {
                out.push('ˈ');
                stressed = true;
            }
            out.push_str(p);
            i += g.len();
            continue;
        }
        let c = b[i];
        let next = b.get(i + 1).copied();
        // Magic e: vowel + consonant + final e.
        let magic = vowel(c) && c != 'y' && i + 2 == b.len() - 1 && b[b.len() - 1] == 'e' && next.is_some_and(|n| !vowel(n));
        let p: &str = match c {
            'a' => if magic { "eɪ" } else { "æ" },
            'e' => if i == b.len() - 1 && i > 0 { "" } else if magic { "i" } else { "ɛ" },
            'i' => if magic { "aɪ" } else { "ɪ" },
            'o' => if magic { "oʊ" } else { "ɑ" },
            'u' => if magic { "u" } else { "ʌ" },
            'y' => if i == 0 { "j" } else if i == b.len() - 1 { "i" } else { "ɪ" },
            'c' => if next.is_some_and(|n| "eiy".contains(n)) { "s" } else { "k" },
            'g' => if next.is_some_and(|n| "eiy".contains(n)) && i > 0 { "ʤ" } else { "ɡ" },
            'j' => "ʤ",
            'r' => "ɹ",
            'x' => "ks",
            'q' => "k",
            _ => "",
        };
        if !stressed && vowel(c) && !p.is_empty() {
            out.push('ˈ');
            stressed = true;
        }
        if p.is_empty() && !vowel(c) {
            // Plain consonant, skip doubled ones (ll, tt).
            if i == 0 || b[i - 1] != c {
                out.push(c);
            }
        } else {
            out.push_str(p);
        }
        i += 1;
    }
    out
}

fn number_words(n: u64) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    match n {
        0..=19 => ONES[n as usize].to_string(),
        20..=99 => {
            let t = TENS[(n / 10) as usize];
            if n % 10 == 0 { t.to_string() } else { format!("{t} {}", ONES[(n % 10) as usize]) }
        }
        100..=999 => {
            let rest = n % 100;
            let head = format!("{} hundred", ONES[(n / 100) as usize]);
            if rest == 0 { head } else { format!("{head} {}", number_words(rest)) }
        }
        1000..=999_999 => {
            let rest = n % 1000;
            let head = format!("{} thousand", number_words(n / 1000));
            if rest == 0 { head } else { format!("{head} {}", number_words(rest)) }
        }
        _ => n.to_string().chars().map(|c| ONES[c.to_digit(10).unwrap_or(0) as usize]).collect::<Vec<_>>().join(" "),
    }
}

/// Speaks the text through the default output and waits until it's done.
pub fn say_blocking(text: &str) -> Result<()> {
    let mut kokoro = Kokoro::load(&dir(), DEFAULT_VOICE)?;
    let samples = kokoro.synthesize(text, 1.0)?;
    // Keep a copy, handy to compare voices or share a sample.
    let spec = hound::WavSpec { channels: 1, sample_rate: SAMPLE_RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    if let Ok(mut w) = hound::WavWriter::create(dir().join("last.wav"), spec) {
        samples.iter().for_each(|s| { let _ = w.write_sample(*s); });
        let _ = w.finalize();
    }
    crate::speech::sound::play_samples_blocking(&samples, SAMPLE_RATE)
}

/// One loaded Kokoro (and which voice it has), shared, so later lines don't wait for loading.
static ENGINE: std::sync::Mutex<Option<(String, Kokoro)>> = std::sync::Mutex::new(None);
/// Rendered lines by voice, speed and text, so fixed lines play instantly after the first time.
static CACHE: std::sync::Mutex<Option<HashMap<String, std::sync::Arc<Vec<f32>>>>> = std::sync::Mutex::new(None);

/// Held while a line plays, so lines never overlap.
static SPEAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The moments me-voice can talk about (one switch each on the Voice page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Stop,
    Sent,
    NewChat,
    Working,
    StillWorking,
    NeedsOk,
    Question,
    Finished,
    Error,
    CommandAnswer,
}

impl Line {
    /// The variants it picks from at random. `{a}` becomes ", sir" (or nothing).
    pub fn variants(self) -> &'static [&'static str] {
        match self {
            Line::Stop => &["Got it{a}.", "Very good.", "On it.", "Right away{a}."],
            Line::Sent => &["Message sent{a}.", "Delivered{a}.", "It's on its way."],
            Line::NewChat => &["New session started in T3 Code."],
            Line::Working => &["Working on it.", "The agent's on it now."],
            Line::StillWorking => &["Still at it{a}.", "Bear with it. Still working."],
            Line::NeedsOk => &["It needs your OK in T3 Code{a}."],
            Line::Question => &["It has a question for you{a}."],
            Line::Finished => &["Finished{a}. Shall I read you the summary?"],
            Line::Error => &["I'm afraid T3 Code isn't running.", "That didn't go through{a}."],
            Line::CommandAnswer => &["Here's the answer{a}."],
        }
    }

    /// Whether this moment is switched on.
    pub fn enabled(self, v: &crate::settings::VoiceSettings) -> bool {
        v.talk_back
            && match self {
                Line::Stop => v.stop_sound == crate::settings::StopSound::Voice,
                Line::Sent => v.say_sent,
                Line::NewChat => v.say_new_chat,
                Line::Working => v.say_working,
                Line::StillWorking => v.say_still_working_minutes > 0,
                Line::NeedsOk => v.say_needs_ok,
                Line::Question => v.say_question,
                Line::Finished => v.say_finished,
                Line::Error => v.say_errors,
                Line::CommandAnswer => v.say_command_answers,
            }
    }

    /// One variant, picked at random, with the form of address filled in.
    pub fn text(self, v: &crate::settings::VoiceSettings) -> String {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let list = self.variants();
        fill(list[nanos as usize % list.len()], &v.address)
    }
}

/// Fills `{a}` with ", sir" / ", Jeroen", or nothing when no address is set.
pub fn fill(template: &str, address: &str) -> String {
    let a = address.trim();
    template.replace("{a}", &if a.is_empty() { String::new() } else { format!(", {a}") })
}

/// Says `line` on a background thread if it's switched on. Returns false when it's off or
/// Kokoro isn't there, so the caller can play a beep instead.
pub fn announce(line: Line, v: &crate::settings::VoiceSettings) -> bool {
    if !line.enabled(v) || !installed() {
        return false;
    }
    say(line.text(v), v.clone());
    true
}

/// Speaks any text on a background thread (Test voice, ▶ buttons, announcements). Plays the
/// stop beep instead if Kokoro fails.
pub fn say(text: String, v: crate::settings::VoiceSettings) {
    say_then(text, v, || {});
}

/// Like `say`, then calls `done` when the line has played (or was stopped / failed).
pub fn say_then(text: String, v: crate::settings::VoiceSettings, done: impl FnOnce() + Send + 'static) {
    if crate::speech::dnd::is_on() {
        crate::app::log::write("do not disturb: not spoken");
        return;
    }
    std::thread::spawn(move || {
        if let Err(e) = speak_blocking(&text, &v) {
            crate::app::log::write(&format!("voice: {e:#}"));
            crate::speech::sound::play(crate::speech::sound::Beep::Stop);
        }
        done();
    });
}

/// Says `text` for `line` if that moment is switched on; `done` runs after it played.
/// Returns false (and doesn't call `done`) when it's off.
pub fn announce_text(line: Line, text: String, v: &crate::settings::VoiceSettings, done: impl FnOnce() + Send + 'static) -> bool {
    if !line.enabled(v) || !installed() {
        return false;
    }
    say_then(text, v.clone(), done);
    true
}

/// Cuts off the line being spoken now.
pub fn stop() {
    crate::speech::sound::STOP_SPEECH.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a line is being spoken right now.
pub fn is_speaking() -> bool {
    SPEAKING.try_lock().is_err()
}

/// The last line spoken, for "repeat".
pub fn last_spoken() -> Option<String> {
    LAST_SPOKEN.lock().unwrap().clone()
}

static LAST_SPOKEN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Renders (cached) and plays `text` with the voice, speed and volume from the settings.
pub fn speak_blocking(text: &str, v: &crate::settings::VoiceSettings) -> Result<()> {
    if crate::speech::dnd::is_on() {
        return Ok(());
    }
    let samples = rendered(text, v)?;
    // One line at a time: a later line waits for the one playing instead of talking over it.
    let _speaking = SPEAKING.lock().unwrap_or_else(|e| e.into_inner());
    crate::speech::sound::STOP_SPEECH.store(false, std::sync::atomic::Ordering::Relaxed);
    *LAST_SPOKEN.lock().unwrap() = Some(text.to_string());
    // 70 % (the default) is the model's own level.
    let gain = v.volume.clamp(0.0, 1.0) / 0.7;
    let scaled: Vec<f32> = samples.iter().map(|s| (s * gain).clamp(-1.0, 1.0)).collect();
    crate::speech::sound::play_samples_blocking(&scaled, SAMPLE_RATE)
}

/// Loads the model and renders the switched-on short lines, so they play instantly.
pub fn warm_up(v: &crate::settings::VoiceSettings) {
    if !v.talk_back || !installed() {
        return;
    }
    let lines = [Line::Stop, Line::Sent, Line::NewChat, Line::Error];
    for line in lines.into_iter().filter(|l| l.enabled(v)) {
        for t in line.variants() {
            if let Err(e) = rendered(&fill(t, &v.address), v) {
                crate::app::log::write(&format!("voice warm-up: {e:#}"));
                return;
            }
        }
    }
}

fn rendered(text: &str, v: &crate::settings::VoiceSettings) -> Result<std::sync::Arc<Vec<f32>>> {
    let voice = if v.voice.is_empty() { DEFAULT_VOICE } else { v.voice.as_str() };
    let key = format!("{voice}|{:.2}|{text}", v.speed);
    if let Some(s) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key).cloned()) {
        return Ok(s);
    }
    let samples = {
        let mut engine = ENGINE.lock().unwrap();
        if engine.as_ref().is_none_or(|(id, _)| id != voice) {
            *engine = Some((voice.to_string(), Kokoro::load(&dir(), voice)?));
        }
        std::sync::Arc::new(engine.as_mut().unwrap().1.synthesize(text, v.speed.clamp(0.5, 2.0))?)
    };
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, samples.clone());
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number_words(42), "forty two");
        assert_eq!(number_words(305), "three hundred five");
    }

    #[test]
    fn sentences() {
        assert_eq!(split_sentences("Got it, sir. On it! Ok"), vec!["Got it, sir.", "On it!", "Ok"]);
    }

    #[test]
    fn known_words_with_endings_are_not_spelled_out() {
        let words = [("deliver", "dəlˈɪvɚ"), ("stop", "stˈɑp"), ("use", "jˈuz"), ("start", "stˈɑɹt"), ("box", "bˈɑks"), ("copy", "kˈɑpi")];
        let lex = Lexicon { words: words.iter().map(|(w, p)| (w.to_string(), p.to_string())).collect() };
        assert_eq!(lex.word("Delivered"), "dəlˈɪvɚd");
        assert_eq!(lex.word("deliverstop"), "dəlˈɪvɚstˌɑp");
        assert!(!lex.word("frobnicate").contains(' '));
        assert!(lex.word("GPT").contains(' ') || lex.word("GPT").is_empty());
        assert_eq!(lex.word("stopped"), "stˈɑpt");
        assert_eq!(lex.word("used"), "jˈuzd");
        assert_eq!(lex.word("started"), "stˈɑɹtᵻd");
        assert_eq!(lex.word("starting"), "stˈɑɹtɪŋ");
        assert_eq!(lex.word("boxes"), "bˈɑksᵻz");
        assert_eq!(lex.word("stops"), "stˈɑps");
        assert_eq!(lex.word("copied"), "kˈɑpid");
    }
}
