//! The tests of beyond-64k fonts of fontTools' `Tests/subset/subset_test.py`, and
//! `test_subset_extended_contextual_layout` of `Tests/ttLib/beyond64k_test.py`.
//!
//! The fonts are made with the same code by `tools/reference/beyond64k_fixtures.py`, since
//! `FontBuilder`, `feaLib` and `otlLib` are not ported yet. fontTools runs some of these on
//! lazily and fully loaded fonts, and on fonts in memory and read back from TTX, which all end
//! up in the same state here. `test_subset_extended_layout` also checks in fontTools that
//! classic tables are not walked recursively, by patching the function that does, which has no
//! counterpart here.

mod common;

use std::path::PathBuf;

use common::*;
use fontoxide_pens::{Pen, Point};
use fontoxide_subset::{Item, Options, Subsetter};
use fontoxide_ttlib::beyond64k::{
	ConvertOptions, convert_layout_formats, lower_tables, upper_tables,
};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::glyf::Glyph;
use fontoxide_ttlib::tables::otl::{Gdef, Gpos, Gsub};
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::tables::tuple_variation::Delta;
use fontoxide_ttlib::tables::{Glyf, Gvar, Hhea, Hmtx, Loca, Maxp, Vhea, Vmtx};
use fontoxide_ttlib::{Font, GlyphSet, Table, TableType, Tag, WriteOptions};

const NOTDEF_DELTAS: [(i32, i32); 12] = [
	(-20, 0),
	(-20, 0),
	(20, 0),
	(20, 0),
	(-10, -5),
	(10, -5),
	(10, 5),
	(-10, 5),
	(0, 0),
	(40, 0),
	(0, 0),
	(0, 0),
];

fn layout_tables<'a>(tags: &'a [&'a str]) -> ConvertOptions<'a> {
	ConvertOptions {
		tables: Some(tags),
		..Default::default()
	}
}

/// `TestTTF-Regular.ttx` with the tables of a fixture.
fn test_ttf_with(name: &str) -> TestResult<Font> {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(tests_path(
		"ttLib/data/TestTTF-Regular.ttx",
	))?)?;
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data")
		.join(format!("{name}.ttx"));
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	Ok(font)
}

/// A font of `tests/data` in WOFF2, read as if from an sfnt file.
fn woff2_fixture(name: &str) -> TestResult<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data")
		.join(format!("{name}.woff2"));
	let mut font = read(std::fs::read(path)?)?;
	font.flavor = None;
	font.flavor_data = None;
	save_and_read(&mut font)
}

fn subset_with(font: &mut Font, options: Options, glyphs: &[&str], text: &str) -> TestResult {
	let mut subsetter = Subsetter::new(options);
	subsetter.populate(glyphs, &[], &[], text);
	subsetter.subset(font)?;
	Ok(())
}

/// The options these tests set.
fn layout_options(retain_gids: bool) -> Options {
	Options {
		retain_gids,
		glyph_names: true,
		layout_features: vec![Item::from("*")],
		..Default::default()
	}
}

fn lookup_list_subtables(
	list: Option<&ot::LookupList>,
	list2: Option<&ot::LookupList2>,
) -> Vec<ot::LookupSubTable> {
	let lookups = match (list, list2) {
		(_, Some(list)) => &list.lookup,
		(Some(list), None) => &list.lookup,
		(None, None) => return Vec::new(),
	};
	lookups
		.iter()
		.flatten()
		.flat_map(|lookup| lookup.sub_table.iter().flatten())
		.map(|subtable| (**subtable).clone())
		.collect()
}

fn gsub_subtables(font: &mut Font) -> TestResult<Vec<ot::LookupSubTable>> {
	let table = &font.require::<Gsub>()?.table;
	Ok(lookup_list_subtables(
		table.lookup_list.as_deref(),
		table.lookup_list2.as_deref(),
	))
}

fn gpos_subtables(font: &mut Font) -> TestResult<Vec<ot::LookupSubTable>> {
	let table = &font.require::<Gpos>()?.table;
	Ok(lookup_list_subtables(
		table.lookup_list.as_deref(),
		table.lookup_list2.as_deref(),
	))
}

