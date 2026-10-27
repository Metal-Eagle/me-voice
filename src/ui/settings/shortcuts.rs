//! Shortcut page: dictation shortcut, extra shortcuts and key capture.

use super::*;

impl SettingsApp {
    pub(super) fn shortcut_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Shortcut", "Tap to start and stop, or hold while you talk.");
        group(ui, |ui| {
            setting_row(ui, "Dictation shortcut", "Click, then press the keys you want. You see them as you press.", |ui| {
                if ui.link("Reset").clicked() {
                    self.edit.hotkey = Settings::default().hotkey;
                }
                self.key_button(ui, Capture::Dictation);
            });
            setting_row(ui, "Cancel key", "Stops listening without typing anything.", |ui| {
                self.key_button(ui, Capture::Cancel);
            });
            setting_row(ui, "How it works", mode_description(self.edit.hotkey_mode), |ui| {
                ui.horizontal(|ui| {
                    for (mode, label) in [
                        (HotkeyMode::HoldOrTap, "Hold or tap"),
                        (HotkeyMode::HoldOnly, "Hold only"),
                        (HotkeyMode::TapOnly, "Tap only"),
                    ] {
                        ui.selectable_value(&mut self.edit.hotkey_mode, mode, label);
                    }
                });
            });
            if self.edit.hotkey_mode == HotkeyMode::HoldOrTap {
                setting_row(ui, "Hold threshold", "Pressed shorter than this counts as a tap.", |ui| {
                    ui.add(egui::Slider::new(&mut self.edit.hold_threshold_ms, 150..=800).suffix(" ms"));
                });
            }
        });
        ui.add_space(22.0);
        theme::section_label(ui, "MORE SHORTCUTS");
        self.extra_shortcuts(ui);
    }

    /// Extra shortcuts: each has a capture button, an action and, for dictation, a profile name.
    pub(super) fn extra_shortcuts(&mut self, ui: &mut egui::Ui) {
        let mut remove = None;
        group(ui, |ui| {
            if self.edit.extra_shortcuts.is_empty() {
                ui.weak("Add a shortcut to dictate with a profile, to send a dictation to T3 Code, or to paste or fix your last dictation.");
                ui.add_space(8.0);
            }
            for i in 0..self.edit.extra_shortcuts.len() {
                theme::row_divider(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    self.key_button(ui, Capture::Extra(i));
                    let extra = &mut self.edit.extra_shortcuts[i];
                    egui::ComboBox::from_id_salt(("extra-action", i))
                        .selected_text(action_label(&extra.action))
                        .show_ui(ui, |ui| {
                            let profile = match &extra.action {
                                ShortcutAction::Dictate { profile } => profile.clone(),
                                _ => String::new(),
                            };
                            if ui.selectable_label(matches!(extra.action, ShortcutAction::Dictate { .. }), "Dictate with profile").clicked() {
                                extra.action = ShortcutAction::Dictate { profile };
                            }
                            if ui.selectable_label(extra.action == ShortcutAction::DictateToT3, "Dictate to T3 Code").clicked() {
                                extra.action = ShortcutAction::DictateToT3;
                            }
                            if ui.selectable_label(extra.action == ShortcutAction::PasteLast, "Paste last dictation").clicked() {
                                extra.action = ShortcutAction::PasteLast;
                            }
                            if ui.selectable_label(extra.action == ShortcutAction::FixLast, "Fix last dictation").clicked() {
                                extra.action = ShortcutAction::FixLast;
                            }
                            if ui.selectable_label(extra.action == ShortcutAction::Paragraph, "Dictate paragraph").clicked() {
                                extra.action = ShortcutAction::Paragraph;
                            }
                            if ui.selectable_label(extra.action == ShortcutAction::Command, "AI command on selection").clicked() {
                                extra.action = ShortcutAction::Command;
                            }
                        });
                    if let ShortcutAction::Dictate { profile } = &mut extra.action {
                        crate::ui::text_menu::add(ui, egui::TextEdit::singleline(profile).hint_text("Profile name").desired_width(140.0));
                    }
                    if ui.small_button("×").on_hover_text("Remove this shortcut").clicked() {
                        remove = Some(i);
                    }
                });
                if let Some(warning) = self.shortcut_conflict(i) {
                    ui.colored_label(ERROR, warning);
                }
                ui.add_space(8.0);
            }
            theme::row_divider(ui);
            ui.add_space(8.0);
            if ui.button("+ Add shortcut").clicked() {
                self.edit.extra_shortcuts.push(ExtraShortcut {
                    hotkey: String::new(),
                    action: ShortcutAction::Dictate { profile: String::new() },
                });
            }
        });
        if let Some(i) = remove {
            self.edit.extra_shortcuts.remove(i);
        }
    }

    /// A warning when extra shortcut `i` is the same key as the dictation shortcut or another extra one.
    pub(super) fn shortcut_conflict(&self, i: usize) -> Option<&'static str> {
        let id = self.edit.extra_shortcuts[i].hotkey.parse::<global_hotkey::hotkey::HotKey>().ok()?.id();
        if self.edit.hotkey.parse::<global_hotkey::hotkey::HotKey>().is_ok_and(|hk| hk.id() == id) {
            return Some("Same as the dictation shortcut.");
        }
        let same_as_other = self
            .edit
            .extra_shortcuts
            .iter()
            .enumerate()
            .any(|(j, e)| j != i && e.hotkey.parse::<global_hotkey::hotkey::HotKey>().is_ok_and(|hk| hk.id() == id));
        same_as_other.then_some("Already used by another shortcut.")
    }

    pub(super) fn key_button(&mut self, ui: &mut egui::Ui, which: Capture) {
        let current = match which {
            Capture::Dictation => self.edit.hotkey.clone(),
            Capture::Cancel => self.edit.cancel_key.clone(),
            Capture::Extra(i) => self.edit.extra_shortcuts.get(i).map(|e| e.hotkey.clone()).unwrap_or_default(),
        };
        let capturing = self.capturing == Some(which);
        let text = if capturing {
            // Live view of the keys held right now, so you can see what is registered.
            let held = ui.ctx().input(|i| held_keys(i.modifiers, &i.keys_down));
            ui.ctx().request_repaint();
            if held.is_empty() { "Press keys…".to_string() } else { format!("{held} + …") }
        } else if current.is_empty() {
            "Click to set".to_string()
        } else {
            current.replace('+', " + ")
        };
        let mut text = egui::RichText::new(text).monospace();
        if capturing {
            text = text.color(ACCENT);
        }
        let mut button = egui::Button::new(text).min_size(egui::vec2(220.0, 32.0));
        if capturing {
            button = button.stroke(egui::Stroke::new(1.5, ACCENT));
        }
        let response = ui.add(button).on_hover_text(if capturing { "Esc to cancel" } else { "Click to change" });
        if response.clicked() && !capturing {
            self.capturing = Some(which);
            self.shortcut_error = None;
            let _ = ipc::send(Command::PauseHotkey);
        }
    }

    pub(super) fn read_captured_keys(&mut self, ctx: &egui::Context) {
        // Finish on the first non-modifier key; modifiers come from the live keyboard
        // state, which is more reliable than the event's own copy (e.g. Ctrl+Alt on Windows).
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers | i.modifiers)),
                _ => None,
            })
        });
        let Some((key, mods)) = pressed else { return };
        let which = self.capturing.take().unwrap();
        let _ = ipc::send(Command::ResumeHotkey);

        if key == egui::Key::Escape && mods.is_none() && which != Capture::Cancel {
            return; // cancelled
        }
        let combo = format!("{}+{}", modifier_names(mods).join("+"), key.name()).trim_start_matches('+').to_string();

        let saved = match which {
            Capture::Dictation => Some(self.saved.hotkey.as_str()),
            Capture::Extra(i) => self.saved.extra_shortcuts.get(i).map(|e| e.hotkey.as_str()),
            Capture::Cancel => None,
        };
        if which != Capture::Cancel && saved != Some(combo.as_str()) {
            if let Err(e) = check_available(&combo) {
                self.shortcut_error = Some(e);
                return;
            }
        }
        match which {
            Capture::Dictation => self.edit.hotkey = combo,
            Capture::Cancel => self.edit.cancel_key = combo,
            Capture::Extra(i) => {
                if let Some(extra) = self.edit.extra_shortcuts.get_mut(i) {
                    extra.hotkey = combo;
                }
            }
        }
    }

}

