//! Picabia's identity in one place: the names the shipped app shows and the id and data folder it
//! keeps apart from FigCraft's, so both run side by side without sharing state.
//! (Crate, package and file-format names stay FigCraft's: renaming them would conflict on every
//! upstream merge.)

/// The name the app shows: window titles, the macOS app menu, error dialogs, `--version`.
pub const APP_NAME: &str = "Picabia";
/// Wayland `app_id` / X11 window class: the desktop entry's name.
pub const APP_ID: &str = "org.picabia.Picabia";
/// The folder under the platform's config directory: preferences, recovery copies, plugins.
pub const CONFIG_DIR: &str = "Picabia";
