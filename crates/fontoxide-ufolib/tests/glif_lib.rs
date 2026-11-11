//! Port of fontTools' `Tests/ufoLib/glifLib_test.py`.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use common::{assert_glif_error, demo_font_glyph_set_path, take_logs, tempdir};
use fontoxide_pens::RecordingPointPen;
use fontoxide_ufolib::glif_lib::write_glyph_to_bytes;
use fontoxide_ufolib::{
	Dict, GlifPointPen, Glyph, GlyphSet, GlyphSetOptions, PointPenAdapter, Result, UfoLibError,
	Value, glyph_name_to_file_name, read_glyph_from_string, write_glyph_to_string,
};

fn glyph_set_dir() -> PathBuf {
	demo_font_glyph_set_path()
}

fn options(ufo_format_version: Option<i64>) -> GlyphSetOptions {
	GlyphSetOptions {
		ufo_format_version: ufo_format_version.map(Into::into),
		..Default::default()
	}
}

/// `dst.writeGlyph(glyphName, g, g.drawPoints)` for the glyph `g` of `src`
/// with its attributes loaded.
fn copy_glyph(src: &GlyphSet, dst: &mut GlyphSet, glyph_name: &str) -> Result<()> {
	let glyph = src.glyph(glyph_name)?;
	dst.write_glyph(
		glyph_name,
		Some(&glyph),
		Some(&mut |pen: &mut GlifPointPen| src.read_glyph(glyph_name, None, Some(pen), None)),
		None,
		None,
	)
}

// GlyphSetTests

#[test]
fn test_round_trip() -> Result<()> {
	let dst_dir = tempdir("roundtrip");
	let src = GlyphSet::new(glyph_set_dir(), options(Some(2)))?;
	let mut dst = GlyphSet::new(dst_dir.path(), options(Some(2)))?;
	for glyph_name in src.keys() {
		copy_glyph(&src, &mut dst, &glyph_name)?;
	}
	// compare raw file data: fontTools compares the lines added and removed
	// by the round trip; here the files must be those fontTools writes
	// (tools/reference/ufolib_fixtures.py)
	let expected_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/roundtrip");
	let mut names = src.keys();
	names.sort();
	for glyph_name in names {
		let file_name = &src.contents[&glyph_name];
		let new = std::fs::read_to_string(dst_dir.join(file_name))?;
		let expected = std::fs::read_to_string(expected_dir.join(file_name))?;
		assert_eq!(
			new, expected,
			"{glyph_name}.glif differs from what fontTools writes"
		);
	}
	Ok(())
}

#[test]
fn test_contents_exist() {
	let dst_dir = tempdir("contentsexist");
	assert_glif_error(GlyphSet::new(
		dst_dir.path(),
		GlyphSetOptions {
			ufo_format_version: Some(2.into()),
			expect_contents_file: true,
			..Default::default()
		},
	));
}

#[test]
fn test_rebuild_contents() -> Result<()> {
	let mut gset = GlyphSet::new(glyph_set_dir(), options(None))?;
	let contents = gset.contents.clone();
	gset.rebuild_contents(Some(false))?;
	assert_eq!(contents, gset.contents);
	Ok(())
}

#[test]
fn test_reverse_contents() -> Result<()> {
	let mut gset = GlyphSet::new(glyph_set_dir(), options(None))?;
	let d: Vec<(String, String)> = gset
		.get_reverse_contents()
		.iter()
		.map(|(k, v)| (v.clone(), k.clone()))
		.collect();
	let org: Vec<(String, String)> = gset
		.contents
		.iter()
		.map(|(k, v)| (k.clone(), v.to_lowercase()))
		.collect();
	let mut d = d;
	let mut org = org;
	d.sort();
	org.sort();
	assert_eq!(d, org);
	Ok(())
}

