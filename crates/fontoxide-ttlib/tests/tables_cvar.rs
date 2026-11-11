//! `Tests/ttLib/tables/_c_v_a_r_test.py`.

mod common;

use std::collections::BTreeMap;

use common::{TestResult, from_xml, get_xml, hex, hex_str};
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{Cvar, Cvt, Fvar};
use fontoxide_ttlib::{Font, TableType};

const CVAR_DATA: &str = "0001 0000 8002 0018 0004 8000 4000 0000 0004 8000 C000 3333 \
	03 02 02 01 01 02 03 01 04 02 09 07 08";

const CVAR_PRIVATE_POINT_DATA: &str = "0001 0000 0002 0018 0009 A000 4000 0000 0009 A000 C000 3333 \
	03 02 02 01 01 02 03 01 04 03 02 02 01 01 02 09 07 08";

const CVAR_XML: [&str; 14] = [
	"<version major=\"1\" minor=\"0\"/>",
	"<tuple>",
	"  <coord axis=\"wght\" value=\"1.0\"/>",
	"  <delta cvt=\"2\" value=\"3\"/>",
	"  <delta cvt=\"3\" value=\"1\"/>",
	"  <delta cvt=\"4\" value=\"4\"/>",
	"</tuple>",
	"<tuple>",
	"  <coord axis=\"wght\" value=\"-1.0\"/>",
	"  <coord axis=\"wdth\" value=\"0.8\"/>",
	"  <delta cvt=\"2\" value=\"9\"/>",
	"  <delta cvt=\"3\" value=\"7\"/>",
	"  <delta cvt=\"4\" value=\"8\"/>",
	"</tuple>",
];

fn variation(axes: &[(&str, (f64, f64, f64))], deltas: [i64; 3]) -> TupleVariation {
	let mut coordinates = vec![None, None];
	coordinates.extend(deltas.map(|d| Some(Delta::Value(Number::Int(d)))));
	TupleVariation {
		axes: axes
			.iter()
			.map(|(tag, region)| ((*tag).to_owned(), *region))
			.collect::<BTreeMap<_, _>>(),
		coordinates,
	}
}

fn cvar_variations() -> Vec<TupleVariation> {
	vec![
		variation(&[("wght", (0.0, 1.0, 1.0))], [3, 1, 4]),
		variation(
			&[
				("wght", (-1.0, -1.0, 0.0)),
				("wdth", (0.0, 0.7999878, 0.7999878)),
			],
			[9, 7, 8],
		),
	]
}

fn assert_variations_almost_equal(v1: &[TupleVariation], v2: &[TupleVariation]) {
	assert_eq!(v1.len(), v2.len());
	for (a, b) in v1.iter().zip(v2) {
		assert_eq!(
			a.axes.keys().collect::<Vec<_>>(),
			b.axes.keys().collect::<Vec<_>>()
		);
		for (tag, s1) in &a.axes {
			let s2 = b.axes[tag];
			for (x, y) in [(s1.0, s2.0), (s1.1, s2.1), (s1.2, s2.2)] {
				assert!((x - y).abs() < 5e-8, "{x} != {y}");
			}
		}
		assert_eq!(a.coordinates, b.coordinates);
	}
}

/// `makeFont`.
fn make_font() -> Font {
	let mut font = Font::new();
	font.insert(Box::new(Cvt {
		values: vec![0, 0, 0, 1000, -2000],
	}));
	let mut fvar = Fvar::default();
	for tag in [b"wght", b"wdth"] {
		fvar.axes.push(Axis {
			axis_tag: *tag,
			..Default::default()
		});
	}
	font.insert(Box::new(fvar));
	font
}

fn cvar_with_variations() -> Cvar {
	Cvar {
		variations: cvar_variations(),
		..Default::default()
	}
}

#[test]
fn test_compile() -> TestResult {
	let mut font = make_font();
	assert_eq!(
		hex_str(&cvar_with_variations().compile_with(&mut font, false)?),
		hex_str(&hex(CVAR_PRIVATE_POINT_DATA))
	);
	Ok(())
}

#[test]
fn test_compile_shared_points() -> TestResult {
	let mut font = make_font();
	assert_eq!(
		hex_str(&cvar_with_variations().compile_with(&mut font, true)?),
		hex_str(&hex(CVAR_DATA))
	);
	Ok(())
}

#[test]
fn test_decompile() -> TestResult {
	let mut font = make_font();
	let cvar = Cvar::decompile(&hex(CVAR_PRIVATE_POINT_DATA), &mut font)?;
	assert_eq!(cvar.major_version, 1);
	assert_eq!(cvar.minor_version, 0);
	assert_variations_almost_equal(&cvar.variations, &cvar_variations());
	Ok(())
}

#[test]
fn test_decompile_shared_points() -> TestResult {
	let mut font = make_font();
	let cvar = Cvar::decompile(&hex(CVAR_DATA), &mut font)?;
	assert_eq!(cvar.major_version, 1);
	assert_eq!(cvar.minor_version, 0);
	assert_variations_almost_equal(&cvar.variations, &cvar_variations());
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let mut font = make_font();
	let mut cvar = Cvar::default();
	from_xml(&mut cvar, &CVAR_XML.join(""), &mut font)?;
	assert_eq!(cvar.major_version, 1);
	assert_eq!(cvar.minor_version, 0);
	assert_variations_almost_equal(&cvar.variations, &cvar_variations());
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let mut font = make_font();
	assert_eq!(get_xml(&cvar_with_variations(), &mut font)?, CVAR_XML);
	Ok(())
}
