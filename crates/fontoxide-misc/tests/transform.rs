//! Port of fontTools' `Tests/misc/transform_test.py`.

use std::f64::consts::PI;

use fontoxide_misc::transform::{DecomposedTransform, IDENTITY, Transform};

fn t(xx: f64, xy: f64, yx: f64, yy: f64, dx: f64, dy: f64) -> Transform {
	Transform::new(xx, xy, yx, yy, dx, dy)
}

/// `pytest.approx` on the six values.
fn assert_approx(a: Transform, e: Transform) {
	for (x, y) in a.to_array().iter().zip(e.to_array()) {
		assert!((x - y).abs() <= (1e-6 * y.abs()).max(1e-12), "{a} != {e}");
	}
}

#[test]
fn test_examples() {
	let id = Transform::default();
	assert_eq!(id.to_string(), "<Transform [1 0 0 1 0 0]>");
	assert_eq!(id.scale(2.0, None), t(2.0, 0.0, 0.0, 2.0, 0.0, 0.0));
	assert_eq!(id.scale(2.5, Some(5.5)), t(2.5, 0.0, 0.0, 5.5, 0.0, 0.0));
	assert_eq!(
		id.scale(2.0, Some(3.0)).transform_point((100.0, 100.0)),
		(200.0, 300.0)
	);
}

// The doctests of `fontTools.misc.transform`.
#[test]
fn doctests() {
	let tr = Transform::offset(2.0, 3.0);
	assert_eq!(tr.transform_point((100.0, 100.0)), (102.0, 103.0));
	let t2 = tr.scale(0.5, None);
	assert_eq!(t2.transform_point((100.0, 100.0)), (52.0, 53.0));
	let t3 = t2.rotate(PI / 2.0);
	assert_eq!(t3.transform_point((0.0, 0.0)), (2.0, 3.0));
	assert_eq!(t3.transform_point((100.0, 100.0)), (-48.0, 53.0));
	let tr = IDENTITY
		.scale(0.5, None)
		.translate(100.0, 200.0)
		.skew(0.1, 0.2);
	assert_eq!(
		tr.transform_points(&[(0.0, 0.0), (1.0, 1.0), (100.0, 100.0)]),
		[
			(50.0, 100.0),
			(50.550167336042726, 100.60135501775433),
			(105.01673360427253, 160.13550177543362)
		]
	);
	let t1 = IDENTITY.scale(2.0, Some(3.0)).translate(4.0, 6.0);
	let t2 = IDENTITY.translate(8.0, 18.0).scale(2.0, Some(3.0));
	assert_eq!(t1, t2);
	let t1 = IDENTITY.scale(0.2, Some(0.3)).translate(0.4, 0.6);
	let t2 = IDENTITY.translate(0.08, 0.18).scale(0.2, Some(0.3));
	assert_eq!(t1.to_string(), "<Transform [0.2 0 0 0.3 0.08 0.18]>");
	assert_eq!(t2.to_string(), "<Transform [0.2 0 0 0.3 0.08 0.18]>");
	assert_ne!(t1, t2);
	assert_eq!(
		Transform::new(2.0, 0.0, 0.0, 3.0, 1.0, 6.0)
			.reverse_transform(&t(4.0, 3.0, 2.0, 1.0, 5.0, 6.0)),
		t(8.0, 6.0, 6.0, 3.0, 21.0, 15.0)
	);
	assert_eq!(
		IDENTITY.skew(PI / 4.0, 0.0).to_string(),
		"<Transform [1 0 1 1 0 0]>"
	);
	assert_eq!(
		DecomposedTransform {
			scale_x: 2.0,
			scale_y: 2.0,
			..Default::default()
		}
		.to_transform()
		.to_string(),
		"<Transform [2 0 0 2 0 0]>"
	);
}

#[test]
fn test_transform_points() {
	let tr = t(2.0, 0.0, 0.0, 3.0, 0.0, 0.0);
	assert_eq!(
		tr.transform_points(&[(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)]),
		[(0.0, 0.0), (0.0, 300.0), (200.0, 300.0), (200.0, 0.0)]
	);
}