#[test]
fn test_reverse_contents2() -> Result<()> {
	let dst_dir = tempdir("reversecontents2");
	let mut src = GlyphSet::new(glyph_set_dir(), options(None))?;
	let mut dst = GlyphSet::new(dst_dir.path(), options(None))?;
	assert!(dst.get_reverse_contents().is_empty());
	for glyph_name in src.keys() {
		copy_glyph(&src, &mut dst, &glyph_name)?;
	}
	let dst_map = dst.get_reverse_contents().clone();
	assert!(!dst_map.is_empty());
	let mut src_map = src.get_reverse_contents().clone();
	let sorted = |m: &indexmap::IndexMap<String, String>| {
		let mut v: Vec<_> = m.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
		v.sort();
		v
	};
	assert_eq!(sorted(&dst_map), sorted(&src_map));
	src_map.shift_remove("a.glif");
	dst.delete_glyph("a")?;
	assert_eq!(sorted(dst.get_reverse_contents()), sorted(&src_map));
	Ok(())
}

#[test]
fn test_custom_file_naming_scheme() -> Result<()> {
	let dst_dir = tempdir("customnaming");
	let src = GlyphSet::new(glyph_set_dir(), options(None))?;
	let mut dst = GlyphSet::new(
		dst_dir.path(),
		GlyphSetOptions {
			glyph_name_to_file_name: Some(Box::new(
				|glyph_name: &str, existing: &HashSet<String>| {
					Ok(format!(
						"prefix{}",
						glyph_name_to_file_name(glyph_name, Some(existing))?
					))
				},
			)),
			..Default::default()
		},
	)?;
	for glyph_name in src.keys() {
		copy_glyph(&src, &mut dst, &glyph_name)?;
	}
	let d: indexmap::IndexMap<String, String> = src
		.contents
		.iter()
		.map(|(k, v)| (k.clone(), format!("prefix{v}")))
		.collect();
	assert_eq!(d, dst.contents);
	Ok(())
}

#[test]
fn test_get_unicodes() -> Result<()> {
	let src = GlyphSet::new(glyph_set_dir(), options(None))?;
	let unicodes = src.get_unicodes(None)?;
	for glyph_name in src.keys() {
		let g = src.glyph(&glyph_name)?;
		match g.unicodes {
			None => assert!(unicodes[&glyph_name].is_empty()),
			Some(u) => assert_eq!(u, unicodes[&glyph_name]),
		}
	}
	Ok(())
}

#[test]
fn test_read_glyph_invalid_xml() -> Result<()> {
	let dst_dir = tempdir("invalidxml");
	// Create a glyph set with three empty glyphs.
	let mut glyph_set = GlyphSet::new(dst_dir.path(), GlyphSetOptions::default())?;
	for name in ["a", "b", "c"] {
		glyph_set.write_glyph(name, Some(&Dict::new()), None, None, None)?;
	}
	// Corrupt the XML of /c.
	std::fs::write(dst_dir.join(&glyph_set.contents["c"]), b"<abc></def>")?;
	// Confirm that reading /a and /b is fine...
	glyph_set.read_glyph("a", Some(&mut Dict::new()), None, None)?;
	glyph_set.read_glyph("b", Some(&mut Dict::new()), None, None)?;
	// ...but that reading /c raises a descriptive library error.
	match glyph_set.read_glyph("c", Some(&mut Dict::new()), None, None) {
		Err(UfoLibError::GlifLib(m)) => {
			let (first, second) = m.split_once('\n').expect("note");
			assert_eq!(first, "GLIF contains invalid XML.");
			assert!(
				second.starts_with("The issue is in glyph 'c', located in '"),
				"{second}"
			);
			assert!(
				second.contains("c.glif") && second.ends_with("'."),
				"{second}"
			);
		}
		other => panic!("{other:?}"),
	}
	Ok(())
}

// FileNameTest