/// `assert_contextual_formats`.
fn assert_contextual_formats(font: &mut Font, extended: bool) -> TestResult {
	use ot::LookupSubTable as St;
	let plain: Vec<Option<u16>> = (if extended { 4..7 } else { 1..4 }).map(Some).collect();
	let chain: Vec<Option<u16>> = (if extended { 4..6 } else { 1..3 }).map(Some).collect();
	let gsub = gsub_subtables(font)?;
	let gpos = gpos_subtables(font)?;
	let formats = |subtables: &[St], chaining: bool| -> Vec<Option<u16>> {
		subtables
			.iter()
			.filter_map(|s| match s {
				St::ContextSubst(t) if !chaining => Some(t.format()),
				St::ContextPos(t) if !chaining => Some(t.format()),
				St::ChainContextSubst(t) if chaining => Some(t.format()),
				St::ChainContextPos(t) if chaining => Some(t.format()),
				_ => None,
			})
			.collect()
	};
	assert_eq!(formats(&gsub, false), plain);
	assert_eq!(formats(&gsub, true), chain);
	assert_eq!(formats(&gpos, false), plain);
	assert_eq!(formats(&gpos, true), chain);
	Ok(())
}

/// `_convert_layout_formats` of `GSUB` and `GPOS`.
fn convert_formats(font: &mut Font, extended: bool) -> TestResult {
	convert_layout_formats(&mut font.require::<Gsub>()?.table, extended)?;
	convert_layout_formats(&mut font.require::<Gpos>()?.table, extended)?;
	Ok(())
}

#[test]
fn test_subset_extended_contextual_layout() -> TestResult {
	for extended_header in [false, true] {
		let mut font = test_ttf_with("beyond64k_subset_contextual")?;
		let mut reference = save_and_read(&mut font)?;
		if extended_header {
			upper_tables(&mut font, &layout_tables(&["GSUB", "GPOS"]))?;
		} else {
			convert_formats(&mut font, true)?;
		}
		let mut font = save_and_read(&mut font)?;

		for target in [&mut reference, &mut font] {
			subset_with(
				target,
				layout_options(false),
				&["space", "period", "ellipsis"],
				"",
			)?;
		}
		assert_contextual_formats(&mut font, true)?;
		let mut font = save_and_read(&mut font)?;
		lower_tables(&mut font, &layout_tables(&["GSUB", "GPOS"]))?;
		for tag in ["GSUB", "GPOS"] {
			assert_eq!(
				to_ttx(&mut font, &[tag])?,
				to_ttx(&mut reference, &[tag])?,
				"{extended_header} {tag}"
			);
		}
	}
	Ok(())
}

#[test]
fn test_uppercase_companion_tables() -> TestResult {
	for vertical in [false, true] {
		for retain_gids in [false, true] {
			let mut temp = TempDir::new()?;
			let mut font = fixture(&format!("uppercase_companion_{}", u8::from(vertical)))?;
			upper_tables(&mut font, &ConvertOptions::default())?;
			let source = temp.path(".ttf");
			let output = temp.path(".ttf");
			std::fs::write(&source, font.write(&WriteOptions::default())?)?;

			let output_arg = format!("--output-file={output}");
			let mut args = vec![source.as_str(), "--unicodes=U+0062", output_arg.as_str()];
			if retain_gids {
				args.push("--retain-gids");
			}
			subset_main(&args)?;
			let mut font = open(&output)?;
			let count = if retain_gids { 3 } else { 2 };
			assert_eq!(font.require::<Maxp>()?.num_glyphs, count);
			assert!(font.contains(Tag::new(b"GLYF")) && font.contains(Tag::new(b"LOCA")));
			assert_eq!(
				font.require::<Loca>()?.locations().len(),
				count as usize + 1
			);
			let metrics = if retain_gids { count } else { 1 };
			assert_eq!(font.require::<Hhea>()?.number_of_h_metrics, metrics);
			let b = font.glyph_id("b")?;
			assert_eq!(font.require::<Hmtx>()?.metrics[&b], (500, 0));
			if vertical {
				assert_eq!(font.require::<Vhea>()?.number_of_v_metrics, metrics);
				assert_eq!(font.require::<Vmtx>()?.metrics[&b], (1000, 0));
			}
			for tag in [
				b"maxp", b"glyf", b"loca", b"hhea", b"hmtx", b"vhea", b"vmtx",
			] {
				assert!(!font.contains(Tag::new(tag)));
			}
		}
	}
	Ok(())
}

