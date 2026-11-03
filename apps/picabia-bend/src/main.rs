//! `picabia-bend <design.figcraft> -o <out.bend> [--font <file>]... [--vocabulary <dir>]`
//! `picabia-bend oracle <design.figcraft> [--font <file>]... < the game's printed layout`
//!
//! Writes the design's screens as a Bend module over the game's interface vocabulary (imported from
//! `--vocabulary`, relative to the output, default `../Vocabulary`). Exits non-zero, listing every
//! layer it refuses, when the design uses what the vocabulary cannot draw.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    oracle: bool,
    design: PathBuf,
    out: PathBuf,
    fonts: Vec<PathBuf>,
    vocabulary: String,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut design, mut out, mut fonts, mut vocabulary, mut oracle) = (None, None, Vec::new(), "../Vocabulary".to_string(), false);
    while let Some(a) = it.next() {
        match a.as_str() {
            "oracle" if design.is_none() && !oracle => oracle = true,
            "-o" | "--out" => out = Some(PathBuf::from(it.next().ok_or("-o needs a file")?)),
            "--font" => fonts.push(PathBuf::from(it.next().ok_or("--font needs a file")?)),
            "--vocabulary" => vocabulary = it.next().ok_or("--vocabulary needs a directory")?,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if design.is_none() => design = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    let design = design.ok_or("which design? (picabia-bend <design.figcraft> -o <out.bend>)")?;
    let out = if oracle { PathBuf::new() } else { out.ok_or("-o <out.bend> is required")? };
    Ok(Args { oracle, design, out, fonts, vocabulary })
}

fn run(a: Args) -> Result<(), String> {
    picabia_bend::load_fonts(&a.fonts)?;
    let (mut doc, notes) = figcraft_format::load_file(&a.design).map_err(|e| format!("{}: {e}", a.design.display()))?;
    for n in notes {
        eprintln!("note: {n}");
    }
    if a.oracle {
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
