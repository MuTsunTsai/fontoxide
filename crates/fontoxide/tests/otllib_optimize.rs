//! The tests of `Tests/otlLib/optimize_test.py` that need `FontBuilder`.

#![cfg(all(feature = "fealib", feature = "otllib", feature = "repacker"))]

use fontoxide::font_builder::FontBuilder;
use fontoxide::misc::Tag;
use fontoxide::otllib::optimize;
use fontoxide::ttlib::beyond64k::{self, ConvertOptions};
use fontoxide::ttlib::ot::types::{ExtensionPos, LookupSubTable, PairPos};
use fontoxide::ttlib::tables::Gpos;
use fontoxide::ttlib::{Font, ReadOptions, TableKey, TtxOptions, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
	fn new(name: &str) -> std::io::Result<Self> {
		let path =
			std::env::temp_dir().join(format!("fontoxide-optimize-{}-{name}", std::process::id()));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// `test_no_crash_with_missing_gpos`: the command line handles fonts without
/// GPOS.
#[test]
fn test_no_crash_with_missing_gpos() -> TestResult {
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(
		".notdef space A Aacute B D"
			.split(' ')
			.map(str::to_owned)
			.collect(),
	);
	assert!(!fb.font.contains(Tag::new(b"GPOS")));
	let dir = TempDir::new("missing-gpos")?;
	let input = dir.0.join("in.ttf");
	fb.save_to(&input)?;
	let output = dir.0.join("out.ttf");
	let (input, output) = (
		input.to_string_lossy().into_owned(),
		output.to_string_lossy().into_owned(),
	);
	let ret = optimize::main(&["--gpos-compression-level", "5", &input, "-o", &output])?;
	assert_eq!(ret, 0);
	assert!(std::path::Path::new(&output).exists());
	Ok(())
}

/// `test_main`: the command line compacts a font built from features.
#[test]
fn test_main() -> TestResult {
	let glyphs: Vec<String> = ".notdef space A Aacute B D"
		.split(' ')
		.map(str::to_owned)
		.collect();
	let features = "
    @A = [A Aacute];
    @B = [B D];
    feature kern {
        pos @A @B -50;
    } kern;
    ";
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(glyphs);
	fb.add_open_type_features(features, None, None, false)?;
	let dir = TempDir::new("main")?;
	let input = dir.0.join("in.ttf");
	fb.save_to(&input)?;
	let output = dir.0.join("out.ttf");
	let (input, output) = (
		input.to_string_lossy().into_owned(),
		output.to_string_lossy().into_owned(),
	);
	let ret = optimize::main(&["--gpos-compression-level", "5", &input, "-o", &output])?;
	assert_eq!(ret, 0);
	assert!(std::path::Path::new(&output).exists());
	Ok(())
}

/// Saves a font and reads it back (`TTFont(BytesIO(saved))`).
fn reload(font: &mut Font) -> Result<Font, Box<dyn std::error::Error>> {
	let data = font.write(&WriteOptions::default())?;
	Ok(Font::read(data, ReadOptions::default())?)
}

/// The TTX of `GPOS` (`getXML(font["GPOS"].toXML, font)`).
fn gpos_xml(font: &mut Font) -> Result<String, Box<dyn std::error::Error>> {
	Ok(font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"GPOS"))],
		..Default::default()
	})?)
}

/// `test_compact_extended_layout`: compacting works on extended table headers
/// and subtable formats, with and without extension lookups.
///
/// fontTools also runs each case with `lazy` loading, which fontoxide does not
/// have; those cases are the same here.
#[test]
fn test_compact_extended_layout() -> TestResult {
	for extended_header in [false, true] {
		for extended_formats in [false, true] {
			for extension in [false, true] {
				compact_extended_layout(extended_header, extended_formats, extension)?;
			}
		}
	}
	Ok(())
}

fn compact_extended_layout(
	extended_header: bool,
	extended_formats: bool,
	extension: bool,
) -> TestResult {
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(
		[".notdef", "a", "b", "c", "d"]
			.iter()
			.map(|s| (*s).to_owned())
			.collect(),
	);
	let use_extension = if extension { "useExtension" } else { "" };
	fb.add_open_type_features(
		&format!(
			"
        lookup PAIRS {use_extension} {{
            pos a b -30;
            pos [a c] [b d] -15;
        }} PAIRS;
        feature kern {{ lookup PAIRS; }} kern;
        "
		),
		None,
		None,
		false,
	)?;
	let original = fb.font.write(&WriteOptions::default())?;
	let mut expected = Font::read(original.clone(), ReadOptions::default())?;
	optimize::gpos::compact(&mut expected, 5)?;
	let mut expected = reload(&mut expected)?;

	let mut font = Font::read(original, ReadOptions::default())?;
	if extended_header {
		beyond64k::upper_tables(
			&mut font,
			&ConvertOptions {
				tables: Some(&["GPOS"]),
				..Default::default()
			},
		)?;
	}
	beyond64k::convert_layout_formats(&mut font.require::<Gpos>()?.table, extended_formats)?;
	let mut font = reload(&mut font)?;
	optimize::gpos::compact(&mut font, 5)?;
	let gpos = font.require::<Gpos>()?;
	let lookup = match (&gpos.lookup_list, &gpos.lookup_list2) {
		(_, Some(l)) if gpos.version >= 0x0001_0002 => &l.lookup[0],
		(Some(l), _) => &l.lookup[0],
		(None, Some(l)) => &l.lookup[0],
		(None, None) => return Err("no lookup list".into()),
	};
	let lookup = lookup.as_deref().ok_or("no lookup")?;
	let formats: Vec<u16> = lookup
		.sub_table
		.iter()
		.map(|st| {
			let st = st.as_deref().ok_or("no subtable")?;
			let st = match st {
				LookupSubTable::ExtensionPos(ExtensionPos::Format1(e)) if extension => {
					e.ext_sub_table.as_deref().ok_or("no ExtSubTable")?
				}
				st => st,
			};
			match st {
				LookupSubTable::PairPos(PairPos::Format1(_)) => Ok(1),
				LookupSubTable::PairPos(PairPos::Format2(_)) => Ok(2),
				LookupSubTable::PairPos(PairPos::Format3(_)) => Ok(3),
				LookupSubTable::PairPos(PairPos::Format4(_)) => Ok(4),
				other => Err(format!("unexpected subtable {other:?}")),
			}
		})
		.collect::<Result<_, String>>()?;
	assert_eq!(
		formats,
		if extended_formats {
			vec![3, 4]
		} else {
			vec![1, 2]
		},
		"{extended_header} {extended_formats} {extension}"
	);
	assert_eq!(
		gpos.version,
		if extended_header {
			0x0001_0002
		} else {
			0x0001_0000
		}
	);
	let mut font = reload(&mut font)?;
	beyond64k::lower_tables(
		&mut font,
		&ConvertOptions {
			tables: Some(&["GPOS"]),
			..Default::default()
		},
	)?;
	assert_eq!(gpos_xml(&mut font)?, gpos_xml(&mut expected)?);
	Ok(())
}
