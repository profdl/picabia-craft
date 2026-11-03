//! Picabia's interface designs as Bend data. A `.figcraft` file's `Screens` page becomes a Bend
//! module over a game's interface vocabulary (ue-bend's `bend/Vocabulary/Ui.bend` and
//! `Ui_Slots.bend`): every screen's tree of frames, texts and pressable instances, its auto layout
//! and constraints as designed, its bound texts with the metrics of their fonts. The game lays it
//! out itself; the module only states what was designed.

pub mod convert;
pub mod emit;
pub mod model;
pub mod oracle;

pub use convert::{Refusal, constructor, design};
pub use emit::{Imports, module};

/// Load the fonts the design is drawn in, and no others: measurement must not depend on the
/// machine's installed fonts.
pub fn load_fonts(files: &[std::path::PathBuf]) -> Result<(), String> {
    figcraft_text::fontdb::opt_out_of_system_fonts();
    let mut db = figcraft_text::fontdb::db().write().map_err(|_| "the font database is poisoned".to_string())?;
    for f in files {
        let bytes = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        if db.add_bytes(bytes).is_empty() {
            return Err(format!("{}: not a font", f.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
