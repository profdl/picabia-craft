//! PICABIA: Picabia's modifier entries in FigCraft's menus, layers panel, Inspect tab and
//! accessibility labels. Each is reached from one marked seam (see `docs/picabia/SEAMS.md`).
//!
//! - Object ▸ Modifiers and the same submenu in the canvas/layer context menu: Add modifier ▸ one
//!   item per kind, Apply modifiers, Remove all modifiers. Every item is a `modifier.*` command.
//! - The layers panel's badge on rows whose layer has an active stack.
//! - The Inspect tab's read-only Modifiers lines and the layer rows' accessible descriptions
//!   ("Rectangle, Linear array ×5").

use egui::{Color32, Rect, Ui, pos2, vec2};
use figcraft_doc::{Document, Node, NodeId};
use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{self, tr, tr_noop};
use crate::icons_picabia::{self, Glyph};
use crate::menus::{item, separator, submenu};
use crate::theme::Tokens;

/// The kinds Add modifier offers, in menu order, with their labels (catalog keys). Kinds this
/// build doesn't evaluate (`figcraft_modifiers::kind_label` is `None`) are left out.
pub const KINDS: &[(&str, &str)] = &[
    ("linearArray", tr_noop("Linear array")),
    ("circularArray", tr_noop("Circular array")),
    ("gridArray", tr_noop("Grid array")),
    ("mirror", tr_noop("Mirror")),
    ("transformEach", tr_noop("Transform each")),
    ("randomize", tr_noop("Randomize")),
    ("wallpaper", tr_noop("Wallpaper")),
];

/// The context menu's row standing for the Modifiers submenu (its command-id slot).
pub const CONTEXT_ROW: &str = "picabia.modifiers";

/// The kinds the Add submenu lists: `(kind id, label key)`.
pub fn menu_kinds() -> impl Iterator<Item = (&'static str, &'static str)> {
    KINDS.iter().copied().filter(|(k, _)| figcraft_modifiers::kind_label(k).is_some())
}

/// What the Modifiers submenu can do for the selection: `(add, apply_or_remove)`: some layer is
/// selected; some selected layer has a stack.
pub fn enabled(app: &FigApp) -> (bool, bool) {
    let Ok(doc) = app.session.doc() else { return (false, false) };
    let sel = app.session.selection();
    let layers: Vec<&Node> = sel.iter().filter_map(|i| doc.node(*i)).filter(|n| !n.is_page()).collect();
    (!layers.is_empty(), layers.iter().any(|n| figcraft_modifiers::has_modifiers(n)))
}

/// The submenu's items: `(label, command, params, enabled)`, separators as an empty command.
pub fn items(app: &FigApp) -> Vec<(String, &'static str, Value, bool)> {
    let (add, stacked) = enabled(app);
    let mut v: Vec<(String, &'static str, Value, bool)> =
        menu_kinds().map(|(kind, label)| (tr(label).to_string(), "modifier.add", json!({"kind": kind}), add)).collect();
    v.push((String::new(), "", Value::Null, false));
    v.push((i18n::command("modifier.apply", "Apply modifiers").into_owned(), "modifier.apply", json!({}), stacked));
    v.push((i18n::command("modifier.remove_all", "Remove all modifiers").into_owned(), "modifier.remove_all", json!({}), stacked));
    v
}

/// Whether Object ▸ Modifiers follows main-menu item `id`: the auto layout group, high enough in
/// the long Object menu to stay on screen.
pub fn object_menu_after(id: &str) -> bool {
    id == "layout.suggest"
}

/// Object ▸ Modifiers (seam in `menus::menu_level`, after [`object_menu_after`]'s item): whether
/// an item ran.
pub fn object_menu(app: &mut FigApp, ui: &mut Ui, prefix: &str, t: &Tokens) -> bool {
    separator(ui);
    modifiers_submenu(app, ui, &format!("{prefix}/Modifiers"), t)
}

/// Put the Modifiers row in the context menu's `entries` (seam in `menus::popups`), after the
/// component actions. Drawn by [`modifiers_submenu`] where the menu meets [`CONTEXT_ROW`].
pub fn context_entry<'a>(entries: &mut Vec<(&'a str, &'a str)>) {
    if let Some(at) = entries.iter().position(|e| e.1 == "component.detach" || e.1 == "component.goToMain") {
        entries.insert(at + 1, (tr("Modifiers"), CONTEXT_ROW));
    }
}