#[test]
fn test_default_file_name_scheme() -> Result<()> {
	for (a, b) in [
		("a", "a.glif"),
		("A", "A_.glif"),
		("Aring", "A_ring.glif"),
		("F_A_B", "F__A__B_.glif"),
		("A.alt", "A_.alt.glif"),
		("A.Alt", "A_.A_lt.glif"),
		(".notdef", "_notdef.glif"),
		("T_H", "T__H_.glif"),
		("T_h", "T__h.glif"),
		("t_h", "t_h.glif"),
		("F_F_I", "F__F__I_.glif"),
		("f_f_i", "f_f_i.glif"),
		("AE", "A_E_.glif"),
		("Ae", "A_e.glif"),
		("ae", "ae.glif"),
		("aE", "aE_.glif"),
		("a.alt", "a.alt.glif"),
		("A.aLt", "A_.aL_t.glif"),
		("A.alT", "A_.alT_.glif"),
		("Aacute_V.swash", "A_acute_V_.swash.glif"),
		(".notdef", "_notdef.glif"),
		("con", "_con.glif"),
		("CON", "C_O_N_.glif"),
		("con.alt", "_con.alt.glif"),
		("alt.con", "alt._con.glif"),
	] {
		assert_eq!(glyph_name_to_file_name(a, None)?, b, "{a}");
	}
	Ok(())
}

#[test]
fn test_conflicting_case_insensitive_file_names() -> Result<()> {
	let tmp = tempdir("conflicting");
	let src = GlyphSet::new(glyph_set_dir(), GlyphSetOptions::default())?;
	let mut dst = GlyphSet::new(tmp.path(), GlyphSetOptions::default())?;
	// `src["a"]`, with no attributes loaded
	assert!(src.contains("a"));
	let glyph = Dict::new();
	dst.write_glyph("a", Some(&glyph), None, None, None)?;
	dst.write_glyph("A", Some(&glyph), None, None, None)?;
	dst.write_glyph("a_", Some(&glyph), None, None, None)?;
	dst.delete_glyph("a_")?;
	dst.write_glyph("a_", Some(&glyph), None, None, None)?;
	dst.write_glyph("A_", Some(&glyph), None, None, None)?;
	dst.write_glyph("i_j", Some(&glyph), None, None, None)?;
	let expected: Vec<(String, String)> = [
		("a", "a.glif"),
		("A", "A_.glif"),
		("a_", "a_000000000000001.glif"),
		("A_", "A__.glif"),
		("i_j", "i_j.glif"),
	]
	.iter()
	.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
	.collect();
	let mut contents: Vec<(String, String)> = dst
		.contents
		.iter()
		.map(|(a, b)| (a.clone(), b.clone()))
		.collect();
	let mut expected = expected;
	contents.sort();
	expected.sort();
	assert_eq!(contents, expected);
	// make sure filenames are unique even on case-insensitive filesystems
	let lower: HashSet<String> = dst.contents.values().map(|f| f.to_lowercase()).collect();
	assert_eq!(lower.len(), 5);
	Ok(())
}

// ReadWriteFuncTest

#[test]
fn test_roundtrip() -> Result<()> {
	let glyph = Glyph {
		name: Some("a".into()),
		unicodes: Some(vec![0x0061]),
		..Default::default()
	};
	let s1 = write_glyph_to_string("a", Some(&glyph), None, None, true)?;
	let mut glyph2 = Glyph::default();
	read_glyph_from_string(&s1, Some(&mut glyph2), None, None, true)?;
	assert_eq!(glyph, glyph2);
	let s2 = write_glyph_to_string(
		glyph2.name.as_deref().unwrap_or_default(),
		Some(&glyph2),
		None,
		None,
		true,
	)?;
	assert_eq!(s1, s2);
	Ok(())
}

#[test]
fn test_xml_declaration() -> Result<()> {
	let s = write_glyph_to_string("a", Some(&Dict::new()), None, None, true)?;
	assert!(s.starts_with("<?xml version='1.0' encoding='UTF-8'?>"));
	Ok(())
}

#[test]
fn test_parse_xml_remove_comments() -> Result<()> {
	let s = b"<?xml version='1.0' encoding='UTF-8'?>
		<!-- a comment -->
		<glyph name=\"A\" format=\"2\">
			<advance width=\"1290\"/>
			<unicode hex=\"0041\"/>
			<!-- another comment -->
		</glyph>
		";
	let mut g = Glyph::default();
	read_glyph_from_string(s, Some(&mut g), None, None, true)?;
	assert_eq!(g.name.as_deref(), Some("A"));
	assert_eq!(g.width, Some(1290.into()));
	assert_eq!(g.unicodes, Some(vec![0x0041]));
	Ok(())
}

