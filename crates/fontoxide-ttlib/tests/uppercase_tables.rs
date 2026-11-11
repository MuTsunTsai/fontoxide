//! The tests of the uppercase companion tables: fontTools' `Tests/ttLib/tables/G_L_Y_F__test.py`,
//! `H_M_T_X__test.py` and `M_A_X_P__test.py`, and those of `Tests/ttLib/ttFont_test.py` about
//! them.

use fontoxide_ttlib::ot::custom::has_extended_glyph_ids;
use fontoxide_ttlib::tables::glyf::{
	ComponentPlacement, FLAG_CUBIC, Glyph, GlyphComponent, Number,
};
use fontoxide_ttlib::tables::{Glyf, Gvar, Hhea, Hmtx, Loca, Maxp, Post, Vhea, Vmtx};
use fontoxide_ttlib::{Font, GlyphSet, Table, TableType, Tag};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const GID_IS_24_BIT: u16 = 0x2000;

const GLYF: Tag = Tag::new(b"GLYF");
const GLYF_LOWER: Tag = Tag::new(b"glyf");
const LOCA: Tag = Tag::new(b"LOCA");
const MAXP: Tag = Tag::new(b"MAXP");
const HHEA: Tag = Tag::new(b"HHEA");
const VHEA: Tag = Tag::new(b"VHEA");
const HMTX: Tag = Tag::new(b"HMTX");
const VMTX: Tag = Tag::new(b"VMTX");
const GVAR: Tag = Tag::new(b"GVAR");

fn names(names: &[&str]) -> Vec<String> {
	names.iter().map(|n| (*n).to_owned()).collect()
}

/// `TTFont(recalcBBoxes=False)`.
fn font_without_recalc() -> Font {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font
}

/// `cubic_glyph`.
fn cubic_glyph() -> Glyph {
	Glyph {
		number_of_contours: 1,
		coordinates: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
		end_pts_of_contours: vec![3],
		flags: vec![FLAG_CUBIC; 4],
		program: Some(Default::default()),
		..Default::default()
	}
}

/// `glyph.compile(glyfTable)` of a table with `glyph` as its only glyph.
fn compile_glyph(glyph: Glyph, extended: bool) -> fontoxide_ttlib::Result<Vec<u8>> {
	let mut font = font_without_recalc();
	font.set_glyph_order(names(&["a"]));
	let mut glyf = Glyf::new_as(if extended { GLYF } else { GLYF_LOWER });
	glyf.glyphs.insert(0, glyph);
	glyf.compile(&mut font)
}

#[test]
fn test_cubic_flag_is_uppercase_only() -> TestResult {
	let data = compile_glyph(cubic_glyph(), true)?;

	let upper_glyph = Glyph::decompile_ext(&data, 1, true)?;
	assert!(upper_glyph.flags.iter().all(|f| f & FLAG_CUBIC != 0));

	let lower_glyph = Glyph::decompile_ext(&data, 1, false)?;
	assert!(!lower_glyph.flags.iter().any(|f| f & FLAG_CUBIC != 0));

	let error = compile_glyph(cubic_glyph(), false).expect_err("a cubic glyph in glyf");
	assert!(error.to_string().contains("extended GLYF"), "{error}");
	Ok(())
}

#[test]
fn test_component_24bit_glyph_id() -> TestResult {
	let component = GlyphComponent {
		glyph: 0x10000,
		flags: 0,
		placement: ComponentPlacement::Offset {
			x: Number::Int(0),
			y: Number::Int(0),
		},
		transform: None,
	};

	let data = component.compile_ext(false, false, 0x10001, true)?;
	let flags = u16::from_be_bytes([data[0], data[1]]);
	assert_eq!(flags, ARGS_ARE_XY_VALUES | GID_IS_24_BIT);
	assert_eq!(data[2..5], [1, 0, 0]);

	let (decompiled, more, have_instructions, used) =
		GlyphComponent::decompile_ext(&data, 0x10001, true)?;
	assert!(!more);
	assert!(!have_instructions);
	assert_eq!(used, data.len());
	assert_eq!(decompiled.glyph, 0x10000);
	Ok(())
}

