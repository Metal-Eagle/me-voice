# Changelog

## 1.2.0 - 2026-10-27

- New: a **T3 Code** page in Settings, shown only when T3 Code is installed on this computer. Switch on **Type into T3 Code** and dictation is typed straight into its prompt instead of going through the clipboard, so what you had copied stays on it.
- Internal: a build warning about an unused import is gone.

## 1.1.0 - 2026-10-27

- Snippets now live in dictionary.json next to words and replacements. An existing snippets.json is merged in automatically (kept as snippets.json.bak).
- The snippet table on Settings → Words matches the voice-commands table.

## 1.0.0 - 2026-10-26

- First public release, licensed under MIT.
- Updates no longer need a GitHub login: they come from the public repository.
- Privacy information explains optional AI providers, screen context and update checks.
- Windows and macOS release builds run the test suite before packaging.
- The macOS app uses an independent bundle identifier.

## 0.11.0 - 2026-10-23

- New: an **Updates** row on Settings → About. **Check now** looks for a new version, and **Install now** downloads and installs it.
- Updates read your GitHub login straight from the Windows Credential Manager instead of running Git.

## 0.10.0 - 2026-10-23

- New: automatic updates. me-voice checks for a new version a minute after it starts and then every 6 hours, downloads it in the background, shows a notification and installs it once you're not dictating. It restarts by itself afterwards.
- New: **Check for updates…** in the tray menu, and an **Automatic updates** switch in Settings → Behaviour.
- Updates use the GitHub login Git has saved on this computer, because the repository is private.
- Internal: the source code is grouped into folders and the build scripts moved to `scripts/`.

## 0.9.1 - 2026-10-23

- Fixed: the 0.9.0 Windows installer crashed a few seconds into dictating on processors without AVX-512. The speech engine is now built for any processor with AVX2.
- Fixed: clicking the tray icon several times opened several settings windows behind other windows. A click now brings the open settings window to the front.
- New: right-click menus. Text boxes have Cut, Copy, Paste and Select all, and selected text (such as in History) has Copy. Ctrl+C, Ctrl+X and Ctrl+V work in text boxes too.

## 0.9.0 - 2026-10-23

- New: **AI clean-up**. Let a local AI server (Ollama, LM Studio) or the Claude CLI fix grammar and punctuation after you dictate. Off by default; set it up under Settings → AI.
- New: **Command Mode**. Select text, hold the Command shortcut and say what to do with it ("make this shorter", "translate to Dutch").
- New: **screen context**. The app name, window title and text near the cursor help spell names right.
- Faster AI: the AI is started while you speak, so the text is ready sooner after you stop. The Claude CLI also starts without your MCP servers and skills.
- New: a macOS build for Apple silicon.

## 0.8.0 - 2026-10-22

- New: **Dictate paragraph**. Add an extra shortcut with the action "Dictate paragraph" and a window opens that shows what you say as you speak. Lock in each sentence by saying "new line", pausing for the set time (default 1.5 s) or pressing the shortcut again. Then paste the whole paragraph, copy it, or clear it.
- New: say "scratch that" to remove the last line; end a sentence with "stop" or "send" to paste.
- New: Settings → About shows these release notes.
