//! The `GlyphCoordinates` doctests of `Lib/fontTools/ttLib/tables/_g_l_y_f.py`.
//!
//! fontoxide has no `GlyphCoordinates` class: coordinates are a
//! `Vec<(f64, f64)>`, which, like the `array("d")` behind `GlyphCoordinates`,
//! compares and copies by value. The in-place operators map onto
//! [`glyf::translate`], [`glyf::scale`] and [`glyf::transform`], which is what
//! `__iadd__`, `__isub__`, `__imul__` and `__itruediv__` call for tuples and
//! numbers.

use fontoxide_ttlib::tables::glyf;

type Coords = Vec<(f64, f64)>;

#[test]
fn glyph_coordinates_translate() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::translate(&mut g, (0.5, 0.0));
	assert_eq!(g, [(1.5, 2.0)]);
}

#[test]
fn glyph_coordinates_scale() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::scale(&mut g, (0.5, 0.0));
	assert_eq!(g, [(0.5, 0.0)]);
}

#[test]
fn glyph_coordinates_transform() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::transform(&mut g, [[0.5, 0.0], [0.2, 0.5]]);
	assert_eq!(g, [(1.0 * 0.5 + 2.0 * 0.2, 1.0)]);
}

#[test]
fn glyph_coordinates_eq_ne() {
	let g: Coords = vec![(1.0, 2.0)];
	let g2: Coords = vec![(1.0, 2.0)];
	let g3: Coords = vec![(1.5, 2.0)];
	assert!(g == g2);
	assert!(g != g3);
	assert!(g2 != g3);
}

#[test]
fn glyph_coordinates_pos() {
	let g: Coords = vec![(1.0, 2.0)];
	let mut g2 = g.clone();
	assert_eq!(g2, [(1.0, 2.0)]);
	glyf::translate(&mut g2, (1.0, 0.0));
	assert_eq!(g2, [(2.0, 2.0)]);
	assert_eq!(g, [(1.0, 2.0)]);
}

#[test]
fn glyph_coordinates_neg() {
	let g: Coords = vec![(1.0, 2.0)];
	let g2: Coords = g.iter().map(|&(x, y)| (-x, -y)).collect();
	assert_eq!(g2, [(-1.0, -2.0)]);
	assert_eq!(g, [(1.0, 2.0)]);
}

/// `g += other` for another `GlyphCoordinates`.
fn add(a: &mut Coords, b: &[(f64, f64)]) {
	assert_eq!(a.len(), b.len());
	for (p, q) in a.iter_mut().zip(b) {
		p.0 += q.0;
		p.1 += q.1;
	}
}

#[test]
fn glyph_coordinates_iadd() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::translate(&mut g, (0.5, 0.0));
	assert_eq!(g, [(1.5, 2.0)]);
	add(&mut g, &[(3.0, 4.0)]);
	assert_eq!(g, [(4.5, 6.0)]);
}

#[test]
fn glyph_coordinates_isub() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::translate(&mut g, (-0.5, -0.0));
	assert_eq!(g, [(0.5, 2.0)]);
	add(&mut g, &[(-3.0, -4.0)]);
	assert_eq!(g, [(-2.5, -2.0)]);
}

#[test]
fn glyph_coordinates_imul() {
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::scale(&mut g, (2.0, 0.5));
	glyf::scale(&mut g, (2.0, 2.0));
	assert_eq!(g, [(4.0, 2.0)]);
	let mut g: Coords = vec![(1.0, 2.0)];
	glyf::scale(&mut g, (2.0, 2.0));
	assert_eq!(g, [(2.0, 4.0)]);
}

#[test]
fn glyph_coordinates_itruediv() {
	let mut g: Coords = vec![(1.0, 3.0)];
	glyf::scale(&mut g, (1.0 / 0.5, 1.0 / 1.5));
	glyf::scale(&mut g, (1.0 / 2.0, 1.0 / 2.0));
	assert_eq!(g, [(1.0, 1.0)]);
}

#[test]
fn glyph_coordinates_bool() {
	// `bool(g)` is whether there are any points, whatever their values.
	let g: Coords = vec![];
	assert!(g.is_empty());
	let g: Coords = vec![(0.0, 0.0), (0.0, 0.0)];
	assert!(!g.is_empty());
	let g: Coords = vec![(0.0, 0.0), (1.0, 0.0)];
	assert!(!g.is_empty());
	let g: Coords = vec![(0.0, 0.5), (0.0, 0.0)];
	assert!(!g.is_empty());
}
