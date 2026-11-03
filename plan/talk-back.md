# Talk back: text-to-speech and voice conversations

## Persona: a computer assistant you talk with (English only)

The status line speaks as a calm, polite AI butler in the style of a sci-fi computer
assistant. It has its own name (working name **"Max"**, changeable in settings, and saying it is
the wake word), not a film character's name or voice.

- **English only:** the phrases, voice and voice commands. Pick an English system voice
  (only as fallback until Kokoro is downloaded: an English system voice such as "Microsoft Aria"). The neural voice
  is Kokoro's Heart voice (`af_heart`), see Engine.
- **Tone:** short, dry, respectful. It calls you "sir" (setting: sir / ma'am / your name / nothing).
- **Phrase sets** with a few variants each, picked at random so it doesn't sound like a loop:

| Moment | Examples |
|---|---|
| Sent | "Sent, sir." · "Passed along to *me-voice*." · "Done. It's on its way." |
| New chat | "New session started in *T3 Code*." |
| Working | "Working on it." · "The agent's on it now." |
| Still working | "Still at it, sir." · "Bear with it. Still working." |
| Needs you | "It's asking permission to run a command. Shall I allow it?" |
| Question | "It has a question for you, sir." (then it reads the question) |
| Done | "Finished, sir. Shall I read you the summary?" |
| Error | "I'm afraid T3 Code isn't running." · "That didn't go through, sir." |
| Greeting (start of a conversation) | "Good morning, sir. What are we working on?" (by time of day) |

- **Talking back:** after it speaks it listens for a few seconds. "Yes / go ahead / allow it",
  "no / deny", "read it", "stop / quiet", "thanks, that's all" (end), or anything else, which is
  sent as the next message to the same thread. "Max, ..." starts a conversation at any time.
- Phrases live in one table in `src/speech/phrases.rs`, so the persona is easy to change.
- Settings (Voice page): name/wake word, form of address, voice, speed, chattiness
  (everything / only important / only done and errors).

## Decision: a separate spoken status line (comes first)

The voice is its **own line**, apart from the chat. It doesn't read the chat reply out. It tells
you what's happening, like an assistant next to you:

| Moment | It says (example) |
|---|---|
| Dictation sent to a T3 thread | "Sent to *me-voice*." |
| New thread started | "Started a new chat in *T3 Code*." |
| Agent starts working | "It's working on it." |
| Still busy after ~30 s / 2 min | "Still working." (at most every 2 minutes) |
| Agent asks a question or needs permission | "It needs your OK to run a command. Say yes or no." |
| Turn finished | "Done. Want to hear the answer?" (if you say "yes", it reads a short summary) |
| Error / send failed | "Couldn't send it, T3 Code isn't running." |

- Short fixed English phrases (see Persona), not AI text, so no extra calls and nothing leaves the PC.
- It comes from the app's own events (`t3_send` result, T3 turn events in `state.sqlite`),
  not from the reply text.
- You can answer it: after a question or "Done" it listens for a short reply ("yes", "no",
  "read it", "stop") or a new message to send to the same thread.
- Settings: status voice on/off, which moments to announce, how often "still working" repeats.
- Reading the full answer aloud (sections 2–3 below) becomes optional and comes later.

## Engine: Kokoro, locally (from the start)

Use **Kokoro-82M** (the model behind voice-generator.pages.dev) running on this PC, not the
system voice. Voice: **Heart** (`af_heart`, American English female, Kokoro's top-rated voice).

- Use the **fp32** model (`kokoro-v1.0.onnx`, full precision, ~310 MB) for generation, not the quantised one, plus the
  `voices-v1.0.bin` style vectors) through ONNX Runtime, which `transcribe-rs` already brings in.
- Text → phonemes: Kokoro needs eSpeak-style phonemes. Options, in order of preference: a Rust
  G2P crate / the `misaki`-style English lexicon bundled as data, or the `espeak-ng` library.
  Decide in a short spike (phase 1a).
- Download it on the Speech page like Whisper (entry in `models.json`, SHA-256 checked). Until
  it's downloaded, fall back to the Windows system voice so nothing breaks.
- Speak sentence by sentence; cache the fixed phrases (Sent, Working, the end phrase) as audio
  after the first time, so they play instantly.
- Settings: voice (default `af_heart`; others selectable later), speed, test button.

## End sound: Max speaks when it stops listening

When recording stops, instead of the falling beep, Max says a short line:
"Got it, sir." · "Very good." · "On it." (random, cached audio, so no delay).

- Setting under **Behaviour → Sounds → Stop sound**: **Beep** (default, the existing falling
  note in `sound.rs`) or **Voice (Max)**. Off still turns all sounds off.
- The start sound stays the rising beep.
- If Kokoro isn't downloaded yet, Voice falls back to the beep.

Revised phases:
**1a.** Kokoro spike: one sentence with `af_heart` on the fp32 model from Rust, choose the phonemizer.
**1b.** Kokoro engine + download on the Speech page + system-voice fallback.
**2.** Spoken stop sound with the Beep / Voice setting (default Beep), status phrases for
send / new thread / errors.
**3.** T3 turn watcher → "working", "still working", "done", "needs your OK".
**4.** Short spoken replies (yes/no/read it/stop), conversation mode, wake word.
**5.** Optional full read-aloud.

---

Goal: me-voice reads replies aloud (from T3 Code, Command Mode or the AI), and you answer
by voice, so you can talk with it hands-free. Stays offline by default, like the dictation does.

```
 you speak ──► Whisper (existing) ──► target: T3 thread / Command Mode / AI
      ▲                                              │
      │                                              ▼
 auto-listen ◄── TTS speaks it ◄── reply text ◄── watch for reply
 (barge-in: start talking or press the shortcut to cut it off)
```

## 1. TTS engine (`src/speech/tts.rs`)

One trait, two back ends, so the cheap one ships first:

```rust
pub trait Voice { fn speak(&self, text: &str, stop: &StopFlag) -> Result<()>; fn voices(&self) -> Vec<VoiceInfo>; }
```

| Back end | Quality | Size | Notes |
|---|---|---|---|
| **System** (Windows `Windows.Media.SpeechSynthesis` / OneCore, macOS `AVSpeechSynthesizer`) | OK | 0 MB | Offline, English voices installed already (British voices may need adding under Windows Settings → Time & language → Speech). Add `Media_SpeechSynthesis` to the `windows` features. **Phase 1.** |
| **Neural, local** (Piper or Kokoro-82M ONNX) | Good | 20–90 MB | ONNX Runtime is already in the build via `transcribe-rs`. Download through `models.json` and the Speech page, like Whisper. **Phase 1 (Kokoro).** |

- Play through `cpal` (already a dependency) so we control stopping and volume.
- Speak sentence by sentence: split the text, synthesize the next sentence while the current one plays, so the first words come out in < 300 ms.
- Before speaking, strip what sounds bad: code blocks ("code block skipped"), Markdown, URLs, file paths → file name only, long tables. Reuse the clean-up helpers in `src/text/cleanup.rs`.
- Pause the microphone while speaking (or use echo cancellation later) so Whisper doesn't transcribe its own voice.

## 2. Getting the reply ("feeding data back")

Three sources, one channel: `ReplySource -> Stream<ReplyChunk>`.

1. **T3 Code threads** (main use). After `t3_send`, watch the thread in T3's `state.sqlite`
   (same read-only connection `apps.rs` uses) for the next assistant message and for the turn
   finishing. Speak when the turn ends (or stream sentences as they arrive). Check which events
   table/projection holds assistant messages and turn-complete; fall back to the T3 CLI if it
   can read a thread.
   - Long agent turns: say only the final message, not tool calls. Optional "summary first" mode:
     speak the first paragraph, then "say *more* for the rest".
   - Permission prompts / questions from T3 (pending requests): read them out and accept
     "yes" / "no" / an answer by voice.
2. **Command Mode / AI clean-up** (`src/text/polish.rs`): the AI's answer is already in hand, speak it instead of (or as well as) pasting.
3. **Selected text / clipboard**: a "Read aloud" shortcut. Small, useful on its own, good first test of the engine.

## 3. Conversation mode (`src/app/conversation.rs`)

A state machine next to `dictation.rs`:

`Idle → Listening → Sending → WaitingForReply → Speaking → Listening …`

- Start with a new shortcut action **Talk with T3** (Shortcut page), or say "let's talk" in a T3 dictation.
- After speaking, it listens again automatically. VAD / the pause timer from paragraph mode ends your turn.
- Barge-in: pressing the shortcut or starting to talk while it speaks stops the voice and starts listening.
- Voice commands: "stop" / "be quiet" (stop speaking), "repeat", "more", "end conversation", "send" (when the paragraph timer is off).
- Ends after N seconds of silence, on Esc, or "end conversation".
- The listening card (`overlay.rs`) gets new states: THINKING (waiting for reply, with the thread name) and SPEAKING (the sentence being said, with the wave from the TTS audio).

## 4. Settings (new **Voice** page)

- Read replies aloud: off / Command Mode only / T3 too
- Engine: System / Neural (download), voice, speed, volume, test button
- Language: follow the dictation language, or fixed
- Conversation: auto-listen after reply, end after silence (s), summary-first, skip code blocks
- Stored in `settings.rs` with serde defaults so old settings files keep loading.

## 5. Phases

1. **System TTS + "Read aloud" shortcut** – engine trait, Windows back end, sentence splitter, stop. (small)
2. **Speak Command Mode answers** – hook into `polish.rs`. (small)
3. **T3 reply watcher** – find assistant messages / turn end in `state.sqlite`, tests on a copy of the DB. (medium, the riskiest bit)
4. **Conversation mode** – state machine, auto-listen, barge-in, overlay states, voice commands. (medium)
5. ~~Neural voice~~ – moved to phase 1: Kokoro is the engine from the start.
6. **Polish** – T3 permission prompts by voice, summary-first, macOS back end, echo cancellation. (later)

## Open questions

- Speak every T3 reply, or only in conversation mode? (proposal: only in conversation mode, plus the Read-aloud shortcut)
- Is a robotic-ish system voice fine for phase 1, or go straight to neural?
- Should the AI first shorten long T3 replies into a spoken summary (costs an AI call), or just read the last message?