#[test]
fn test_read_invalid_xml() {
	let mut empty_glyph = Glyph::default();
	match read_glyph_from_string(b"<abc></def>", Some(&mut empty_glyph), None, None, true) {
		Err(UfoLibError::GlifLib(m)) => assert!(m.contains("GLIF contains invalid XML")),
		other => panic!("{other:?}"),
	}
}

#[test]
fn test_read_unsupported_format_version() -> Result<()> {
	let s = "<?xml version='1.0' encoding='utf-8'?>
		<glyph name=\"A\" format=\"0\" formatMinor=\"0\">
			<advance width=\"500\"/>
			<unicode hex=\"0041\"/>
		</glyph>
		";
	// validate=True by default
	assert!(matches!(
		read_glyph_from_string(s, Some(&mut Glyph::default()), None, None, true),
		Err(UfoLibError::UnsupportedGlifFormat(_))
	));
	take_logs();
	read_glyph_from_string(s, Some(&mut Glyph::default()), None, None, false)?;
	let logs = take_logs();
	assert_eq!(logs.len(), 1, "{logs:?}");
	assert!(logs[0].contains("Unsupported GLIF format"));
	assert!(logs[0].contains("Assuming the latest supported version"));
	Ok(())
}

#[test]
fn test_read_allow_format_versions() -> Result<()> {
	let s = "<?xml version='1.0' encoding='utf-8'?>
		<glyph name=\"A\" format=\"2\">
			<advance width=\"500\"/>
			<unicode hex=\"0041\"/>
		</glyph>
		";
	let read = |versions: &[(i64, i64)]| {
		let versions: Vec<_> = versions.iter().map(|&v| v.into()).collect();
		read_glyph_from_string(s, Some(&mut Glyph::default()), None, Some(&versions), true)
	};
	// these two calls are are equivalent
	read(&[(1, 0), (2, 0)])?;
	read_glyph_from_string(
		s,
		Some(&mut Glyph::default()),
		None,
		Some(&[1.into(), 2.into()]),
		true,
	)?;
	// if at least one supported formatVersion, unsupported ones are ignored
	read(&[(2, 0), (123, 456)])?;
	match read(&[(0, 0), (2001, 0)]) {
		Err(UfoLibError::Value(m)) => {
			assert!(
				m.contains("None of the requested GLIF formatVersions are supported"),
				"{m}"
			);
		}
		other => panic!("{other:?}"),
	}
	match read(&[(1, 0)]) {
		Err(UfoLibError::GlifLib(m)) => assert!(m.contains("Forbidden GLIF format version"), "{m}"),
		other => panic!("{other:?}"),
	}
	Ok(())
}

#[test]
fn test_read_ensure_x_y() {
	let s = "<?xml version='1.0' encoding='utf-8'?>
		<glyph name=\"A\" format=\"2\">
			<outline>
				<contour>
					<point x=\"545\" y=\"0\" type=\"line\"/>
					<point x=\"638\" type=\"line\"/>
				</contour>
			</outline>
		</glyph>
		";
	let mut pen = RecordingPointPen::default();
	for validate in [true, false] {
		match read_glyph_from_string(
			s,
			Some(&mut Glyph::default()),
			Some(&mut PointPenAdapter(&mut pen)),
			None,
			validate,
		) {
			Err(UfoLibError::GlifLib(m)) => assert!(m.contains("Required y attribute"), "{m}"),
			other => panic!("{other:?}"),
		}
	}
}

#[test]
fn test_glyph_set_unsupported_ufo_format_version() {
	let tmp = tempdir("unsupportedufo");
	for v in [(0, 0), (0, 1)] {
		assert!(matches!(
			GlyphSet::new(
				tmp.path(),
				GlyphSetOptions {
					ufo_format_version: Some(v.into()),
					..Default::default()
				},
			),
			Err(UfoLibError::UnsupportedUfoFormat(_))
		));
	}
}