/// The `extended` case of `test_subset_null_gdef_varstore`.
#[test]
fn test_subset_null_gdef_varstore_extended() -> TestResult {
	for keep_classes in [false, true] {
		let mut font = fixture(&format!("null_gdef_varstore_{}", u8::from(keep_classes)))?;
		font.require::<Gdef>()?.version = 0x0001_0004;
		let mut font = save_and_read(&mut font)?;
		assert!(font.require::<Gdef>()?.var_store.is_none());

		let mut subsetter = Subsetter::new(Options::default());
		subsetter.populate(&[], &[], &[], "a");
		subsetter.subset(&mut font)?;
		let gdef_tag = Tag::new(b"GDEF");
		assert_eq!(font.contains(gdef_tag), keep_classes);
		if keep_classes {
			let a = font.glyph_id("a")?;
			let gdef = font.require::<Gdef>()?;
			let classes = gdef.glyph_class_def2.as_ref().ok_or("no classes")?;
			assert_eq!(classes.class_defs.iter().collect::<Vec<_>>(), [(&a, &1)]);
			assert_eq!(gdef.version, 0x0001_0004);
		}
		assert_eq!(save_and_read(&mut font)?.contains(gdef_tag), keep_classes);
	}
	Ok(())
}

fn delta_pair(delta: &Delta) -> (i32, i32) {
	match *delta {
		Delta::Point(x, y) => (x.value() as i32, y.value() as i32),
		Delta::Value(v) => (v.value() as i32, 0),
	}
}

/// The `beyond64k` case of `test_subset_variable_notdef_table_order`.
#[test]
fn test_subset_variable_notdef_table_order_beyond64k() -> TestResult {
	let mut font = fixture("variable_notdef_beyond64k")?;
	let mut subsetter = Subsetter::new(Options::default());
	subsetter.populate(&[], &[], &[0x41], "");
	subsetter.subset(&mut font)?;
	let mut result = save_and_read(&mut font)?;
	assert!(result.contains(Tag::new(b"GLYF")) && result.contains(Tag::new(b"GVAR")));
	assert_eq!(result.require::<Glyf>()?.glyphs[&0].number_of_contours, 0);
	let gvar = result.require::<Gvar>()?;
	assert_eq!(gvar.get(0), Some(&[][..]));
	let coordinates: Vec<(i32, i32)> = gvar.get(1).ok_or("no A")?[0]
		.coordinates
		.iter()
		.map(|d| d.as_ref().map(delta_pair).ok_or("no delta"))
		.collect::<Result<_, _>>()?;
	assert_eq!(coordinates, NOTDEF_DELTAS);
	Ok(())
}

