# me-voice

Offline voice-to-text dictation. Press a shortcut, talk, and the text is pasted where your cursor is.
Speech is transcribed on your device with downloaded [FUTO Whisper ACFT](https://keyboard.futo.tech/voice-input-models) models.

Dictation audio and text are not sent to me-voice servers. Optional AI clean-up and Command Mode send dictated text to the provider you choose: a local server, a remote server you explicitly allow, or Anthropic through the Claude CLI. If screen context is enabled, that context is sent to a local AI server; it is sent to Claude only when **Send context to Claude** is enabled. me-voice also contacts GitHub to check for updates when automatic updates are on or you check manually, and downloads speech models from their configured hosts.

When it isn't dictating, the app sleeps: it measured **0 ms CPU over 10 seconds and about 3 MB of RAM** on Windows.

## Installing

Run `me-voice-setup-<version>.exe` (build it with `.\scripts\build.ps1 -Installer`, see below).

- Installs for your user only, so no admin rights are needed. It goes to `%LOCALAPPDATA%\Programs\me-voice`.
- Adds Start Menu shortcuts for **me-voice** and **me-voice settings**.
- Optional: start when you sign in to Windows, and a desktop shortcut.
- The speech model isn't included, which keeps the installer at about 5 MB. On first start the settings window opens: click **Download** on the **Speech** page.
- The app used to be called **mc-voice**. Installing me-voice removes the old install, its shortcuts and its start-at-login entry, and on first start your settings, history and models move from the `mc-voice` folders to the `me-voice` ones.
- Updates: me-voice checks GitHub a minute after it starts and then every 6 hours. A new release is downloaded and installed in the background, with a notification, and the app restarts by itself (never while you're dictating). Use **Check for updates…** in the tray menu to check right away, or turn it off with **Automatic updates** in Settings.
- To uninstall, use Windows **Settings → Apps → Installed apps → me-voice**. Your settings and downloaded models are kept. Delete `%APPDATA%\me-voice` and `%LOCALAPPDATA%\me-voice` to remove them too.

## Using it

| Action | How |
|---|---|
| Start dictating | Press **F5** (tray icon dot turns red) |
| Stop and paste | **Tap** the shortcut again, or **hold** it while talking and let go (depends on **How it works**, see below) |
| Dictate with a profile | Press an extra shortcut set to **Dictate with profile** (Shortcut page) |
| Paste the last dictation | Press an extra shortcut set to **Paste last dictation** |
| Fix the last dictation | Press an extra shortcut set to **Fix last dictation** |
| Dictate a paragraph | Press an extra shortcut set to **Dictate paragraph**: a window opens that shows everything you say (see below) |
| Cancel | **Esc** while recording |
| Settings | Click the tray icon, or right-click → **Settings…** |
| Quit | Right-click the tray icon → **Quit** (closes everything, including the settings window) |

Tray icon dot: orange = ready, red = listening, yellow = transcribing.

While listening you hear a short rising beep and a small card appears at the top of the screen. Its top line shows the status (LISTENING, TRANSCRIBING, PASTED) with a live sound wave; below it are the words as they are recognised, then the final text for a moment after it's pasted. A falling beep means it stopped. The pill never takes focus, so the text still goes into your app, and clicks pass through it. Turn the beeps off under **Behaviour → Sounds**.

Only one me-voice runs at a time: starting it again opens the settings window, and opening settings again brings the existing window to the front. (The settings window is its own small process that exists only while it's open; Quit closes it too.)

On first start, if the model isn't there yet, the settings window opens. Download it on the **Speech** page.

### Dictating a paragraph

Add an extra shortcut with the action **Dictate paragraph** (Shortcut page). Pressing it opens a window and starts listening. You see two things: **Your paragraph** (the finished lines, editable) and **Saying now** (the words as you speak them).

Say a sentence, then lock it in, and it is transcribed and added under the previous line:

- say **"new line"** (or "nieuwe regel") at the end of the sentence,
- or pause for the set time (default 1.5 s, change or switch off with the slider),
- or press the paragraph shortcut again, or click **New line**.

Click **Paste** (or Ctrl+Enter) to paste the whole paragraph where your cursor was, **Copy** to put it on the clipboard, **Clear** to start over, **Esc** or **Cancel** to throw it away. Saying only "scratch that" removes the last line; ending a sentence with "stop" or "send" pastes.

### Settings window

A menu on the left with these pages:

- **Dashboard**: how much faster than typing you are, speaking time vs. typing time, time saved, words captured, sessions, average words per minute, words per session a 30-day trend chart (hover for a day's count) and where you dictate (words per program over 30 days; only the program name is stored, never window titles). Set your typing speed here (default 40 wpm); typing time is your words at that speed.
- **History**: every dictation with date, word count and length, newest first. Search, copy a dictation, or clear the history. Switch off "Keep the text" to store only the numbers.
- **Shortcut**: dictation shortcut, how it works, cancel key, hold threshold. Click the shortcut button and hold your keys: the button shows live which keys it sees (e.g. `Ctrl + Alt + …`), and the shortcut is set when you press the last, non-modifier key.
  - **How it works**: *Hold or tap* (default) starts on press; a quick tap keeps listening until you tap again, and holding it while talking stops when you let go. *Hold only* listens while the key is held and stops when you let go. *Tap only* toggles on each press and ignores releases. The hold threshold applies to *Hold or tap* only.
  - **More shortcuts**: add extra shortcuts that work like the dictation shortcut but do something else: *Dictate with profile* dictates with the profile you name, *Paste last dictation* pastes the previous text again, and *Fix last dictation* opens the fix window. A shortcut that equals the dictation shortcut or another extra one shows a warning and should be changed.
- **Speech**: microphone (or the system default), model, download status, language, and the **live preview model**. If the chosen microphone is unplugged, the system default is used.
- **Behaviour**: paste via clipboard (default) or type out; free memory after N minutes idle; beeps; start at login.
- **Words**: add, edit and remove snippets right in the window; dictionary words (one per line); filler-word removal; spoken formatting.
- **About**: version, plus buttons to open the models and settings folders.

The window can be resized; on narrow widths each setting's control moves under its description. Click **Save** (or press **Ctrl+S**) to apply. The running app picks up changes immediately.

### Words: dictionary, snippets and clean-up

Edit snippets and dictionary words on the **Words** page. They are stored in the settings folder (`%APPDATA%\me-voice\config` on Windows), so you can also edit the files:

- `dictionary.json`: names and terms to spell exactly, plus replacements.
  ```json
  { "words": ["Kubernetes", "Anna"], "replacements": { "cuber netties": "Kubernetes" } }
  ```
- `snippets.json`: say the phrase, get the text. If the whole dictation is the phrase, only the snippet is inserted.
  ```json
  { "my sign off": "Kind regards,\nAlex" }
  ```
- `commands.json`: your own voice commands. Say the phrase anywhere in a dictation and the text is inserted instead. Use `\n` for a line break.
  ```json
  { "my address": "Main Street 1\nAmsterdam" }
  ```
- **Filler words** ("um", "uh", repeated words) are removed.
- **Spoken formatting** (English, or Dutch when the language is Dutch) becomes the real thing:
  - "new line" / "nieuwe regel", "new paragraph" / "nieuwe alinea"
  - "full stop" / "punt", "comma" / "komma", "question mark" / "vraagteken", "exclamation mark" / "uitroepteken", "colon" / "dubbele punt"
  - "bullet point" / "opsommingsteken" starts a new line with "- "
  - "open quote" … "close quote" / "aanhalingsteken openen" … "aanhalingsteken sluiten" put the words in quotes
  - "all caps" word / "hoofdletters" word capitalises the next word
- **Whole-dictation commands**: saying only "scratch that" / "weg ermee" undoes the previous paste (Ctrl+Z) and pastes nothing. "send" / "enter" and "stop" at the end are listed on the same table.

### History and dashboard data

Each dictation is added to `%LOCALAPPDATA%\me-voice\data\history.jsonl` (time, length, word count and, unless switched off, the text), plus the name of the program it went into (e.g. `ms-teams.exe`, never the window title). It never leaves your computer.

### Test switches (environment variables)

For development and speed testing; none of these are needed for normal use.

| Variable | Effect |
|---|---|
| `ME_VOICE_HOME` | Keep settings, models, history and log in this folder |
| `ME_VOICE_DEV=1` / `0` | Force the dev instance on or off |
| `ME_VOICE_TEST_WAV` | Play this 16 kHz WAV as if it were the microphone |
| `ME_VOICE_NO_INSERT=1` | Don't paste (for automated tests) |
| `ME_VOICE_MODEL` | Model id for `transcribe` |
| `ME_VOICE_THREADS` | Number of CPU threads |
| `ME_VOICE_FLASH=1` | Flash attention (measured slower on CPU) |
| `ME_VOICE_FALLBACK=1` | Whisper's temperature fallback (off: it re-decodes and stumbles) |
| `ME_VOICE_SCREENSHOT` | Settings window only: save it to this `.bmp` file once it has drawn, then close (with `ME_VOICE_PAGE=speech` etc. to pick the page) |

### Portable or test setup

Set `ME_VOICE_HOME` to a folder to keep settings, models, history and the log in that folder instead of the standard locations.

## Troubleshooting

The app writes a short log to `%LOCALAPPDATA%\me-voice\data\me-voice.log`. It records whether the shortcut was registered, every shortcut press and release, when listening started, and how many characters were transcribed. The log never contains the transcribed text.

## Building

### Windows (one-time setup)

You need:

1. **Rust**: `winget install --id Rustlang.Rustup -e`
2. **Visual Studio** or **Build Tools** with the "Desktop development with C++" workload
3. **CMake**: `winget install --id Kitware.CMake -e`
4. **LLVM** (for libclang): `winget install --id LLVM.LLVM -e`

Restart your terminal (or VS Code) after installing.

### Build and run

Always build through `scripts\build.ps1`. It loads the Visual Studio build environment that whisper.cpp needs, and passes the compiler optimisation flags (`/O2`) that the whisper build otherwise drops on Windows. A release built with plain `cargo build --release` transcribes 5-10x slower.

```powershell
.\scripts\build.ps1             # debug build   -> target\debug\me-voice.exe
.\scripts\build.ps1 -Release    # release build -> target\release\me-voice.exe (about 9 MB, no console window)
.\scripts\build.ps1 -Test       # unit tests
.\scripts\build.ps1 -Installer  # release build + installer -> installer\output\me-voice-setup-<version>.exe
```

The installer needs [Inno Setup](https://jrsoftware.org/isinfo.php): `winget install --id JRSoftware.InnoSetup -e`.
The installer script is [`installer/me-voice.iss`](installer/me-voice.iss). The version comes from `Cargo.toml`.

The first build takes several minutes because it compiles whisper.cpp. Later builds are quick.

Run it:

```powershell
.\target\release\me-voice.exe
```

### Making a release

A release is the optimised app plus the installer you give to others (or install yourself).

1. **Bump the version and write the release notes:**
   ```powershell
   .\scripts\release.ps1 -Bump minor    # or major / patch
   ```
   This updates `Cargo.toml` and `Cargo.lock` and adds a section to [`CHANGELOG.md`](CHANGELOG.md) from the commit subjects since the last `v*` tag. Edit the notes, commit, then tag the release (`git tag v<version>`) so the next run only lists newer commits. `.\scripts\release.ps1 -Notes` just prints the notes. The installer file name and the version shown under Settings → About both come from `Cargo.toml`.
2. **Run the tests:**
   ```powershell
   .\scripts\build.ps1 -Test
   ```
3. **Close me-voice** if it's running from `target\release` (right-click the tray icon → **Quit**, or `me-voice quit`). Otherwise Windows locks the exe and the build fails with "Access is denied".
4. **Build the release and the installer:**
   ```powershell
   .\scripts\build.ps1 -Installer
   ```
   This makes an optimised build (link-time optimisation, debug symbols stripped, no console window) and then the installer.
5. **The result:**
   - `target\release\me-voice.exe`: the app itself (about 9 MB). It runs on its own; the model downloads on first start.
   - `installer\output\me-voice-setup-<version>.exe`: the installer (about 5 MB) to share or run.
6. **Check it:** run the installer, start me-voice from the Start Menu and dictate a sentence. In the settings window there must be no **DEV** label; that label means a debug build was used.

Notes:

- Release builds are the normal single instance. Debug builds (`.\scripts\build.ps1` without `-Release`) run as the separate dev instance, so don't share those.
- `.\scripts\build.ps1 -Release` builds only the exe, without the installer.
- Neither the exe nor the installer is code-signed yet, so Windows SmartScreen may warn on first run ("More info" → "Run anyway").

### In VS Code

Open the folder and install the recommended extensions when asked (rust-analyzer, Even Better TOML, CodeLLDB).
Then use **Terminal → Run Task…**:

- `me-voice: build (debug)` (also **Ctrl+Shift+B**)
- `me-voice: build (release)`
- `me-voice: build installer` (the release build plus the installer, see [Making a release](#making-a-release))
- `me-voice: test`
- `me-voice: run (debug)`
- `me-voice: open settings window`

If rust-analyzer reports errors in `whisper-rs-sys`, run the debug build task once and reload the window.

### Development instance

Debug builds (`.\scripts\build.ps1`) run as a separate **dev** instance, so you can test next to the installed app without the two getting in each other's way. The dev instance has its own settings, history, log and lock (folders ending in `-dev`), its own sockets, and shows **DEV** in the settings window. Models are shared, so nothing is downloaded twice.

- `me-voice.exe --dev` runs a release build as the dev instance.
- `ME_VOICE_DEV=0` runs a debug build as the normal instance; `ME_VOICE_DEV=1` forces dev.
- Give the dev instance a different shortcut (Shortcut page), or the two will compete for the same keys.

### Command-line options

```
me-voice                       start the tray app (a second start opens settings)
me-voice settings              open the settings window
me-voice toggle                start/stop dictation in the running app (for desktop shortcuts)
me-voice quit                  quit the running app (same as Quit in the tray menu)
me-voice microphones           list the available microphones
me-voice transcribe file.wav   transcribe a 16 kHz WAV and print the result and timing
me-voice live-test file.wav    play a 16 kHz WAV through the live preview and voice commands, printing each update
```

### macOS and Linux

The code is cross-platform, but so far it's only tested on Windows. The macOS build script has not been run on a Mac yet.

- **macOS**: install Xcode command-line tools, Rust, CMake and LLVM (`brew install cmake llvm`; optionally `librsvg` for the app icon), then run [`build-macos.sh`](scripts/build-macos.sh) on a Mac:
  ```bash
  ./scripts/build-macos.sh              # release build -> dist/me-voice.app
  ./scripts/build-macos.sh --dmg        # also installer/output/me-voice-<version>.dmg
  ./scripts/build-macos.sh --test       # unit tests
  ```
  Apple silicon only: Intel Macs aren't supported, because the ONNX Runtime crate has no prebuilt binary for them. The app is a menu-bar app (no Dock icon) with an ad-hoc signature, not a Developer ID one, so on first open use right-click → **Open**. Allow **Microphone** and **Accessibility** access when asked. The bundle metadata is in [`packaging/macos/Info.plist`](packaging/macos/Info.plist).
- **Linux**: install CMake, clang/libclang, and the ALSA, GTK and xdotool development packages.
  - X11 works.
  - On **Wayland**, global shortcuts are blocked. Bind a shortcut in your desktop settings to `me-voice toggle` instead.
- **WSL (Ubuntu)**: builds the Linux version, not the Mac one. The macOS app and `.dmg` need Apple's tools, so run `scripts/build-macos.sh` on a Mac (or a macOS CI runner). In WSL:
  ```bash
  sudo apt update
  sudo apt install -y build-essential cmake clang libclang-dev pkg-config libasound2-dev libgtk-3-dev libxdo-dev libxkbcommon-dev libwayland-dev libx11-dev libayatana-appindicator3-dev libssl-dev
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && . "$HOME/.cargo/env"

  cd /path/to/repo   # adjust to where you cloned the repository
  export LIBCLANG_PATH=$(dirname "$(find /usr/lib -name 'libclang*.so*' | head -1)")
  export BINDGEN_EXTRA_CLANG_ARGS=""
  CARGO_TARGET_DIR=$HOME/me-voice-target cargo build --release   # -> ~/me-voice-target/release/me-voice
  ```
  If `apt` says a package has no installation candidate, run `sudo apt update` first. `CARGO_TARGET_DIR` keeps Linux files out of the Windows `target\` folder and is faster than building on `/mnt/c`. The tray icon and shortcut only work if WSL has a display (WSLg).
- On Linux, set `LIBCLANG_PATH` and `BINDGEN_EXTRA_CLANG_ARGS` in your environment (they override the Windows paths in `.cargo/config.toml`) and build with `cargo build --release`.

## Speed

Measured on an Intel Core Ultra 7 265H (CPU only), 20 seconds of speech:

| Model | Transcription | Memory |
|---|---|---|
| English 244 (default, most accurate) | 1.3 s | 252 MB |
| English 74 | 0.4 s | 78 MB |
| English 39 | 0.2 s | 42 MB |

The **live preview** (the words in the card while you talk) uses the small English 39 model by default, so it updates about three times a second at 30-90 ms per update, while the pasted text still comes from the main model. Change both on the **Speech** page. Models load on first use and are unloaded after the idle time set under **Behaviour**; while idle the app uses about 3 MB and no CPU.

## Models and languages

The available models are listed in [`models.json`](models.json). The default is **English 244** (`futo-en-244`).

**To switch model or language:** open Settings → **Speech**, click a model card (**Download** first if it isn't ready), choose the language (**Auto**, **English**, **Nederlands**, or more under *More languages*), and **Save**.

There are two engines:

- **Whisper** (whisper.cpp): English and multilingual models. English-only models always transcribe as English.
- **Parakeet TDT 0.6B v3** (NVIDIA, int8 ONNX): 25 European languages with automatic language detection. Choose **Auto** to let it detect the language. It is a folder of four files (about 640 MB), downloaded together.

**To add a model:** copy `models.json` to the settings folder and add an entry:

```json
{
  "id": "my-model",
  "name": "My model",
  "url": "https://example.com/model.bin",
  "file": "my-model.bin",
  "sha256": null,
  "languages": ["en"],
  "acft": false
}
```

- The model must be a whisper.cpp (GGML) `.bin` file, unless `"engine": "parakeet"` is set. A Parakeet model is a folder: list its files in `"files"` (each with `url`, `file` and `sha256`) instead of `url`.
- `"sizeMb"` is the approximate download size shown on the Speech page.
- Set `"acft": true` only for ACFT-finetuned models. Those can skip Whisper's 30-second padding, which makes short dictations much faster.
- `sha256` is checked after download when it's set.

Models are stored in `%LOCALAPPDATA%\me-voice\data\models` on Windows.

## Project layout

```
models.json          built-in model list
scripts/
  build.ps1          Windows build script
  build-macos.sh     macOS build script
  release.ps1        version bump and changelog
installer/           Inno Setup script (Windows)
packaging/           macOS app bundle files
plan/                design explorations and research
src/
  main.rs            command-line entry point
  settings.rs        settings.json
  app/               the tray app
    mod.rs           event loop: shortcut → record → transcribe → paste
    dictation.rs     transcribing a recording, AI polish
    tray.rs          tray icon and menu
    ipc.rs           messages between app, settings window and `toggle`
    instance.rs      single-instance lock
    power.rs         process priority, power throttling
    log.rs           log file
    update.rs        updates from GitHub releases
  speech/            sound in, text out
    audio.rs         microphone capture (open only while recording)
    transcriber.rs   whisper.cpp wrapper, audio_ctx, idle unload
    models.rs        model list, download, checksum
    sound.rs         start/stop beeps
  text/              what happens to the text
    cleanup.rs       dictionary, snippets, filler words, spoken formatting
    polish.rs        AI clean-up
    context.rs       screen and clipboard context for the AI
    insert.rs        paste via clipboard (restores it) or type out
    foreground.rs    the app in front
    history.rs       dictation history and dashboard numbers
    profiles.rs      per-app profiles
  ui/                windows
    settings/        settings window (egui), one file per page
    theme.rs         look of the settings window: fonts, colours, cards, icons
    pages.rs         Dashboard and History pages
    overlay.rs       the listening pill at the top of the screen
    paragraph_window.rs  the "Dictate paragraph" window (own process: me-voice paragraph)
    fix_window.rs    the "Fix last dictation" window
    text_menu.rs     right-click menu for text fields
    window_state.rs  remembered window positions
    icon.rs          app icon
```

## Licences

- The app source is licensed under [MIT](LICENSE). Dependencies and model files have their own licenses and terms.
- The models are OpenAI Whisper finetuned by FUTO. OpenAI's Whisper weights are MIT, and FUTO's whisper-acft code is MIT. The model files themselves don't state a licence, so check with FUTO before redistributing them.