#[test]
fn test_transform_vector() {
	let tr = t(2.0, 0.0, 0.0, 3.0, -10.0, 30.0);
	assert_eq!(tr.transform_vector((-4.0, 5.0)), (-8.0, 15.0));
	assert_eq!(
		tr.transform_vectors(&[(-4.0, 5.0), (-6.0, 7.0)]),
		[(-8.0, 15.0), (-12.0, 21.0)]
	);
}

#[test]
fn test_translate_and_scale() {
	assert_eq!(
		IDENTITY.translate(20.0, 30.0),
		t(1.0, 0.0, 0.0, 1.0, 20.0, 30.0)
	);
	assert_eq!(IDENTITY.scale(5.0, None), t(5.0, 0.0, 0.0, 5.0, 0.0, 0.0));
	assert_eq!(
		IDENTITY.scale(5.0, Some(6.0)),
		t(5.0, 0.0, 0.0, 6.0, 0.0, 0.0)
	);
}

#[test]
fn test_rotate() {
	assert_eq!(IDENTITY.rotate(PI / 2.0), t(0.0, 1.0, -1.0, 0.0, 0.0, 0.0));
	assert_eq!(IDENTITY.rotate(-PI / 2.0), t(0.0, -1.0, 1.0, 0.0, 0.0, 0.0));
	assert_approx(
		IDENTITY.rotate(30f64.to_radians()),
		t(0.866025, 0.5, -0.5, 0.866025, 0.0, 0.0),
	);
}

#[test]
fn test_skew() {
	assert_approx(
		IDENTITY.skew(PI / 4.0, 0.0),
		t(1.0, 0.0, 1.0, 1.0, 0.0, 0.0),
	);
}

#[test]
fn test_transform() {
	let tr = t(2.0, 0.0, 0.0, 3.0, 1.0, 6.0);
	assert_eq!(
		tr.transform(&t(4.0, 3.0, 2.0, 1.0, 5.0, 6.0)),
		t(8.0, 9.0, 4.0, 3.0, 11.0, 24.0)
	);
}

#[test]
fn test_reverse_transform() {
	let tr = t(2.0, 0.0, 0.0, 3.0, 1.0, 6.0);
	assert_eq!(
		tr.reverse_transform(&t(4.0, 3.0, 2.0, 1.0, 5.0, 6.0)),
		t(8.0, 6.0, 6.0, 3.0, 21.0, 15.0)
	);
	let tr = t(4.0, 3.0, 2.0, 1.0, 5.0, 6.0);
	assert_eq!(
		tr.transform(&t(2.0, 0.0, 0.0, 3.0, 1.0, 6.0)),
		t(8.0, 6.0, 6.0, 3.0, 21.0, 15.0)
	);
}

#[test]
fn test_inverse() {
	let tr = IDENTITY.translate(2.0, 3.0).scale(4.0, Some(5.0));
	assert_eq!(tr.transform_point((10.0, 20.0)), (42.0, 103.0));
	assert_eq!(tr.inverse().transform_point((42.0, 103.0)), (10.0, 20.0));
	assert_eq!(IDENTITY.inverse(), IDENTITY);
}

#[test]
fn test_to_ps() {
	let tr = IDENTITY.scale(2.0, Some(3.0)).translate(4.0, 5.0);
	assert_eq!(tr.to_ps(), "[2 0 0 3 8 15]");
}

#[test]
fn test_ne_bool_repr() {
	assert_ne!(IDENTITY, t(2.0, 0.0, 0.0, 2.0, 0.0, 0.0));
	assert!(IDENTITY.is_identity());
	assert!(!t(2.0, 0.0, 0.0, 2.0, 0.0, 0.0).is_identity());
	assert!(!t(1.0, 0.0, 0.0, 1.0, 1.0, 0.0).is_identity());
	assert_eq!(
		t(1.0, 2.0, 3.0, 4.0, 5.0, 6.0).to_string(),
		"<Transform [1 2 3 4 5 6]>"
	);
}

#[test]
fn test_offset_and_scale() {
	assert_eq!(Transform::offset(0.0, 0.0), IDENTITY);
	assert_eq!(Transform::offset(1.0, 0.0), t(1.0, 0.0, 0.0, 1.0, 1.0, 0.0));
	assert_eq!(Transform::offset(1.0, 2.0), t(1.0, 0.0, 0.0, 1.0, 1.0, 2.0));
	assert_eq!(Transform::scaling(1.0, None), IDENTITY);
	assert_eq!(
		Transform::scaling(2.0, None),
		t(2.0, 0.0, 0.0, 2.0, 0.0, 0.0)
	);
	assert_eq!(
		Transform::scaling(1.0, Some(2.0)),
		t(1.0, 0.0, 0.0, 2.0, 0.0, 0.0)
	);
}

