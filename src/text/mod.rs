//! What happens to the text after transcription: clean-up, AI polish, screen
//! context, pasting it into the app in front, history and per-app profiles.

pub mod cleanup;
pub mod context;
pub mod foreground;
pub mod history;
pub mod insert;
pub mod polish;
pub mod profiles;
