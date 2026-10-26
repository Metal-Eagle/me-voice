//! AI page: provider, polish style and screen context.

use super::*;

impl SettingsApp {
    pub(super) fn ai_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "AI", "Optional clean-up of your dictations, and spoken commands on selected text.");
        let e = &mut self.edit;
        group(ui, |ui| {
            setting_row(ui, "Provider", "A local server keeps everything on this computer. Claude sends the text to Anthropic.", |ui| {
                egui::ComboBox::from_id_salt("ai-provider")
                    .selected_text(provider_label(e.ai_provider))
                    .show_ui(ui, |ui| {
                        for p in [AiProvider::Off, AiProvider::Local, AiProvider::ClaudeCli] {
                            ui.selectable_value(&mut e.ai_provider, p, provider_label(p));
                        }
                    });
            });
            match e.ai_provider {
                AiProvider::Off => {}
                AiProvider::Local => {
                    setting_row(ui, "Server", "OpenAI-compatible address. Ollama: http://localhost:11434/v1, LM Studio: http://localhost:1234/v1.", |ui| {
                        crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut e.ai_url).desired_width(240.0));
                    });
                    setting_row(ui, "Model", "As the server names it, e.g. qwen2.5:3b (ollama pull qwen2.5:3b).", |ui| {
                        crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut e.ai_model).desired_width(240.0));
                    });
                    setting_row(ui, "Allow other computers", "Off: only a server on this computer is used.", |ui| {
                        toggle(ui, &mut e.ai_allow_remote);
                    });
                }
                AiProvider::ClaudeCli => {
                    setting_row(ui, "Claude model", "Uses the Claude CLI you are signed in to (claude -p). haiku is fastest.", |ui| {
                        egui::ComboBox::from_id_salt("claude-model").selected_text(e.claude_model.as_str()).show_ui(ui, |ui| {
                            for m in ["haiku", "sonnet", "opus"] {
                                ui.selectable_value(&mut e.claude_model, m.to_string(), m);
                            }
                        });
                    });
                }
            }
            if e.ai_provider != AiProvider::Off {
                setting_row(ui, "Wait at most", "Then the text is pasted without AI.", |ui| {
                    ui.add(egui::Slider::new(&mut e.ai_timeout_seconds, 2..=30).suffix(" s"));
                });
                setting_row(ui, "Test", "Sends a short sentence with the settings above (also before saving).", |ui| {
                    let result = self.ai_test.lock().unwrap().clone();
                    if let Some(r) = &result {
                        ui.add(egui::Label::new(egui::RichText::new(r).weak().size(13.0)).truncate());
                    }
                    if ui.button("Test").clicked() {
                        *self.ai_test.lock().unwrap() = Some("Testing…".into());
                        let (settings, out, ctx) = (e.clone(), self.ai_test.clone(), ui.ctx().clone());
                        std::thread::spawn(move || {
                            let text = match crate::text::polish::test(&settings) {
                                Ok((answer, took)) => format!("{:.1} s: {answer}", took.as_secs_f32()),
                                Err(err) => format!("Failed: {err}"),
                            };
                            *out.lock().unwrap() = Some(text);
                            ctx.request_repaint();
                        });
                    }
                });
            }
        });

        theme::section_label(ui, "CLEAN-UP");
        group(ui, |ui| {
            setting_row(ui, "Clean up every dictation", "Short dictations, snippets and \"scratch that\" are left alone. Profiles can switch it per app.", |ui| {
                toggle(ui, &mut e.ai_polish);
            });
            setting_row(ui, "Style", "What the AI should do with your text. Profiles can set their own.", |ui| {
                crate::ui::text_menu::add(ui, egui::TextEdit::multiline(&mut e.ai_style).desired_rows(2).desired_width(280.0));
            });
            setting_row(ui, "AI command on selection", "Add an extra shortcut with this action (Shortcut page). Select text, press it and say what to do, e.g. \"make this shorter\". Nothing selected: the answer is inserted.", |_| {});
        });

        theme::section_label(ui, "SCREEN CONTEXT");
        group(ui, |ui| {
            setting_row(ui, "Read from the screen", "Names and terms on screen help spelling. Read when you start talking, never stored.", |ui| {
                egui::ComboBox::from_id_salt("context")
                    .selected_text(context_label(e.context))
                    .show_ui(ui, |ui| {
                        for c in [ContextLevel::Off, ContextLevel::Window, ContextLevel::Text] {
                            ui.selectable_value(&mut e.context, c, context_label(c));
                        }
                    });
            });
            setting_row(ui, "Use the clipboard", "Can hold private text such as passwords.", |ui| {
                toggle(ui, &mut e.context_clipboard);
            });
            setting_row(ui, "Send context to Claude", "Off: Claude only gets your dictation.", |ui| {
                toggle(ui, &mut e.context_to_claude);
            });
        });
    }
}

pub(super) fn provider_label(p: AiProvider) -> &'static str {
    match p {
        AiProvider::Off => "Off",
        AiProvider::Local => "Local server (Ollama)",
        AiProvider::ClaudeCli => "Claude CLI (cloud)",
    }
}

pub(super) fn context_label(c: ContextLevel) -> &'static str {
    match c {
        ContextLevel::Off => "Nothing",
        ContextLevel::Window => "App and window title",
        ContextLevel::Text => "Also the text you work in",
    }
}
