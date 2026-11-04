//! Voice page: Kokoro talks back, and what you can say to it.

use super::*;
use crate::settings::{StopSound, VoiceSettings};
use crate::speech::tts::{fill, installed, say, Line};

/// The moments that can be spoken, with their titles on the page.
const LINES: [(Line, &str); 10] = [
    (Line::Stop, "Listening stops"),
    (Line::Sent, "Sent to T3 Code"),
    (Line::NewChat, "New chat started"),
    (Line::Working, "Agent is working"),
    (Line::StillWorking, "Still working"),
    (Line::NeedsOk, "Needs your OK"),
    (Line::Question, "Has a question"),
    (Line::Finished, "Finished"),
    (Line::Error, "Something went wrong"),
    (Line::CommandAnswer, "Command Mode answers"),
];

/// The on/off switch behind a spoken moment, if it has one (the stop sound and still-working
/// interval have their own menus).
fn line_switch(v: &mut VoiceSettings, line: Line) -> Option<&mut bool> {
    match line {
        Line::Sent => Some(&mut v.say_sent),
        Line::NewChat => Some(&mut v.say_new_chat),
        Line::Working => Some(&mut v.say_working),
        Line::NeedsOk => Some(&mut v.say_needs_ok),
        Line::Question => Some(&mut v.say_question),
        Line::Finished => Some(&mut v.say_finished),
        Line::Error => Some(&mut v.say_errors),
        Line::CommandAnswer => Some(&mut v.say_command_answers),
        Line::Stop | Line::StillWorking => None,
    }
}

fn stop_sound_label(s: StopSound) -> &'static str {
    match s {
        StopSound::Beep => "Beep",
        StopSound::Voice => "Voice",
        StopSound::Off => "Off",
    }
}

fn still_working_label(minutes: u32) -> &'static str {
    match minutes {
        0 => "Never",
        1 => "Every 1 min",
        2 => "Every 2 min",
        _ => "Every 5 min",
    }
}

/// A round play button with a drawn triangle (the UI font has no ▶ glyph).
fn play_button(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), sense);
    let visuals = ui.style().interact(&response);
    let fill = if response.hovered() && enabled { theme::ACCENT } else { visuals.bg_fill };
    let ink = if !enabled {
        ui.visuals().weak_text_color()
    } else if response.hovered() {
        egui::Color32::WHITE
    } else {
        ui.visuals().text_color()
    };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 14.0, fill);
    let c = rect.center() + egui::vec2(1.5, 0.0);
    painter.add(egui::Shape::convex_polygon(
        vec![c + egui::vec2(-4.0, -5.5), c + egui::vec2(5.5, 0.0), c + egui::vec2(-4.0, 5.5)],
        ink,
        egui::Stroke::NONE,
    ));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn address_label(address: &str) -> &str {
    match address {
        "" => "nothing",
        a => a,
    }
}

impl SettingsApp {
    pub(super) fn voice_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Voice", "me-voice talks back: it tells you what's happening and you answer by voice. Everything runs on this PC.");
        let v = &mut self.edit.voice;
        let is_installed = installed();