#[test]
fn test_compile_updates_locked_uppercase_tables() -> TestResult {
	let mut font = font_without_recalc();
	font.set_glyph_order(names(&[".notdef", "a"]));
	let mut glyf = Glyf::new_as(GLYF);
	glyf.glyphs.insert(0, Glyph::default());
	glyf.glyphs.insert(1, Glyph::default());
	font.insert(Box::new(glyf));
	font.insert(Box::new(Loca::new_as(LOCA)));
	let mut maxp = Maxp::new_as(MAXP);
	maxp.table_version = 0x0000_5000;
	font.insert(Box::new(maxp));

	font.table_data(GLYF)?;
	assert_eq!(font.require::<Loca>()?.locations().len(), 3);
	assert_eq!(font.require::<Maxp>()?.num_glyphs, 2);
	Ok(())
}

/// `makeHeader` with every field zero but the version and the count.
fn make_hhea(count: u32) -> Hhea {
	let mut table = Hhea::new_as(HHEA);
	table.table_version = 0x0001_0000;
	table.number_of_h_metrics = count;
	table
}

#[test]
fn test_extended_header_metric_count() -> TestResult {
	let mut font = font_without_recalc();

	let mut hhea = make_hhea(0x10000);
	let data = hhea.compile(&mut font)?;
	assert_eq!(data.len(), 38);
	assert_eq!(data[34..], [0, 1, 0, 0]);
	let decompiled = Hhea::decompile_as(HHEA, &data, &mut font)?;
	assert_eq!(decompiled.number_of_h_metrics, 0x10000);

	let mut vhea = Vhea::new_as(VHEA);
	vhea.table_version = 0x0001_0000;
	vhea.number_of_v_metrics = 0x10000;
	let data = vhea.compile(&mut font)?;
	assert_eq!(data.len(), 38);
	assert_eq!(data[34..], [0, 1, 0, 0]);
	let decompiled = Vhea::decompile_as(VHEA, &data, &mut font)?;
	assert_eq!(decompiled.number_of_v_metrics, 0x10000);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_HMTX_uses_locked_uppercase_tables() -> TestResult {
	let mut font = font_without_recalc();
	font.set_glyph_order(names(&[".notdef", "a", "b"]));
	let mut maxp = Maxp::new_as(MAXP);
	maxp.num_glyphs = 3;
	font.insert(Box::new(maxp));
	font.insert(Box::new(make_hhea(2)));

	let hmtx = Hmtx::decompile_as(HMTX, b"\x01\xf4\x00\x0a\x02\x58\x00\x14\x00\x1e", &mut font)?;
	assert_eq!(
		hmtx.metrics.into_iter().collect::<Vec<_>>(),
		[(0, (500, 10)), (1, (600, 20)), (2, (600, 30))]
	);

	let mut hmtx =
		Hmtx::decompile_as(HMTX, b"\x01\xf4\x00\x0a\x02\x58\x00\x14\x00\x1e", &mut font)?;
	assert!(hmtx.extended);
	hmtx.compile(&mut font)?;
	assert_eq!(font.require::<Hhea>()?.number_of_h_metrics, 2);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_VMTX_uses_locked_uppercase_tables() {
	let table = Vmtx::new_as(VMTX);
	assert_eq!(Font::locked_tag::<Vhea>(table.extended), VHEA);
	assert_eq!(Font::locked_tag::<Maxp>(table.extended), MAXP);
}

/// `Tests/ttLib/tables/M_A_X_P__test.py::test_compile_decompile`.
#[test]
fn test_compile_decompile() -> TestResult {
	let mut font = font_without_recalc();
	font.set_glyph_order(names(&[".notdef", "a", "b"]));
	let mut table = Maxp::new_as(MAXP);
	table.table_version = 0x0000_5000;

	let data = table.compile(&mut font)?;
	assert_eq!(data, b"\x00\x00\x50\x00\x00\x00\x03");

	let decompiled = Maxp::decompile_as(MAXP, &data, &mut font)?;
	assert_eq!(decompiled.table_version, 0x0000_5000);
	assert_eq!(decompiled.num_glyphs, 3);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_getGlyphOrder_prefers_MAXP() -> TestResult {
	let mut font = Font::new();
	font.insert(Box::new(Maxp {
		num_glyphs: 1,
		..Default::default()
	}));
	let mut maxp = Maxp::new_as(MAXP);
	maxp.num_glyphs = 2;
	font.insert(Box::new(maxp));

	assert_eq!(font.glyph_order()?.names(), [".notdef", "glyph00001"]);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_getGlyphOrder_from_post_uses_MAXP() -> TestResult {
	let mut font = Font::new();
	let mut maxp = Maxp::new_as(MAXP);
	maxp.num_glyphs = 2;
	font.insert(Box::new(maxp));
	// A header of format 1.0, which `decode_format_1_0` reads no more of.
	let mut data = vec![0u8; 32];
	data[1] = 1;
	let post = Post::decompile(&data, &mut font)?;
	font.insert(Box::new(post));

	assert_eq!(font.glyph_order()?.names(), [".notdef", ".null"]);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_getGlyphSet_prefers_locked_uppercase_tables() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef"]));
	let mut glyf = Glyf::new_as(GLYF);
	glyf.glyphs.insert(0, Glyph::default());
	font.insert(Box::new(glyf));
	font.insert(Box::new(Glyf::new_as(GLYF_LOWER)));
	let mut hmtx = Hmtx::new_as(HMTX);
	hmtx.set(0, (500, 0));
	font.insert(Box::new(hmtx));
	font.insert(Box::new(Gvar::new_as(GVAR)));

	let glyph_set = GlyphSet::new(&mut font)?;

	let glyf = glyph_set.glyf_table().ok_or("no glyf table")?;
	assert!(glyf.extended);
	assert_eq!(glyf.glyphs.len(), 1);
	assert_eq!(glyph_set.h_metrics()[&0], (500, 0));
	assert!(glyph_set.gvar_table().ok_or("no gvar table")?.extended);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_hasExtendedGlyphIDs() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(vec!["glyph".to_owned(); 0x10000]);
	assert!(!has_extended_glyph_ids(&mut font)?);
	font.set_glyph_order(vec!["glyph".to_owned(); 0x10001]);
	assert!(has_extended_glyph_ids(&mut font)?);
	Ok(())
}

/// `Tests/ttLib/tables/G_L_Y_F__test.py::test_split_glyphs_use_uppercase_table_tag`: the glyph
/// files of a split `GLYF` are wrapped in `<GLYF>`. (fontTools' test stubs the glyph out; this
/// one has a real outline.)
#[test]
fn test_split_glyphs_use_uppercase_table_tag() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&["a"]));
	let mut glyf = Glyf::new_as(GLYF);
	glyf.glyphs.insert(
		0,
		Glyph {
			number_of_contours: 1,
			coordinates: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
			end_pts_of_contours: vec![2],
			flags: vec![1; 3],
			..Default::default()
		},
	);
	let path = std::path::Path::new("tmp").join("GLYF.ttx");
	let path = path.to_string_lossy();

	let mut writer = fontoxide_misc::xml_writer::XmlWriter::new();
	let files = glyf.to_xml_split(&mut writer, &mut font, &path)?;

	let [glyph_file] = files.as_slice() else {
		panic!("one glyph file: {files:?}");
	};
	assert_ne!(glyph_file.path, path);
	assert!(
		glyph_file.contents.contains("<GLYF>"),
		"{}",
		glyph_file.contents
	);
	Ok(())
}

/// `Tests/ttLib/ttFont_test.py::test_saveXML_splitGlyphs_for_GLYF`: `GLYF` is dumped with its
/// glyphs split too. (fontTools' test checks that `toXML` gets `splitGlyphs=True`; this one checks
/// the files.)
#[test]
#[allow(non_snake_case)]
fn test_saveXML_splitGlyphs_for_GLYF() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&["a"]));
	let mut glyf = Glyf::new_as(GLYF);
	glyf.glyphs.insert(
		0,
		Glyph {
			number_of_contours: 1,
			coordinates: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
			end_pts_of_contours: vec![2],
			flags: vec![1; 3],
			..Default::default()
		},
	);
	font.insert(Box::new(glyf));

	let files = font.to_ttx_files(
		"font.ttx",
		&fontoxide_ttlib::TtxOptions {
			tables: vec![fontoxide_ttlib::TableKey::Table(GLYF)],
			split_glyphs: true,
			..Default::default()
		},
	)?;
	let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
	assert_eq!(
		paths,
		["font.ttx", "font.G_L_Y_F_.a.ttx", "font.G_L_Y_F_.ttx"]
	);
	assert!(
		files[0]
			.contents
			.contains("<GLYF src=\"font.G_L_Y_F_.ttx\"/>")
	);
	assert!(
		files[2]
			.contents
			.contains("<TTGlyph src=\"font.G_L_Y_F_.a.ttx\"/>")
	);
	Ok(())
}