/// The Modifiers submenu under tree key `key`: Add modifier ▸ kinds, Apply modifiers, Remove all
/// modifiers. Returns whether an item ran.
pub fn modifiers_submenu(app: &mut FigApp, ui: &mut Ui, key: &str, t: &Tokens) -> bool {
    let rows = items(app);
    let picked = submenu(ui, key, tr("Modifiers"), t, |ui| {
        let mut picked = None;
        let (kinds, rest): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r.1 == "modifier.add");
        let add = submenu(ui, &format!("{key}/Add"), tr("Add modifier"), t, |ui| {
            let mut picked = None;
            for (label, id, params, on) in &kinds {
                if item(ui, label, "", *on, false, t) && *on {
                    picked = Some((*id, params.clone()));
                }
            }
            picked
        });
        picked = picked.or(add);
        for (label, id, params, on) in rest {
            if id.is_empty() {
                separator(ui);
            } else if item(ui, label, crate::shortcuts::shortcut(id).unwrap_or(""), *on, false, t) && *on {
                picked = Some((*id, params.clone()));
            }
        }
        picked
    });
    let Some((id, params)) = picked else { return false };
    if !crate::menus::dev_mode_refuses(app, id) {
        app.run(id, params);
    }
    true
}

/// Whether `n` has an enabled stack this build draws (the layers badge shows then).
pub fn active(n: &Node) -> bool {
    matches!(figcraft_modifiers::node_stack(n), Some(Ok(s)) if s.is_active())
}

/// Room the layers panel keeps right of a row's name for the badge (seam in `layers.rs`).
pub fn badge_room(n: &Node) -> f32 {
    if active(n) { BADGE + 6.0 } else { 0.0 }
}

const BADGE: f32 = 12.0;

/// The modifier badge on layer row `r` (`rr`: the row's inset rect), left of the padlock and eye.
pub fn layer_badge(ui: &Ui, rr: Rect, r: Rect, n: &Node, color: Color32) {
    if active(n) {
        let at = Rect::from_center_size(pos2(rr.max.x - 58.0, r.center().y), vec2(BADGE, BADGE));
        icons_picabia::paint(ui.painter(), at, Glyph::Modifiers, color);
    }
}

/// `n`'s enabled modifiers as read-only lines, translated: "Linear array ×5", "Mirror Vertical axis".
pub fn lines(n: &Node) -> Vec<String> {
    figcraft_engine::cmd::modifier::summary(n)
        .into_iter()
        .map(|(label, amount)| if amount.is_empty() { tr(&label).to_string() } else { format!("{} {}", tr(&label), tr(&amount)) })
        .collect()
}

/// A layer row's accessible description: its type, then its modifiers (seam in `layers.rs`).
pub fn a11y_kind(kind: &str, n: &Node) -> String {
    let lines = lines(n);
    if lines.is_empty() { kind.to_string() } else { format!("{kind}, {}", lines.join(", ")) }
}

/// The Inspect tab's Modifiers section for layer `id` (seam in `panels/right/inspect.rs`), or
/// `None` without enabled modifiers.
pub fn inspect_section(doc: &Document, id: NodeId) -> Option<figcraft_engine::devinspect::Section> {
    let rows: Vec<_> = figcraft_engine::cmd::modifier::summary(doc.node(id)?)
        .into_iter()
        .map(|(label, amount)| figcraft_engine::devinspect::Prop { label, value: tr(&amount).to_string(), color: None, token: None })
        .collect();
    (!rows.is_empty()).then_some(figcraft_engine::devinspect::Section { title: tr_noop("Modifiers"), rows })
}

#[cfg(test)]
#[path = "picabia_ui_tests.rs"]
mod tests;
