//! Text for the voice: turns an agent's Markdown answer into text that sounds
//! right when spoken, splits it into chunks for "read it" and "more", and works
//! out what a short spoken reply means. Pure functions, no I/O.

#![allow(dead_code)]

use crate::settings::VoiceSettings;

const CODE_SKIPPED: &str = "Code skipped.";
const TABLE_SKIPPED: &str = "Table skipped.";

/// Turns an agent's Markdown answer into text that sounds right when spoken.
pub fn speakable(markdown: &str) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let mut fence: Option<&str> = None;
    let mut in_table = false;
    for line in markdown.lines() {
        let t = line.trim();
        if let Some(marker) = fence {
            if t.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| t.starts_with(*m)) {
            fence = Some(marker);
            pieces.push(CODE_SKIPPED.into());
            in_table = false;
            continue;
        }
        if t.starts_with('|') {
            if !in_table {
                pieces.push(TABLE_SKIPPED.into());
                in_table = true;
            }
            continue;
        }
        in_table = false;
        let (text, is_item) = block_text(t);
        if !text.chars().any(char::is_alphanumeric) {
            continue;
        }
        let ends_sentence = text.chars().last().is_some_and(|c| matches!(c, '.' | '!' | '?' | ':'));
        if is_item && !ends_sentence {
            pieces.push(text + ".");
        } else {
            pieces.push(text);
        }
    }
    pieces.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strips the block-level Markdown from one line (quote, heading, list marker)
/// and converts its inline Markdown. Returns the text and whether it was a list item.
fn block_text(line: &str) -> (String, bool) {
    let mut s = line;
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    if s.starts_with('#') {
        let rest = s.trim_start_matches('#');
        if rest.is_empty() || rest.starts_with(' ') {
            s = rest.trim_start();
        }
    }
    let mut is_item = false;
    if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")).or_else(|| s.strip_prefix("+ ")) {
        s = rest.trim_start();
        is_item = true;
    } else if let Some(rest) = numbered_rest(s) {
        s = rest;
        is_item = true;
    }
    (shorten_paths(&inline(s)), is_item)
}

/// `12. text` or `12) text` -> `text`.
fn numbered_rest(s: &str) -> Option<&str> {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = s[digits..].strip_prefix(|c: char| c == '.' || c == ')')?;
    let rest = rest.strip_prefix(' ')?;
    Some(rest.trim_start())
}

