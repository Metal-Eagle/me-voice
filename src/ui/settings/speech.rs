//! Speech page: model cards, downloads and language.

use super::*;

impl SettingsApp {
    pub(super) fn speech_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Speech", "Which microphone to listen to, and how speech is recognised.");
        group(ui, |ui| {
            setting_row(ui, "Microphone", "Falls back to the system default when unplugged.", |ui| {
                if ui.button("Refresh").on_hover_text("Look for newly connected microphones").clicked() {
                    self.microphones = crate::speech::audio::microphones();
                }
                let current = self.edit.microphone.clone().unwrap_or_else(|| "System default".into());
                egui::ComboBox::from_id_salt("microphone").width(190.0).selected_text(current).truncate().show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.edit.microphone, None, "System default");
                    for name in &self.microphones {
                        ui.selectable_value(&mut self.edit.microphone, Some(name.clone()), name);
                    }
                });
            });
        });

        // The models as large cards: click anywhere on one to use it.
        let models = self.models.clone();
        if let Some(id) = self.select_when_ready.clone() {
            if let Some(model) = models.iter().find(|m| m.id == id && m.is_downloaded()) {
                self.select_model(model);
                self.select_when_ready = None;
            } else if self.download.as_ref().is_some_and(|d| {
                let d = d.lock().unwrap();
                d.finished && d.error.is_some()
            }) {
                self.select_when_ready = None;
            }
        }
        for (label, section) in model_sections(&models) {
            theme::section_label(ui, &label);
            self.model_grid(ui, &section);
            ui.add_space(12.0);
        }

        group(ui, |ui| {
            let languages = self.models.iter().find(|m| m.id == self.edit.model_id).map(|m| m.languages.clone()).unwrap_or_default();
            setting_row(ui, "Language", "The language you speak. Auto lets Parakeet and multilingual Whisper detect it.", |ui| {
                self.language_control(ui, &languages);
            });

            let preview = self.models.iter().find(|m| m.id == self.edit.preview_model_id).cloned();
            setting_row(ui, "Live preview model", "A small model makes the live text fast and light. The pasted text still uses the model above.", |ui| {
                if let Some(model) = preview.as_ref().filter(|m| !m.is_downloaded()) {
                    self.download_status(ui, model);
                }
                egui::ComboBox::from_id_salt("preview-model")
                    .selected_text(preview.as_ref().map_or("Same as main model", |m| m.name.as_str()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.edit.preview_model_id, String::new(), "Same as main model");
                        for m in &self.models {
                            ui.selectable_value(&mut self.edit.preview_model_id, m.id.clone(), &m.name);
                        }
                    });
            });
        });
    }

    /// Model cards in equal columns that fill the width (fewer columns on narrow windows).
    pub(super) fn model_grid(&mut self, ui: &mut egui::Ui, models: &[Model]) {
        let width = ui.available_width();
        let columns = ((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor().max(1.0) as usize;
        let card_width = (width - CARD_GAP * (columns as f32 - 1.0)) / columns as f32;
        for row in models.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CARD_GAP;
                for model in row {
                    self.model_card(ui, model, card_width);
                }
            });
            ui.add_space(CARD_GAP - ui.spacing().item_spacing.y);
        }
    }

    /// One model as a card: name and what it's good at, tags, and status and size at the bottom.
    pub(super) fn model_card(&mut self, ui: &mut egui::Ui, model: &Model, width: f32) {
        let selected = self.edit.model_id == model.id;
        let response = theme::choice_card(ui, egui::vec2(width, CARD_HEIGHT), selected, |ui| {
            let (title, blurb) = model_title(model);
            // Clear of the radio mark on the right.
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width() - 30.0, 40.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(egui::Label::new(egui::RichText::new(title).family(theme::semibold()).size(15.5)).truncate());
                ui.add(egui::Label::new(egui::RichText::new(blurb).size(12.5).weak()).truncate());
            });
            ui.add_space(6.0);
            // The tag row keeps its height when empty, so all cards line up.
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_height(20.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                for (text, colour) in model_tags(model) {
                    theme::tag(ui, text, colour);
                }
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if self.select_when_ready.as_deref() == Some(model.id.as_str()) && !model.is_downloaded() {
                        self.download_status(ui, model);
                    } else if selected && model.is_downloaded() {
                        ui.label(egui::RichText::new("●  In use").size(13.0).family(theme::semibold()).color(ACCENT));
                    } else {
                        self.download_status(ui, model);
                    }
                    if let Some(mb) = model.size_mb {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{mb} MB")).size(12.5).weak());
                        });
                    }
                });
            });
        });
        if response.clicked() {
            if model.is_downloaded() {
                self.select_when_ready = None;
                self.select_model(model);
            } else if self.start_download(model) {
                self.select_when_ready = Some(model.id.clone());
            }
        }
    }

    pub(super) fn select_model(&mut self, model: &Model) {
        self.edit.model_id = model.id.clone();
        if !model.languages.contains(&self.edit.language) {
            self.edit.language = model.languages.first().cloned().unwrap_or_else(|| "en".into());
        }
    }

    /// The common languages as a segmented control, the rest in a menu.
    pub(super) fn language_control(&mut self, ui: &mut egui::Ui, languages: &[String]) {
        const COMMON: [&str; 3] = ["auto", "en", "nl"];
        let others: Vec<&String> = languages.iter().filter(|c| !COMMON.contains(&c.as_str())).collect();
        if !others.is_empty() {
            let in_menu = others.iter().any(|c| **c == self.edit.language);
            let current = if in_menu { language_name(&self.edit.language).to_string() } else { "More…".to_string() };
            egui::ComboBox::from_id_salt("language-more").width(110.0).selected_text(current).show_ui(ui, |ui| {
                for code in others {
                    ui.selectable_value(&mut self.edit.language, code.clone(), language_name(code));
                }
            });
        }
        let common: Vec<(String, String)> = COMMON
            .iter()
            .filter(|c| languages.iter().any(|l| l == *c))
            .map(|c| {
                let label = match *c {
                    "auto" => "Auto",
                    "en" => "English",
                    _ => language_name(c),
                };
                (c.to_string(), label.to_string())
            })
            .collect();
        if common.len() > 1 {
            theme::segmented(ui, &mut self.edit.language, &common);
        } else if let Some((_, label)) = common.first() {
            ui.label(label.as_str());
        }
    }

    /// Ready, a Download button, or the progress of this model's download.
    pub(super) fn download_status(&mut self, ui: &mut egui::Ui, model: &Model) {
        let mut busy = false;
        if let Some(dl) = &self.download {
            let d = dl.lock().unwrap();
            busy = !d.finished;
            if d.model_id == model.id {
                if let Some(err) = &d.error {
                    ui.colored_label(ERROR, err);
                    return;
                }
                if !d.finished {
                    let fraction = d.total.map_or(0.0, |t| d.done as f32 / t as f32);
                    ui.add(egui::ProgressBar::new(fraction).desired_width(ui.available_width().min(240.0)).show_percentage().fill(ACCENT));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
            }
        }
        if model.is_downloaded() {
            ui.label(egui::RichText::new("●  Ready").size(13.0).color(OK));
        } else if ui
            .add_enabled(!busy, theme::primary_button("Download"))
            .on_disabled_hover_text("Another model is downloading")
            .clicked()
        {
            self.start_download(model);
        }
    }

    /// Starts downloading a model in the background. False when another download is running.
    pub(super) fn start_download(&mut self, model: &Model) -> bool {
        if self.download.as_ref().is_some_and(|d| !d.lock().unwrap().finished) {
            return false;
        }
        let state = Arc::new(Mutex::new(Download { model_id: model.id.clone(), ..Default::default() }));
        self.download = Some(state.clone());
        let model = model.clone();
        std::thread::spawn(move || {
            let progress_state = state.clone();
            let result = model.download(move |done, total| {
                let mut d = progress_state.lock().unwrap();
                d.done = done;
                d.total = total;
            });
            let mut d = state.lock().unwrap();
            d.finished = true;
            d.error = result.err().map(|e| e.to_string());
        });
        true
    }

}

