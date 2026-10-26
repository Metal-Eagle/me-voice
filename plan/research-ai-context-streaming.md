# me-voice: research plan for AI clean-up, context and streaming (October 2026)

Three features from the gap list:

- **A.** AI clean-up and Command Mode, using a local model (Ollama) or the Claude CLI.
- **B.** Context-aware recognition.
- **C.** Text that streams into the app while you speak.

For each one this file covers how it could fit the current code, the options, a recommendation, and the questions to answer with spikes before building.

Where it hooks in today:
- `app::transcribe` (`src/app.rs`): raw Whisper/Parakeet text → `cleanup::clean` → `insert::insert`.
- `Dictionary::prompt()` (`src/cleanup.rs`): the dictionary words, passed to Whisper as `initial_prompt`.
- `foreground::app_name()`: the exe name, already used by profiles.
- The live preview (`Partial`, every 0.3 s over the last 10 s) and the paragraph window's sentence lock.

---

## A. AI clean-up and Command Mode

### What it does
- **Polish**: after `cleanup::clean`, an LLM fixes grammar and punctuation and sets the tone. A profile can give a different instruction per app ("casual, no full stop" for Teams, "formal" for Outlook).
- **Command Mode**: a new extra-shortcut action. It copies the selected text, you say an instruction ("make this shorter", "translate to Dutch"), and the result replaces the selection.

### Provider design
One small trait, so the rest of the app doesn't care which provider is used:

```rust
trait Polisher { fn run(&self, system: &str, user: &str, timeout: Duration) -> Result<String>; }
```

