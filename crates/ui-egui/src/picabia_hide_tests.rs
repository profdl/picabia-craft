//! PICABIA: prototyping, Dev Mode, boards and live sessions left out of the interface: off by
//! default; on, the panels, keys, menu commands, palette rows, home screen entries and toolbar
//! modes go, while the engine still runs the commands for agents and variables stay.

use egui::{Event, Key, Modifiers};
use figcraft_engine::Session;
use serde_json::json;

use crate::RightTab;
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text};
use crate::menus::PaletteTab;
use crate::present_tests::id;

fn hidden() -> Ui {
    let mut u = Ui::new(Session::new());
    super::hide(&mut u.app);
    u.frame(vec![]);
    u
}

fn press(u: &mut Ui, key: Key, modifiers: Modifiers) {
    u.frame(vec![
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]);
}

fn palette_keys(u: &Ui, q: &str) -> Vec<String> {
    crate::menus::palette_sections(&u.app, q, PaletteTab::All).into_iter().flat_map(|(_, rows)| rows.into_iter().map(|r| r.key)).collect()
}

#[test]
fn the_hidden_commands_are_the_four_features_and_updates_only() {
    for id in [
        "proto.present",
        "proto.preview",
        "proto.flow.add",
        "view.prototype",
        "view.designPrototype",
        "view.devMode",
        "dev.focus",
        "collab.share",
        "collab.join",
        "view.multiplayerCursors",
        "file.newBoard",
        "board.sticky",
        "tool.sticky",
        "help.checkForUpdates",
        "app.checkForUpdates",
    ] {
        assert!(super::hidden_id(id), "{id} hidden");
    }
    for id in ["variable.duplicate", "modifier.add", "view.design", "file.new", "tool.frame", "annotation.add", "comment.add", "file.export"] {
        assert!(!super::hidden_id(id), "{id} kept");
    }
}

#[test]
fn off_by_default_as_upstream() {
    let mut u = Ui::new(Session::new());
    assert!(!u.app.ui.picabia_hide);
    press(&mut u, Key::E, Modifiers::SHIFT);
    assert_eq!(u.app.ui.right_tab, RightTab::Prototype, "Shift+E still opens Prototype upstream");
    assert!(palette_keys(&u, "present").iter().any(|k| k == "command:proto.present"));
}

#[test]
fn panel_keys_and_menu_commands_stay_in_design() {
    let mut u = hidden();
    press(&mut u, Key::E, Modifiers::SHIFT);
    assert_eq!(u.app.ui.right_tab, RightTab::Design, "Shift+E");
    press(&mut u, Key::D, Modifiers::SHIFT);
    assert_eq!(u.app.ui.right_tab, RightTab::Design, "Shift+D");
    press(&mut u, Key::Num9, Modifiers::ALT);
    assert_eq!(u.app.ui.right_tab, RightTab::Design, "Alt+9");
    for id in ["view.prototype", "view.devMode", "proto.present"] {
        crate::menus::invoke(&mut u.app, id);
        u.frame(vec![]);
        assert_eq!(u.app.ui.right_tab, RightTab::Design, "{id}");
        assert!(u.app.ui.present.is_none(), "{id} presents nothing");
    }
    let docs = u.app.session.docs.len();
    crate::menus::invoke(&mut u.app, "file.newBoard");
    assert_eq!(u.app.session.docs.len(), docs, "no new board");
}

#[test]
fn a_tab_asked_for_otherwise_falls_back_to_design() {
    let mut u = hidden();
    for tab in [RightTab::Prototype, RightTab::Inspect] {
        u.app.ui.right_tab = tab;
        u.frame(vec![]);
        assert_eq!(u.app.ui.right_tab, RightTab::Design, "{tab:?}");
        assert!(!u.app.session.tool.dev_mode, "{tab:?}: not in Dev Mode");
    }
}

#[test]
fn agents_still_run_prototype_commands() {
    let mut s = Session::new();
    let frame = id(&s.execute("node.create", &json!({"type": "frame", "width": 400, "height": 300})).unwrap());
    let mut u = Ui::new(s);
    super::hide(&mut u.app);
    assert!(u.app.run("proto.flow.add", json!({"start": frame})).is_some(), "flows still edit");
    u.app.run("proto.present", json!({"start": frame}));
    assert!(u.app.ui.present.is_some(), "an agent can still present");
}

#[test]
fn the_palette_leaves_out_hidden_commands_and_keeps_variables() {
    let mut u = hidden();
    u.app.session.prefs.recent_actions = vec!["command:proto.present".into(), "command:file.new".into()];
    let keys = palette_keys(&u, "");
    assert!(!keys.iter().any(|k| k == "command:proto.present"), "not in Recents");
    assert!(keys.iter().any(|k| k == "command:file.new"));
    for q in ["present", "prototype", "dev mode", "live", "board", "share"] {
        let keys = palette_keys(&u, q);
        assert!(!keys.iter().any(|k| k.strip_prefix("command:").is_some_and(super::hidden_id)), "{q}: {keys:?}");
    }
    assert!(palette_keys(&u, "variable").iter().any(|k| k.starts_with("command:variable.")), "variables stay");
}

#[test]
fn the_right_panel_has_no_share_present_or_prototype() {
    let mut u = hidden();
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&shapes, "Design"));
    for s in ["Share", "Prototype"] {
        assert!(!has_text(&shapes, s), "{s}");
    }
}

#[test]
fn the_toolbar_keeps_draw_and_design() {
    let mut u = Ui::new(Session::new());
    let full = crate::toolbar::bar_width(&u.app);
    super::hide(&mut u.app);
    assert_eq!(full - crate::toolbar::bar_width(&u.app), 64.0, "two mode cells fewer");
    assert!(super::mode_shown(&u.app, 0) && super::mode_shown(&u.app, 1));
    assert!(!super::mode_shown(&u.app, 2) && !super::mode_shown(&u.app, 3));
}

#[test]
fn the_home_screen_has_no_boards_joins_or_online_files() {
    let mut u = hidden();
    u.app.ui.home = true;
    u.app.ui.home_view.section = crate::home::Section::Online;
    u.frame(vec![]);
    assert_eq!(u.app.ui.home_view.section, crate::home::Section::Recents);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&shapes, "New design file"));
    for s in ["New board", "Join with a code", "Online", "Share with a code", "Prototype"] {
        assert!(!has_text(&shapes, s), "{s}");
    }
}

#[test]
fn the_shortcuts_panel_lists_no_prototype_keys() {
    let mut u = hidden();
    u.app.ui.shortcuts_panel = true;
    u.frame(vec![]); // A window lays itself out before it draws.
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&shapes, "Essential"));
    for s in ["Prototype", "Present", "Dev Mode on/off"] {
        assert!(!has_text(&shapes, s), "{s}");
    }
}
