//! Picabia's interface designs for a game. A `.figcraft` file's `Screens` page is converted once
//! (`convert`, into `model`'s tree of frames, texts and pressable instances, its auto layout and
//! constraints as designed) and emitted for the side that draws it: as Unreal Slate bound to the
//! game's screens (`slate`, held to the game's `schema` both ways), or as a Bend module over a
//! game's own interface vocabulary with the metrics of its fonts (`emit`, and `oracle`, which
//! holds a game's layout of it to Picabia's).

pub mod convert;
pub mod emit;
pub mod model;
pub mod oracle;
pub mod schema;
pub mod slate;

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
