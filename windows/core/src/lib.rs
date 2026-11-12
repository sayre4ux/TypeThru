//! Everything in TypeThru for Windows that does not need Windows: text rules, Markdown, typing
//! speeds, shortcut rules, the update feed and its signature check, release notes, the reading
//! order of recognized text, and saved settings. The same rules as the Mac app; both pass
//! `shared/test-cases.json`.
#![forbid(unsafe_code)]

pub mod notes;
pub mod settings;
pub mod text_rules;
pub mod typing;
pub mod update_feed;