#[test]
fn test_glyph_set_write_glyph_format_version() -> Result<()> {
	let tmp = tempdir("writeformat");
	let src = GlyphSet::new(glyph_set_dir(), GlyphSetOptions::default())?;
	let mut dst = GlyphSet::new(tmp.path(), options(Some(2)))?;
	// `src["A"]`, with no attributes loaded
	assert!(src.contains("A"));
	let glyph = Dict::new();

	// no explicit formatVersion passed: use the more recent GLIF formatVersion
	// that is supported by given ufoFormatVersion (GLIF 1 for UFO 2)
	dst.write_glyph("A", Some(&glyph), None, None, None)?;
	let glif = String::from_utf8(dst.get_glif("A")?).expect("utf-8");
	assert!(glif.contains("format=\"1\""));
	assert!(!glif.contains("formatMinor")); // omitted when 0

	// explicit, unknown formatVersion
	assert!(matches!(
		dst.write_glyph("A", Some(&glyph), None, Some((0, 0).into()), None),
		Err(UfoLibError::UnsupportedGlifFormat(_))
	));

	// explicit, known formatVersion but unsupported by given ufoFormatVersion
	match dst.write_glyph("A", Some(&glyph), None, Some((2, 0).into()), None) {
		Err(UfoLibError::UnsupportedGlifFormat(m)) => {
			assert!(m.starts_with("Unsupported GLIF format version "), "{m}");
			assert!(m.contains("for UFO format version"), "{m}");
		}
		other => panic!("{other:?}"),
	}
	Ok(())
}

#[test]
fn test_get_glif_modification_time() -> Result<()> {
	let gs = GlyphSet::new(glyph_set_dir(), GlyphSetOptions::default())?;
	assert!(gs.get_glif_modification_time("a")?.is_some());
	Ok(())
}

#[test]
fn test_read_glyph_with_point_pen_and_no_glyph_object() -> Result<()> {
	let gs = GlyphSet::new(glyph_set_dir(), GlyphSetOptions::default())?;
	let mut pen = RecordingPointPen::default();
	gs.read_glyph("A", None, Some(&mut PointPenAdapter(&mut pen)), None)?;
	let ops = format!("{:?}", pen.value);
	assert!(!pen.value.is_empty());
	assert!(ops.contains("BeginPath"), "{ops}");
	assert!(ops.contains("EndPath"), "{ops}");
	Ok(())
}

#[test]
fn test_read_glyph_from_string_with_point_pen_and_no_glyph_object() -> Result<()> {
	let glif_data = b"<?xml version='1.0' encoding='UTF-8'?>
<glyph name=\"test\" format=\"2\">
    <outline>
        <contour>
            <point x=\"0\" y=\"0\" type=\"line\"/>
            <point x=\"100\" y=\"0\" type=\"line\"/>
        </contour>
    </outline>
</glyph>
";
	let mut pen = RecordingPointPen::default();
	read_glyph_from_string(
		glif_data,
		None,
		Some(&mut PointPenAdapter(&mut pen)),
		None,
		true,
	)?;
	let ops = format!("{:?}", pen.value);
	assert!(!pen.value.is_empty());
	assert!(ops.contains("BeginPath"), "{ops}");
	assert!(ops.contains("AddPoint"), "{ops}");
	assert!(ops.contains("EndPath"), "{ops}");
	Ok(())
}

/// Not in fontTools: the bytes and the string writers agree.
#[test]
fn test_write_glyph_to_bytes() -> Result<()> {
	let glyph: Dict = [("width".to_owned(), Value::Integer(100))]
		.into_iter()
		.collect();
	assert_eq!(
		write_glyph_to_bytes("a", Some(&glyph), None, None, true)?,
		write_glyph_to_string("a", Some(&glyph), None, None, true)?.into_bytes()
	);
	Ok(())
}