#[test]
fn test_subset_extended_layout() -> TestResult {
	for extension in [false, true] {
		for extended_header in [false, true] {
			for extended_formats in [false, true] {
				for retain_gids in [false, true] {
					let name = format!("extended_layout_{}", u8::from(extension));
					let case =
						format!("{extension} {extended_header} {extended_formats} {retain_gids}");
					let mut reference = fixture(&name)?;
					let mut font = fixture(&name)?;
					if extended_header {
						upper_tables(&mut font, &layout_tables(&["GDEF", "GSUB", "GPOS"]))?;
					}
					convert_formats(&mut font, extended_formats)?;
					let mut font = save_and_read(&mut font)?;

					for target in [&mut reference, &mut font] {
						subset_with(
							target,
							layout_options(retain_gids),
							&[],
							"abcd\u{301}\u{300}",
						)?;
					}
					let order = glyph_names(&mut font)?;
					assert_eq!(order, glyph_names(&mut reference)?, "{case}");
					assert!(order.iter().any(|n| n == "a.alt"), "{case}");
					assert!(order.iter().any(|n| n == "a_b"), "{case}");
					assert!(!order.iter().any(|n| n == "unused"), "{case}");

					let layout_version = if extended_header {
						0x0001_0002
					} else {
						0x0001_0000
					};
					assert_eq!(font.require::<Gsub>()?.version, layout_version, "{case}");
					assert_eq!(font.require::<Gpos>()?.version, layout_version, "{case}");
					let gdef_version = if extended_header {
						0x0001_0004
					} else {
						0x0001_0002
					};
					assert_eq!(font.require::<Gdef>()?.version, gdef_version, "{case}");
					let mut font = save_and_read(&mut font)?;
					lower_tables(&mut font, &layout_tables(&["GDEF", "GSUB", "GPOS"]))?;
					for tag in ["GDEF", "GSUB", "GPOS"] {
						assert_eq!(
							to_ttx(&mut font, &[tag])?,
							to_ttx(&mut reference, &[tag])?,
							"{case} {tag}"
						);
					}
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_subset_extended_layout_high_glyph_ids() -> TestResult {
	for retain_gids in [false, true] {
		let mut font = woff2_fixture("high_glyph_ids")?;
		let options = Options {
			retain_gids,
			..Default::default()
		};
		subset_with(&mut font, options, &[], "ab")?;
		let mut font = save_and_read(&mut font)?;
		assert_eq!(
			font.glyph_order()?.len(),
			if retain_gids { 0x10003 } else { 4 }
		);

		let gsub = gsub_subtables(&mut font)?;
		let ot::LookupSubTable::LigatureSubst(ligatures) = &gsub[0] else {
			return Err("no LigatureSubst".into());
		};
		let a = if retain_gids { 0x10000 } else { 1 };
		let b = if retain_gids { 0x10001 } else { 2 };
		let a_b = if retain_gids { 0x10002 } else { 3 };
		let ligature = &ligatures.ligatures[&a][0];
		assert_eq!(ligature.lig_glyph, a_b);
		assert_eq!(ligature.component, [b]);
		assert!(font.require::<Gsub>()?.lookup_list2.is_some());

		let gdef = font.require::<Gdef>()?;
		let classes = gdef.glyph_class_def2.as_ref().ok_or("no GlyphClassDef2")?;
		assert_eq!(classes.class_defs[&a_b], 2);

		assert!(font.require::<Gpos>()?.lookup_list2.is_some());
		let gpos = gpos_subtables(&mut font)?;
		let check = |second_glyph: u32, value1: &Option<ValueRecord>| -> TestResult {
			assert_eq!(second_glyph, b);
			let value1 = value1.as_ref().ok_or("no Value1")?;
			assert_eq!(value1.x_advance, Some(-30));
			Ok(())
		};
		match &gpos[0] {
			ot::LookupSubTable::PairPos(ot::PairPos::Format1(pair)) => {
				let record = &pair.pair_set[0]
					.as_ref()
					.ok_or("no PairSet")?
					.pair_value_record[0];
				check(record.second_glyph, &record.value1)?;
			}
			ot::LookupSubTable::PairPos(ot::PairPos::Format3(pair)) => {
				let record = &pair.pair_set[0]
					.as_ref()
					.ok_or("no PairSet")?
					.pair_value_record[0];
				check(record.second_glyph, &record.value1)?;
			}
			_ => return Err("no PairPos".into()),
		}
	}
	Ok(())
}

#[test]
fn test_subset_remaps_24bit_glyf_component() -> TestResult {
	const COMPOSITE: &[u8] = b"\xff\xff\
		\0\0\0\0\0\0\0\0\
		\x21\x02\
		\x01\x00\x00\
		\x0a\x14\
		\0\x03\xb0\x01\x21";
	for hinting in [false, true] {
		let mut glyf = Glyf::new_as(Tag::new(b"GLYF"));
		for gid in [0, 0x10000] {
			glyf.glyphs.insert(
				gid,
				Glyph {
					data: Some(Vec::new()),
					..Default::default()
				},
			);
		}
		let mut composite = Glyph::decompile_ext(COMPOSITE, 0x10002, true)?;
		composite.data = Some(COMPOSITE.to_vec());
		glyf.glyphs.insert(0x10001, composite);

		// `glyf.subset_glyphs`: here, the glyphs are remapped with the glyph order.
		let mut remap = |gid: u32| {
			Ok(match gid {
				0 => 0,
				0x10000 => 1,
				0x10001 => 2,
				_ => return Err(fontoxide_ttlib::Error::GlyphNotFound(gid.to_string())),
			})
		};
		glyf.remap_glyphs(&mut remap)?;
		// `glyf.prune_post_subset`.
		for glyph in glyf.glyphs.values_mut() {
			glyph.trim_ext(!hinting, true)?;
		}

		assert_eq!(glyf.glyphs.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
		let data = glyf.glyphs[&2].data.clone().ok_or("no data")?;
		assert_eq!(data[12..15], [0, 0, 1]);
		let composite = Glyph::decompile_ext(&data, 3, true)?;
		assert_eq!(composite.components[0].glyph, 1);
		let (_, transform) = Glyph::component_info(&composite.components[0])?;
		assert_eq!((transform.dx, transform.dy), (10.0, 20.0));
		assert_eq!(composite.program.is_some(), hinting);
		if hinting {
			assert_eq!(
				composite.program,
				Some(Program::from_bytecode(b"\xb0\x01\x21"))
			);
		}
	}
	Ok(())
}

type Outline = Vec<(&'static str, Vec<Point>)>;

/// `DecomposingRecordingPen`.
#[derive(Default)]
struct Recording(Outline);

impl fontoxide_pens::SegmentPen for Recording {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("moveTo", vec![pt]));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("lineTo", vec![pt]));
		Ok(())
	}
	fn curve_to_one(
		&mut self,
		_current: Option<Point>,
		pt1: Point,
		pt2: Point,
		pt3: Point,
	) -> fontoxide_pens::Result<()> {
		self.0.push(("curveTo", vec![pt1, pt2, pt3]));
		Ok(())
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("closePath", Vec::new()));
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("endPath", Vec::new()));
		Ok(())
	}
}

fn outline(font: &mut Font, glyph: &str) -> TestResult<Outline> {
	let glyph_set = GlyphSet::new(font)?;
	let mut pen = fontoxide_pens::BasePen::with_inner(Recording::default(), Some(&glyph_set));
	glyph_set.draw(glyph, &mut pen as &mut dyn Pen)?;
	Ok(pen.inner.0)
}

/// The `extended` case of `test_subset_composite_closure`.
#[test]
fn test_subset_composite_closure_extended() -> TestResult {
	for retain_gids in [false, true] {
		for nested in [false, true] {
			for varc in [false, true] {
				let name = format!("composite_closure_{}_{}", u8::from(nested), u8::from(varc));
				let mut font = fixture(&name)?;
				upper_tables(&mut font, &ConvertOptions::default())?;
				let mut font = save_and_read(&mut font)?;
				let expected = outline(&mut font, "composite")?;
				assert!(!expected.is_empty());
				let options = Options {
					glyph_names: true,
					retain_gids,
					..Default::default()
				};
				let mut subsetter = Subsetter::new(options);
				subsetter.populate(&[], &[], &[65], "");
				subsetter.subset(&mut font)?;
				let order = glyph_names(&mut font)?;
				assert!(order.iter().any(|n| n == "leaf"), "{name}");
				if nested {
					assert!(order.iter().any(|n| n == "unused"), "{name}");
				}
				let mut font = save_and_read(&mut font)?;
				assert_eq!(outline(&mut font, "composite")?, expected, "{name}");
			}
		}
	}
	Ok(())
}