/// Converts inline Markdown: code spans, links, bare URLs and emphasis markers.
fn inline(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            match find_char(&chars, i + 1, '`') {
                Some(j) => {
                    out.extend(&chars[i + 1..j]);
                    i = j + 1;
                }
                None => i += 1,
            }
            continue;
        }
        if c == '[' {
            if let Some(close) = find_char(&chars, i + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = find_char(&chars, close + 2, ')') {
                        let text: String = chars[i + 1..close].iter().collect();
                        out.push_str(&inline(&text));
                        i = end + 1;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
            continue;
        }
        if starts_with_url(&chars, i) {
            let end = chars[i..].iter().position(|c| c.is_whitespace()).map_or(chars.len(), |p| i + p);
            let mut stop = end;
            while stop > i && matches!(chars[stop - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '>' | '"' | '\'') {
                stop -= 1;
            }
            out.push_str("a link");
            out.extend(&chars[stop..end]);
            i = end;
            continue;
        }
        if (c == '*' || c == '_') && is_emphasis(&chars, i) {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    chars.get(from..)?.iter().position(|&c| c == target).map(|p| from + p)
}

fn starts_with_url(chars: &[char], i: usize) -> bool {
    let head: String = chars[i..].iter().take(8).collect();
    head.starts_with("http://") || head.starts_with("https://")
}

/// A `*` or `_` is emphasis when it opens a word (followed by text, preceded by a
/// non-word character) or closes one. Mid-word underscores (`snake_case`) and
/// spaced-out operators (`2 * 3`) are kept.
fn is_emphasis(chars: &[char], i: usize) -> bool {
    let prev = if i == 0 { None } else { Some(chars[i - 1]) };
    let next = chars.get(i + 1).copied();
    let opens = next.is_some_and(|n| !n.is_whitespace()) && prev.map_or(true, |p| !p.is_alphanumeric());
    let closes = prev.is_some_and(|p| !p.is_whitespace()) && next.map_or(true, |n| !n.is_alphanumeric());
    opens || closes
}

/// Replaces file paths (`src/app/mod.rs`, `C:\Users\x\file.txt`) with their file name.
fn shorten_paths(text: &str) -> String {
    text.split_whitespace().map(shorten_token).collect::<Vec<_>>().join(" ")
}

fn shorten_token(token: &str) -> String {
    let lead_len = token.len() - token.trim_start_matches(|c: char| "([{\"'".contains(c)).len();
    let lead = &token[..lead_len];
    let body = &token[lead_len..];
    let core = body.trim_end_matches(|c: char| ".,;:!?)]}\"'".contains(c));
    let tail = &body[core.len()..];
    if !core.contains(|c: char| c == '/' || c == '\\') {
        return token.to_string();
    }
    let parts: Vec<&str> = core.split(|c: char| c == '/' || c == '\\').filter(|p| !p.is_empty()).collect();
    match parts.last() {
        Some(name) if parts.len() >= 3 || name.contains('.') => format!("{lead}{name}{tail}"),
        _ => token.to_string(),
    }
}

/// Splits text into pieces of at most `max_chars` at sentence ends (. ! ? followed by
/// space), for "read it" then "more". A single sentence longer than `max_chars` is
/// split at the last space before the limit.
pub fn chunks(text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.max(1);
    let mut sentences: Vec<String> = Vec::new();
    let mut current_words: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        current_words.push(word);
        if word.ends_with(|c: char| matches!(c, '.' | '!' | '?')) {
            sentences.push(current_words.join(" "));
            current_words.clear();
        }
    }
    if !current_words.is_empty() {
        sentences.push(current_words.join(" "));
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for sentence in sentences {
        let len = sentence.chars().count();
        if len > max {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.extend(split_long(&sentence, max));
        } else if current.is_empty() {
            current = sentence;
        } else if current.chars().count() + 1 + len <= max {
            current.push(' ');
            current.push_str(&sentence);
        } else {
            out.push(std::mem::replace(&mut current, sentence));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Greedy word packing for one sentence longer than the limit. A word longer than
/// the limit on its own is cut at the limit.
fn split_long(sentence: &str, max: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in sentence.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > max {
            let head: String = word.chars().take(max).collect();
            word = word.chars().skip(max).collect();
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            parts.push(head);
        }
        if cur.is_empty() {
            cur = word;
        } else if cur.chars().count() + 1 + word.chars().count() <= max {
            cur.push(' ');
            cur.push_str(&word);
        } else {
            parts.push(std::mem::replace(&mut cur, word));
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Yes,
    No,
    Read,
    More,
    Repeat,
    Stop,
    Status,
    End,
    Other(String),
}

/// What a short spoken answer means. Lowercase, trim, drop punctuation, drop a leading
/// "ok"/"okay"/"please" and trailing "please", "thanks", "thank you", and a trailing form
/// of address (", sir", ", ma'am", or `v.address`). Only the WHOLE remaining utterance
/// counts as a command; anything longer is Other(original trimmed text). A command whose
/// switch (cmd_*) is off becomes Other(original text). Empty input -> Other(String::new()).
pub fn parse_reply(text: &str, v: &VoiceSettings) -> Reply {
    let original = text.trim();
    if original.is_empty() {
        return Reply::Other(String::new());
    }
    let other = || Reply::Other(original.to_string());

    let address_words = normalize(&v.address);
    let address: Vec<&str> = address_words.iter().map(String::as_str).collect();
    let fixed: [&[&str]; 5] = [&["please"], &["thanks"], &["thank", "you"], &["sir"], &["ma'am"]];
    let mut tails: Vec<&[&str]> = fixed.to_vec();
    if !address.is_empty() {
        tails.push(&address);
    }

    let mut words = normalize(original);
    let mut changed = true;
    while changed {
        changed = false;
        if matches!(words.first().map(String::as_str), Some("ok" | "okay" | "please")) {
            words.remove(0);
            changed = true;
            continue;
        }
        if let Some(n) = tails.iter().find(|tail| ends_with_words(&words, tail)).map(|tail| tail.len()) {
            words.truncate(words.len() - n);
            changed = true;
        }
    }

    let phrase = words.join(" ");
    let (reply, switch_on) = match phrase.as_str() {
        "yes" | "yeah" | "yep" | "sure" | "allow" | "allow it" | "go ahead" | "do it" | "approve" => (Reply::Yes, v.cmd_yes),
        "no" | "nope" | "deny" | "deny it" | "don't" | "do not" | "cancel" => (Reply::No, v.cmd_no),
        "read it" | "read" | "read it out" | "summary" | "read the summary" | "what did it say" | "tell me" => {
            (Reply::Read, v.cmd_read)
        }
        "more" | "continue" | "go on" | "keep going" | "next" => (Reply::More, v.cmd_more),
        "repeat" | "repeat that" | "say again" | "say that again" | "again" | "pardon" | "what" => {
            (Reply::Repeat, v.cmd_repeat)
        }
        "stop" | "quiet" | "be quiet" | "silence" | "shut up" | "enough" | "stop talking" => (Reply::Stop, v.cmd_stop),
        "status" | "what's it doing" | "what is it doing" | "what's running" | "what is running" | "what's happening"
        | "update" => (Reply::Status, v.cmd_status),
        "that's all" | "that is all" | "end" | "goodbye" | "bye" | "done" | "never mind" | "nothing" => {
            (Reply::End, v.cmd_end)
        }
        _ => return other(),
    };
    if switch_on {
        reply
    } else {
        other()
    }
}

/// Lowercases, turns punctuation into spaces (keeping apostrophes), and splits into words.
fn normalize(s: &str) -> Vec<String> {
    let cleaned: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            '\u{2019}' | '\u{2018}' => '\'',
            c if c.is_alphanumeric() || c == '\'' => c,
            _ => ' ',
        })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn ends_with_words(words: &[String], tail: &[&str]) -> bool {
    !tail.is_empty()
        && words.len() >= tail.len()
        && words[words.len() - tail.len()..].iter().zip(tail).all(|(w, t)| w.as_str() == *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice() -> VoiceSettings {
        VoiceSettings::default()
    }

    // ---- speakable ----

    #[test]
    fn emphasis_markers_removed() {
        assert_eq!(
            speakable("Hello **world** and *there* and __this__ and _that_."),
            "Hello world and there and this and that."
        );
    }

    #[test]
    fn fenced_code_is_replaced_once_per_block() {
        assert_eq!(speakable("Before\n```rust\nlet x = 1;\n```\nAfter"), "Before Code skipped. After");
        assert_eq!(speakable("```\na\n```\nmiddle\n~~~\nb\n~~~"), "Code skipped. middle Code skipped.");
    }

    #[test]
    fn empty_fence_still_says_code_skipped() {
        assert_eq!(speakable("```\n```"), "Code skipped.");
    }

    #[test]
    fn unclosed_fence_swallows_the_rest() {
        assert_eq!(speakable("Intro\n```\nnever closed\nmore"), "Intro Code skipped.");
    }

    #[test]
    fn inline_code_keeps_content() {
        assert_eq!(speakable("Run `cargo test` now."), "Run cargo test now.");
    }

    #[test]
    fn links_become_their_text() {
        assert_eq!(speakable("See [the docs](https://example.com/docs) now."), "See the docs now.");
    }

    #[test]
    fn bare_urls_become_a_link() {
        assert_eq!(speakable("Visit https://example.com/a?b=1, then."), "Visit a link, then.");
        assert_eq!(speakable("Try http://x.y/z)"), "Try a link)");
    }

    #[test]
    fn headings_lose_their_hashes() {
        assert_eq!(speakable("# Title\n\n## Sub heading\nText."), "Title Sub heading Text.");
        assert_eq!(speakable("#hashtag stays"), "#hashtag stays");
    }

    #[test]
    fn bullets_become_sentences() {
        assert_eq!(
            speakable("- first\n* second.\n+ third!\n1. fourth:\n2) fifth"),
            "first. second. third! fourth: fifth."
        );
        assert_eq!(speakable("- **Note**: keep"), "Note: keep.");
        assert_eq!(speakable("- See [docs](http://x.y)"), "See docs.");
    }

    #[test]
    fn blockquotes_are_unwrapped() {
        assert_eq!(speakable("> quoted line"), "quoted line");
        assert_eq!(speakable("> - quoted item"), "quoted item.");
    }

    #[test]
    fn table_is_skipped_once_per_table() {
        assert_eq!(speakable("| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter."), "Table skipped. After.");
        assert_eq!(speakable("| a |\n\ntext\n\n| b |"), "Table skipped. text Table skipped.");
    }

    #[test]
    fn file_paths_become_file_names() {
        assert_eq!(
            speakable("Edit src/app/mod.rs and C:\\Users\\x\\file.txt."),
            "Edit mod.rs and file.txt."
        );
        assert_eq!(speakable("Open `src/text/speakable.rs`."), "Open speakable.rs.");
    }

    #[test]
    fn slashes_between_words_are_kept() {
        assert_eq!(speakable("Use and/or here."), "Use and/or here.");
        assert_eq!(speakable("Ratio 1/2 holds."), "Ratio 1/2 holds.");
    }

    #[test]
    fn identifiers_and_operators_are_kept() {
        assert_eq!(speakable("Call snake_case_name here."), "Call snake_case_name here.");
        assert_eq!(speakable("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(speakable("2*3"), "2*3");
    }

    #[test]
    fn whitespace_is_collapsed() {
        assert_eq!(speakable("  a   b \n\n\n c  "), "a b c");
        assert_eq!(speakable(""), "");
        assert_eq!(speakable("   \n  "), "");
    }

    #[test]
    fn horizontal_rules_are_dropped() {
        assert_eq!(speakable("a\n---\nb"), "a b");
    }

    // ---- chunks ----

    #[test]
    fn chunks_pack_whole_sentences() {
        assert_eq!(chunks("One. Two. Three.", 10), vec!["One. Two.", "Three."]);
        assert_eq!(chunks("One. Two. Three.", 100), vec!["One. Two. Three."]);
    }

    #[test]
    fn chunks_split_on_question_and_exclamation() {
        assert_eq!(chunks("Really? Yes! Ok.", 8), vec!["Really?", "Yes! Ok."]);
    }

    #[test]
    fn long_sentence_splits_at_last_space_before_limit() {
        assert_eq!(
            chunks("alpha beta gamma delta epsilon.", 12),
            vec!["alpha beta", "gamma delta", "epsilon."]
        );
    }

    #[test]
    fn word_longer_than_limit_is_cut() {
        assert_eq!(chunks("abcdefghijklmnop", 5), vec!["abcde", "fghij", "klmno", "p"]);
    }

    #[test]
    fn chunks_of_empty_text_is_empty() {
        assert!(chunks("", 10).is_empty());
        assert!(chunks("   \n ", 10).is_empty());
    }

    #[test]
    fn chunks_respect_limit_and_keep_all_words() {
        let text = "The first sentence is here. A second one follows it! Is there a third? \
                    Yes, and it runs on for quite a while without stopping at all so it must be split up.";
        for max in [1, 7, 16, 30, 64, 500] {
            let parts = chunks(text, max);
            assert!(parts.iter().all(|p| p.chars().count() <= max), "max {max}: {parts:?}");
            let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(squeeze(&parts.join(" ")), squeeze(text), "max {max}");
        }
    }

    // ---- parse_reply ----

    fn parse(text: &str) -> Reply {
        parse_reply(text, &voice())
    }

    #[test]
    fn whisper_style_commands() {
        assert_eq!(parse("Yes."), Reply::Yes);
        assert_eq!(parse("Read it, sir."), Reply::Read);
        assert_eq!(parse("That's all, thank you."), Reply::End);
        assert_eq!(parse("No, thanks."), Reply::No);
        assert_eq!(parse("Please stop."), Reply::Stop);
        assert_eq!(parse("Okay, read it."), Reply::Read);
        assert_eq!(parse("Yes, please"), Reply::Yes);
        assert_eq!(parse("Repeat that, ma'am"), Reply::Repeat);
        assert_eq!(parse("Okay, sure, thanks"), Reply::Yes);
        assert_eq!(parse("Please yes"), Reply::Yes);
    }

    #[test]
    fn every_phrase_maps_to_its_reply() {
        let cases: &[(&str, Reply)] = &[
            ("yes", Reply::Yes),
            ("yeah", Reply::Yes),
            ("yep", Reply::Yes),
            ("sure", Reply::Yes),
            ("allow", Reply::Yes),
            ("Allow it!", Reply::Yes),
            ("go ahead", Reply::Yes),
            ("do it", Reply::Yes),
            ("approve", Reply::Yes),
            ("no", Reply::No),
            ("nope", Reply::No),
            ("deny", Reply::No),
            ("deny it", Reply::No),
            ("Don't.", Reply::No),
            ("do not", Reply::No),
            ("cancel", Reply::No),
            ("read it", Reply::Read),
            ("read", Reply::Read),
            ("read it out", Reply::Read),
            ("summary", Reply::Read),
            ("read the summary", Reply::Read),
            ("what did it say", Reply::Read),
            ("tell me", Reply::Read),
            ("more", Reply::More),
            ("continue", Reply::More),
            ("go on", Reply::More),
            ("keep going", Reply::More),
            ("Next!", Reply::More),
            ("repeat", Reply::Repeat),
            ("repeat that", Reply::Repeat),
            ("say again", Reply::Repeat),
            ("say that again", Reply::Repeat),
            ("again", Reply::Repeat),
            ("pardon", Reply::Repeat),
            ("What?", Reply::Repeat),
            ("stop", Reply::Stop),
            ("quiet", Reply::Stop),
            ("be quiet", Reply::Stop),
            ("silence", Reply::Stop),
            ("shut up", Reply::Stop),
            ("Enough.", Reply::Stop),
            ("stop talking", Reply::Stop),
            ("status", Reply::Status),
            ("what's it doing?", Reply::Status),
            ("what is it doing", Reply::Status),
            ("what's running", Reply::Status),
            ("what is running", Reply::Status),
            ("what's happening", Reply::Status),
            ("Update.", Reply::Status),
            ("that's all", Reply::End),
            ("that is all", Reply::End),
            ("end", Reply::End),
            ("goodbye", Reply::End),
            ("Bye!", Reply::End),
            ("done", Reply::End),
            ("never mind", Reply::End),
            ("nothing", Reply::End),
        ];
        for (text, expected) in cases {
            assert_eq!(parse(text), *expected, "for {text:?}");
        }
    }

    #[test]
    fn curly_apostrophes_match() {
        assert_eq!(parse("Don\u{2019}t."), Reply::No);
        assert_eq!(parse("That\u{2019}s all"), Reply::End);
    }

    #[test]
    fn longer_utterances_are_other_with_original_text() {
        assert_eq!(
            parse("Yes, but change the file first"),
            Reply::Other("Yes, but change the file first".into())
        );
        assert_eq!(parse("Read it out loud"), Reply::Other("Read it out loud".into()));
        assert_eq!(parse("  Fix the tests please  "), Reply::Other("Fix the tests please".into()));
    }

    #[test]
    fn empty_input_is_empty_other() {
        assert_eq!(parse(""), Reply::Other(String::new()));
        assert_eq!(parse("   "), Reply::Other(String::new()));
    }

    #[test]
    fn filler_only_utterances_are_other() {
        assert_eq!(parse("ok"), Reply::Other("ok".into()));
        assert_eq!(parse("Thanks"), Reply::Other("Thanks".into()));
        assert_eq!(parse("Please."), Reply::Other("Please.".into()));
        assert_eq!(parse("Sir"), Reply::Other("Sir".into()));
    }

    #[test]
    fn punctuation_is_ignored_inside_the_phrase() {
        assert_eq!(parse("Read... it!"), Reply::Read);
        assert_eq!(parse("\"yes\""), Reply::Yes);
    }

    #[test]
    fn address_is_stripped_with_or_without_comma() {
        assert_eq!(parse("Yes sir"), Reply::Yes);
        assert_eq!(parse("Yes, ma'am."), Reply::Yes);
        assert_eq!(parse("Yes, Captain."), Reply::Other("Yes, Captain.".into()));
    }

    #[test]
    fn configured_address_is_stripped() {
        let mut v = voice();
        v.address = "Captain".into();
        assert_eq!(parse_reply("Yes, Captain.", &v), Reply::Yes);
        assert_eq!(parse_reply("Read it, captain", &v), Reply::Read);
    }

    #[test]
    fn empty_address_strips_nothing_extra() {
        let mut v = voice();
        v.address = String::new();
        assert_eq!(parse_reply("Yes, sir.", &v), Reply::Yes);
        assert_eq!(parse_reply("Read it, Captain", &v), Reply::Other("Read it, Captain".into()));
    }

    #[test]
    fn disabled_switch_becomes_other_with_original_text() {
        let mut v = voice();
        v.cmd_yes = false;
        assert_eq!(parse_reply("  Yes. ", &v), Reply::Other("Yes.".into()));
        v = voice();
        v.cmd_stop = false;
        assert_eq!(parse_reply("stop", &v), Reply::Other("stop".into()));
        assert_eq!(parse_reply("Please stop.", &v), Reply::Other("Please stop.".into()));
        v = voice();
        v.cmd_status = false;
        assert_eq!(parse_reply("Status", &v), Reply::Other("Status".into()));
    }

    #[test]
    fn each_switch_controls_only_its_own_reply() {
        let mut v = voice();
        v.cmd_read = false;
        v.cmd_end = false;
        assert_eq!(parse_reply("read it", &v), Reply::Other("read it".into()));
        assert_eq!(parse_reply("bye", &v), Reply::Other("bye".into()));
        assert_eq!(parse_reply("more", &v), Reply::More);
        assert_eq!(parse_reply("no", &v), Reply::No);
    }
}
