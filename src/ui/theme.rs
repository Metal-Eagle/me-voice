//! Look and feel of the settings window: fonts, colours for light and dark mode,
//! cards, sidebar items and small vector icons. Change the design here.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, RichText, Stroke, TextStyle, Vec2};

/// Brand blue, matching the app icon.
pub const ACCENT: Color32 = Color32::from_rgb(0x1E, 0x9B, 0xFF);
pub const GREEN: Color32 = Color32::from_rgb(0x34, 0xC7, 0x59);
pub const ORANGE: Color32 = Color32::from_rgb(0xFF, 0x95, 0x00);
pub const PURPLE: Color32 = Color32::from_rgb(0xAF, 0x52, 0xDE);
pub const RED: Color32 = Color32::from_rgb(0xE0, 0x4F, 0x45);

/// Font family for titles and labels (Segoe UI Semibold on Windows).
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn title(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(semibold()).size(size)
}

struct Palette {
    window: Color32,
    sidebar: Color32,
    card: Color32,
    field: Color32,
    widget: Color32,
    widget_hover: Color32,
    border: Color32,
    text: Color32,
    weak: Color32,
}

const DARK: Palette = Palette {
    window: Color32::from_rgb(0x16, 0x18, 0x1B),
    sidebar: Color32::from_rgb(0x11, 0x13, 0x15),
    card: Color32::from_rgb(0x1F, 0x22, 0x26),
    field: Color32::from_rgb(0x12, 0x14, 0x17),
    widget: Color32::from_rgb(0x2A, 0x2E, 0x34),
    widget_hover: Color32::from_rgb(0x34, 0x39, 0x40),
    border: Color32::from_rgb(0x2B, 0x2F, 0x35),
    text: Color32::from_rgb(0xE8, 0xEA, 0xED),
    weak: Color32::from_rgb(0x8E, 0x95, 0x9E),
};

const LIGHT: Palette = Palette {
    window: Color32::from_rgb(0xF4, 0xF5, 0xF7),
    sidebar: Color32::from_rgb(0xEA, 0xEC, 0xF0),
    card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    field: Color32::from_rgb(0xF7, 0xF8, 0xFA),
    widget: Color32::from_rgb(0xEC, 0xEE, 0xF2),
    widget_hover: Color32::from_rgb(0xE1, 0xE4, 0xE9),
    border: Color32::from_rgb(0xE0, 0xE3, 0xE8),
    text: Color32::from_rgb(0x1C, 0x1E, 0x21),
    weak: Color32::from_rgb(0x6B, 0x72, 0x7C),
};

pub fn sidebar_fill(ui: &egui::Ui) -> Color32 {
    palette(ui).sidebar
}

fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode { &DARK } else { &LIGHT }
}

/// Fonts, spacing and colours for both light and dark mode (follows the system).
pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    for (theme, p) in [(egui::Theme::Dark, &DARK), (egui::Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (TextStyle::Heading, FontId::new(24.0, semibold())),
                (TextStyle::Body, FontId::proportional(14.5)),
                (TextStyle::Button, FontId::proportional(14.0)),
                (TextStyle::Small, FontId::proportional(12.5)),
                (TextStyle::Monospace, FontId::monospace(13.5)),
            ]
            .into();
            style.spacing.item_spacing = Vec2::new(10.0, 8.0);
            style.spacing.button_padding = Vec2::new(14.0, 7.0);
            style.spacing.interact_size.y = 32.0;
            style.spacing.combo_width = 240.0;

            let v = &mut style.visuals;
            v.panel_fill = p.window;
            v.window_fill = p.card;
            v.extreme_bg_color = p.field;
            v.faint_bg_color = p.card;
            v.override_text_color = Some(p.text);
            v.weak_text_color = Some(p.weak);
            // Translucent accent (about 40%) so selected text stays readable; the selection
            // stroke is also the colour egui paints selected text with, so use the normal text colour.
            v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x1E, 0x9B, 0xFF, 0x66);
            v.selection.stroke = Stroke::new(1.0, p.text);
            v.hyperlink_color = ACCENT;
            v.window_corner_radius = CornerRadius::same(12);
            v.menu_corner_radius = CornerRadius::same(10);
            v.window_stroke = Stroke::new(1.0, p.border);

            let radius = CornerRadius::same(8);
            let w = &mut v.widgets;
            w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
            w.noninteractive.corner_radius = radius;
            for (state, fill) in [(&mut w.inactive, p.widget), (&mut w.hovered, p.widget_hover), (&mut w.active, p.widget_hover), (&mut w.open, p.widget)] {
                state.bg_fill = fill;
                state.weak_bg_fill = fill;
                state.bg_stroke = Stroke::NONE;
                state.corner_radius = radius;
                state.expansion = 0.0;
            }
            w.hovered.bg_stroke = Stroke::new(1.0, p.border);
        });
    }
}