| Provider | How | Pros | Cons |
|---|---|---|---|
| **Ollama / LM Studio / llama.cpp server** | HTTP POST to `http://localhost:11434/v1/chat/completions` (OpenAI-compatible; Ollama's own API is `/api/chat`). `ureq` is already a dependency. | Fully local, so the privacy promise holds. Fast with small models (Qwen 2.5/3 3B–7B, Llama 3.2 3B, Gemma). One code path covers every OpenAI-compatible server. | The user has to install and run Ollama and pull a model. Quality depends on the model. |
| **Claude CLI** (`claude -p`) | Start `claude -p --output-format json --model <haiku/sonnet> --system-prompt "…"` with the transcript on stdin. Turn tools off (`--tools ""` / `--disallowedTools`), use `--max-turns 1`, and run it in an empty working folder so it can't read a project. | Uses the user's existing Claude login or subscription, with no API key stored in me-voice. Best quality. | **The text goes to the cloud**: it has to be clearly opt-in and shown in the pill. The process start is slow (likely 2–5 s), which needs measuring. It depends on the installed CLI version and its flags. |
| *(later)* Anthropic API directly | HTTPS with an API key | Faster than starting the CLI | Needs a stored key, and the code rules here don't allow entering keys. Leave it out for now. |

Recommendation: build **OpenAI-compatible HTTP first** (it covers Ollama, LM Studio and llama.cpp), then **Claude CLI as a second provider**. Both go behind one setting: *AI provider: Off / Local server / Claude CLI*.

### Rules to keep me-voice me-voice
- Off by default. Nothing runs at idle: an HTTP request or a child process is started only per dictation, so idle stays at 0 CPU / 3 MB.
- The local provider refuses non-localhost URLs unless an "allow remote" box is ticked.
- The pill shows *POLISHING* (local) or *POLISHING · CLAUDE* (cloud). History stores both the raw and the polished text.
- **Timeout plus fallback**: if the LLM takes longer than N seconds (default 4 s local, 10 s Claude) or fails, paste the cleaned raw text. Never lose a dictation.
- Skip polishing for snippets, voice commands and very short dictations (fewer than 4 words).
- System prompt: "Return only the corrected text. Do not answer questions in the text. Do not add content." Then a post-check: if the output is more than about 1.5× longer than the input, or starts with "Sure"/"Here is", use the raw text. That stops the model *answering* the dictation instead of fixing it.

### Where in the code
- New `src/polish.rs` (trait, the HTTP provider, the CLI provider, the prompt builder).
- `settings.rs`: `ai_provider`, `ai_url`, `ai_model`, `ai_timeout`, plus `ProfileOverrides.ai_style` / `ai_enabled`.
- `app.rs`: call it after `transcribe` on the worker thread, before `insert`. Add a new `ShortcutAction::Command`.
- `insert.rs`: `copy_selection()` (Ctrl+C with the old clipboard saved and restored), which Command Mode needs.
- Settings UI: a new **AI** page with provider, URL/model, a **Test** button and a timing readout.

### Spikes (half a day each)
1. Time an Ollama round trip with `qwen2.5:3b` and `llama3.2:3b` on a typical laptop CPU, for 30- and 100-word inputs. The goal is under 1.5 s.
2. Time a `claude -p` cold start on Windows with tools off, and check which flags the installed version really supports (`claude --help`). Check that it works without a TTY from a process with no console (use `CREATE_NO_WINDOW` so no console flashes up).
3. Build a small set of about 30 real dictations from History, compare raw, Ollama and Claude, and count how often the model "answers" or changes the meaning.

---

## B. Context-aware recognition

### Idea
Before transcribing, collect a few hundred characters of context and use it in two places:
1. **Whisper `initial_prompt`**, already used for dictionary words. Adding names and terms from the screen makes the model spell them right. The limit is about 224 tokens, so the context has to be short and ranked.
2. **The LLM prompt** (feature A), where it can be much longer.

Parakeet has no prompt input, so for Parakeet the context only helps via the LLM (or a later hotword-boosting option).

### Sources (Windows first), cheapest first
| Source | API | Notes |
|---|---|---|
| App name | `foreground::app_name()` | Already there |
| Window title | `GetWindowTextW` | Chat or document name, recipient. The README promises titles are never *stored*: keep that, use the title only in memory. |
| Text near the cursor / selected text | **UI Automation** (`IUIAutomation::GetFocusedElement` → `TextPattern` / `ValuePattern`), via the `windows` crate | Best signal: the email you're replying to, the line you're editing. Works in Edge/Chrome (for accessible pages), Office, Teams and most native apps. Some Electron apps and terminals give nothing. |
| Clipboard text | `arboard` | Opt-in only (it can hold passwords). |
| Screen OCR | `Windows.Media.Ocr` (built into Windows) | A fallback when UIA has nothing. It costs more (about 100–300 ms) and is the most sensitive, so leave it opt-in, or for later. |

### Turning context into a prompt
- Pull out **proper nouns and rare words** (capitalised words, CamelCase, words with digits, and words not in a small common-word list), then put them first, followed by the dictionary words, trimmed to the token limit. Don't put whole sentences into Whisper's prompt: a long prompt makes Whisper copy its style and sometimes repeat it.
- Collect it **when recording starts**, on a background thread, so it's ready before the final transcription without delaying the start beep. Only use it for the final pass, not the live preview.

### Privacy
Context is never written to history or the log. There's a setting with three levels: *Off / App + window title / + text near cursor* (and separate boxes for clipboard and OCR). The Claude provider only gets context when the user has ticked "send context to Claude".

### Spikes
1. A UIA probe tool (`me-voice context` CLI command) that prints what it reads in Outlook, Teams, Chrome, Word, VS Code, Slack and Windows Terminal. That gives a coverage table.
2. Accuracy test: 20 dictations with names from the screen, with and without the context prompt, using the existing `ME_VOICE_TEST_WAV` switch. Measure name accuracy and check for prompt-copying problems.
3. Measure the UIA call time. It must stay under about 50 ms, or run fully in the background.

---

## C. Streaming text into the app while you speak

### The core problem
Live partial text changes as more audio comes in: Whisper revises earlier words. Typing into someone else's app can't be taken back cleanly, because backspacing is risky if the user clicks somewhere else, and many apps autocomplete or send on Enter.

### Options
| Option | How | Verdict |
|---|---|---|
| 1. **Commit stable words** (LocalAgreement, as in `whisper_streaming`) | Each partial pass is compared with the previous one. Words that are the same in 2 passes in a row and are older than about 1 s count as *confirmed* and get typed (`InsertMode::Type`). The unconfirmed tail stays in the overlay card only. When you stop, the final pass types only the part after what was already confirmed. | **Recommended for v1.** No backspacing, works in every app, and reuses the live preview loop. |
| 2. Commit per sentence | Like the paragraph window: a pause or "new line" locks the sentence in, it is fully transcribed and cleaned, and then pasted. | Easy, and quality is the same as today. Text comes in sentence by sentence, not word by word. **A good first step** that can share code with `paragraph_window.rs`. |
| 3. Type everything and correct with backspace | Diff the old and new partial text and send Backspace × n plus the new text | Fragile: it breaks when focus moves, with autocomplete, and with rich editors. Leave it out. |
| 4. **TSF / IME composition** | me-voice becomes a Windows Text Services Framework text input processor. The unconfirmed text shows as underlined "composition" text inside the app, can be replaced freely, and is committed at the end. That's how Windows Voice Typing does it. | The proper solution, but large: a COM in-process DLL registered as an input method, with a different setup and a different security picture. Research only, maybe v2. |

### Effects on other features
- **Clean-up**: filler removal and spoken formatting have to run per committed chunk. "new line" and "full stop" are fine. "open quote … close quote", "all caps" and snippets need the whole text, so in streaming mode keep the chunk open until the phrase is complete, or switch those off.
- **AI polish (A)** can't change text that's already typed. Streaming and polish are either/or per profile.
- **Paste mode** doesn't fit word-by-word streaming (it would fill the clipboard many times), so streaming uses Type mode. A sentence-by-sentence stream (option 2) can still paste.
- **Cost**: partial passes already run while recording. The light model makes the confirmed words less accurate, so streaming should run the partial passes on the *main* model over a sliding window, which needs measuring on CPU. Feature 6 (GPU) helps a lot here.
- **Safety**: stop streaming if the foreground window changes during the dictation (`GetForegroundWindow` check before each chunk). Keep the rest in the card and paste it only when you confirm.

### Spikes
1. Run LocalAgreement-2 offline on test WAVs: for 2 / 3 matching passes, measure how far confirmed text lags behind speech and how many confirmed words were wrong, compared with the final transcript.
2. CPU load with the main 244 model doing partial passes every 0.5 s. Can a laptop keep up in real time?
3. Typing test of `enigo.text()` chunks in Word, Teams, Chrome, Notepad and the terminal: do characters get lost or come out of order with long chunks?

---

## Suggested order
1. **A, local HTTP provider + Polish**, then **Command Mode**. This gives the most value and the smallest risk.
2. **B, app + title + UIA text near the cursor**, feeding both the Whisper prompt and the LLM prompt.
3. **A, Claude CLI provider**, after the cold-start spike.
4. **C, option 2 (sentence by sentence)**, then **option 1 (stable words)** if the spikes look good. Keep TSF as a long-term research item.

## Open questions for you
- Is it OK that the Claude CLI option sends dictated text (and maybe context) to the cloud, as long as it's clearly labelled and opt-in?
- Should Command Mode also work with no selection (speak a prompt, insert the answer), or only edit selected text?
- For streaming: is sentence by sentence good enough to start with, or is word by word the point?
