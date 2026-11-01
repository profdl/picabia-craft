use super::*;
use figcraft_geom::kurbo::Shape;

fn never() -> bool {
    false
}

fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Rgba {
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            pixels.extend_from_slice(&f(x, y));
        }
    }
    Rgba { width: w, height: h, pixels }
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

#[test]
fn preset_table_matches_vectorcraft() {
    use Method::{Abutting as A, Overlapping as O};
    use Mode::{BlackAndWhite as B, Color as C, Grayscale as G, Logo as L};
    use Palette::{Automatic as Au, Limited as Li};
    // id, mode, palette, colors, detail, threshold, paths, corners, noise, method, snap, ignoreWhite, fills, strokes
    #[rustfmt::skip]
    let want = [
        ("default",           B, Li, 6,  50.0, 128, 50.0, 75.0, 25,  A, false, false, true,  false),
        ("autoColor",         C, Au, 6,  85.0, 128, 65.0, 60.0, 16,  A, false, false, true,  false),
        ("highFidelityPhoto", C, Li, 64, 50.0, 128, 90.0, 25.0, 4,   O, false, false, true,  false),
        ("lowFidelityPhoto",  C, Li, 20, 50.0, 128, 60.0, 50.0, 12,  O, false, false, true,  false),
        ("threeColors",       C, Li, 3,  50.0, 128, 60.0, 60.0, 20,  A, false, false, true,  false),
        ("sixColors",         C, Li, 6,  50.0, 128, 65.0, 60.0, 16,  A, false, false, true,  false),
        ("sixteenColors",     C, Li, 16, 50.0, 128, 70.0, 55.0, 10,  A, false, false, true,  false),
        ("shadesOfGray",      G, Li, 8,  50.0, 128, 60.0, 50.0, 10,  O, false, false, true,  false),
        ("blackAndWhiteLogo", B, Li, 6,  50.0, 128, 95.0, 80.0, 8,   A, true,  true,  true,  false),
        ("sketchedArt",       B, Li, 6,  50.0, 150, 50.0, 50.0, 100, A, false, true,  true,  false),
        ("silhouettes",       B, Li, 6,  50.0, 200, 40.0, 60.0, 30,  A, false, true,  true,  false),
        ("lineArt",           B, Li, 6,  50.0, 128, 80.0, 70.0, 10,  A, false, true,  true,  false),
        ("technicalDrawing",  B, Li, 6,  50.0, 128, 95.0, 90.0, 4,   A, true,  true,  true,  false),
        ("outline",           B, Li, 6,  50.0, 128, 80.0, 70.0, 10,  A, false, true,  false, true),
        ("flatLogo",          L, Li, 8,  50.0, 128, 60.0, 50.0, 6,   A, true,  false, true,  false),
    ];
    assert_eq!(want.len(), PRESETS.len());
    for (row, (id, _)) in want.iter().zip(PRESETS.iter()) {
        assert_eq!(row.0, *id);
        let s = preset_settings(id).unwrap_or_default();
        let got = format!(
            "{:?}",
            (
                (s.mode, s.palette, s.colors, s.color_detail, s.threshold, s.paths, s.corners),
                (s.noise, s.method, s.snap_curves_to_lines, s.ignore_white, s.fills, s.strokes)
            )
        );
        let w = format!("{:?}", ((row.1, row.2, row.3, row.4, row.5, row.6, row.7), (row.8, row.9, row.10, row.11, row.12, row.13)));
        assert_eq!(got, w, "preset {id}");
        assert_eq!(s.stroke_width, 10.0);
        assert_eq!(s.ignore_color, None);
        // and back: the settings make VectorCraft's own preset parameters
        let vc = preset(PRESETS.iter().find(|(i, _)| i == id).map_or("", |p| p.1)).unwrap_or_default();
        assert_eq!(params(&s), vc, "params of {id}");
    }
    assert!(preset_settings("custom").is_none() && preset_settings("nope").is_none());
    assert_eq!(TraceSettings::default(), preset_settings("default").unwrap_or_default());
}

#[test]
fn every_vectorcraft_preset_is_mapped() {
    for name in vectorcraft_trace::PRESET_NAMES {
        assert!(PRESETS.iter().any(|(_, n)| n == name), "{name} has no id");
    }
}

