//! Dashboard and History pages of the settings window.

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::text::history::{self, format_duration, Entry, Stats};
use crate::ui::theme::{badge, card, Icon, ACCENT as BLUE, GREEN, ORANGE, PURPLE, RED};

/// Small caps label above a number.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).family(crate::ui::theme::semibold()).size(11.0).weak().extra_letter_spacing(1.2));
}

fn stat_card(ui: &mut egui::Ui, colour: Color32, icon: Icon, label: &str, value: String) {
    card(ui, None, |ui| {
        ui.horizontal(|ui| {
            badge(ui, colour, icon);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.weak(label);
                ui.label(crate::ui::theme::title(value, 22.0));
            });
        });
    });
}

pub fn dashboard(ui: &mut egui::Ui, stats: &Stats, typing_wpm: &mut u32) {
    if stats.sessions == 0 {
        ui.heading("Dashboard");
        ui.add_space(8.0);
        card(ui, Some(BLUE), |ui| {
            ui.label(crate::ui::theme::title("No dictations yet", 18.0));
            ui.weak("Press your shortcut and start talking. Your numbers show up here.");
        });
        return;
    }

    // Headline: how much faster than typing.
    card(ui, None, |ui| {
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                // Centre the three-part headline.
                let text = format!("You are {:.1}x faster with me-voice", stats.speedup());
                let width = ui.fonts_mut(|f| f.layout_no_wrap(text, egui::FontId::new(24.0, crate::ui::theme::semibold()), Color32::WHITE).size().x);
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(crate::ui::theme::title("You are ", 24.0));
                ui.label(crate::ui::theme::title(format!("{:.1}x faster", stats.speedup()), 24.0).color(GREEN));
                ui.label(crate::ui::theme::title(" with me-voice", 24.0));
            });
        });
        ui.add_space(10.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], Some(GREEN), |ui| {
                ui.horizontal(|ui| {
                    badge(ui, GREEN, Icon::Mic);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(crate::ui::theme::title(format_duration(stats.speaking_seconds), 20.0));
                        caption(ui, "Speaking time");
                    });
                });
            });
            card(&mut cols[1], Some(ORANGE), |ui| {
                ui.horizontal(|ui| {
                    badge(ui, ORANGE, Icon::Keyboard);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(crate::ui::theme::title(format_duration(stats.typing_seconds), 20.0));
                        caption(ui, "Typing time");
                    });
                });
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                caption(ui, "Time saved");
                ui.label(crate::ui::theme::title(format_duration(stats.time_saved_seconds()), 30.0).color(BLUE));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::DragValue::new(typing_wpm).range(10..=150).suffix(" wpm"))
                    .on_hover_text("Your typing speed, used to work out typing time. Drag or click to change.");
                ui.weak("Typing speed");
            });
        });
    });
    ui.add_space(10.0);

    ui.columns(2, |cols| {
        stat_card(&mut cols[0], BLUE, Icon::Lines, "Words captured", group_thousands(stats.words));
        stat_card(&mut cols[1], GREEN, Icon::Mic, "Voice-to-text sessions", group_thousands(stats.sessions));
    });
    ui.add_space(4.0);
    ui.columns(2, |cols| {
        stat_card(&mut cols[0], ORANGE, Icon::Gauge, "Average words per minute", format!("{:.1}", stats.words_per_minute));
        stat_card(&mut cols[1], PURPLE, Icon::Bars, "Words per session", format!("{:.1}", stats.words_per_session));
    });
    ui.add_space(10.0);

    card(ui, None, |ui| {
        ui.label(crate::ui::theme::title("30-day trend", 15.0));
        ui.weak("Words per day");
        ui.add_space(6.0);
        trend_chart(ui, &stats.last_30_days);
    });

    if !stats.per_app.is_empty() {
        ui.add_space(10.0);
        caption(ui, "Where you dictate · last 30 days");
        ui.add_space(4.0);
        card(ui, None, |ui| {
            let max = stats.per_app.iter().map(|(_, w)| *w).max().unwrap_or(1).max(1);
            for (i, (label, words)) in stats.per_app.iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                }
                // Only the top row carries the unit, the rest show plain numbers.
                let count = if i == 0 { format!("{} words", group_thousands(*words)) } else { group_thousands(*words) };
                app_row(ui, label, *words as f32 / max as f32, &count);
            }
        });
    }
}

