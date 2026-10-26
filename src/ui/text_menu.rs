//! Right-click menu (Cut, Copy, Paste, Select all) for egui text boxes, which have none
//! of their own, and Copy for selected label text. It is the system's own menu (Windows or macOS). Cut and Paste are given
//! to the text box as keyboard actions, so egui's own editing (and undo) handles them.

use eframe::egui::{self, text::CCursorRange, text_edit::TextEditState, text_selection::LabelSelectionState, Event, Id, PointerButton, Response, TextEdit, Ui};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem};

/// Where a right-clicked text box waits for the menu to be shown: its id.
fn pending_id() -> Id {
    Id::unique("text_menu_pending")
}

/// Where a text box's selection and text from before the right-click are kept: the
/// right-click itself moves the cursor, which would leave nothing to copy or cut.
fn selection_id(text_box: Id) -> Id {
    text_box.with("text_menu_selection")
}

/// Adds a text box with a right-click menu. Use instead of `ui.add(TextEdit::...)`.
pub fn add(ui: &mut Ui, edit: TextEdit) -> Response {
    let output = edit.show(ui);
    let response = output.response.response;
    let id = response.id;
    let (right_down, right_clicked) =
        ui.input(|i| (i.pointer.button_down(PointerButton::Secondary), i.pointer.button_clicked(PointerButton::Secondary)));
    if right_clicked && response.hovered() {
        ui.ctx().memory_mut(|m| m.request_focus(id));
        ui.ctx().data_mut(|d| d.insert_temp(pending_id(), id));
        ui.ctx().request_repaint();
    } else if !right_down && !right_clicked {
        let text = output.galley.text().to_string();
        ui.ctx().data_mut(|d| d.insert_temp(selection_id(id), (output.cursor_range, text)));
    }
    response
}

/// Set when label text (not a text box) is selected and right-clicked.
fn label_pending_id() -> Id {
    Id::unique("text_menu_label_pending")
}

/// Call at the start of each frame, before the text boxes are drawn: shows the menu for a
/// text box or selected label text right-clicked in the last frame, and carries out the
/// chosen item.
pub fn show_pending(ctx: &egui::Context, frame: &eframe::Frame) {
    let (target, label) = ctx.data_mut(|d| {
        let target = d.get_temp::<Id>(pending_id());
        let label = d.get_temp::<bool>(label_pending_id()).unwrap_or(false);
        d.remove::<Id>(pending_id());
        d.remove::<bool>(label_pending_id());
        (target, label)
    });
    // A right-click with label text selected; checked now, before this frame's labels
    // run, so a text box under the pointer (handled above, next frame) takes precedence.
    if target.is_none()
        && ctx.input(|i| i.pointer.button_clicked(PointerButton::Secondary))
        && ctx.plugin::<LabelSelectionState>().lock().has_selection()
    {
        ctx.data_mut(|d| d.insert_temp(label_pending_id(), true));
        ctx.request_repaint();
    }
    let Some(target) = target else {
        if label {
            let copy = MenuItem::new("Copy", true, None);
            let menu = Menu::new();
            let _ = menu.append(&copy);
            if popup(&menu, frame).is_some_and(|e| e.id == *copy.id()) {
                // egui copies the selected label text when it sees a Copy event.
                ctx.input_mut(|i| i.events.push(Event::Copy));
                ctx.request_repaint();
            }
        }
        return;
    };
    let (selection, text) = ctx
        .data(|d| d.get_temp::<(Option<CCursorRange>, String)>(selection_id(target)))
        .unwrap_or_default();
    let selected: String = match selection {
        Some(range) => {
            let r = range.as_sorted_char_range();
            text.chars().skip(r.start.0).take(r.end.0 - r.start.0).collect()
        }
        None => String::new(),
    };

    let cut = MenuItem::new("Cut", !selected.is_empty(), None);
    let copy = MenuItem::new("Copy", !selected.is_empty(), None);
    let paste = MenuItem::new("Paste", true, None);
    let select_all = MenuItem::new("Select all", !text.is_empty(), None);
    let menu = Menu::new();
    let _ = menu.append_items(&[&cut, &copy, &paste, &PredefinedMenuItem::separator(), &select_all]);

    let Some(chosen) = popup(&menu, frame) else { return };

    // Put the selection back as it was before the right-click, and focus the text box.
    let mut state = TextEditState::load(ctx, target).unwrap_or_default();
    if chosen.id == *select_all.id() {
        state.cursor.set_char_range(Some(CCursorRange::select_all_str(&text)));
    } else {
        state.cursor.set_char_range(selection);
    }
    state.store(ctx, target);
    ctx.memory_mut(|m| m.request_focus(target));

    if chosen.id == *copy.id() {
        ctx.copy_text(selected);
    } else if chosen.id == *cut.id() {
        ctx.input_mut(|i| i.events.push(Event::Cut));
    } else if chosen.id == *paste.id() {
        if let Ok(clip) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            ctx.input_mut(|i| i.events.push(Event::Paste(clip)));
        }
    }
    ctx.request_repaint();
}

/// Shows `menu` at the pointer as the system's own menu and returns the chosen item, if any.
fn popup(menu: &Menu, frame: &eframe::Frame) -> Option<MenuEvent> {
    // Clear events left over from an earlier menu, then show this one; it returns once closed.
    while MenuEvent::receiver().try_recv().is_ok() {}
    let handle = frame.window_handle().ok()?;
    // Safety: the handle belongs to this window, which is open while the menu is shown.
    unsafe {
        match handle.as_raw() {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => {
                menu.show_context_menu_for_hwnd(h.hwnd.get(), None);
            }
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(h) => {
                menu.show_context_menu_for_nsview(h.ns_view.as_ptr(), None);
            }
            _ => return None,
        }
    }
    MenuEvent::receiver().try_recv().ok()
}
