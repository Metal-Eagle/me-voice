//! T3 Code page: dictation typed straight into T3 Code's prompt, kept as a profile for its program.

use super::*;

use crate::app::apps::T3_CODE_EXE;
use crate::settings::ProfileOverrides;

impl SettingsApp {
    pub(super) fn t3_page(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "T3 Code", "Dictation for the T3 Code app.");
        let was = t3_typing(&self.edit.profiles);
        let mut on = was;
        group(ui, |ui| {
            setting_row(ui, "Type into T3 Code", "Types dictation straight into the prompt instead of pasting it through the clipboard.", |ui| {
                toggle(ui, &mut on);
            });
        });
        if on != was {
            set_t3_typing(&mut self.edit.profiles, on);
        }
    }
}

/// Whether the profile for T3 Code types its text.
fn t3_typing(profiles: &[Profile]) -> bool {
    profiles
        .iter()
        .find(|p| p.apps.iter().any(|a| a.eq_ignore_ascii_case(T3_CODE_EXE)))
        .is_some_and(|p| p.overrides.insert_mode == Some(InsertMode::Type))
}

/// Turns typing on or off in the profile that lists T3 Code, creating it when needed.
/// Turning it off removes the profile only when nothing else is set on it.
fn set_t3_typing(profiles: &mut Vec<Profile>, on: bool) {
    let index = profiles.iter().position(|p| p.apps.iter().any(|a| a.eq_ignore_ascii_case(T3_CODE_EXE)));
    match index {
        Some(i) => {
            let profile = &mut profiles[i];
            profile.overrides.insert_mode = on.then_some(InsertMode::Type);
            let empty = profile.overrides == ProfileOverrides::default() && profile.apps.len() == 1;
            if !on && empty {
                profiles.remove(i);
            }
        }
        None if on => profiles.push(Profile {
            name: "T3 Code".into(),
            apps: vec![T3_CODE_EXE.into()],
            overrides: ProfileOverrides { insert_mode: Some(InsertMode::Type), ..Default::default() },
        }),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t3_profile(apps: &[&str], insert_mode: Option<InsertMode>) -> Profile {
        Profile {
            name: "T3 Code".into(),
            apps: apps.iter().map(|a| a.to_string()).collect(),
            overrides: ProfileOverrides { insert_mode, ..Default::default() },
        }
    }

    #[test]
    fn toggle_on_creates_then_off_removes() {
        let mut profiles = vec![];
        set_t3_typing(&mut profiles, true);
        assert!(t3_typing(&profiles));
        set_t3_typing(&mut profiles, false);
        assert!(profiles.is_empty());
    }

    #[test]
    fn toggle_reuses_existing_profile_and_keeps_its_other_overrides() {
        let mut profiles = vec![Profile {
            overrides: ProfileOverrides { press_enter_after_paste: Some(false), ..Default::default() },
            ..t3_profile(&[T3_CODE_EXE], None)
        }];
        set_t3_typing(&mut profiles, true);
        assert_eq!(profiles.len(), 1);
        assert!(t3_typing(&profiles));
        set_t3_typing(&mut profiles, false);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].overrides.press_enter_after_paste, Some(false));
        assert!(!t3_typing(&profiles));
    }

    #[test]
    fn toggle_off_keeps_profile_with_other_apps() {
        let mut profiles = vec![t3_profile(&[T3_CODE_EXE, "other.exe"], Some(InsertMode::Type))];
        set_t3_typing(&mut profiles, false);
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].overrides.insert_mode.is_none());
    }
}
