# me-voice: feature ideas (October 2026)

Ideas taken from similar dictation apps, filtered for what fits me-voice: offline,
private, tiny when idle (3 MB, 0 CPU), Windows first.

## What the others do

| App | Licence / platform | Ideas worth taking |
|---|---|---|
| **Handy** (cjpais/Handy) | Open source, Rust + Tauri, Win/Mac/Linux | Parakeet V3 model next to Whisper, Silero VAD to drop silence, hold-only / toggle-only shortcut modes, CLI remote control |
| **OpenWhispr** | Open source, Win/Mac/Linux | Choice of Whisper sizes or NVIDIA Parakeet, optional AI clean-up |
| **VoiceInk** | Open source (paid binary), macOS | **Power Mode**: settings per app/website (model, language, prompt, auto-send); local LLM clean-up via Ollama; on-device OCR of the active window as context |
| **Wispr Flow** | Commercial, cloud | Tone per app (formal in email, casual in chat), **Command Mode** (select text, speak an edit instruction), dictionary that learns from your corrections |
| **Chirp** | Open source, Windows | Parakeet V3 on CPU: similar accuracy to Whisper large-v3, much faster |
| **WhisperWriter**, **Whispering**, **Buzz** | Open source | Local server providers (Speaches), file transcription (Buzz) |

me-voice already has: hold/tap shortcut, live preview, snippets, dictionary,
filler removal, spoken formatting, stop/send voice commands, history, dashboard,
idle unload. That covers most of the "basic" list; the gaps are below.

## Proposed features, in order

### 1. Per-app profiles ("modes")  ·  medium effort  ·  high value
Detect the foreground window's process name (`GetForegroundWindow` +
`QueryFullProcessImageNameW`) when recording starts and apply a profile:
language, model, insert mode (some apps dislike paste), auto-Enter, filler
removal, spoken formatting, and a capitalisation/punctuation style
(e.g. no trailing full stop in Slack/Teams, lowercase in terminals).
- Files: `settings.rs` (profiles list + fallback), `app.rs` (pick profile in
  `start_recording`), new Profiles page in `settings_ui.rs`.
- Cost at idle: none; one Win32 call per dictation.

### 2. Voice activity detection (VAD)  ·  small–medium  ·  high value
Trim leading/trailing silence and long pauses before transcription (Silero via
a small ONNX model, or start with an energy gate already in `audio.rs`).
Faster transcripts, fewer Whisper hallucinations on silence ("Thank you."),
and enables **auto-stop after N seconds of silence** as an option.
- Files: `audio.rs`, `transcriber.rs`, setting on Behaviour page.

### 3. Parakeet V3 as a second engine  ·  large  ·  high value
Multilingual (25 European languages incl. Dutch), auto language detection,
fast on CPU. Add an engine trait in `transcriber.rs` with whisper.cpp and an
ONNX/Parakeet backend (Handy uses `transcribe-rs`); list it in `models.json`
with an `engine` field. Measure binary size and idle memory against today's
numbers before committing.

### 4. Learn from corrections  ·  medium  ·  medium–high
After pasting, offer "fix last dictation" from the tray/pill or a shortcut:
the user edits the text in a small window, and changed words are offered as
dictionary replacements or new dictionary words. Also: "re-paste last
dictation" shortcut (cheap and very handy when focus was wrong).
- Files: `pages.rs` (History already has the text), `cleanup.rs`, `insert.rs`.

### 5. Optional local LLM clean-up / Command Mode  ·  medium  ·  medium
Strictly opt-in and local: send the transcript to a user-configured
OpenAI-compatible endpoint on localhost (Ollama, LM Studio). Two uses:
- **Polish**: grammar, punctuation, tone; prompt can come from the per-app profile.
- **Command Mode**: second shortcut; copies the selected text, you speak an
  instruction ("make this shorter", "translate to Dutch"), the result replaces
  the selection.
Keep it off by default and show clearly in the pill when it is used. Refuse
non-localhost URLs unless the user explicitly allows them, so the "nothing
leaves your machine" promise stays true.

### 6. More voice commands  ·  small  ·  medium
Extend `cleanup::voice_command` / spoken formatting: "scratch that" (delete
last dictation's text), "all caps …", "bullet point", "open quote / close
quote", "numbered list", Dutch equivalents. Make the command list editable
on the Words page like snippets.

### 7. Shortcut modes and a second shortcut  ·  small  ·  medium
Hold-only / toggle-only option (Handy has it; avoids accidental tap-latching),
and an optional second shortcut bound to a different profile/language
(e.g. F5 = English, Shift+F5 = Dutch).

### 8. Transcribe a file / drop audio  ·  small  ·  low–medium
The `transcribe` CLI already exists. Expose it in Settings: drop a WAV/MP3/M4A
(resample with `rubato`, decode with `symphonia`), get text with copy/save.
Useful for voice memos.

### 9. Smaller polish items
- Keep the last N seconds of audio so a failed paste or wrong window can be
  re-transcribed from History.
- Dashboard: per-app word counts once profiles exist.
- Code signing for the installer (SmartScreen warning, already noted in README).
- Auto-update check (opt-in, only fetches a version file).

## Suggested order

1. VAD + auto-stop on silence (2), more voice commands (6), shortcut modes (7): quick wins.
2. Per-app profiles (1): the biggest usability gain, and it is the base for 5.
3. Re-paste / fix last dictation (4).
4. Parakeet engine (3): measure first, it changes the size story.
5. Local LLM polish and Command Mode (5), opt-in.

## Open questions for you
- Is Dutch (or other non-English) dictation a goal? That decides how early Parakeet / multilingual models come in.
- Is an optional local LLM acceptable, or should me-voice stay "Whisper only"?
- Must macOS/Linux stay in scope for new features (per-app detection is OS-specific)?

## Sources
- Handy: https://github.com/cjpais/Handy , https://betterstack.com/community/guides/ai/handy-offline/
- VoiceInk Power Mode: https://tryvoiceink.com/docs/power-mode , https://tryvoiceink.com/docs/enhancements-configuring-models
- OpenWhispr vs VoiceInk: https://openwhispr.com/compare/voiceink
- Wispr Flow overview: https://eesel.ai/blog/wispr-flow-overview , https://sidsaladi.substack.com/p/wispr-flow-101-the-complete-guide
- Open-source alternatives lists: https://openalternative.co/alternatives/superwhisper , https://alternativeto.net/software/wispr-flow/?p=5
- Chirp (Parakeet on Windows): https://github.com/whamp/chirp
- Whispering: https://hn.svelte.dev/item/44942731
