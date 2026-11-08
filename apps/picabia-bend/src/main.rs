//! `picabia-bend slate <design.figcraft> --schema <file> --public <dir> --private <dir> [--font <file>]...`
//!
//! Writes the design's screens as Unreal Slate bound to the game's screens as its schema states
//! them (`BendScreens.h` into `--public`, `BendScreens.cpp` and `BendScreensWire.h` into
//! `--private`). Exits non-zero, listing every layer it refuses, when the design uses what Slate
//! is not generated for or disagrees with the game's screens.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    design: PathBuf,
    fonts: Vec<PathBuf>,
    schema: PathBuf,
    public: PathBuf,
    private: PathBuf,
}

const USAGE: &str = "picabia-bend slate <design.figcraft> --schema <file> --public <dir> --private <dir> [--font <file>]...";

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    if it.next().as_deref() != Some("slate") {
        return Err(format!("usage: {USAGE}"));
    }
    let (mut design, mut fonts, mut schema, mut public, mut private) = (None, Vec::new(), None, None, None);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--schema" => schema = Some(PathBuf::from(it.next().ok_or("--schema needs a file")?)),
            "--public" => public = Some(PathBuf::from(it.next().ok_or("--public needs a directory")?)),
            "--private" => private = Some(PathBuf::from(it.next().ok_or("--private needs a directory")?)),
            "--font" => fonts.push(PathBuf::from(it.next().ok_or("--font needs a file")?)),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if design.is_none() => design = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    match (design, schema, public, private) {
        (Some(design), Some(schema), Some(public), Some(private)) => Ok(Args { design, fonts, schema, public, private }),
        _ => Err(format!("slate needs a design, --schema <file>, --public <dir> and --private <dir> ({USAGE})")),
    }
}

fn listed(rs: &[picabia_bend::Refusal], what: &str) -> String {
    let lines: Vec<String> = rs.iter().map(|r| format!("  {r}")).collect();
    format!("{} layer(s) {what}:\n{}", rs.len(), lines.join("\n"))
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> Result<(), String> {
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn run(a: Args) -> Result<(), String> {
    picabia_bend::load_fonts(&a.fonts)?;
    let (mut doc, notes) = figcraft_format::load_file(&a.design).map_err(|e| format!("{}: {e}", a.design.display()))?;
    for n in notes {
        eprintln!("note: {n}");
    }
    let text = std::fs::read_to_string(&a.schema).map_err(|e| format!("{}: {e}", a.schema.display()))?;
    let schema = picabia_bend::schema::parse(&text)?;
    let d = picabia_bend::design(&mut doc).map_err(|rs| listed(&rs, "cannot be drawn by the engine"))?;
    let source = a.design.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let files = picabia_bend::slate::slate(&d, &schema, &format!("ui/{source}"))
        .map_err(|rs| listed(&rs, "disagree with the game's screens or cannot be drawn"))?;
    write(&a.public, picabia_bend::slate::HEADER, &files.header)?;
    write(&a.private, picabia_bend::slate::SOURCE, &files.source)?;
    write(&a.private, picabia_bend::slate::WIRE, &files.wire)
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("picabia-bend: {e}");
            ExitCode::FAILURE
        }
    }
}
