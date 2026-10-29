//! `wallpaper`: the 17 plane crystallographic (wallpaper) groups.
//!
//! A group is a lattice (two translation vectors) plus the motif transforms inside one lattice
//! cell. Copy `k` is cell `k / ops` (row-major: column varies fastest) and op `k % ops`, so copy 0
//! (cell 0, op 0) is the identity: the layer itself. The symmetry origin (rotation centres on it,
//! mirror axes through it) is the top-left corner of the layer's bounds plus `originX`/`originY`;
//! the layer is the motif.
//!
//! The ops follow the standard descriptions of the groups (coordinates as fractions of the cell
//! basis, the "general position" lists of the International Tables), written out here as 2x3
//! matrices `x' = a x + b y + t`. The lattice per group:
//!
//! - oblique (`p1`, `p2`): `a = (w, 0)`, `b = h (cos θ, sin θ)`.
//! - rectangular (`pm`, `pg`, `pmm`, `pmg`, `pgg`): `a = (w, 0)`, `b = (0, h)`.
//! - centred rectangular (`cm`, `cmm`): the ops use the conventional `w x h` rectangle; the lattice
//!   is its primitive one, `a = (w, 0)`, `b = (w/2, h/2)`, so a cell holds 2 (`cm`) or 4 (`cmm`)
//!   motifs, not the 4 or 8 of the conventional cell.
//! - square (`p4`, `p4m`, `p4g`): `a = (w, 0)`, `b = (0, w)`; `cellHeight` is ignored.
//! - hexagonal (`p3`, `p3m1`, `p31m`, `p6`, `p6m`): `a = (w, 0)`, `b = w (1/2, √3/2)`; `w` is the
//!   lattice constant, `cellHeight` is ignored.

use figcraft_geom::{Affine, Point};

use super::{Clamp, Ctx, Stage, StageResult};
use crate::MAX_INSTANCES_PER_NODE;
use crate::eval::Guide;
use crate::model::{Wallpaper, WallpaperGroup};

/// The most cells along one side.
pub(crate) const MAX_CELLS: u32 = 100;

type Row = [f64; 3];

/// `x' = x[0] x + x[1] y + x[2]`, `y' = y[0] x + y[1] y + y[2]` in the group's fractional basis.
fn op(x: Row, y: Row) -> Affine {
    Affine::new([x[0], y[0], x[1], y[1], x[2], y[2]])
}

const H: f64 = 0.5;

/// The motif transforms of `g` in its fractional basis; the first is the identity.
fn fractional_ops(g: WallpaperGroup) -> Vec<Affine> {
    use WallpaperGroup::*;
    let id = op([1., 0., 0.], [0., 1., 0.]);
    let c2 = op([-1., 0., 0.], [0., -1., 0.]);
    let mx = op([-1., 0., 0.], [0., 1., 0.]); // x -> -x
    let my = op([1., 0., 0.], [0., -1., 0.]); // y -> -y
    let r4 = op([0., -1., 0.], [1., 0., 0.]); // (x,y) -> (-y, x)
    let r4i = op([0., 1., 0.], [-1., 0., 0.]);
    let diag = op([0., 1., 0.], [1., 0., 0.]); // (y, x)
    let adiag = op([0., -1., 0.], [-1., 0., 0.]); // (-y, -x)
    // Hexagonal basis (120 degrees between a and b).
    let r3 = op([0., -1., 0.], [1., -1., 0.]); // (-y, x-y)
    let r3i = op([-1., 1., 0.], [-1., 0., 0.]); // (-x+y, -x)
    let r6 = op([1., -1., 0.], [1., 0., 0.]); // (x-y, x)
    let r6i = op([0., 1., 0.], [-1., 1., 0.]); // (y, -x+y)
    // p3m1 mirrors, then p31m mirrors.
    let m1 = [adiag, op([-1., 1., 0.], [0., 1., 0.]), op([1., 0., 0.], [1., -1., 0.])];
    let m2 = [diag, op([1., -1., 0.], [0., -1., 0.]), op([-1., 0., 0.], [-1., 1., 0.])];
    match g {
        P1 => vec![id],
        P2 => vec![id, c2],
        Pm | Cm => vec![id, mx],
        Pg => vec![id, op([-1., 0., 0.], [0., 1., H])],
        Pmm | Cmm => vec![id, mx, my, c2],
        Pmg => vec![id, c2, op([-1., 0., H], [0., 1., 0.]), op([1., 0., H], [0., -1., 0.])],
        Pgg => vec![id, c2, op([-1., 0., H], [0., 1., H]), op([1., 0., H], [0., -1., H])],
        P4 => vec![id, r4, c2, r4i],
        P4m => vec![id, r4, c2, r4i, mx, my, diag, adiag],
        P4g => vec![
            id,
            c2,
            r4,
            r4i,
            op([-1., 0., H], [0., 1., H]),
            op([1., 0., H], [0., -1., H]),
            op([0., 1., H], [1., 0., H]),
            op([0., -1., H], [-1., 0., H]),
        ],
        P3 => vec![id, r3, r3i],
        P3m1 => vec![id, r3, r3i, m1[0], m1[1], m1[2]],
        P31m => vec![id, r3, r3i, m2[0], m2[1], m2[2]],
        P6 => vec![id, r3, r3i, c2, r6i, r6],
        P6m => vec![id, r3, r3i, c2, r6i, r6, m1[0], m1[1], m1[2], m2[0], m2[1], m2[2]],
    }
}