#[test]
fn test_decompose() {
	let d = t(2.0, 0.0, 0.0, 3.0, 5.0, 7.0).to_decomposed();
	assert_eq!(
		(d.scale_x, d.scale_y, d.translate_x, d.translate_y),
		(2.0, 3.0, 5.0, 7.0)
	);
	let d = t(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0).to_decomposed();
	assert_eq!((d.scale_x, d.scale_y, d.rotation), (-1.0, 1.0, 0.0));
	let d = t(1.0, 0.0, 0.0, -1.0, 0.0, 0.0).to_decomposed();
	assert_eq!((d.scale_x, d.scale_y, d.rotation), (1.0, -1.0, 0.0));
}

#[test]
fn test_from_transform_zero_first_column() {
	let yx_yy = [
		(1.0, -1.0),
		(-1.0, -1.0),
		(0.0, -1.0),
		(-0.0, -1.0),
		(1.0, 0.0),
		(-1.0, 0.0),
		(1.0, 1.0),
		(-1.0, 1.0),
		(0.0, 1.0),
		(0.0, 0.0),
	];
	for xx in [0.0, -0.0] {
		for (yx, yy) in yx_yy {
			let transform = t(xx, 0.0, yx, yy, 3.0, 5.0);
			let d = DecomposedTransform::from_transform(&transform);
			assert!(-180.0 < d.rotation && d.rotation <= 180.0);
			assert_eq!(d.skew_y, 0.0);
			assert_approx(d.to_transform(), transform);
		}
	}
}

#[test]
fn test_decomposed_identity_scale_to_transform() {
	let d = DecomposedTransform::default();
	assert!(d.is_identity());
	assert_eq!(
		d,
		DecomposedTransform {
			scale_x: 1.0,
			..Default::default()
		}
	);
	let d = DecomposedTransform {
		scale_x: 2.0,
		scale_y: 3.0,
		..Default::default()
	};
	assert_eq!((d.scale_x, d.scale_y), (2.0, 3.0));
	assert_eq!(d.to_transform(), t(2.0, 0.0, 0.0, 3.0, 0.0, 0.0));
}

#[test]
fn test_roundtrip() {
	let d =
		|scale_x: f64, scale_y: f64, rotation: f64, skew_x: f64, skew_y: f64| DecomposedTransform {
			scale_x,
			scale_y,
			rotation,
			skew_x,
			skew_y,
			..Default::default()
		};
	let mut cases = vec![
		d(1.0, 0.0, 0.0, 0.0, 0.0),
		d(0.0, 1.0, 0.0, 0.0, 0.0),
		d(1.0, 0.0, 30.0, 0.0, 0.0),
		d(0.0, 1.0, 30.0, 0.0, 0.0),
		d(1.0, 1.0, 0.0, 0.0, 0.0),
		d(-1.0, 1.0, 0.0, 0.0, 0.0),
		d(1.0, -1.0, 0.0, 0.0, 0.0),
		d(-1.0, -1.0, 0.0, 0.0, 0.0),
		d(1.0, 1.0, 90.0, 0.0, 0.0),
		d(1.0, 1.0, -90.0, 0.0, 0.0),
		d(1.0, 1.0, 0.0, 45.0, 0.0),
		d(1.0, 1.0, 0.0, 0.0, 45.0),
	];
	for scale in [-1.0, -2.0] {
		for rotation in [0.0, 30.0, -30.0] {
			if scale == -2.0 && rotation == 0.0 {
				continue;
			}
			cases.push(d(scale, 1.0, rotation, 45.0, 0.0));
			cases.push(d(scale, 1.0, rotation, 0.0, 45.0));
			cases.push(d(1.0, scale, rotation, 45.0, 0.0));
			cases.push(d(1.0, scale, rotation, 0.0, 45.0));
		}
	}
	assert_eq!(cases.len(), 32);
	for decomposed in cases {
		let tr = decomposed.to_transform();
		assert_approx(tr.to_decomposed().to_transform(), tr);
	}
}
