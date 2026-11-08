//! The game's screens as its own source of truth states them (ue-bend's
//! `bend/Interface/Screens_Schema.bend` prints this): each screen, the commands it admits and the
//! slots it reads, with the C names its generated header gives them. The design is held to it both
//! ways before anything is generated: a design naming a screen, slot, flag or action the game does
//! not have, or leaving one the game has unbound, is refused, every mismatch at once.
//!
//! The format is one fact a line, its fields separated by tabs:
//!
//! ```text
//! screen    <name> <c lower> <C upper>
//! command   <screen> <name> <C enumerator>
//! count     <screen> <slot> <C field>
//! flag      <screen> <slot> <C field>
//! selection <screen> <slot> <C selection field> <C command field>
//! ```

use std::collections::BTreeSet;

use crate::convert::{Refusal, constructor};
use crate::model::{Constructor, Design, UiNode, Words};

/// A command a screen admits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// As the design names it (`start-game`), and as the engine's code does (`StartGame`).
    pub name: String,
    pub key: Constructor,
    /// Its enumerator in the screen's `ub_<screen>_command`.
    pub c_enum: String,
}

/// What a slot reads, and the C fields it is read into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A count, shown as text by a `{slot}` text.
    Count { field: String },
    /// Whether a part shows: an instance's `@shown`.
    Flag { field: String },
    /// Which of the screen's commands shows its `State=Selected` variant, if any.
    Selection { field: String, command_field: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub name: String,
    pub key: Constructor,
    pub kind: SlotKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub name: String,
    pub key: Constructor,
    /// `hud_play`: the C names `ub_hud_play_read`, `ub_hud_play_command`, ...
    pub c_lower: String,
    /// `HUD_PLAY`: `UB_SCREEN_HUD_PLAY`, `UB_HUD_PLAY_READ_WORDS`, ...
    pub c_upper: String,
    pub commands: Vec<Command>,
    pub slots: Vec<Slot>,
}

impl Screen {
    /// The slot that says which command shows selected, if the screen reads one.
    pub fn selection(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| matches!(s.kind, SlotKind::Selection { .. }))
    }

    pub fn command(&self, key: &Constructor) -> Option<&Command> {
        self.commands.iter().find(|c| c.key == *key)
    }

    pub fn slot(&self, key: &Constructor) -> Option<&Slot> {
        self.slots.iter().find(|s| s.key == *key)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub screens: Vec<Screen>,
}

impl Schema {
    pub fn screen(&self, key: &Constructor) -> Option<&Screen> {
        self.screens.iter().find(|s| s.key == *key)
    }
}

fn key(line: usize, name: &str) -> Result<Constructor, String> {
    constructor(name).ok_or_else(|| format!("schema line {line}: `{name}` is not a name (letters and digits, in kebab or snake case)"))
}

fn c_name(line: usize, name: &str) -> Result<String, String> {
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit());
    if ok { Ok(name.to_string()) } else { Err(format!("schema line {line}: `{name}` is not a C name")) }
}

fn screen_at<'a>(screens: &'a mut [Screen], line: usize, name: &str) -> Result<&'a mut Screen, String> {
    screens.iter_mut().find(|s| s.name == name).ok_or_else(|| format!("schema line {line}: no screen `{name}` is stated before it"))
}

fn slot(screens: &mut [Screen], line: usize, screen: &str, name: &str, kind: SlotKind) -> Result<(), String> {
    let key = key(line, name)?;
    let s = screen_at(screens, line, screen)?;
    if s.slots.iter().any(|x| x.key == key) {
        return Err(format!("schema line {line}: screen `{screen}` reads `{name}` twice"));
    }
    if matches!(kind, SlotKind::Selection { .. }) && s.selection().is_some() {
        return Err(format!("schema line {line}: screen `{screen}` reads a second selection"));
    }
    s.slots.push(Slot { name: name.to_string(), key, kind });
    Ok(())
}

