//! `picabia-bend <design.figcraft> -o <out.bend> [--font <file>]... [--vocabulary <dir>]`
//! `picabia-bend oracle <design.figcraft> [--font <file>]... < the game's printed layout`
//! `picabia-bend slate <design.figcraft> --schema <file> --public <dir> --private <dir> [--font <file>]...`
//!
//! Writes the design's screens as a Bend module over the game's interface vocabulary (imported from
//! `--vocabulary`, relative to the output, default `../Vocabulary`); or, with `slate`, as Unreal
//! Slate bound to the game's screens as its schema states them (`BendScreens.h` into `--public`,
//! `BendScreens.cpp` and `BendScreensWire.h` into `--private`). Exits non-zero, listing every
//! layer it refuses, when the design uses what the target cannot draw or disagrees with the
//! game's screens.

use std::path::PathBuf;
use std::process::ExitCode;

#[derive(PartialEq, Eq)]
enum Mode {
    Bend,
    Oracle,
    Slate,
}

struct Args {
    mode: Mode,
    design: PathBuf,
    out: PathBuf,
    fonts: Vec<PathBuf>,
    vocabulary: String,
    schema: Option<PathBuf>,
    public: Option<PathBuf>,
    private: Option<PathBuf>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut design, mut out, mut fonts, mut vocabulary, mut mode) = (None, None, Vec::new(), "../Vocabulary".to_string(), Mode::Bend);
    let (mut schema, mut public, mut private) = (None, None, None);
    while let Some(a) = it.next() {
        match a.as_str() {
            "oracle" if design.is_none() && mode == Mode::Bend => mode = Mode::Oracle,
            "slate" if design.is_none() && mode == Mode::Bend => mode = Mode::Slate,
            "--schema" => schema = Some(PathBuf::from(it.next().ok_or("--schema needs a file")?)),
            "--public" => public = Some(PathBuf::from(it.next().ok_or("--public needs a directory")?)),
            "--private" => private = Some(PathBuf::from(it.next().ok_or("--private needs a directory")?)),
            "-o" | "--out" => out = Some(PathBuf::from(it.next().ok_or("-o needs a file")?)),
            "--font" => fonts.push(PathBuf::from(it.next().ok_or("--font needs a file")?)),
            "--vocabulary" => vocabulary = it.next().ok_or("--vocabulary needs a directory")?,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if design.is_none() => design = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    let design = design.ok_or("which design? (picabia-bend <design.figcraft> -o <out.bend>)")?;
    let out = if mode == Mode::Bend { out.ok_or("-o <out.bend> is required")? } else { PathBuf::new() };
    if mode == Mode::Slate && (schema.is_none() || public.is_none() || private.is_none()) {
        return Err("slate needs --schema <file>, --public <dir> and --private <dir>".into());
    }
    Ok(Args { mode, design, out, fonts, vocabulary, schema, public, private })
}

fn listed(rs: &[picabia_bend::Refusal], what: &str) -> String {
    let lines: Vec<String> = rs.iter().map(|r| format!("  {r}")).collect();
    format!("{} layer(s) {what}:\n{}", rs.len(), lines.join("\n"))
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> Result<(), String> {
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn slate(a: &Args, doc: &mut figcraft_doc::Document) -> Result<(), String> {
    let (Some(schema), Some(public), Some(private)) = (&a.schema, &a.public, &a.private) else {
        return Err("slate needs --schema, --public and --private".into());
    };
    let text = std::fs::read_to_string(schema).map_err(|e| format!("{}: {e}", schema.display()))?;
    let schema = picabia_bend::schema::parse(&text)?;
    let d = picabia_bend::design(doc).map_err(|rs| listed(&rs, "cannot be drawn by the engine"))?;
    let source = a.design.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let files = picabia_bend::slate::slate(&d, &schema, &format!("ui/{source}"))
        .map_err(|rs| listed(&rs, "disagree with the game's screens or cannot be drawn"))?;
    write(public, picabia_bend::slate::HEADER, &files.header)?;
    write(private, picabia_bend::slate::SOURCE, &files.source)?;
    write(private, picabia_bend::slate::WIRE, &files.wire)
}

fn run(a: Args) -> Result<(), String> {
    picabia_bend::load_fonts(&a.fonts)?;
    let (mut doc, notes) = figcraft_format::load_file(&a.design).map_err(|e| format!("{}: {e}", a.design.display()))?;
    for n in notes {
        eprintln!("note: {n}");
    }
    if a.mode == Mode::Slate {
        return slate(&a, &mut doc);
    }
    if a.mode == Mode::Oracle {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).map_err(|e| e.to_string())?;
        let cases = picabia_bend::oracle::parse(&input)?;
        let bad = picabia_bend::oracle::compare(&doc, &cases)?;
        if !bad.is_empty() {
            return Err(format!("the game's layout disagrees with Picabia's in {} place(s):\n  {}", bad.len(), bad.join("\n  ")));
        }
        println!("{} case(s) laid out as Picabia lays them out", cases.len());
        return Ok(());
    }
    let d = picabia_bend::design(&mut doc).map_err(|rs| {
        let lines: Vec<String> = rs.iter().map(|r| format!("  {r}")).collect();
        format!("{} layer(s) cannot be drawn by the game:\n{}", rs.len(), lines.join("\n"))
    })?;
    let source = a.design.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let text = picabia_bend::module(&d, &source, &picabia_bend::Imports { vocabulary: a.vocabulary });
    std::fs::write(&a.out, text).map_err(|e| format!("{}: {e}", a.out.display()))
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
