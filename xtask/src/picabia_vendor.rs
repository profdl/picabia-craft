//! PICABIA: the vendored VectorCraft crates (`vendor/vectorcraft/`, see its README.md).
//!
//! - `cargo xtask vendor-check` fails when a vendored file differs from `HASHES`, is missing or is extra.
//! - `cargo xtask vendor-vectorcraft <commit> [--from <vectorcraft checkout>]` re-extracts the crates at
//!   a commit with `git archive`, keeps our `Cargo.toml` files, and rewrites the pin in the README and `HASHES`.
//!
//! Every vendored file except the manifests is byte-identical to VectorCraft's at the pinned commit.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The vendored crates (directory names under `vendor/vectorcraft/` and under VectorCraft's `crates/`).
const CRATES: [&str; 3] = ["geom", "pathops", "trace"];
/// Upstream files that are not vendored (they need `vectorcraft-testkit`, which depends on VectorCraft's engine).
const EXCLUDED: [&str; 2] = ["geom/tests/prop_geom.rs", "pathops/tests/it/prop_pathops.rs"];
/// Our manifests: the source of truth, never overwritten by a sync and not in `HASHES`.
const MANIFEST: &str = "Cargo.toml";
const DEFAULT_FROM: &str = "../../vectorcraft/picabia-vector";
const FROM_ENV: &str = "VECTORCRAFT_DIR";

fn vendor_dir(root: &Path) -> PathBuf {
    root.join("vendor").join("vectorcraft")
}

fn rel_of(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}

fn is_manifest(rel: &str) -> bool {
    rel.rsplit('/').next() == Some(MANIFEST) && rel.matches('/').count() == 1
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() && !ft.is_symlink() {
            walk(&p, out);
        } else if ft.is_file() {
            out.push(p);
        }
    }
}

/// Hash of every vendored non-manifest file under `base` (`geom/..`, `pathops/..`, `trace/..`), by relative path.
fn hash_tree(base: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = Vec::new();
    for c in CRATES {
        walk(&base.join(c), &mut files);
    }
    let mut map = BTreeMap::new();
    for f in files {
        let rel = rel_of(base, &f);
        if is_manifest(&rel) {
            continue;
        }
        let bytes = std::fs::read(&f).map_err(|e| format!("read {rel}: {e}"))?;
        map.insert(rel, format!("{:x}", Sha256::digest(&bytes)));
    }
    Ok(map)
}

fn render_hashes(map: &BTreeMap<String, String>) -> String {
    map.iter().map(|(p, h)| format!("{h}  {p}\n")).collect()
}

fn parse_hashes(text: &str) -> BTreeMap<String, String> {
    text.lines().filter_map(|l| l.split_once("  ")).map(|(h, p)| (p.to_string(), h.to_string())).collect()
}

/// `cargo xtask vendor-check`.
pub fn check(root: &Path) -> bool {
    let base = vendor_dir(root);
    let Ok(text) = std::fs::read_to_string(base.join("HASHES")) else {
        eprintln!("vendor-check: vendor/vectorcraft/HASHES is missing");
        return false;
    };
    let want = parse_hashes(&text);
    let have = match hash_tree(&base) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("vendor-check: {e}");
            return false;
        }
    };
    let mut ok = true;
    for (p, h) in &want {
        match have.get(p) {
            None => {
                eprintln!("vendor-check: missing {p}");
                ok = false;
            }
            Some(x) if x != h => {
                eprintln!(
                    "vendor-check: {p} differs from the pinned VectorCraft file (vendored .rs files are never edited; see vendor/vectorcraft/README.md)"
                );
                ok = false;
            }
            Some(_) => {}
        }
    }
    for p in have.keys().filter(|p| !want.contains_key(*p)) {
        eprintln!("vendor-check: extra file {p} (not in HASHES)");
        ok = false;
    }
    if ok {
        eprintln!("vendor-check: ok ({} files)", want.len());
    }
    ok
}

fn git(from: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git").arg("-C").arg(from).args(args).stderr(Stdio::inherit()).output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() { Ok(out.stdout) } else { Err(format!("git {} failed", args.join(" "))) }
}

/// `cargo xtask vendor-vectorcraft <commit> [--from <path>]`.
pub fn sync(root: &Path, args: &[String]) -> bool {
    match sync_inner(root, args) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("vendor-vectorcraft: {e}");
            false
        }
    }
}