/// Explains the chosen shortcut mode under "How it works".
pub(super) fn mode_description(mode: HotkeyMode) -> &'static str {
    match mode {
        HotkeyMode::HoldOrTap => "Tap to start and tap again to stop, or hold while you talk and let go.",
        HotkeyMode::HoldOnly => "Hold the shortcut while you talk. Letting go stops and pastes.",
        HotkeyMode::TapOnly => "Tap to start and tap again to stop. Letting go does nothing.",
    }
}

pub(super) fn action_label(action: &ShortcutAction) -> &'static str {
    match action {
        ShortcutAction::Dictate { .. } => "Dictate with profile",
        ShortcutAction::DictateToT3 => "Dictate to T3 Code",
        ShortcutAction::PasteLast => "Paste last dictation",
        ShortcutAction::FixLast => "Fix last dictation",
        ShortcutAction::Paragraph => "Dictate paragraph",
        ShortcutAction::Command => "AI command on selection",
    }
}

/// Tries to register the shortcut briefly to see whether another app already uses it.
pub(super) fn check_available(combo: &str) -> Result<(), String> {
    use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
    let hk: HotKey = combo.parse().map_err(|e| format!("{combo} can't be used as a shortcut: {e}"))?;
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    manager.register(hk).map_err(|_| format!("{combo} is already used by another app"))?;
    let _ = manager.unregister(hk);
    Ok(())
}

pub(super) fn modifier_names(mods: egui::Modifiers) -> Vec<&'static str> {
    let mut names = Vec::new();
    if mods.ctrl {
        names.push("Ctrl");
    }
    if mods.alt {
        names.push("Alt");
    }
    if mods.shift {
        names.push("Shift");
    }
    if mods.mac_cmd {
        names.push("Cmd");
    }
    names
}

/// "Ctrl + Alt + Space" for the keys currently held down.
pub(super) fn held_keys(mods: egui::Modifiers, keys: &std::collections::HashSet<egui::Key>) -> String {
    let mut parts: Vec<&str> = modifier_names(mods);
    let mut others: Vec<&str> = keys.iter().map(|k| k.name()).collect();
    others.sort();
    parts.extend(others);
    parts.join(" + ")
}
