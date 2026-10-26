//! "Fix last dictation" window: edit the last text, paste it again, and teach the
//! dictionary the words that were changed. Runs as its own short-lived process
//! (`me-voice fix-last <file>`), like the settings window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::text::cleanup::{self, Dictionary};
use crate::settings::{data_dir, is_dev, Settings};
use crate::ui::theme;

/// A stretch of words that changed: `old` in the original, `new` in the edited text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub old: String,
    pub new: String,
}

/// Word-level diff (longest common subsequence), with neighbouring changes grouped.
pub fn word_diff(old: &str, new: &str) -> Vec<Change> {
    let a: Vec<&str> = old.split_whitespace().collect();
    let b: Vec<&str> = new.split_whitespace().collect();
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: common words in a[i..] and b[j..].
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut changes = Vec::new();
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(&mut changes, &mut removed, &mut added);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            added.push(b[j]);
            j += 1;
        } else {
            removed.push(a[i]);
            i += 1;
        }
    }
    flush(&mut changes, &mut removed, &mut added);
    changes
}

fn flush(changes: &mut Vec<Change>, removed: &mut Vec<&str>, added: &mut Vec<&str>) {
    if !removed.is_empty() || !added.is_empty() {
        changes.push(Change { old: removed.join(" "), new: added.join(" ") });
        removed.clear();
        added.clear();
    }
}

/// A word as the dictionary stores it: without surrounding punctuation.
fn clean_word(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// Opens the fix window for `text` as a separate process. The text goes through a file in
/// the data folder, which the window deletes as soon as it has read it.
pub fn spawn(text: &str) {
    let dir = data_dir();
    let path = dir.join("fix-last.txt");
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text)).is_err() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("fix-last").arg(&path);
        if is_dev() {
            cmd.arg("--dev");
        }
        let _ = cmd.spawn();
    }
}

/// Runs the window for the text in `path`, then pastes the fixed text if asked to.
pub fn run(path: &str) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow!("fix-last: {e}"))?;
    let _ = std::fs::remove_file(path);

    let outcome = Arc::new(Mutex::new(None));
    let app = FixApp {
        original: text.clone(),
        text,
        dictionary: cleanup::load_dictionary(),
        added: HashSet::new(),
        skipped: HashSet::new(),
        outcome: outcome.clone(),
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title(if is_dev() { "Fix last dictation (dev)" } else { "Fix last dictation" })
        .with_inner_size([520.0, 420.0])
        .with_min_inner_size([420.0, 340.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "me-voice fix",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow!("fix window: {e}"))?;

    // Only after the window has closed, and after a moment, so focus is back in the app
    // that was in front.
    let pasted = outcome.lock().unwrap().take();
    if let Some(text) = pasted {
        std::thread::sleep(Duration::from_millis(300));
        crate::text::insert::insert(&text, Settings::load().insert_mode, false)?;
    }
    Ok(())
}

struct FixApp {
    original: String,
    text: String,
    dictionary: Dictionary,
    /// Changes already added to the dictionary (keyed by `change_key`).
    added: HashSet<String>,
    /// Changes the user chose to skip.
    skipped: HashSet<String>,
    /// Set by "Paste fixed text"; read once the window has closed.
    outcome: Arc<Mutex<Option<String>>>,
}

fn change_key(change: &Change) -> String {
    format!("{}>{}", change.old, change.new)
}

impl FixApp {
    fn change_row(&mut self, ui: &mut egui::Ui, change: &Change) {
        let key = change_key(change);
        if self.skipped.contains(&key) || change.new.trim().is_empty() {
            return;
        }
        let label = if change.old.is_empty() { format!("+ {}", change.new) } else { format!("{} → {}", change.old, change.new) };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).family(theme::semibold()));
            if self.added.contains(&key) {
                ui.weak("Added");
                return;
            }
            if !change.old.trim().is_empty() && ui.small_button("Add replacement").clicked() {
                let from = clean_word(&change.old).to_lowercase();
                let to = clean_word(&change.new);
                if !from.is_empty() && !to.is_empty() {
                    self.dictionary.replacements.insert(from, to);
                    self.save();
                    self.added.insert(key.clone());
                }
            }
            if ui.small_button("Add word").clicked() {
                let word = clean_word(&change.new);
                if !word.is_empty() {
                    if !self.dictionary.words.contains(&word) {
                        self.dictionary.words.push(word);
                    }
                    self.save();
                    self.added.insert(key.clone());
                }
            }
            if ui.small_button("Skip").clicked() {
                self.skipped.insert(key.clone());
            }
        });
    }

    fn save(&self) {
        if let Err(e) = cleanup::save_dictionary(&self.dictionary) {
            crate::app::log::write(&format!("dictionary: {e}"));
        }
    }
}

impl eframe::App for FixApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::ui::text_menu::show_pending(ui.ctx(), frame);
        ui.add_space(4.0);
        ui.heading("Fix the text");
        ui.weak("Edit it and paste again, or just teach the dictionary.");
        ui.add_space(6.0);
        crate::ui::text_menu::add(ui, egui::TextEdit::multiline(&mut self.text).desired_width(f32::INFINITY).desired_rows(7));

        theme::section_label(ui, "REMEMBER FOR NEXT TIME");
        let changes = word_diff(&self.original, &self.text);
        if changes.iter().all(|c| c.new.trim().is_empty()) {
            ui.weak("No changes yet. Edit a word above and it shows up here.");
        }
        egui::ScrollArea::vertical().max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
            for change in &changes {
                self.change_row(ui, change);
            }
        });

        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(theme::primary_button("Paste fixed text")).clicked() {
                *self.outcome.lock().unwrap() = Some(self.text.clone());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(self.text.clone());
            }
            if ui.button("Cancel").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(old: &str, new: &str) -> Change {
        Change { old: old.to_string(), new: new.to_string() }
    }

    #[test]
    fn no_change_gives_no_diff() {
        assert!(word_diff("hello there", "hello there").is_empty());
    }

    #[test]
    fn one_replaced_word() {
        assert_eq!(word_diff("I like tee a lot", "I like tea a lot"), vec![change("tee", "tea")]);
    }

    #[test]
    fn inserted_word() {
        assert_eq!(word_diff("hello world", "hello big world"), vec![change("", "big")]);
    }

    #[test]
    fn two_replacements() {
        assert_eq!(word_diff("a b c d e", "a X c Y e"), vec![change("b", "X"), change("d", "Y")]);
    }
}