fn sync_inner(root: &Path, args: &[String]) -> Result<(), String> {
    let mut commit = None;
    let mut from = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--from" {
            from = Some(PathBuf::from(it.next().ok_or("--from needs a path")?));
        } else {
            commit = Some(a.as_str());
        }
    }
    let commit = commit.ok_or("usage: cargo xtask vendor-vectorcraft <commit> [--from <vectorcraft checkout>]")?;
    let from = from.or_else(|| std::env::var_os(FROM_ENV).map(PathBuf::from)).unwrap_or_else(|| root.join(DEFAULT_FROM));
    let spec = format!("{commit}^{{commit}}");
    let full = String::from_utf8_lossy(&git(&from, &["rev-parse", "--verify", &spec])?).trim().to_string();
    if full.len() != 40 || !full.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("not a commit id: {full}"));
    }
    let base = vendor_dir(root);

    // Our manifests are the source of truth: read them before the crate directories are replaced.
    let mut manifests = Vec::new();
    for c in CRATES {
        let text = std::fs::read(base.join(c).join(MANIFEST)).map_err(|e| format!("read {c}/{MANIFEST}: {e}"))?;
        manifests.push((c, text));
    }

    // Extract the pinned tree (never the working tree) into a scratch folder.
    let tmp = std::env::temp_dir().join(format!("picabia-vendor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let paths: Vec<String> = CRATES.iter().map(|c| format!("crates/{c}")).collect();
    let mut gargs = vec!["archive", full.as_str()];
    gargs.extend(paths.iter().map(String::as_str));
    let tar = git(&from, &gargs)?;
    let mut child = Command::new("tar").arg("-x").arg("-C").arg(&tmp).stdin(Stdio::piped()).spawn().map_err(|e| format!("tar: {e}"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or("tar: no stdin")?;
        stdin.write_all(&tar).map_err(|e| format!("tar: {e}"))?;
    }
    if !child.wait().map_err(|e| format!("tar: {e}"))?.success() {
        return Err("tar failed".into());
    }

    let result = install(&base, &tmp, &manifests, &full);
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    eprintln!("vendor-vectorcraft: vendored {full}");
    eprintln!(
        "vendor-vectorcraft: review upstream manifest changes (our Cargo.toml files were kept): git -C {} diff <old>..{full} -- crates/geom/Cargo.toml crates/pathops/Cargo.toml crates/trace/Cargo.toml",
        from.display()
    );
    Ok(())
}

fn install(base: &Path, tmp: &Path, manifests: &[(&str, Vec<u8>)], full: &str) -> Result<(), String> {
    let mut files = Vec::new();
    for c in CRATES {
        walk(&tmp.join("crates").join(c), &mut files);
    }
    for c in CRATES {
        let _ = std::fs::remove_dir_all(base.join(c));
    }
    for f in files {
        let rel = rel_of(&tmp.join("crates"), &f);
        if is_manifest(&rel) || EXCLUDED.contains(&rel.as_str()) {
            continue;
        }
        let dst = base.join(&rel);
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        std::fs::copy(&f, &dst).map_err(|e| format!("copy {rel}: {e}"))?;
    }
    for (c, text) in manifests {
        let dst = base.join(c);
        std::fs::create_dir_all(&dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
        std::fs::write(dst.join(MANIFEST), text).map_err(|e| format!("write {c}/{MANIFEST}: {e}"))?;
    }

    // The pathops integration test harness lists the upstream modules by hand: flag a module it lacks.
    let main_rs = std::fs::read_to_string(base.join("pathops/tests/it/main.rs")).unwrap_or_default();
    let harness = std::fs::read_to_string(base.join("picabia-tests/pathops_it.rs")).unwrap_or_default();
    for m in main_rs.lines().filter_map(|l| l.strip_prefix("mod ").and_then(|m| m.strip_suffix(';'))) {
        if m != "common" && !EXCLUDED.iter().any(|e| e.ends_with(&format!("/{m}.rs"))) && !harness.contains(&format!("mod {m};")) {
            eprintln!("vendor-vectorcraft: WARNING picabia-tests/pathops_it.rs lacks `mod {m};` (new upstream test module)");
        }
    }

    // The pin in the README and the hash list.
    let readme_path = base.join("README.md");
    let readme = std::fs::read_to_string(&readme_path).map_err(|e| format!("read README.md: {e}"))?;
    let mut done = false;
    let mut text: String = readme
        .lines()
        .map(|l| {
            if l.starts_with("Pinned commit:") {
                done = true;
                format!("Pinned commit: `{full}`\n")
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    if !done {
        text.push_str(&format!("Pinned commit: `{full}`\n"));
    }
    std::fs::write(&readme_path, text).map_err(|e| format!("write README.md: {e}"))?;
    let hashes = hash_tree(base)?;
    std::fs::write(base.join("HASHES"), render_hashes(&hashes)).map_err(|e| format!("write HASHES: {e}"))?;
    Ok(())
}