        group(ui, |ui| {
            setting_row(ui, "Kokoro · Heart", "Kokoro-82M, full quality (fp32) · 310 MB · on this PC", |ui| {
                if is_installed {
                    ui.label(egui::RichText::new("Ready").size(11.0).color(OK));
                } else {
                    ui.label(egui::RichText::new("Not downloaded").size(11.0).weak());
                }
            });
            setting_row(ui, "Test", "Says a short sentence with the settings below.", |ui| {
                if play_button(ui, is_installed).on_hover_text("Test voice").clicked() {
                    let text = fill("Good morning{a}. Voice is ready.", &v.address);
                    say(text, v.clone());
                }
            });
            setting_row(ui, "Talk back", "Turn all spoken lines on or off. Beeps still follow Behaviour → Sounds.", |ui| {
                toggle(ui, &mut v.talk_back);
            });
            setting_row(ui, "Voice", "American English. More voices can be added later.", |ui| {
                egui::ComboBox::from_id_salt("voice-name")
                    .selected_text(if v.voice == "af_heart" { "Heart" } else { v.voice.as_str() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut v.voice, "af_heart".to_string(), "Heart");
                    });
            });
            setting_row(ui, "Speed", "", |ui| {
                ui.add(egui::Slider::new(&mut v.speed, 0.5..=2.0).suffix("×").fixed_decimals(1));
            });
            setting_row(ui, "Volume", "", |ui| {
                ui.add(egui::Slider::new(&mut v.volume, 0.0..=1.0).custom_formatter(|n, _| format!("{:.0}%", n * 100.0)));
            });
            setting_row(ui, "Call me", "How it addresses you. Pick \"Something else…\" to type a name.", |ui| {
                // A preset, or "Something else…": then a name field appears left of the menu.
                let presets = ["sir", "ma'am", ""];
                let custom_id = egui::Id::NULL.with("voice-address-custom");
                let mut custom = ui.data(|d| d.get_temp::<bool>(custom_id)).unwrap_or(!presets.contains(&v.address.as_str()));
                let shown = if custom { "Something else…".to_string() } else { address_label(&v.address).to_string() };
                // Right to left: the menu on the right, the name field just before it.
                egui::ComboBox::from_id_salt("voice-address").selected_text(shown).show_ui(ui, |ui| {
                    for p in presets {
                        if ui.selectable_label(!custom && v.address == p, address_label(p)).clicked() {
                            v.address = p.to_string();
                            custom = false;
                        }
                    }
                    if ui.selectable_label(custom, "Something else…").clicked() && !custom {
                        custom = true;
                        v.address.clear();
                    }
                });
                if custom {
                    let field = egui::TextEdit::singleline(&mut v.address).hint_text("Your name").desired_width(140.0);
                    crate::ui::text_menu::add(ui, field);
                }
                ui.data_mut(|d| d.insert_temp(custom_id, custom));
            });
        });

        let talk_back = v.talk_back;
        ui.add_enabled_ui(talk_back, |ui| {
            theme::section_label(ui, "SAY OUT LOUD WHEN…");
            group(ui, |ui| {
                for (line, title) in LINES {
                    let example = format!("\"{}\"", fill(line.variants()[0], &v.address));
                    setting_row(ui, title, &example, |ui| {
                        {
                            // Right to left: the play button first, so they line up in one column.
                            if play_button(ui, is_installed).on_hover_text("Play").clicked() {
                                say(line.text(v), v.clone());
                            }
                            ui.add_space(6.0);
                            match line {
                                Line::Stop => {
                                    egui::ComboBox::from_id_salt("stop-sound")
                                        .selected_text(stop_sound_label(v.stop_sound))
                                        .show_ui(ui, |ui| {
                                            for s in [StopSound::Beep, StopSound::Voice, StopSound::Off] {
                                                ui.selectable_value(&mut v.stop_sound, s, stop_sound_label(s));
                                            }
                                        });
                                }
                                Line::StillWorking => {
                                    egui::ComboBox::from_id_salt("still-working")
                                        .selected_text(still_working_label(v.say_still_working_minutes))
                                        .show_ui(ui, |ui| {
                                            for m in [0, 1, 2, 5] {
                                                ui.selectable_value(&mut v.say_still_working_minutes, m, still_working_label(m));
                                            }
                                        });
                                }
                                _ => {
                                    if let Some(on) = line_switch(v, line) {
                                        toggle(ui, on);
                                    }
                                }
                            }
                        }
                    });
                }
            });

            theme::section_label(ui, "YOU CAN SAY");
            group(ui, |ui| {
                setting_row(ui, "yes / read it", "Read the answer (short)", |ui| {
                    toggle(ui, &mut v.cmd_read);
                });
                setting_row(ui, "more", "Keep reading", |ui| {
                    toggle(ui, &mut v.cmd_more);
                });
                setting_row(ui, "repeat", "Say the last line again", |ui| {
                    toggle(ui, &mut v.cmd_repeat);
                });
                setting_row(ui, "stop / quiet", "Stop talking now", |ui| {
                    toggle(ui, &mut v.cmd_stop);
                });
                setting_row(ui, "status", "\"What's it doing?\"", |ui| {
                    toggle(ui, &mut v.cmd_status);
                });
                setting_row(ui, "no / that's all", "End the conversation", |ui| {
                    toggle(ui, &mut v.cmd_end);
                });
                setting_row(ui, "Anything else", "Is sent as your next message to the same T3 thread.", |ui| {
                    toggle(ui, &mut v.cmd_other);
                });
            });

            theme::section_label(ui, "CONVERSATION");
            group(ui, |ui| {
                setting_row(ui, "Listen after speaking", "After a question or \"Finished\", it listens for your answer.", |ui| {
                    toggle(ui, &mut v.listen_after_speaking);
                });
                setting_row(ui, "Wait for an answer", "", |ui| {
                    ui.add(egui::Slider::new(&mut v.answer_wait_seconds, 2..=20).suffix(" s"));
                });
                setting_row(ui, "Interrupt by talking", "Start speaking or press the shortcut to cut it off.", |ui| {
                    toggle(ui, &mut v.interrupt_by_talking);
                });
                setting_row(ui, "Wait for quiet", "While another app plays sound (music, a call), T3 updates wait until it stops.", |ui| {
                    toggle(ui, &mut v.wait_for_quiet);
                });
            });
        });

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Kokoro and its files stay on this PC. Nothing you hear is generated online.").size(12.0).weak());
    }
}