#[test]
fn params_clamp() {
    let s = TraceSettings {
        colors: 9999,
        color_detail: f64::NAN,
        paths: -5.0,
        corners: 1e9,
        noise: 0,
        stroke_width: f64::INFINITY,
        fills: false,
        strokes: false,
        ignore_white: true,
        ignore_color: Some([255, 0, 16]),
        ..TraceSettings::default()
    };
    let p = params(&s);
    assert_eq!((p.colors, p.color_detail, p.paths, p.corners, p.noise, p.stroke_width), (256, 50.0, 0.0, 100.0, 1, 10.0));
    assert!(p.fills && !p.strokes);
    assert_eq!(p.ignore_color.as_deref(), Some("#ff0010"));
    let logo = params(&TraceSettings { mode: Mode::Logo, colors: 200, ..TraceSettings::default() });
    assert_eq!(logo.colors, 10);
    // ignore_color only counts with ignore_white
    assert_eq!(params(&TraceSettings { ignore_color: Some([1, 2, 3]), ..TraceSettings::default() }).ignore_color, None);
}

#[test]
fn fit_size_cases() {
    assert_eq!(fit_size(100, 100, 1 << 20), (100, 100));
    let (w, h) = fit_size(20000, 20000, MAX_TRACE_PIXELS);
    assert!(u64::from(w) * u64::from(h) <= MAX_TRACE_PIXELS && w > 1000);
    let (w, h) = fit_size(u32::MAX, 1, 1000);
    assert_eq!((h, u64::from(w) <= 1000), (1, true));
    let (w, h) = fit_size(1, u32::MAX, 1000);
    assert_eq!((w, u64::from(h) <= 1000), (1, true));
    assert_eq!(fit_size(0, 5, 10), (0, 5));
    assert_eq!(fit_size(u32::MAX, u32::MAX, 1), (1, 1));
}

#[test]
fn checker_traces_to_one_path_per_black_cell() {
    let i = img(64, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { BLACK } else { WHITE });
    let s = preset_settings("blackAndWhiteLogo").unwrap_or_default();
    let t = run(&i, &s, 1_000_000, &never).unwrap();
    assert_eq!(t.paths.len(), 32);
    assert!(t.paths.iter().all(|p| p.rgb == [0, 0, 0] && p.stroke.is_none()));
    let area: f64 = t.paths.iter().map(|p| p.path.area().abs()).sum();
    assert!((area - 32.0 * 64.0).abs() < 32.0 * 64.0 * 0.05, "area {area}");
    assert_eq!((t.size, t.source_size, t.downscaled), ((64, 64), (64, 64), false));
    assert!(t.anchors >= 32 * 4);
}

#[test]
fn red_disc_in_color_mode_is_a_disc_path() {
    let (cx, cy, r) = (50.0f64, 50.0f64, 30.0f64);
    let i = img(100, 100, |x, y| {
        let d = ((f64::from(x) + 0.5 - cx).powi(2) + (f64::from(y) + 0.5 - cy).powi(2)).sqrt();
        if d <= r { [220, 20, 20, 255] } else { WHITE }
    });
    let s = TraceSettings { mode: Mode::Color, colors: 2, ignore_white: true, ..TraceSettings::default() };
    let t = run(&i, &s, 1_000_000, &never).unwrap();
    assert_eq!(t.paths.len(), 1, "palette {:?}", t.palette);
    let p = &t.paths[0];
    assert!(p.rgb[0] > 200 && p.rgb[1] < 60 && p.rgb[2] < 60);
    let area = p.path.area().abs();
    let disc = std::f64::consts::PI * r * r;
    assert!((area - disc).abs() < disc * 0.03, "area {area} vs {disc}");
    let b = p.path.bounding_box();
    assert!((b.center().x - cx).abs() < 1.0 && (b.center().y - cy).abs() < 1.0);
    assert!((b.width() - 2.0 * r).abs() < 2.0);
}

#[test]
fn empty_and_tiny_images() {
    let s = TraceSettings::default();
    for (w, h) in [(0, 0), (0, 10), (10, 0)] {
        let t = run(&Rgba { width: w, height: h, pixels: Vec::new() }, &s, 100, &never).unwrap();
        assert!(t.paths.is_empty());
    }
    for p in ["default", "autoColor", "flatLogo", "outline", "highFidelityPhoto"] {
        let s = preset_settings(p).unwrap_or_default();
        let _ = run(&img(1, 1, |_, _| BLACK), &s, 100, &never);
        let _ = run(&img(1, 1, |_, _| [0, 0, 0, 0]), &s, 100, &never);
    }
}

