//! Not in fontTools: the test fonts of `Tests/ufoLib/testdata` converted
//! between UFO versions, compared file by file with what fontTools writes
//! (`tools/reference/ufolib_fixtures.py`).

mod common;

use std::path::{Path, PathBuf};

use common::{tempdir, testdata};
use fontoxide_ufolib::{
	GlifPointPen, Result, UfoReader, UfoWriter, UfoWriterOptions, WriterGlyphSetOptions,
};

/// `convert_font` of the fixture script.
fn convert_font(version: i64, target_version: i64, dest: &Path) -> Result<()> {
	let reader = UfoReader::new(
		testdata().join(format!("TestFont1 (UFO{version}).ufo")),
		true,
	)?;
	let mut writer = UfoWriter::new(
		dest,
		UfoWriterOptions {
			format_version: Some(target_version.into()),
			..Default::default()
		},
	)?;
	writer.set_kerning_group_conversion_rename_maps(
		&reader.get_kerning_group_conversion_rename_maps(None)?,
	);
	writer.write_info(&reader.read_info(None)?, None)?;
	writer.write_groups(&reader.read_groups(None)?, None)?;
	writer.write_kerning(&reader.read_kerning(None)?, None)?;
	writer.write_lib(&reader.read_lib(None)?, None)?;
	if target_version >= 2 {
		writer.write_features(&reader.read_features()?, None)?;
	}
	let src = reader.get_glyph_set(None, None, None)?;
	let mut dst = writer.get_glyph_set(None, true, WriterGlyphSetOptions::default())?;
	for glyph_name in src.keys() {
		let glyph = src.glyph(&glyph_name)?;
		dst.write_glyph(
			&glyph_name,
			Some(&glyph),
			Some(&mut |pen: &mut GlifPointPen| src.read_glyph(&glyph_name, None, Some(pen), None)),
			None,
			None,
		)?;
	}
	dst.write_contents()?;
	writer.write_layer_contents(None, None)?;
	writer.close()
}

fn files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
	let mut entries: Vec<_> = std::fs::read_dir(dir)
		.expect("read_dir")
		.map(|e| e.expect("entry"))
		.collect();
	entries.sort_by_key(|e| e.file_name());
	for entry in entries {
		let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
		if entry.path().is_dir() {
			files(&entry.path(), &format!("{name}/"), out);
		} else {
			out.push(name);
		}
	}
}

#[test]
fn test_convert_matches_fonttools() -> Result<()> {
	let expected_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/convert");
	let tmp = tempdir("convert");
	for version in 1..=3 {
		for target_version in version..=3 {
			let name = format!("UFO{version} to {target_version}.ufo");
			let dest = tmp.join(&name);
			convert_font(version, target_version, &dest)?;
			let expected = expected_root.join(&name);
			let mut expected_files = Vec::new();
			files(&expected, "", &mut expected_files);
			let mut actual_files = Vec::new();
			files(&dest, "", &mut actual_files);
			assert_eq!(actual_files, expected_files, "{name}");
			assert!(!actual_files.is_empty());
			for file in actual_files {
				let a = std::fs::read(dest.join(&file))?;
				let e = std::fs::read(expected.join(&file))?;
				assert!(
					a == e,
					"{name}/{file} differs:\n{}\n---\n{}",
					String::from_utf8_lossy(&a),
					String::from_utf8_lossy(&e)
				);
			}
		}
	}
	Ok(())
}