/// Segoe UI (and Semibold) on Windows when available; egui's bundled font otherwise.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let read = |name: &str| -> Option<Vec<u8>> {
        let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from)?.join("Fonts");
        std::fs::read(dir.join(name)).ok()
    };
    if let Some(regular) = read("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(regular).into());
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    let semibold_name = if let Some(bold) = read("seguisb.ttf") {
        fonts.font_data.insert("segoe-semibold".into(), egui::FontData::from_owned(bold).into());
        "segoe-semibold"
    } else {
        "Ubuntu-Light"
    };
    let mut family = vec![semibold_name.to_string()];
    family.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(semibold(), family);
    ctx.set_fonts(fonts);
}

/// A rounded card. `tint` gives it a soft colour (dashboard highlights).
pub fn card<R>(ui: &mut egui::Ui, tint: Option<Color32>, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = palette(ui);
    let fill = match tint {
        Some(c) => c.gamma_multiply(if ui.visuals().dark_mode { 0.14 } else { 0.09 }),
        None => p.card,
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A card holding setting rows; rows get thin dividers between them.
pub fn group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(18, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.data_mut(|d| d.insert_temp(first_row_id(), true));
            add(ui);
        });
    ui.add_space(14.0);
}

fn first_row_id() -> egui::Id {
    egui::Id::unique("me-voice-first-row")
}

/// Thin line between rows in a group (skipped before the first row).
pub fn row_divider(ui: &mut egui::Ui) {
    let first = ui.data_mut(|d| d.get_temp::<bool>(first_row_id()).unwrap_or(false));
    if first {
        ui.data_mut(|d| d.insert_temp(first_row_id(), false));
        return;
    }
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(rect.left()..=rect.right(), y, Stroke::new(1.0, palette(ui).border));
    ui.add_space(1.0);
}

/// Small grey heading above a group.
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).family(semibold()).size(13.0).weak());
    ui.add_space(2.0);
}

/// Small square button with a painted cross (Segoe UI has no ✕ glyph), for removing a row.
pub fn remove_button(ui: &mut egui::Ui) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    let (c, r) = (rect.center(), 4.5);
    let stroke = Stroke::new(1.5, visuals.fg_stroke.color);
    painter.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
    painter.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
    response.on_hover_text("Remove")
}

