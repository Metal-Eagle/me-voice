//! Per-app profiles: settings that change depending on the app in front.

use crate::settings::{Profile, ProfileOverrides, Settings};

/// The settings to use for one dictation: the user's settings with the matching profile's
/// changes applied. `forced` (a profile name, from an extra shortcut) wins over `app`.
pub fn effective(settings: &Settings, app: Option<&str>, forced: Option<&str>) -> Settings {
    let mut out = settings.clone();
    if let Some(profile) = matching(settings, app, forced) {
        apply(&mut out, &profile.overrides);
    }
    out
}

/// The profile for this dictation: the forced one if it exists, else the first profile
/// that lists `app` (both compared without case).
pub fn matching<'a>(settings: &'a Settings, app: Option<&str>, forced: Option<&str>) -> Option<&'a Profile> {
    let forced = forced.and_then(|name| settings.profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name)));
    forced.or_else(|| {
        let app = app?;
        settings.profiles.iter().find(|p| p.apps.iter().any(|a| a.eq_ignore_ascii_case(app)))
    })
}

/// Copies every override that is set onto `settings`.
fn apply(settings: &mut Settings, o: &ProfileOverrides) {
    if let Some(v) = &o.language {
        settings.language = v.clone();
    }
    if let Some(v) = &o.model_id {
        settings.model_id = v.clone();
    }
    if let Some(v) = o.insert_mode {
        settings.insert_mode = v;
    }
    if let Some(v) = o.press_enter_after_paste {
        settings.press_enter_after_paste = v;
    }
    if let Some(v) = o.drop_final_period {
        settings.drop_final_period = v;
    }
    if let Some(v) = o.remove_fillers {
        settings.remove_fillers = v;
    }
    if let Some(v) = o.spoken_formatting {
        settings.spoken_formatting = v;
    }
    if let Some(v) = o.ai_polish {
        settings.ai_polish = v;
    }
    if let Some(v) = &o.ai_style {
        settings.ai_style = v.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::InsertMode;

    fn settings() -> Settings {
        let chat = Profile {
            name: "Chat".into(),
            apps: vec!["ms-teams.exe".into(), "slack.exe".into()],
            overrides: ProfileOverrides {
                press_enter_after_paste: Some(true),
                insert_mode: Some(InsertMode::Type),
                ..Default::default()
            },
        };
        let dutch = Profile {
            name: "Nederlands".into(),
            apps: vec![],
            overrides: ProfileOverrides { language: Some("nl".into()), ..Default::default() },
        };
        Settings { profiles: vec![chat, dutch], ..Default::default() }
    }

    #[test]
    fn no_match_keeps_defaults() {
        let s = settings();
        assert_eq!(effective(&s, Some("notepad.exe"), None), s);
        assert_eq!(effective(&s, None, None), s);
        assert!(matching(&s, Some("notepad.exe"), None).is_none());
    }

    #[test]
    fn app_match_applies_overrides_only() {
        let s = settings();
        let e = effective(&s, Some("slack.exe"), None);
        assert!(e.press_enter_after_paste);
        assert_eq!(e.insert_mode, InsertMode::Type);
        assert_eq!(e.language, s.language);
    }

    #[test]
    fn forced_profile_wins_over_app() {
        let s = settings();
        let e = effective(&s, Some("ms-teams.exe"), Some("Nederlands"));
        assert_eq!(e.language, "nl");
        assert!(!e.press_enter_after_paste);
    }

    #[test]
    fn matching_ignores_case() {
        let s = settings();
        assert_eq!(matching(&s, Some("MS-Teams.EXE"), None).map(|p| p.name.as_str()), Some("Chat"));
        assert_eq!(matching(&s, None, Some("nederlands")).map(|p| p.name.as_str()), Some("Nederlands"));
    }
}