/// One program in the "Where you dictate" card: badge, name, share bar and word count.
fn app_row(ui: &mut egui::Ui, label: &str, share: f32, count: &str) {
    let other = label == "Other";
    let colour = if other { ui.visuals().weak_text_color().gamma_multiply(0.5) } else { app_colour(label) };
    let bar = if other { colour } else { BLUE };
    ui.horizontal(|ui| {
        let (square, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
        ui.painter().rect_filled(square, egui::CornerRadius::same(6), colour);
        let letter = label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        ui.painter().text(square.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::new(13.0, crate::ui::theme::semibold()), Color32::WHITE);
        ui.add_space(8.0);

        let name_w = (ui.available_width() * 0.32).clamp(70.0, 140.0);
        ui.add_sized([name_w, 26.0], egui::Label::new(RichText::new(label)).truncate());

        let count_w = 96.0;
        let bar_w = (ui.available_width() - count_w - 8.0).max(24.0);
        let (track, _) = ui.allocate_exact_size(Vec2::new(bar_w, 8.0), egui::Sense::hover());
        ui.painter().rect_filled(track, egui::CornerRadius::same(4), ui.visuals().widgets.inactive.bg_fill);
        let fill = egui::Rect::from_min_size(track.min, Vec2::new(track.width() * share.clamp(0.0, 1.0), track.height()));
        ui.painter().rect_filled(fill, egui::CornerRadius::same(4), bar);

        ui.add_space(8.0);
        ui.allocate_ui_with_layout(Vec2::new(count_w, 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(count);
        });
    });
}

/// Stable colour for a program: the same name always gets the same theme colour.
fn app_colour(label: &str) -> Color32 {
    const COLOURS: [Color32; 5] = [BLUE, GREEN, ORANGE, PURPLE, RED];
    let hash = label.bytes().fold(5381u32, |h, b| h.wrapping_mul(33).wrapping_add(b as u32));
    COLOURS[hash as usize % COLOURS.len()]
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Smooth area chart with gridlines, a y-axis on the right and weekly date labels.
fn trend_chart(ui: &mut egui::Ui, days: &[u64; 30]) {
    let height = 200.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let grid = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let text = ui.visuals().weak_text_color();

    let plot = egui::Rect::from_min_max(rect.min + Vec2::new(4.0, 6.0), rect.max - Vec2::new(44.0, 22.0));
    let max = nice_max(*days.iter().max().unwrap_or(&0));

    // Horizontal gridlines with values on the right.
    for i in 0..=3 {
        let v = max * i / 3;
        let y = plot.bottom() - plot.height() * i as f32 / 3.0;
        painter.line_segment([egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], Stroke::new(1.0, grid));
        painter.text(egui::pos2(plot.right() + 8.0, y), egui::Align2::LEFT_CENTER, v.to_string(), egui::FontId::proportional(11.0), text);
    }

    let point = |i: usize, v: u64| {
        egui::pos2(
            plot.left() + plot.width() * i as f32 / 29.0,
            plot.bottom() - plot.height() * (v as f32 / max.max(1) as f32),
        )
    };

    // Date labels once a week, ending today.
    let today = chrono::Local::now().date_naive();
    for i in (1..30).step_by(7) {
        let date = today - chrono::Days::new((29 - i) as u64);
        let x = point(i, 0).x;
        painter.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], Stroke::new(0.5, grid.gamma_multiply(0.6)));
        painter.text(egui::pos2(x, plot.bottom() + 6.0), egui::Align2::CENTER_TOP, date.format("%-d %b").to_string(), egui::FontId::proportional(11.0), text);
    }

    // Catmull-Rom smoothing through the daily points.
    let raw: Vec<egui::Pos2> = days.iter().enumerate().map(|(i, &v)| point(i, v)).collect();
    let mut smooth = Vec::new();
    for i in 0..raw.len() - 1 {
        let p0 = raw[i.saturating_sub(1)];
        let (p1, p2) = (raw[i], raw[i + 1]);
        let p3 = raw[(i + 2).min(raw.len() - 1)];
        for s in 0..8 {
            let t = s as f32 / 8.0;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            let y = f(p0.y, p1.y, p2.y, p3.y).clamp(plot.top(), plot.bottom());
            smooth.push(egui::pos2(f(p0.x, p1.x, p2.x, p3.x), y));
        }
    }
    smooth.push(*raw.last().unwrap());

    // Filled area as thin vertical strips (works for any curve shape), then the line.
    let fill = BLUE.gamma_multiply(0.18);
    for w in smooth.windows(2) {
        let quad = vec![w[0], w[1], egui::pos2(w[1].x, plot.bottom()), egui::pos2(w[0].x, plot.bottom())];
        painter.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
    }
    painter.add(egui::Shape::line(smooth, Stroke::new(2.0, BLUE)));

    // Hover: show the day's number.
    if let Some(pos) = response.hover_pos() {
        let i = (((pos.x - plot.left()) / plot.width() * 29.0).round() as i64).clamp(0, 29) as usize;
        let p = raw[i];
        painter.circle_filled(p, 4.0, BLUE);
        let date = today - chrono::Days::new((29 - i) as u64);
        response.on_hover_text_at_pointer(format!("{}: {} words", date.format("%a %-d %b"), days[i]));
    }
}