/// The lattice basis `(a, b)` of `g` for a `w x h` cell and oblique angle `angle` (degrees).
pub(crate) fn lattice(g: WallpaperGroup, w: f64, h: f64, angle: f64) -> ([f64; 2], [f64; 2]) {
    use WallpaperGroup::*;
    match g {
        P1 | P2 => {
            let t = angle.to_radians();
            ([w, 0.0], [h * t.cos(), h * t.sin()])
        }
        Pm | Pg | Pmm | Pmg | Pgg => ([w, 0.0], [0.0, h]),
        Cm | Cmm => ([w, 0.0], [w / 2.0, h / 2.0]),
        P4 | P4m | P4g => ([w, 0.0], [0.0, w]),
        P3 | P3m1 | P31m | P6 | P6m => ([w, 0.0], [w / 2.0, w * 3f64.sqrt() / 2.0]),
    }
}

/// The basis the fractional ops are written in: the lattice for most groups, the conventional
/// rectangle for the centred ones, the 120-degree basis for the hexagonal ones.
fn ops_basis(g: WallpaperGroup, w: f64, h: f64, angle: f64) -> Affine {
    use WallpaperGroup::*;
    let (a, b) = match g {
        Cm | Cmm => ([w, 0.0], [0.0, h]),
        P3 | P3m1 | P31m | P6 | P6m => ([w, 0.0], [-w / 2.0, w * 3f64.sqrt() / 2.0]),
        _ => lattice(g, w, h, angle),
    };
    Affine::new([a[0], a[1], b[0], b[1], 0.0, 0.0])
}

/// The motif transforms of `g` about the symmetry origin (0, 0), in points; the first is the
/// identity. Their count is the number of motif copies per lattice cell.
pub(crate) fn group_ops(g: WallpaperGroup, w: f64, h: f64, angle: f64) -> Vec<Affine> {
    let b = ops_basis(g, w, h, angle);
    let inv = b.inverse();
    fractional_ops(g).into_iter().map(|m| b * m * inv).collect()
}

pub(crate) fn stage(p: &Wallpaper, ctx: &Ctx) -> StageResult {
    let mut warnings = Vec::new();
    let mut c = Clamp { warnings: &mut warnings };
    let cols = clamp_cells(&mut c, "cols", p.cols);
    let rows = clamp_cells(&mut c, "rows", p.rows);
    let w = c.within("cellWidth", p.cell_width, 0.001, crate::MAX_LENGTH, 100.0);
    let h = c.within("cellHeight", p.cell_height, 0.001, crate::MAX_LENGTH, 100.0);
    let angle = c.within("angle", p.angle, 1.0, 179.0, 90.0);
    let origin = Point::new(ctx.bounds.x0 + c.length("originX", p.origin_x), ctx.bounds.y0 + c.length("originY", p.origin_y));
    let per_cell = fractional_ops(p.group).len();
    let total = rows.saturating_mul(cols).saturating_mul(per_cell);
    // Checked before allocating anything per copy.
    if total.saturating_mul(ctx.n_prev) > MAX_INSTANCES_PER_NODE {
        return StageResult { stage: Stage::TooMany(total), guides: Vec::new(), warnings };
    }
    let ops = group_ops(p.group, w, h, angle);
    let (a, b) = lattice(p.group, w, h, angle);
    let to_origin = Affine::translate(origin.to_vec2());
    let from_origin = Affine::translate(-origin.to_vec2());
    let mut copies = Vec::with_capacity(total);
    for j in 0..rows {
        for i in 0..cols {
            let cell = Affine::translate((a[0] * i as f64 + b[0] * j as f64, a[1] * i as f64 + b[1] * j as f64));
            for o in &ops {
                copies.push(cell * to_origin * *o * from_origin);
            }
        }
    }
    // The lattice: one line per cell edge along each basis vector.
    let at = |i: usize, j: usize| Point::new(origin.x + a[0] * i as f64 + b[0] * j as f64, origin.y + a[1] * i as f64 + b[1] * j as f64);
    let mut guides = Vec::with_capacity(rows + cols + 2);
    for j in 0..=rows {
        guides.push(Guide::Line { from: at(0, j), to: at(cols, j) });
    }
    for i in 0..=cols {
        guides.push(Guide::Line { from: at(i, 0), to: at(i, rows) });
    }
    StageResult { stage: Stage::Array(copies), guides, warnings }
}