/// Sidebar item with icon; returns true when clicked.
pub fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> bool {
    let size = Vec2::new(ui.available_width(), 38.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = palette(ui);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 9.0, ACCENT.gamma_multiply(if ui.visuals().dark_mode { 0.22 } else { 0.14 }));
        let bar = egui::Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 9.0), Vec2::new(3.0, 18.0));
        painter.rect_filled(bar, 2.0, ACCENT);
    } else if response.hovered() {
        painter.rect_filled(rect, 9.0, p.widget.gamma_multiply(0.7));
    }
    let colour = if selected { ACCENT } else { p.weak };
    paint_icon(painter, Pos2::new(rect.left() + 22.0, rect.center().y), icon, colour, 0.85);
    let text_colour = if selected { p.text } else { p.text.gamma_multiply(0.85) };
    let font = if selected { FontId::new(14.5, semibold()) } else { FontId::proportional(14.5) };
    painter.text(Pos2::new(rect.left() + 42.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font, text_colour);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

/// A large card for picking one option (the models on the Speech page). The whole card is
/// the click target; `add` draws the contents, which may hold buttons of their own.
pub fn choice_card(ui: &mut egui::Ui, size: Vec2, selected: bool, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = palette(ui);
    let hovered = ui.rect_contains_pointer(rect);
    let fill = if selected {
        blend(p.card, ACCENT, if ui.visuals().dark_mode { 0.12 } else { 0.06 })
    } else if hovered {
        blend(p.card, p.widget, 0.5)
    } else {
        p.card
    };
    let stroke = if selected {
        Stroke::new(2.0, ACCENT)
    } else if hovered {
        Stroke::new(1.0, ACCENT.gamma_multiply(0.6))
    } else {
        Stroke::new(1.0, p.border)
    };
    ui.painter().rect(rect, 12.0, fill, stroke, egui::StrokeKind::Inside);

    // Radio mark in the top-right corner.
    let dot = Pos2::new(rect.right() - 22.0, rect.top() + 24.0);
    if selected {
        ui.painter().circle_filled(dot, 9.0, ACCENT);
        ui.painter().circle_filled(dot, 3.5, Color32::WHITE);
    } else {
        ui.painter().circle_stroke(dot, 8.0, Stroke::new(1.5, if hovered { ACCENT } else { p.weak.gamma_multiply(0.7) }));
    }

    let inner = egui::Rect::from_min_max(rect.min + Vec2::new(16.0, 14.0), rect.max - Vec2::new(16.0, 14.0));
    // A child that doesn't move the parent's cursor, so cards sit exactly side by side.
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    add(&mut content);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Small coloured label, e.g. DEFAULT or MULTILINGUAL on a model card.
pub fn tag(ui: &mut egui::Ui, text: &str, colour: Color32) {
    egui::Frame::new()
        .fill(colour.gamma_multiply(if ui.visuals().dark_mode { 0.2 } else { 0.13 }))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(text).family(semibold()).size(11.0).color(colour)));
}

/// A row of options in a rounded track, the chosen one raised. Returns true when changed.
pub fn segmented<T: PartialEq + Clone>(ui: &mut egui::Ui, value: &mut T, options: &[(T, String)]) -> bool {
    let p = palette(ui);
    let mut changed = false;
    egui::Frame::new().fill(p.widget).corner_radius(CornerRadius::same(9)).inner_margin(Margin::same(3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // Inside a right-to-left row (setting controls) the buttons are laid out from the
            // right, so add them in reverse to keep their reading order.
            let reverse = ui.layout().main_dir() == egui::Direction::RightToLeft;
            let ordered: Vec<&(T, String)> = if reverse { options.iter().rev().collect() } else { options.iter().collect() };
            for (option, label) in ordered {
                let on = value == option;
                let text = if on { RichText::new(label).family(semibold()) } else { RichText::new(label).color(p.weak) };
                let button = egui::Button::new(text.size(13.5))
                    .fill(if on { p.card } else { Color32::TRANSPARENT })
                    .stroke(if on { Stroke::new(1.0, p.border) } else { Stroke::NONE })
                    .corner_radius(CornerRadius::same(7))
                    .min_size(Vec2::new(0.0, 28.0));
                let response = ui.add(button);
                if response.hovered() && !on {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() && !on {
                    *value = option.clone();
                    changed = true;
                }
            }
        });
    });
    changed
}

/// `a` mixed with `t` of `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Filled accent button.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).family(semibold()).color(Color32::WHITE)).fill(ACCENT)
}

#[derive(Clone, Copy)]
pub enum Icon {
    Mic,
    Keyboard,
    Lines,
    Gauge,
    Bars,
    Grid,
    Clock,
    Sliders,
    Text,
    Info,
    Layers,
    Sparkle,
}

/// Coloured round badge with an icon (dashboard cards).
pub fn badge(ui: &mut egui::Ui, colour: Color32, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 10.0, colour.gamma_multiply(0.18));
    paint_icon(ui.painter(), rect.center(), icon, colour, 1.0);
}

