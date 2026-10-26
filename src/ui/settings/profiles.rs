//! Profiles page: per-app settings.

use super::*;

impl SettingsApp {
    pub(super) fn profiles_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Profiles", "Different settings per app. The app in front when you start dictating picks the profile.");
        if ui.available_width() < WIDE {
            self.profile_list(ui);
            self.profile_editor(ui);
        } else {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    self.profile_list(ui);
                });
                ui.vertical(|ui| self.profile_editor(ui));
            });
        }
    }

    /// Default first (not editable here), then each profile and "+ New profile".
    pub(super) fn profile_list(&mut self, ui: &mut egui::Ui) {
        group(ui, |ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Default").family(theme::semibold()));
            ui.weak("Everything else");
            ui.add_space(6.0);
            for (i, profile) in self.edit.profiles.iter().enumerate() {
                if ui.selectable_label(i == self.selected_profile, &profile.name).clicked() {
                    self.selected_profile = i;
                }
                let apps = if profile.apps.is_empty() { "No apps yet".to_string() } else { profile.apps.join(", ") };
                ui.add(egui::Label::new(egui::RichText::new(apps).weak().size(12.0)).wrap());
                ui.add_space(6.0);
            }
            if ui.button("+ New profile").clicked() {
                self.edit.profiles.push(Profile { name: "New profile".into(), ..Default::default() });
                self.selected_profile = self.edit.profiles.len() - 1;
            }
            ui.add_space(8.0);
        });
    }

    /// Editor for the selected profile: name, apps, the settings it changes, and delete.
    pub(super) fn profile_editor(&mut self, ui: &mut egui::Ui) {
        if self.selected_profile >= self.edit.profiles.len() {
            ui.weak("Pick a profile, or make a new one with + New profile.");
            return;
        }
        let index = self.selected_profile;

        group(ui, |ui| {
            let profile = &mut self.edit.profiles[index];
            setting_row(ui, "Name", "", |ui| {
                crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut profile.name).desired_width(200.0));
            });
        });

        theme::section_label(ui, "APPS");
        group(ui, |ui| {
            ui.add_space(10.0);
            let apps = &mut self.edit.profiles[index].apps;
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, app) in apps.iter().enumerate() {
                    ui.label(app.as_str());
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                    ui.add_space(8.0);
                }
            });
            if let Some(i) = remove {
                apps.remove(i);
            }
            ui.horizontal(|ui| {
                let field = crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut self.new_app).hint_text("ms-teams.exe").desired_width(200.0));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Add").clicked() || enter {
                    if let Some(app) = normal_app_name(&self.new_app) {
                        if !apps.contains(&app) {
                            apps.push(app);
                        }
                    }
                    self.new_app.clear();
                }
            });
            ui.add_space(10.0);
        });

        // Choices for the override combos: every language the models offer, and the models.
        let mut codes: Vec<String> = self.models.iter().flat_map(|m| m.languages.iter().cloned()).collect();
        codes.sort();
        codes.dedup();
        let languages: Vec<(String, String)> = codes.iter().map(|c| (c.clone(), language_name(c).to_string())).collect();
        let models: Vec<(String, String)> = self.models.iter().map(|m| (m.id.clone(), m.name.clone())).collect();

        theme::section_label(ui, "CHANGES FROM DEFAULT");
        group(ui, |ui| {
            let o = &mut self.edit.profiles[index].overrides;
            setting_row(ui, "Language", "", |ui| override_combo(ui, "profile-language", &mut o.language, &languages));
            setting_row(ui, "Model", "", |ui| override_combo(ui, "profile-model", &mut o.model_id, &models));
            let insert = [(InsertMode::Paste, "Pasting".to_string()), (InsertMode::Type, "Typing".to_string())];
            setting_row(ui, "Insert text by", "", |ui| override_combo(ui, "profile-insert", &mut o.insert_mode, &insert));
            let on_off = |on: &str, off: &str| [(true, on.to_string()), (false, off.to_string())];
            setting_row(ui, "Press Enter after pasting", "", |ui| {
                override_combo(ui, "profile-enter", &mut o.press_enter_after_paste, &on_off("On", "Off"));
            });
            setting_row(ui, "Ending", "", |ui| {
                override_combo(ui, "profile-ending", &mut o.drop_final_period, &on_off("No full stop", "Keep"));
            });
            setting_row(ui, "Remove filler words", "", |ui| {
                override_combo(ui, "profile-fillers", &mut o.remove_fillers, &on_off("On", "Off"));
            });
            setting_row(ui, "Spoken formatting", "", |ui| {
                override_combo(ui, "profile-formatting", &mut o.spoken_formatting, &on_off("On", "Off"));
            });
            setting_row(ui, "AI clean-up", "Needs a provider on the AI page.", |ui| {
                override_combo(ui, "profile-ai", &mut o.ai_polish, &on_off("On", "Off"));
            });
            setting_row(ui, "AI style", "Empty: same as Default.", |ui| {
                let mut style = o.ai_style.clone().unwrap_or_default();
                let field = egui::TextEdit::singleline(&mut style).hint_text("Casual, short, no full stop").desired_width(220.0);
                if crate::ui::text_menu::add(ui, field).changed() {
                    o.ai_style = (!style.trim().is_empty()).then_some(style);
                }
            });
        });

        ui.add_space(4.0);
        if ui.add(egui::Button::new(egui::RichText::new("Delete profile").color(ERROR))).clicked() {
            self.edit.profiles.remove(index);
            self.selected_profile = index.saturating_sub(1);
        }
    }
}

/// A profile's app name as stored: lower-case, with ".exe" added when missing.
pub(super) fn normal_app_name(input: &str) -> Option<String> {
    let name = input.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    Some(if name.ends_with(".exe") { name } else { format!("{name}.exe") })
}

/// Dropdown for one profile override: "Same as Default" (None) or one of `options`.
pub(super) fn override_combo<T: Clone + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut Option<T>, options: &[(T, String)]) {
    let current = options.iter().find(|(v, _)| Some(v) == value.as_ref()).map_or("Same as Default".to_string(), |(_, label)| label.clone());
    egui::ComboBox::from_id_salt(id).width(190.0).selected_text(current).show_ui(ui, |ui| {
        ui.selectable_value(value, None, "Same as Default");
        for (v, label) in options {
            ui.selectable_value(value, Some(v.clone()), label.as_str());
        }
    });
}