fn clamp_cells(c: &mut Clamp<'_>, name: &str, v: u32) -> usize {
    if v > MAX_CELLS {
        c.warnings.push(format!("`{name}` is limited to {MAX_CELLS}"));
    }
    usize::try_from(v.clamp(1, MAX_CELLS)).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use figcraft_geom::{Affine, Rect};
    use serde_json::json;

    use super::*;
    use crate::{Output, Stack, evaluate, parse};

    const B: Rect = Rect::new(10.0, 20.0, 30.0, 35.0);
    const COUNTS: [usize; 17] = [1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 8, 8, 3, 6, 6, 6, 12];

    fn eval(m: serde_json::Value) -> Output {
        let s: Stack = parse(&json!({"v": 1, "stack": [m]}).to_string()).unwrap();
        evaluate(&s, B)
    }

    fn det(a: Affine) -> f64 {
        let c = a.as_coeffs();
        c[0] * c[3] - c[1] * c[2]
    }

    fn orthonormal(a: Affine) -> bool {
        let c = a.as_coeffs();
        let (c0, c1, c2, c3) = (c[0], c[1], c[2], c[3]);
        (c0 * c0 + c1 * c1 - 1.0).abs() < 1e-9 && (c2 * c2 + c3 * c3 - 1.0).abs() < 1e-9 && (c0 * c2 + c1 * c3).abs() < 1e-9
    }

    #[test]
    fn copies_per_cell() {
        for (g, n) in WallpaperGroup::ALL.iter().zip(COUNTS) {
            assert_eq!(group_ops(*g, 100.0, 80.0, 70.0).len(), n, "{g:?}");
            let out = eval(json!({"id": "w", "kind": "wallpaper", "group": g, "cols": 2, "rows": 3}));
            assert_eq!(out.instances.len(), 6 * n, "{g:?}");
            assert!(out.warnings.is_empty(), "{:?}", out.warnings);
            assert_eq!(out.instances[0].transform, Affine::IDENTITY);
        }
    }

    #[test]
    fn every_transform_is_an_isometry() {
        for g in WallpaperGroup::ALL {
            for (w, h, ang) in [(100.0, 100.0, 90.0), (120.0, 70.0, 60.0)] {
                for o in group_ops(g, w, h, ang) {
                    assert!((det(o).abs() - 1.0).abs() < 1e-9, "{g:?} {o:?}");
                    assert!(orthonormal(o), "{g:?} {o:?}");
                }
            }
            let out = eval(json!({"id": "w", "kind": "wallpaper", "group": g, "cellWidth": 90, "cellHeight": 50, "angle": 65}));
            for i in &out.instances {
                assert!((det(i.transform).abs() - 1.0).abs() < 1e-9 && orthonormal(i.transform), "{g:?}");
            }
        }
    }

    #[test]
    fn reflections_exist_exactly_in_the_groups_that_have_them() {
        use WallpaperGroup::*;
        let with = [Pm, Pg, Cm, Pmm, Pmg, Pgg, Cmm, P4m, P4g, P3m1, P31m, P6m];
        for g in WallpaperGroup::ALL {
            let has = group_ops(g, 100.0, 100.0, 90.0).iter().any(|o| det(*o) < 0.0);
            assert_eq!(has, with.contains(&g), "{g:?}");
        }
    }

    /// The ops together with the lattice translations close under composition.
    #[test]
    fn ops_form_a_group_modulo_the_lattice() {
        for g in WallpaperGroup::ALL {
            let (w, h, ang) = (100.0, 70.0, 75.0);
            let ops = group_ops(g, w, h, ang);
            let (a, b) = lattice(g, w, h, ang);
            // Lattice coordinates of a point: solve p = u a + v b.
            let d = a[0] * b[1] - a[1] * b[0];
            let is_lattice = |p: figcraft_geom::Point| {
                let (u, v) = ((p.x * b[1] - p.y * b[0]) / d, (a[0] * p.y - a[1] * p.x) / d);
                (u - u.round()).abs() < 1e-9 && (v - v.round()).abs() < 1e-9
            };
            for x in &ops {
                for y in &ops {
                    let z = *x * *y;
                    let found = ops.iter().any(|o| {
                        let l = z.as_coeffs();
                        let m = o.as_coeffs();
                        (0..4).all(|i| (l[i] - m[i]).abs() < 1e-9) && is_lattice(figcraft_geom::Point::new(l[4] - m[4], l[5] - m[5]))
                    });
                    assert!(found, "{g:?} not closed");
                }
            }
        }
    }

    #[test]
    fn cells_are_lattice_translates() {
        let out = eval(json!({"kind": "wallpaper", "group": "p1", "cols": 3, "rows": 2, "cellWidth": 50, "cellHeight": 40}));
        let c = out.instances[4].transform.as_coeffs(); // row 1, col 1
        assert!((c[4] - 50.0).abs() < 1e-9 && (c[5] - 40.0).abs() < 1e-9);
        // The oblique angle slants the second axis.
        let out = eval(json!({"kind": "wallpaper", "group": "p2", "cols": 1, "rows": 2, "cellWidth": 50, "cellHeight": 40, "angle": 60}));
        let c = out.instances[2].transform.as_coeffs();
        assert!((c[4] - 20.0).abs() < 1e-9 && (c[5] - 40.0 * 3f64.sqrt() / 2.0).abs() < 1e-9);
    }

    #[test]
    fn mirror_axis_runs_through_the_bounds_corner() {
        let out = eval(json!({"kind": "wallpaper", "group": "pm", "cols": 1, "rows": 1}));
        let m = out.instances[1].transform;
        // x = 10 is the axis: the corner is fixed and x = 30 goes to x = -10.
        assert!((m * figcraft_geom::Point::new(10.0, 25.0) - figcraft_geom::Point::new(10.0, 25.0)).hypot() < 1e-9);
        assert!((m * figcraft_geom::Point::new(30.0, 25.0) - figcraft_geom::Point::new(-10.0, 25.0)).hypot() < 1e-9);
    }

    #[test]
    fn caps() {
        // 100 x 100 x 12 would be 120 000: nothing is produced, with a warning.
        let out = eval(json!({"id": "w", "kind": "wallpaper", "group": "p6m", "cols": 100, "rows": 100}));
        assert!(out.instances.len() <= crate::MAX_INSTANCES_PER_NODE);
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(out.warnings[0].modifier.as_deref(), Some("w"));
        // 100 x 100 x 1 = 10 000 fits.
        let out = eval(json!({"kind": "wallpaper", "group": "p1", "cols": 100, "rows": 100}));
        assert_eq!(out.instances.len(), 10_000);
        assert_eq!(out.guides.len(), 202);
        // Past 100 cells a side is limited.
        let out = eval(json!({"kind": "wallpaper", "group": "p1", "cols": 500, "rows": 1}));
        assert_eq!(out.instances.len(), 100);
        assert_eq!(out.warnings.len(), 1);
        // Stacked after another array the product is capped too.
        let s = parse(
            &json!({"v": 1, "stack": [{"id": "a", "kind": "linearArray", "count": 200}, {"id": "w", "kind": "wallpaper", "group": "p4", "cols": 10, "rows": 10}]})
                .to_string(),
        )
        .unwrap();
        let out = evaluate(&s, B);
        assert_eq!(out.instances.len(), 200);
    }

    #[test]
    fn nan_and_huge_params_are_clamped() {
        let huge = json!({"kind": "wallpaper", "group": "cmm", "cols": 2, "rows": 2, "cellWidth": 1e300, "cellHeight": -5, "angle": 1e300, "originX": 1e300, "originY": -1e300});
        let out = eval(huge);
        assert_eq!(out.instances.len(), 16);
        assert!(out.instances.iter().all(|i| i.transform.is_finite()));
        assert!(!out.warnings.is_empty());
        let mut m = crate::Modifier::new("m1", "wallpaper").unwrap();
        if let crate::ModifierKind::Wallpaper(p) = &mut m.kind {
            p.cell_width = f64::NAN;
            p.cell_height = f64::INFINITY;
            p.angle = f64::NAN;
            p.origin_x = f64::NAN;
        }
        let mut s = Stack::default();
        s.modifiers.push(m);
        let out = evaluate(&s, B);
        assert_eq!(out.instances.len(), 9 * 8);
        assert!(out.instances.iter().all(|i| i.transform.is_finite()));
    }

    #[test]
    fn json_round_trip_and_bad_group() {
        let m = crate::Modifier::new("m1", "wallpaper").unwrap();
        let v = m.to_value();
        assert_eq!(v["group"], "p4m");
        assert_eq!(crate::Modifier::from_value(&v).unwrap(), m);
        for g in WallpaperGroup::ALL {
            let v = serde_json::to_value(g).unwrap();
            assert_eq!(serde_json::from_value::<WallpaperGroup>(v).unwrap(), g);
        }
        // An unknown group name doesn't read: the modifier is kept as unknown and ignored.
        let m = crate::Modifier::from_value(&json!({"id": "x", "kind": "wallpaper", "group": "p5"})).unwrap();
        assert!(matches!(m.kind, crate::ModifierKind::Other { .. }));
        assert_eq!(m.to_value()["group"], "p5");
    }
}