pub(super) fn language_name(code: &str) -> &str {
    match code {
        "auto" => "Detect automatically",
        "en" => "English",
        "nl" => "Nederlands",
        "bg" => "Bulgarian",
        "hr" => "Croatian",
        "cs" => "Czech",
        "da" => "Danish",
        "et" => "Estonian",
        "fi" => "Finnish",
        "el" => "Greek",
        "hu" => "Hungarian",
        "lv" => "Latvian",
        "lt" => "Lithuanian",
        "mt" => "Maltese",
        "ro" => "Romanian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sv" => "Swedish",
        "uk" => "Ukrainian",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "pl" => "Polish",
        "ru" => "Russian",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        other => other,
    }
}

/// Models grouped into labelled sections, in list order (e.g. "PARAKEET V3 · 25 LANGUAGES").
pub(super) fn model_sections(models: &[Model]) -> Vec<(String, Vec<Model>)> {
    let mut sections: Vec<(String, Vec<Model>)> = Vec::new();
    for model in models {
        let label = section_label(model);
        match sections.iter().position(|(l, _)| *l == label) {
            Some(i) => sections[i].1.push(model.clone()),
            None => sections.push((label, vec![model.clone()])),
        }
    }
    sections
}

pub(super) fn section_label(model: &Model) -> String {
    let engine = match (model.engine, model.acft) {
        (Engine::Parakeet, _) => "PARAKEET V3",
        (Engine::Whisper, true) => "WHISPER (FUTO ACFT)",
        (Engine::Whisper, false) => "WHISPER",
    };
    let english_only = model.languages.iter().all(|l| l.as_str() == "en");
    let languages = if english_only {
        "ENGLISH".to_string()
    } else {
        format!("{} LANGUAGES", model.languages.iter().filter(|l| l.as_str() != "auto").count())
    };
    format!("{engine} · {languages}")
}

