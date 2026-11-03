//! Behaviour page: how dictations are pasted, sounds, start at login, silence.

use super::*;

impl SettingsApp {
    pub(super) fn behaviour_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Behaviour", "How the text gets into your app, and what runs in the background.");
        group(ui, |ui| {
            setting_row(ui, "Insert text by", "Pasting is fastest; your clipboard is restored afterwards.", |ui| {
                egui::ComboBox::from_id_salt("insert")
                    .selected_text(match self.edit.insert_mode {
                        InsertMode::Paste => "Pasting",
                        InsertMode::Type => "Typing",
                        InsertMode::T3 => "T3 native",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.edit.insert_mode, InsertMode::Paste, "Pasting");
                        ui.selectable_value(&mut self.edit.insert_mode, InsertMode::Type, "Typing");
                    });
            });
            setting_row(ui, "Press Enter after pasting", "Sends the text right away, for chat apps. A send word does it once.", |ui| {
                toggle(ui, &mut self.edit.press_enter_after_paste);
            });
            setting_row(ui, "Ending", "Chat messages usually skip the last full stop.", |ui| {
                egui::ComboBox::from_id_salt("ending")
                    .selected_text(if self.edit.drop_final_period { "No full stop" } else { "Keep" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.edit.drop_final_period, false, "Keep");
                        ui.selectable_value(&mut self.edit.drop_final_period, true, "No full stop");
                    });
            });
            setting_row(ui, "Free memory after", "Unloads the model when you haven't dictated for a while.", |ui| {
                egui::ComboBox::from_id_salt("unload")
                    .selected_text(unload_label(self.edit.unload_after_minutes))
                    .show_ui(ui, |ui| {
                        for m in [0, 1, 5, 15, 60, -1] {
                            ui.selectable_value(&mut self.edit.unload_after_minutes, m, unload_label(m));
                        }
                    });
            });
            setting_row(ui, "Sounds", "A short beep when listening starts and stops.", |ui| {
                toggle(ui, &mut self.edit.sounds);
            });
            setting_row(ui, "Start at login", "Start me-voice when you sign in.", |ui| {
                ui.add_enabled_ui(crate::app::login::SUPPORTED, |ui| {
                    toggle(ui, &mut self.edit.start_at_login);
                }).response.on_disabled_hover_text("Start at login is not supported on this platform.");
            });
            setting_row(ui, "Automatic updates", "Download and install new versions in the background.", |ui| {
                toggle(ui, &mut self.edit.auto_update);
            });
        });

        theme::section_label(ui, "SILENCE");
        group(ui, |ui| {
            setting_row(ui, "Trim silence", "Skip quiet parts before transcribing.", |ui| {
                toggle(ui, &mut self.edit.trim_silence);
            });
            setting_row(ui, "Stop after silence", "Stops listening this long after you stop talking, with a countdown in the card. Only after a tap to start; holding the key always wins.", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.edit.auto_stop_seconds, 0..=10)
                        .custom_formatter(|v, _| if v < 0.5 { "Off".to_string() } else { format!("{v:.0} s") }),
                );
            });
            setting_row(ui, "Silence level", "Raise it in noisy rooms.", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.edit.silence_level, 0.01..=0.2)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                );
            });
        });
    }

}

pub(super) fn unload_label(minutes: i64) -> String {
    match minutes {
        m if m < 0 => "Never".into(),
        0 => "Immediately".into(),
        1 => "1 minute idle".into(),
        m => format!("{m} minutes idle"),
    }
}