/// The schema, or the first line it cannot read.
pub fn parse(text: &str) -> Result<Schema, String> {
    let mut screens: Vec<Screen> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = raw.split('\t').collect();
        match f.as_slice() {
            ["screen", name, lower, upper] => {
                let key = key(line, name)?;
                if screens.iter().any(|s| s.key == key) {
                    return Err(format!("schema line {line}: screen `{name}` is stated twice"));
                }
                screens.push(Screen {
                    name: name.to_string(),
                    key,
                    c_lower: c_name(line, lower)?,
                    c_upper: c_name(line, upper)?,
                    commands: Vec::new(),
                    slots: Vec::new(),
                });
            }
            ["command", screen, name, c_enum] => {
                let key = key(line, name)?;
                let c_enum = c_name(line, c_enum)?;
                let s = screen_at(&mut screens, line, screen)?;
                if s.command(&key).is_some() {
                    return Err(format!("schema line {line}: screen `{screen}` admits `{name}` twice"));
                }
                s.commands.push(Command { name: name.to_string(), key, c_enum });
            }
            ["count", screen, name, field] => slot(&mut screens, line, screen, name, SlotKind::Count { field: c_name(line, field)? })?,
            ["flag", screen, name, field] => slot(&mut screens, line, screen, name, SlotKind::Flag { field: c_name(line, field)? })?,
            ["selection", screen, name, field, command_field] => slot(
                &mut screens,
                line,
                screen,
                name,
                SlotKind::Selection { field: c_name(line, field)?, command_field: c_name(line, command_field)? },
            )?,
            _ => return Err(format!("schema line {line}: `{raw}` is not a screen, command, count, flag or selection")),
        }
    }
    if screens.is_empty() {
        return Err("the schema states no screen".into());
    }
    Ok(Schema { screens })
}

/// What a screen's design binds: its texts' slots, its flags and its actions.
#[derive(Default)]
struct Bound {
    texts: BTreeSet<Constructor>,
    flags: BTreeSet<Constructor>,
    actions: BTreeSet<Constructor>,
}

fn bound(n: &UiNode, b: &mut Bound) {
    match n {
        UiNode::Frame { kids, .. } => kids.iter().for_each(|k| bound(k, b)),
        UiNode::Label { words: Words::Bound { slot, .. }, .. } => {
            b.texts.insert(slot.clone());
        }
        UiNode::Label { .. } => {}
        UiNode::Shown { flag, node } => {
            b.flags.insert(flag.clone());
            bound(node, b);
        }
        UiNode::Pressable { action, idle, .. } => {
            b.actions.insert(action.clone());
            bound(idle, b);
        }
    }
}

fn refuse(rs: &mut Vec<Refusal>, layer: &str, why: String) {
    rs.push(Refusal { layer: layer.to_string(), why });
}

fn held_screen(rs: &mut Vec<Refusal>, s: &Screen, root: &UiNode) {
    let layer = format!("{}/{}", crate::convert::SCREENS_PAGE, s.name);
    let mut b = Bound::default();
    bound(root, &mut b);
    for t in &b.texts {
        match s.slot(t).map(|x| &x.kind) {
            Some(SlotKind::Count { .. }) => {}
            Some(_) => refuse(rs, &layer, format!("the text slot `{}` is not a count of {}'s", t.0, s.name)),
            None => refuse(rs, &layer, format!("{} reads no slot `{}` (its text names one the game does not have)", s.name, t.0)),
        }
    }
    for f in &b.flags {
        match s.slot(f).map(|x| &x.kind) {
            Some(SlotKind::Flag { .. }) => {}
            Some(_) => refuse(rs, &layer, format!("`{}` is not a flag of {}'s", f.0, s.name)),
            None => refuse(rs, &layer, format!("{} reads no flag `{}` (an @shown names one the game does not have)", s.name, f.0)),
        }
    }
    for a in &b.actions {
        if s.command(a).is_none() {
            refuse(rs, &layer, format!("{} admits no command `{}` (an @action names one the game does not have)", s.name, a.0));
        }
    }
    for slot in &s.slots {
        let used = match slot.kind {
            SlotKind::Count { .. } => b.texts.contains(&slot.key),
            SlotKind::Flag { .. } => b.flags.contains(&slot.key),
            SlotKind::Selection { .. } => !b.actions.is_empty(),
        };
        if !used {
            refuse(rs, &layer, format!("the game's slot `{}` of {} is bound by nothing in the design", slot.name, s.name));
        }
    }
    for c in &s.commands {
        if !b.actions.contains(&c.key) {
            refuse(rs, &layer, format!("the game's command `{}` of {} is given by nothing in the design", c.name, s.name));
        }
    }
}

/// Every way the design and the game's screens disagree, or none.
pub fn hold(design: &Design, schema: &Schema) -> Result<(), Vec<Refusal>> {
    let mut rs = Vec::new();
    for (which, root) in &design.screens {
        match schema.screen(which) {
            Some(s) => held_screen(&mut rs, s, root),
            None => refuse(&mut rs, &format!("{}/{}", crate::convert::SCREENS_PAGE, which.0), "the game has no such screen".into()),
        }
    }
    for s in &schema.screens {
        if !design.screens.iter().any(|(k, _)| *k == s.key) {
            refuse(&mut rs, crate::convert::SCREENS_PAGE, format!("the game's screen `{}` has no frame in the design", s.name));
        }
    }
    if rs.is_empty() { Ok(()) } else { Err(rs) }
}
