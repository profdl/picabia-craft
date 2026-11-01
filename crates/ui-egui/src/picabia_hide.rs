//! PICABIA: Picabia leaves FigCraft's prototyping, Dev Mode, boards and live sessions out of the
//! interface: the menus, the Actions palette, shortcuts, the Keyboard shortcuts panel, the
//! toolbar's modes, the right panel's top bar (people, Present, Share) and its Prototype tab, and
//! the home screen (New board, Join with a code, Online, their Getting started tips). Each surface
//! is one marked seam (see `docs/picabia/SEAMS.md`).
//!
//! The commands stay in the engine, so the CLI, MCP and control channel still run them (agents can
//! still drive prototypes), and documents keep their interactions, flows and annotations.
//! Off by default, as upstream behaves (so upstream's tests run unchanged); the desktop and web
//! apps turn it on with [`hide`].

use crate::{FigApp, RightTab};
use figcraft_engine::keymap::Group;

/// Command families the interface leaves out.
const PREFIXES: &[&str] = &["proto.", "dev.", "collab.", "board."];

/// Single commands the interface leaves out: the panels and toggles of the hidden features, New
/// board and the board-only tools.
const IDS: &[&str] = &[
    "view.prototype",
    "view.designPrototype",
    "view.devMode",
    "view.multiplayerCursors",
    "panel.prototype",
    "panel.designPrototype",
    "panel.devMode",
    "file.newBoard",
    "tool.sticky",
    "tool.shape",
    "tool.connector",
    "tool.marker",
    "tool.highlighter",
    "tool.eraser",
    "tool.stamp",
];

/// Whether command `id` belongs to a hidden feature (whether or not hiding is on).
pub fn hidden_id(id: &str) -> bool {
    IDS.contains(&id) || PREFIXES.iter().any(|p| id.starts_with(p))
}

/// Whether hiding is on in this app.
pub fn on(app: &FigApp) -> bool {
    app.ui.picabia_hide
}

/// Whether the interface leaves command `id` out (menus, palette, shortcuts, tips).
pub fn hides(app: &FigApp, id: &str) -> bool {
    on(app) && hidden_id(id)
}

/// Turn hiding on (the desktop and web apps, at startup).
pub fn hide(app: &mut FigApp) {
    app.ui.picabia_hide = true;
    settle(app);
}

/// Each frame (seam in `app.rs`, before Dev Mode syncs): whatever asked for the Prototype or Dev
/// Mode tab (`view.prototype` from an agent, `ui.set {rightTab}`) or the home screen's Online
/// section gets the Design tab and Recents instead.
pub fn settle(app: &mut FigApp) {
    if !on(app) {
        return;
    }
    if matches!(app.ui.right_tab, RightTab::Prototype | RightTab::Inspect) {
        app.ui.right_tab = RightTab::Design;
    }
    if app.ui.home_view.section == crate::home::Section::Online {
        app.ui.home_view.section = crate::home::Section::Recents;
    }
}

/// Whether the toolbar's mode switcher shows mode `i` (Draw, Design, Prototype, Inspect).
pub fn mode_shown(app: &FigApp, i: usize) -> bool {
    !(on(app) && i >= 2)
}

/// How many of the mode switcher's cells are hidden (seam in `toolbar.rs`, for the pill's width;
/// a board has no switcher).
pub fn hidden_modes(app: &FigApp, board: bool) -> f32 {
    if on(app) && !board { 2.0 } else { 0.0 }
}

/// Whether the right panel shows tab `tab`, given [`on`].
pub fn tab_shown(hide: bool, tab: RightTab) -> bool {
    !(hide && tab != RightTab::Design)
}

/// Whether the Keyboard shortcuts panel lists group `g` (its tab and its rows), given [`on`].
pub fn group_listed(hide: bool, g: Group) -> bool {
    !(hide && g == Group::Prototype)
}

/// Whether the Actions palette keeps the row with key `key` (`command:<id>` rows of hidden
/// commands go, Recents included).
pub fn keeps_row(app: &FigApp, key: &str) -> bool {
    !key.strip_prefix("command:").is_some_and(|id| hides(app, id))
}

#[cfg(test)]
#[path = "picabia_hide_tests.rs"]
mod tests;