/// Tags shown on a model card: the default models, and multilingual ones.
pub(super) fn model_tags(model: &Model) -> Vec<(&'static str, egui::Color32)> {
    let defaults = Settings::default();
    let mut tags = Vec::new();
    if model.id == defaults.model_id {
        tags.push(("DEFAULT", ACCENT));
    }
    if model.id == defaults.preview_model_id {
        tags.push(("PREVIEW", OK));
    }
    if model.languages.len() > 2 {
        tags.push(("MULTILINGUAL", theme::PURPLE));
    }
    tags
}

/// A model's short name and what it is good at: "English 244" and "Most accurate".
pub(super) fn model_title(model: &Model) -> (String, String) {
    let title = model.name.split(" (").next().unwrap_or(&model.name).to_string();
    let blurb = if model.engine == Engine::Parakeet {
        "Fast, detects the language"
    } else if model.id.ends_with("-244") {
        "Most accurate"
    } else if model.id.ends_with("-74") {
        "Balanced speed and accuracy"
    } else if model.id.ends_with("-39") {
        "Fastest, lightest"
    } else {
        ""
    };
    (title, blurb.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(json: &str) -> Model {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn section_labels_name_engine_and_languages() {
        let parakeet = model(r#"{"id":"p","name":"P","file":"p","sha256":null,"languages":["auto","en","nl"],"acft":false,"engine":"parakeet"}"#);
        assert_eq!(section_label(&parakeet), "PARAKEET V3 · 2 LANGUAGES");
        let english = model(r#"{"id":"e","name":"E","url":"u","file":"e.bin","sha256":null,"languages":["en"],"acft":true}"#);
        assert_eq!(section_label(&english), "WHISPER (FUTO ACFT) · ENGLISH");
    }
}