/// Simple line icons drawn with the painter (no icon font needed). `s` scales them.
pub fn paint_icon(p: &egui::Painter, c: Pos2, icon: Icon, colour: Color32, s: f32) {
    let stroke = Stroke::new(1.8 * s.max(0.9), colour);
    let at = |x: f32, y: f32| Pos2::new(c.x + x * s, c.y + y * s);
    let arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32| -> Vec<Pos2> {
        (0..=16).map(|i| {
            let a = from + (to - from) * i as f32 / 16.0;
            at(cx + r * a.cos(), cy + r * a.sin())
        }).collect()
    };
    use std::f32::consts::PI;
    match icon {
        Icon::Mic => {
            p.rect_filled(egui::Rect::from_center_size(at(0.0, -2.5), Vec2::new(6.0, 10.0) * s), 3.0 * s, colour);
            p.add(egui::Shape::line(arc(0.0, -1.0, 6.0, 0.0, PI), stroke));
            p.line_segment([at(0.0, 5.0), at(0.0, 8.0)], stroke);
        }
        Icon::Keyboard => {
            p.rect_stroke(egui::Rect::from_center_size(c, Vec2::new(18.0, 12.0) * s), 2.5 * s, stroke, egui::StrokeKind::Middle);
            for row in [-2.5_f32, 1.0] {
                for col in [-5.0_f32, -1.7, 1.7, 5.0] {
                    p.circle_filled(at(col, row), 1.0 * s, colour);
                }
            }
            p.line_segment([at(-4.0, 4.0), at(4.0, 4.0)], stroke);
        }
        Icon::Lines | Icon::Text => {
            for (y, w) in [(-5.0_f32, 16.0_f32), (0.0, 16.0), (5.0, 10.0)] {
                p.line_segment([at(-8.0, y), at(-8.0 + w, y)], stroke);
            }
        }
        Icon::Gauge => {
            p.add(egui::Shape::line(arc(0.0, 3.0, 8.0, PI, 2.0 * PI), stroke));
            p.line_segment([at(0.0, 3.0), at(4.5, -3.0)], stroke);
            p.circle_filled(at(0.0, 3.0), 2.0 * s, colour);
        }
        Icon::Bars => {
            for (x, h) in [(-6.0_f32, 7.0_f32), (0.0, 12.0), (6.0, 9.0)] {
                p.rect_filled(egui::Rect::from_min_max(at(x - 2.0, 6.0 - h), at(x + 2.0, 6.0)), 1.0, colour);
            }
        }
        Icon::Grid => {
            for (x, y) in [(-4.5_f32, -4.5_f32), (4.5, -4.5), (-4.5, 4.5), (4.5, 4.5)] {
                p.rect_stroke(egui::Rect::from_center_size(at(x, y), Vec2::splat(6.5 * s)), 1.5 * s, stroke, egui::StrokeKind::Middle);
            }
        }
        Icon::Clock => {
            p.circle_stroke(c, 8.0 * s, stroke);
            p.line_segment([c, at(0.0, -5.0)], stroke);
            p.line_segment([c, at(3.5, 2.0)], stroke);
        }
        Icon::Sliders => {
            for (y, knob) in [(-5.0_f32, 3.0_f32), (0.0, -4.0), (5.0, 1.0)] {
                p.line_segment([at(-8.0, y), at(8.0, y)], stroke);
                p.circle_filled(at(knob, y), 2.6 * s, colour);
            }
        }
        Icon::Sparkle => {
            // A four-pointed star.
            let points: Vec<egui::Pos2> = (0..8)
                .map(|i| {
                    let r = if i % 2 == 0 { 8.5 } else { 2.6 };
                    let a = i as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
                    at(r * a.cos(), r * a.sin())
                })
                .collect();
            p.add(egui::Shape::closed_line(points, stroke));
        }
        Icon::Info => {
            p.circle_stroke(c, 8.0 * s, stroke);
            p.line_segment([at(0.0, -1.0), at(0.0, 4.5)], stroke);
            p.circle_filled(at(0.0, -4.5), 1.2 * s, colour);
        }
        Icon::Layers => {
            // Three stacked diamonds.
            for y in [-5.0_f32, 0.0, 5.0] {
                p.add(egui::Shape::closed_line(vec![at(-8.0, y), at(0.0, y - 4.0), at(8.0, y), at(0.0, y + 4.0)], stroke));
            }
        }
    }
}