#[test]
fn transparent_and_one_colour_images() {
    for p in ["default", "sixColors", "shadesOfGray", "flatLogo", "outline"] {
        let s = preset_settings(p).unwrap_or_default();
        let clear = run(&img(40, 40, |_, _| [0, 0, 0, 0]), &s, 100_000, &never).unwrap();
        assert!(clear.paths.is_empty(), "{p}");
        let one = run(&img(40, 40, |_, _| [30, 90, 200, 255]), &s, 100_000, &never);
        assert!(one.is_ok() || matches!(one, Err(BridgeError::Failed(_))), "{p}");
    }
}

#[test]
fn bad_length_is_an_error() {
    assert!(matches!(Rgba::new(2, 2, vec![0; 15]), Err(BridgeError::BadImage(_))));
    assert!(Rgba::new(2, 2, vec![0; 16]).is_ok());
    let bad = Rgba { width: u32::MAX, height: u32::MAX, pixels: vec![0; 4] };
    assert!(matches!(run(&bad, &TraceSettings::default(), 10, &never), Err(BridgeError::BadImage(_))));
}

#[test]
fn big_images_are_scaled_down_to_the_budget() {
    // Just over the budget: a bit more than MAX_TRACE_PIXELS, left half black, right half white.
    let w = 4096u32;
    let h = (MAX_TRACE_PIXELS / u64::from(w)) as u32 + 64;
    let i = img(w, h, |x, _| if x < w / 2 { BLACK } else { WHITE });
    let s = TraceSettings { ignore_white: true, ..TraceSettings::default() };
    let t = run(&i, &s, 1_000_000, &never).unwrap();
    assert!(t.downscaled);
    assert_eq!(t.source_size, (w, h));
    assert!(u64::from(t.size.0) * u64::from(t.size.1) <= MAX_TRACE_PIXELS && t.size.0 < w);
    assert_eq!(t.paths.len(), 1);
    let b = t.paths[0].path.bounding_box();
    assert!((b.width() - f64::from(t.size.0) / 2.0).abs() < 2.0, "{b:?}");
}

#[test]
fn thin_lines_trace_as_strokes() {
    let i = img(64, 64, |x, y| if y == 32 && (8..56).contains(&x) { BLACK } else { WHITE });
    let s = preset_settings("outline").unwrap_or_default();
    let t = run(&i, &s, 100_000, &never).unwrap();
    assert!(!t.paths.is_empty());
    assert!(t.paths.iter().any(|p| p.stroke.is_some()), "{:?}", t.paths.iter().map(|p| p.stroke).collect::<Vec<_>>());
    let b = t.paths.iter().fold(None, |acc: Option<figcraft_geom::kurbo::Rect>, p| {
        let r = p.path.bounding_box();
        Some(acc.map_or(r, |a| a.union(r)))
    });
    let b = b.unwrap_or_default();
    assert!(b.width() > 40.0 && b.height() < 4.0, "{b:?}");
}

#[test]
fn cancel_and_anchor_budget() {
    let i = img(64, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { BLACK } else { WHITE });
    let s = TraceSettings::default();
    assert_eq!(run(&i, &s, 1_000_000, &|| true), Err(BridgeError::Cancelled));
    assert_eq!(run(&i, &s, 3, &never), Err(BridgeError::TooComplex));
}

/// A deterministic photo-like image: smooth colour gradients, a few soft blobs and hash noise.
fn photo(w: u32, h: u32) -> Rgba {
    img(w, h, |x, y| {
        let (fx, fy) = (f64::from(x) / f64::from(w), f64::from(y) / f64::from(h));
        let mut n = u64::from(x).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(y).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        n ^= n >> 29;
        n = n.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        n ^= n >> 32;
        let noise = (n & 0xFF) as f64 / 255.0 - 0.5;
        let blob = ((fx * 9.0).sin() * (fy * 7.0).cos() + 1.0) * 0.5;
        let c = |base: f64, k: f64| ((base * 0.55 + blob * 0.35 + noise * k) * 255.0).clamp(0.0, 255.0) as u8;
        [c(fx, 0.12), c(fy, 0.12), c(1.0 - fx * fy, 0.12), 255]
    })
}

/// `cargo test --release -p figcraft-vcbridge -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: run in release"]
fn bench_4000x3000_photo() {
    let i = photo(4000, 3000);
    for name in ["default", "sixteenColors", "highFidelityPhoto"] {
        let s = preset_settings(name).unwrap_or_default();
        let t0 = std::time::Instant::now();
        let r = run(&i, &s, MAX_ANCHORS, &never);
        let dt = t0.elapsed();
        match r {
            Ok(t) => println!("bench {name}: {dt:?}, {} paths, {} anchors", t.paths.len(), t.anchors),
            Err(e) => println!("bench {name}: {dt:?}, {e}"),
        }
    }
}
