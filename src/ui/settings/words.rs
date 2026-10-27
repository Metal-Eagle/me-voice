//! Words page: dictionary, snippets and voice commands.

use super::*;

impl SettingsApp {
    pub(super) fn words_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Words", "Tidy up what you say before it's typed. Applies on the next dictation.");

        theme::section_label(ui, "SNIPPETS");
        group(ui, |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(egui::RichText::new("Say only the phrase and the whole dictation becomes the text. Saved in dictionary.json.").weak().size(13.0)).wrap());
            ui.add_space(6.0);
            let mut remove = None;
            let width = ui.available_width();
            let say_w = (width * 0.28).max(120.0);
            let text_w = (width - say_w - 60.0).max(140.0);
            egui::Grid::new("snippets").num_columns(3).spacing([8.0, 8.0]).show(ui, |ui| {
                for head in ["SAY", "INSERTS", ""] {
                    ui.label(egui::RichText::new(head).weak().size(11.0));
                }
                ui.end_row();
                for (i, (phrase, text)) in self.snippets.iter_mut().enumerate() {
                    crate::ui::text_menu::add(ui, egui::TextEdit::singleline(phrase).hint_text("my sign off").desired_width(say_w).margin(egui::vec2(8.0, 6.0)));
                    crate::ui::text_menu::add(ui, egui::TextEdit::multiline(text).hint_text("Kind regards,
Alex").desired_rows(1).desired_width(text_w).margin(egui::vec2(8.0, 6.0)));
                    if ui.button("✕").on_hover_text("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if self.snippets.is_empty() {
                ui.weak("No snippets yet.");
            }
            if let Some(i) = remove {
                self.snippets.remove(i);
            }
            if ui.button("+ Add snippet").clicked() {
                self.snippets.push((String::new(), String::new()));
            }
            ui.add_space(10.0);
        });

        theme::section_label(ui, "DICTIONARY");
        group(ui, |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(egui::RichText::new("Names and terms to always spell exactly like this, one per line. They also help recognition.").weak().size(13.0)).wrap());
            ui.add_space(6.0);
            crate::ui::text_menu::add(ui, egui::TextEdit::multiline(&mut self.dictionary).hint_text("Kubernetes
Anna
Zoe").desired_rows(4).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)));
            ui.horizontal(|ui| {
                ui.weak("Replacements (for example \"cuber netties\" becomes \"Kubernetes\") and snippets live in");
                if ui.link("dictionary.json").clicked() {
                    crate::text::cleanup::load_dictionary();
                    open_path(&config_dir().join("dictionary.json"));
                }
            });
            ui.add_space(8.0);
        });

        theme::section_label(ui, "VOICE COMMANDS");
        group(ui, |ui| {
            setting_row(ui, "Voice commands", "End with a stop or send word instead of pressing the shortcut.", |ui| {
                toggle(ui, &mut self.edit.voice_commands);
            });
            setting_row(ui, "Stop words", "Stop listening and paste. Only counts as the very last word.", |ui| {
                ui.add_enabled_ui(self.edit.voice_commands, |ui| {
                    crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut self.edit.stop_words).hint_text("stop").desired_width(200.0));
                });
            });
            setting_row(ui, "Send words", "Stop, paste and press Enter (sends a chat message).", |ui| {
                ui.add_enabled_ui(self.edit.voice_commands, |ui| {
                    crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut self.edit.send_words).hint_text("enter, send").desired_width(200.0));
                });
            });
        });
        group(ui, |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(egui::RichText::new("Say these while dictating. Your own commands are inserted wherever you say them.").weak().size(13.0)).wrap());
            ui.add_space(6.0);
            self.command_table(ui);
            ui.add_space(10.0);
        });

        theme::section_label(ui, "CLEAN-UP");
        group(ui, |ui| {
            setting_row(ui, "Remove filler words", "Drops \"um\", \"uh\" and repeated words.", |ui| {
                toggle(ui, &mut self.edit.remove_fillers);
            });
            setting_row(ui, "Spoken formatting", "Say \"new line\", \"comma\" or \"question mark\".", |ui| {
                toggle(ui, &mut self.edit.spoken_formatting);
            });
        });
    }

    /// SAY / DOES / WHERE table: built-in commands (read-only), then the user's own (editable).
    /// The WHERE column is hidden on narrow widths.
    pub(super) fn command_table(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let wide = width >= 540.0;
        let send = non_empty(&self.edit.send_words, "enter, send");
        let stop = non_empty(&self.edit.stop_words, "stop");
        let built_in = [
            ("new line", "nieuwe regel", "Line break", Scope::Anywhere),
            ("new paragraph", "nieuwe alinea", "Blank line", Scope::Anywhere),
            ("full stop", "punt", "Types a full stop (.)", Scope::Anywhere),
            ("comma", "komma", "Types a comma (,)", Scope::Anywhere),
            ("question mark", "vraagteken", "Types a question mark (?)", Scope::Anywhere),
            ("exclamation mark", "uitroepteken", "Types an exclamation mark (!)", Scope::Anywhere),
            ("colon", "dubbele punt", "Types a colon (:)", Scope::Anywhere),
            ("bullet point", "opsommingsteken", "New line starting with \"- \"", Scope::Anywhere),
            ("open quote … close quote", "aanhalingsteken openen … sluiten", "Puts the words in “ … ”", Scope::Anywhere),
            ("all caps word", "hoofdletters woord", "Next word in capitals", Scope::Anywhere),
            ("scratch that", "weg ermee", "Undo the last paste", Scope::Whole),
        ];
        let say_w = (width * 0.28).max(120.0);
        let does_w = (width - say_w - if wide { 250.0 } else { 90.0 }).max(140.0);
        let mut remove = None;
        egui::Grid::new("voice_commands").num_columns(if wide { 4 } else { 3 }).spacing([8.0, 8.0]).show(ui, |ui| {
            for head in if wide { vec!["SAY", "DOES", "WHERE", ""] } else { vec!["SAY", "DOES", ""] } {
                ui.label(egui::RichText::new(head).weak().size(11.0));
            }
            ui.end_row();
            for (say, alias, does, scope) in built_in {
                say_cell(ui, say, alias);
                ui.label(does);
                if wide {
                    scope_tag(ui, scope);
                }
                ui.label("");
                ui.end_row();
            }
            for (say, does) in [(send.as_str(), "Paste, then press Enter"), (stop.as_str(), "Stop listening")] {
                ui.label(egui::RichText::new(say).strong());
                ui.label(does);
                if wide {
                    scope_tag(ui, Scope::End);
                }
                ui.label("");
                ui.end_row();
            }
            for (i, (say, text)) in self.commands.iter_mut().enumerate() {
                crate::ui::text_menu::add(ui, egui::TextEdit::singleline(say).hint_text("sign off").desired_width(say_w).margin(egui::vec2(8.0, 6.0)));
                crate::ui::text_menu::add(ui, egui::TextEdit::multiline(text).hint_text("Kind regards,\nAlex").desired_rows(1).desired_width(does_w).margin(egui::vec2(8.0, 6.0)));
                if wide {
                    scope_tag(ui, Scope::Anywhere);
                }
                if ui.button("✕").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.commands.remove(i);
        }
        if ui.button("+ Add command").clicked() {
            self.commands.push((String::new(), String::new()));
        }
    }

}