/// Rounds the chart's top value up to 1, 2 or 5 × a power of ten.
fn nice_max(v: u64) -> u64 {
    if v == 0 {
        return 30;
    }
    let mut step = 1;
    loop {
        for m in [1, 2, 5] {
            if m * step * 3 >= v {
                return m * step * 3;
            }
        }
        step *= 10;
    }
}

/// State of the History page.
#[derive(Default)]
pub struct HistoryView {
    pub entries: Vec<Entry>,
    pub search: String,
    pub confirm_clear: bool,
}

pub fn history(ui: &mut egui::Ui, view: &mut HistoryView, keep_text: &mut bool) {
    ui.heading("History");
    ui.weak("Your past dictations, newest first. Stored only on this computer.");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        crate::ui::text_menu::add(ui, egui::TextEdit::singleline(&mut view.search).hint_text("Search…").desired_width(240.0));
        if ui.button("Refresh").clicked() {
            view.entries = history::load();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if view.confirm_clear {
                if ui.add(egui::Button::new(RichText::new("Yes, delete all").color(Color32::WHITE)).fill(RED)).clicked() {
                    history::clear();
                    view.entries.clear();
                    view.confirm_clear = false;
                }
                if ui.button("Keep").clicked() {
                    view.confirm_clear = false;
                }
            } else if ui.add_enabled(!view.entries.is_empty(), egui::Button::new("Clear history…")).clicked() {
                view.confirm_clear = true;
            }
        });
    });
    ui.checkbox(keep_text, "Keep the text of each dictation (the dashboard numbers are always kept)");
    ui.add_space(8.0);

    let needle = view.search.to_lowercase();
    let mut shown = 0;
    for entry in view.entries.iter().rev() {
        if !needle.is_empty() && !entry.text.to_lowercase().contains(&needle) {
            continue;
        }
        shown += 1;
        if shown > 300 {
            ui.weak("Showing the latest 300 dictations.");
            break;
        }
        card(ui, None, |ui| {
            ui.horizontal(|ui| {
                let when = chrono::DateTime::from_timestamp(entry.time, 0)
                    .map(|t| t.with_timezone(&chrono::Local).format("%a %-d %b, %H:%M").to_string())
                    .unwrap_or_default();
                ui.label(crate::ui::theme::title(when, 14.5));
                ui.weak(format!("· {} words · {}", entry.words, format_duration(entry.seconds as f64)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !entry.text.is_empty() {
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(entry.text.clone());
                        }
                        if ui.small_button("Fix…").clicked() {
                            crate::ui::fix_window::spawn(&entry.text);
                        }
                    }
                });
            });
            if entry.text.is_empty() {
                ui.weak("(text not kept)");
            } else {
                ui.add(egui::Label::new(&entry.text).wrap());
            }
        });
        ui.add_space(2.0);
    }
    if shown == 0 {
        ui.weak(if view.entries.is_empty() { "No dictations yet." } else { "Nothing matches your search." });
    }
}
