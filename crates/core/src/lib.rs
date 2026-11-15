//! UI-independent logic: settings now, scanners/models later. Keep Slint out of this crate so a CLI can reuse it.
pub mod clock;
pub mod files;
pub mod settings;
