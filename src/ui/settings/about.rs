//! About page: version, updates and release notes.

use super::*;
use crate::app::update::{self, Release};

/// Where the About page's update check is.
pub(super) enum UpdateState {
    NotChecked,
    Checking,
    UpToDate,
    /// Newer release found; it can be installed from here when it has a Windows installer.
    Available(Release),
    Downloading,
    /// The installer is running; it closes me-voice and starts the new version.
    Installing,
    Failed(String),
}

impl SettingsApp {
    pub(super) fn about_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "About", "Speech is transcribed locally. Optional AI sends text to your chosen provider.");
        group(ui, |ui| {
            setting_row(ui, "Version", "", |ui| {
                ui.label(format!("me-voice {}", update::CURRENT));
            });
            self.update_row(ui);
            folder_rows(ui);
        });
        ui.add_space(20.0);
        ui.heading("What's new");
        ui.add_space(8.0);
        group(ui, |ui| release_notes(ui, CHANGELOG));
    }

    fn update_row(&mut self, ui: &mut egui::Ui) {
        let description = match &*self.update.lock().unwrap() {
            UpdateState::NotChecked => "Looks for a new version on GitHub.".to_string(),
            UpdateState::Checking => "Checking…".to_string(),
            UpdateState::UpToDate => "You're up to date.".to_string(),
            UpdateState::Available(r) if update::can_install(r) => format!("me-voice {} is available.", r.version),
            UpdateState::Available(r) => format!("me-voice {} is available at {}", r.version, update::releases_page()),
            UpdateState::Downloading => "Downloading the update…".to_string(),
            UpdateState::Installing => "Installing. me-voice restarts in a moment.".to_string(),
            UpdateState::Failed(e) => format!("Couldn't check for updates: {e}"),
        };
        let state = self.update.clone();
        setting_row(ui, "Updates", &description, |ui| {
            let mut guard = state.lock().unwrap();
            match &*guard {
                UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing => {
                    ui.spinner();
                }
                UpdateState::Available(r) if update::can_install(r) => {
                    if ui.add(theme::primary_button("Install now")).clicked() {
                        let release = r.clone();
                        *guard = UpdateState::Downloading;
                        let ctx = ui.ctx().clone();
                        let state = state.clone();
                        std::thread::spawn(move || {
                            let result = update::download(&release).and_then(|path| update::install(&path));
                            *state.lock().unwrap() = match result {
                                Ok(()) => UpdateState::Installing,
                                Err(e) => UpdateState::Failed(format!("{e:#}")),
                            };
                            ctx.request_repaint();
                        });
                    }
                }
                _ => {
                    if ui.button("Check now").clicked() {
                        *guard = UpdateState::Checking;
                        let ctx = ui.ctx().clone();
                        let state = state.clone();
                        std::thread::spawn(move || {
                            *state.lock().unwrap() = match update::check() {
                                Ok(Some(release)) => UpdateState::Available(release),
                                Ok(None) => UpdateState::UpToDate,
                                Err(e) => UpdateState::Failed(format!("{e:#}")),
                            };
                            ctx.request_repaint();
                        });
                    }
                }
            }
        });
    }
}

/// Buttons that open the models folder, the settings folder and the log.
fn folder_rows(ui: &mut egui::Ui) {
    setting_row(ui, "Models", "FUTO Whisper ACFT, stored on this computer.", |ui| {
        if ui.button("Open folder").clicked() {
            let _ = std::fs::create_dir_all(models_dir());
            open_path(&models_dir());
        }
    });
    setting_row(ui, "Settings files", "settings.json, dictionary.json (words, replacements, snippets) and commands.json.", |ui| {
        if ui.button("Open folder").clicked() {
            let _ = std::fs::create_dir_all(config_dir());
            open_path(&config_dir());
        }
    });
    setting_row(ui, "Log", "Useful when something doesn't work.", |ui| {
        if ui.button("Open log").clicked() {
            open_path(&crate::app::log::path());
        }
    });
}

/// The release notes, embedded at build time from CHANGELOG.md.
pub(super) const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// One line of CHANGELOG.md as the About page shows it.
#[derive(Debug, PartialEq)]
pub(super) enum NoteLine<'a> {
    Version(&'a str),
    Item(&'a str),
}

/// The version headings (`## 0.8.0 - date`) and bullets of a changelog; the title and blank lines are skipped.
pub(super) fn parse_notes(text: &str) -> Vec<NoteLine<'_>> {
    let mut unreleased = false;
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(version) = line.strip_prefix("## ") {
                unreleased = version.eq_ignore_ascii_case("Unreleased");
                (!unreleased).then_some(NoteLine::Version(version))
            } else if unreleased {
                None
            } else {
                line.strip_prefix("- ").map(NoteLine::Item)
            }
        })
        .collect()
}

pub(super) fn release_notes(ui: &mut egui::Ui, text: &str) {
    for line in parse_notes(text) {
        match line {
            NoteLine::Version(version) => {
                theme::row_divider(ui);
                ui.add_space(10.0);
                ui.label(egui::RichText::new(version).family(theme::semibold()));
            }
            NoteLine::Item(item) => {
                ui.add(egui::Label::new(format!("•  {item}")).wrap());
            }
        }
    }
    ui.add_space(10.0);
}

#[cfg(test)]
mod release_notes_tests {
    use super::*;

    #[test]
    fn parses_versions_and_bullets() {
        let text = "# Changelog\n\n## 0.8.0 - 2026-10-22\n\n- First\n- Second\n\n## 0.7.0 - 2026-10-01\n\n- Old\n";
        assert_eq!(
            parse_notes(text),
            vec![
                NoteLine::Version("0.8.0 - 2026-10-22"),
                NoteLine::Item("First"),
                NoteLine::Item("Second"),
                NoteLine::Version("0.7.0 - 2026-10-01"),
                NoteLine::Item("Old"),
            ]
        );
    }

    #[test]
    fn skips_unreleased_notes() {
        let text = "# Changelog\n\n## Unreleased\n\n- Not published yet\n\n## 0.8.0 - 2026-10-22\n\n- Released\n";
        assert_eq!(
            parse_notes(text),
            vec![NoteLine::Version("0.8.0 - 2026-10-22"), NoteLine::Item("Released")]
        );
    }

    #[test]
    fn bundled_changelog_has_current_version() {
        let first = parse_notes(CHANGELOG).into_iter().next();
        assert!(matches!(first, Some(NoteLine::Version(v)) if v.starts_with(env!("CARGO_PKG_VERSION"))));
    }
}
