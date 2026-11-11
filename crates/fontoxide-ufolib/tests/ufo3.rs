//! Port of fontTools' `Tests/ufoLib/UFO3_test.py`.
//!
//! The font info read and write tests are tables of the attribute values
//! that fontTools' tests set, as Python literals, and whether reading or
//! writing must fail.

mod common;

use std::path::{Path, PathBuf};

use common::{
	TempDir, assert_glif_error, assert_ufo_error, check_read, check_write, font_info_version3,
	py_dict, py_value, read_plist, tempdir, testdata, write_plist,
};
use fontoxide_ufolib::{
	Dict, Result, UfoReader, UfoWriter, UfoWriterOptions, Value, WriterGlyphSetOptions,
};

fn writer(path: &Path) -> Result<UfoWriter> {
	UfoWriter::new(path, UfoWriterOptions::default())
}

fn writer_version(path: &Path, version: i64) -> Result<UfoWriter> {
	UfoWriter::new(
		path,
		UfoWriterOptions {
			format_version: Some(version.into()),
			..Default::default()
		},
	)
}

// --------------
// fontinfo.plist
// --------------

// ReadFontInfoVersion3TestCase

#[test]
fn test_read() -> Result<()> {
	let dir = tempdir("ufo3read");
	write_plist(
		&dir.join("metainfo.plist"),
		&py_value("{'creator': 'test', 'formatVersion': 3}"),
	);
	let original = font_info_version3();
	write_plist(&dir.join("fontinfo.plist"), &Value::Dict(original.clone()));
	let reader = UfoReader::new(dir.path(), true)?;
	let info = reader.read_info(None)?;
	let read: Dict = original
		.keys()
		.map(|attr| (attr.clone(), info.get(attr).cloned().expect(attr)))
		.collect();
	assert_eq!(original, read);
	Ok(())
}

// WriteFontInfoVersion3TestCase

#[test]
fn test_write() -> Result<()> {
	let dir = tempdir("ufo3write");
	let ufo = dir.join("test.ufo");
	let writer = writer_version(&ufo, 3)?;
	writer.write_info(&font_info_version3(), None)?;
	let written = read_plist(&ufo.join("fontinfo.plist"));
	let written = written.as_dict().expect("dict");
	for (attr, original) in font_info_version3() {
		assert_eq!(written.get(&attr), Some(&original), "{attr}");
	}
	Ok(())
}

// ------
// layers
// ------

/// `makeUFO` of the layer tests: `layer_contents` is a Python literal;
/// `None` gives the default three layers and an empty literal none.
fn make_ufo(ufo_path: &Path, meta_info: Option<&str>, layer_contents: Option<&str>) {
	clear_ufo(ufo_path);
	std::fs::create_dir_all(ufo_path).expect("mkdir");
	let meta = meta_info.unwrap_or("dict(creator='test', formatVersion=3)");
	write_plist(&ufo_path.join("metainfo.plist"), &py_value(meta));
	let contents = py_value(layer_contents.unwrap_or(
		"[('public.default', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
	));
	let empty = match &contents {
		Value::Array(a) => a.is_empty(),
		Value::Dict(d) => d.is_empty(),
		_ => false,
	};
	let contents = if empty {
		py_value("[('', 'glyphs')]")
	} else {
		write_plist(&ufo_path.join("layercontents.plist"), &contents);
		contents
	};
	let Value::Array(entries) = contents else {
		panic!("layer contents");
	};
	for entry in entries {
		let Value::Array(pair) = entry else {
			panic!("entry");
		};
		let Value::String(directory) = &pair[1] else {
			panic!("directory");
		};
		let glyphs_path = ufo_path.join(directory);
		std::fs::create_dir(&glyphs_path).expect("mkdir");
		write_plist(
			&glyphs_path.join("contents.plist"),
			&py_value("dict(a='a.glif')"),
		);
		std::fs::write(glyphs_path.join("a.glif"), " ").expect("write");
	}
}

fn clear_ufo(ufo_path: &Path) {
	if ufo_path.exists() {
		std::fs::remove_dir_all(ufo_path).expect("rmtree");
	}
}

/// Replaces `layercontents.plist`.
fn set_layer_contents(ufo_path: &Path, contents: &str) {
	let path = ufo_path.join("layercontents.plist");
	std::fs::remove_file(&path).expect("remove");
	write_plist(&path, &py_value(contents));
}

struct LayerTest {
	_dir: TempDir,
	ufo_path: PathBuf,
}

fn layer_test() -> LayerTest {
	let dir = tempdir("layers");
	let ufo_path = dir.join("test.ufo");
	LayerTest {
		_dir: dir,
		ufo_path,
	}
}

// UFO3ReadLayersTestCase

mod read_layers {
	use super::*;

	#[test]
	fn test_valid_read() -> Result<()> {
		let t = layer_test();
		// UFO 1
		make_ufo(
			&t.ufo_path,
			Some("dict(creator='test', formatVersion=1)"),
			Some("dict()"),
		);
		UfoReader::new(&t.ufo_path, true)?.get_glyph_set(None, None, None)?;
		// UFO 2
		make_ufo(
			&t.ufo_path,
			Some("dict(creator='test', formatVersion=2)"),
			Some("dict()"),
		);
		UfoReader::new(&t.ufo_path, true)?.get_glyph_set(None, None, None)?;
		// UFO 3
		make_ufo(&t.ufo_path, None, None);
		UfoReader::new(&t.ufo_path, true)?.get_glyph_set(None, None, None)?;
		Ok(())
	}

	#[test]
	fn test_missing_layer_contents() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		std::fs::remove_file(t.ufo_path.join("layercontents.plist"))?;
		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_ufo_error(reader.get_glyph_set(None, None, None));
		Ok(())
	}

	#[test]
	fn test_invalid_layer_contents_format() -> Result<()> {
		let t = layer_test();
		// bogus
		make_ufo(&t.ufo_path, None, None);
		let path = t.ufo_path.join("layercontents.plist");
		std::fs::remove_file(&path)?;
		std::fs::write(&path, "test")?;
		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_ufo_error(reader.get_glyph_set(None, None, None));
		// dict
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(
			&t.ufo_path,
			"{'public.default': 'glyphs', 'layer 1': 'glyphs.layer 1', 'layer 2': 'glyphs.layer 2'}",
		);
		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_ufo_error(reader.get_glyph_set(None, None, None));
		Ok(())
	}

	fn read_fails(contents: &str) -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(&t.ufo_path, contents);
		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_ufo_error(reader.get_glyph_set(None, None, None));
		Ok(())
	}

	#[test]
	fn test_invalid_layer_contents_name_format() -> Result<()> {
		read_fails("[(1, 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]")
	}

	#[test]
	fn test_invalid_layer_contents_directory_format() -> Result<()> {
		read_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 1), ('layer 2', 'glyphs.layer 2')]",
		)
	}

	#[test]
	fn test_layer_contents_has_missing_directory() -> Result<()> {
		read_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.doesnotexist'), ('layer 2', 'glyphs.layer 2')]",
		)
	}

	#[test]
	fn test_missing_default_layer() -> Result<()> {
		read_fails("[('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]")
	}

	#[test]
	fn test_duplicate_layer_name() -> Result<()> {
		read_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 1', 'glyphs.layer 2')]",
		)
	}

	#[test]
	fn test_duplicate_layer_directory() -> Result<()> {
		read_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 1')]",
		)
	}

	#[test]
	fn test_default_layer_no_name() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(
			&t.ufo_path,
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
		);
		UfoReader::new(&t.ufo_path, true)?.get_glyph_set(None, None, None)?;
		Ok(())
	}

	#[test]
	fn test_default_layer_name() -> Result<()> {
		let t = layer_test();
		let contents = "[('custom name', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]";
		// get the name
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(&t.ufo_path, contents);
		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_eq!(reader.get_default_layer_name(None)?, "custom name");
		// get the glyph set
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(&t.ufo_path, contents);
		let reader = UfoReader::new(&t.ufo_path, true)?;
		reader.get_glyph_set(Some("custom name"), None, None)?;
		Ok(())
	}

	#[test]
	fn test_layer_order() -> Result<()> {
		let t = layer_test();
		for (contents, expected) in [
			(
				"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
				["public.foregound", "layer 1", "layer 2"],
			),
			(
				"[('layer 1', 'glyphs.layer 1'), ('public.foregound', 'glyphs'), ('layer 2', 'glyphs.layer 2')]",
				["layer 1", "public.foregound", "layer 2"],
			),
			(
				"[('layer 2', 'glyphs.layer 2'), ('layer 1', 'glyphs.layer 1'), ('public.foregound', 'glyphs')]",
				["layer 2", "layer 1", "public.foregound"],
			),
		] {
			make_ufo(&t.ufo_path, None, None);
			set_layer_contents(&t.ufo_path, contents);
			let reader = UfoReader::new(&t.ufo_path, true)?;
			assert_eq!(reader.get_layer_names(None)?, expected);
		}
		Ok(())
	}
}

// UFO3WriteLayersTestCase

mod write_layers {
	use super::*;

	fn layer_contents(ufo_path: &Path) -> Value {
		read_plist(&ufo_path.join("layercontents.plist"))
	}

	fn opts() -> WriterGlyphSetOptions {
		WriterGlyphSetOptions::default()
	}

	#[test]
	fn test_missing_layer_contents() {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		std::fs::remove_file(t.ufo_path.join("layercontents.plist")).expect("remove");
		assert_ufo_error(writer(&t.ufo_path));
	}

	#[test]
	fn test_invalid_layer_contents_format() -> Result<()> {
		let t = layer_test();
		// bogus
		make_ufo(&t.ufo_path, None, None);
		let path = t.ufo_path.join("layercontents.plist");
		std::fs::remove_file(&path)?;
		std::fs::write(&path, "test")?;
		assert_ufo_error(writer(&t.ufo_path));
		// dict
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(
			&t.ufo_path,
			"{'public.default': 'glyphs', 'layer 1': 'glyphs.layer 1', 'layer 2': 'glyphs.layer 2'}",
		);
		assert_ufo_error(writer(&t.ufo_path));
		Ok(())
	}

	fn write_fails(contents: &str) {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(&t.ufo_path, contents);
		assert_ufo_error(writer(&t.ufo_path));
	}

	#[test]
	fn test_invalid_layer_contents_name_format() {
		write_fails(
			"[(1, 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
		);
	}

	#[test]
	fn test_invalid_layer_contents_directory_format() {
		write_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 1), ('layer 2', 'glyphs.layer 2')]",
		);
	}

	#[test]
	fn test_layer_contents_has_missing_directory() {
		write_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.doesnotexist'), ('layer 2', 'glyphs.layer 2')]",
		);
	}

	#[test]
	fn test_missing_default_layer() {
		write_fails("[('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]");
	}

	#[test]
	fn test_duplicate_layer_name() {
		write_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 1', 'glyphs.layer 2')]",
		);
	}

	#[test]
	fn test_duplicate_layer_directory() {
		write_fails(
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 1')]",
		);
	}

	#[test]
	fn test_default_layer_no_name() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(
			&t.ufo_path,
			"[('public.foregound', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
		);
		writer(&t.ufo_path)?;
		Ok(())
	}

	#[test]
	fn test_default_layer_name() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		set_layer_contents(
			&t.ufo_path,
			"[('custom name', 'glyphs'), ('layer 1', 'glyphs.layer 1'), ('layer 2', 'glyphs.layer 2')]",
		);
		writer(&t.ufo_path)?;
		Ok(())
	}

	#[test]
	fn test_up_convert_1_to_3() -> Result<()> {
		let t = layer_test();
		make_ufo(
			&t.ufo_path,
			Some("dict(creator='test', formatVersion=1)"),
			Some("dict()"),
		);
		let writer = writer(&t.ufo_path)?;
		writer.write_layer_contents(Some(&[Some("public.default")]), None)?;
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value("[['public.default', 'glyphs']]")
		);
		Ok(())
	}

	#[test]
	fn test_up_convert_2_to_3() -> Result<()> {
		let t = layer_test();
		make_ufo(
			&t.ufo_path,
			Some("dict(creator='test', formatVersion=2)"),
			Some("dict()"),
		);
		let writer = writer(&t.ufo_path)?;
		writer.write_layer_contents(Some(&[Some("public.default")]), None)?;
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value("[['public.default', 'glyphs']]")
		);
		Ok(())
	}

	#[test]
	fn test_down_convert_3_to_1() {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		assert_ufo_error(writer_version(&t.ufo_path, 1));
	}

	#[test]
	fn test_down_convert_3_to_2() {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		assert_ufo_error(writer_version(&t.ufo_path, 2));
	}

	#[test]
	fn test_get_glyph_sets() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		// hack contents.plist
		write_plist(
			&t.ufo_path.join("glyphs.layer 1").join("contents.plist"),
			&py_value("dict(b='a.glif')"),
		);
		write_plist(
			&t.ufo_path.join("glyphs.layer 2").join("contents.plist"),
			&py_value("dict(c='a.glif')"),
		);
		// now test
		let mut writer = writer(&t.ufo_path)?;
		// default
		assert_eq!(writer.get_glyph_set(None, true, opts())?.keys(), ["a"]);
		// layer 1
		assert_eq!(
			writer.get_glyph_set(Some("layer 1"), false, opts())?.keys(),
			["b"]
		);
		// layer 2
		assert_eq!(
			writer.get_glyph_set(Some("layer 2"), false, opts())?.keys(),
			["c"]
		);
		Ok(())
	}

	#[test]
	fn test_get_glyph_set_no_contents() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		std::fs::remove_file(t.ufo_path.join("glyphs.layer 1").join("contents.plist"))?;

		let reader = UfoReader::new(&t.ufo_path, true)?;
		assert_glif_error(reader.get_glyph_set(Some("layer 1"), None, None));

		let mut writer = writer(&t.ufo_path)?;
		assert_glif_error(writer.get_glyph_set(
			Some("layer 1"),
			false,
			WriterGlyphSetOptions {
				expect_contents_file: true,
				..Default::default()
			},
		));
		drop(writer);

		// There's a separate code path for < v3 UFOs.
		write_plist(
			&t.ufo_path.join("metainfo.plist"),
			&py_value("dict(creator='test', formatVersion=2)"),
		);
		std::fs::remove_file(t.ufo_path.join("glyphs").join("contents.plist"))?;
		let mut writer = writer_version(&t.ufo_path, 2)?;
		assert_glif_error(writer.get_glyph_set(
			None,
			true,
			WriterGlyphSetOptions {
				expect_contents_file: true,
				..Default::default()
			},
		));
		Ok(())
	}

	#[test]
	fn test_new_font_one_layer() -> Result<()> {
		let t = layer_test();
		clear_ufo(&t.ufo_path);
		let mut writer = writer(&t.ufo_path)?;
		writer.get_glyph_set(None, true, opts())?;
		writer.write_layer_contents(Some(&[Some("public.default")]), None)?;
		// directory
		assert!(t.ufo_path.join("glyphs").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value("[['public.default', 'glyphs']]")
		);
		Ok(())
	}

	#[test]
	fn test_new_font_three_layers() -> Result<()> {
		let t = layer_test();
		clear_ufo(&t.ufo_path);
		let mut writer = writer(&t.ufo_path)?;
		writer.get_glyph_set(Some("layer 1"), false, opts())?;
		writer.get_glyph_set(None, true, opts())?;
		writer.get_glyph_set(Some("layer 2"), false, opts())?;
		writer.write_layer_contents(
			Some(&[Some("layer 1"), Some("public.default"), Some("layer 2")]),
			None,
		)?;
		// directories
		assert!(t.ufo_path.join("glyphs").exists());
		assert!(t.ufo_path.join("glyphs.layer 1").exists());
		assert!(t.ufo_path.join("glyphs.layer 2").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value(
				"[['layer 1', 'glyphs.layer 1'], ['public.default', 'glyphs'], ['layer 2', 'glyphs.layer 2']]"
			)
		);
		Ok(())
	}

	#[test]
	fn test_add_layer_to_existing_font() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		writer.get_glyph_set(Some("layer 3"), false, opts())?;
		writer.write_layer_contents(
			Some(&[
				Some("public.default"),
				Some("layer 1"),
				Some("layer 2"),
				Some("layer 3"),
			]),
			None,
		)?;
		// directories
		for d in [
			"glyphs",
			"glyphs.layer 1",
			"glyphs.layer 2",
			"glyphs.layer 3",
		] {
			assert!(t.ufo_path.join(d).exists(), "{d}");
		}
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value(
				"[['public.default', 'glyphs'], ['layer 1', 'glyphs.layer 1'], ['layer 2', 'glyphs.layer 2'], ['layer 3', 'glyphs.layer 3']]"
			)
		);
		Ok(())
	}

	#[test]
	fn test_rename_layer() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		writer.rename_glyph_set(Some("layer 1"), Some("layer 3"), false)?;
		writer.write_layer_contents(
			Some(&[Some("public.default"), Some("layer 3"), Some("layer 2")]),
			None,
		)?;
		// directories
		assert!(t.ufo_path.join("glyphs").exists());
		assert!(!t.ufo_path.join("glyphs.layer 1").exists());
		assert!(t.ufo_path.join("glyphs.layer 2").exists());
		assert!(t.ufo_path.join("glyphs.layer 3").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value(
				"[['public.default', 'glyphs'], ['layer 3', 'glyphs.layer 3'], ['layer 2', 'glyphs.layer 2']]"
			)
		);
		Ok(())
	}

	#[test]
	fn test_rename_layer_default() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		writer.rename_glyph_set(Some("public.default"), Some("layer xxx"), false)?;
		writer.rename_glyph_set(Some("layer 1"), Some("layer 1"), true)?;
		writer.write_layer_contents(
			Some(&[Some("layer xxx"), Some("layer 1"), Some("layer 2")]),
			None,
		)?;
		assert!(t.ufo_path.join("glyphs").exists());
		assert!(!t.ufo_path.join("glyphs.layer 1").exists());
		assert!(t.ufo_path.join("glyphs.layer 2").exists());
		assert!(t.ufo_path.join("glyphs.layer xxx").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value(
				"[['layer xxx', 'glyphs.layer xxx'], ['layer 1', 'glyphs'], ['layer 2', 'glyphs.layer 2']]"
			)
		);
		Ok(())
	}

	#[test]
	fn test_rename_layer_duplicate_name() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		assert_ufo_error(writer.rename_glyph_set(Some("layer 1"), Some("layer 2"), false));
		Ok(())
	}

	#[test]
	fn test_rename_layer_unknown_name() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		assert_ufo_error(writer.rename_glyph_set(Some("does not exist"), Some("layer 2"), false));
		Ok(())
	}

	#[test]
	fn test_remove_layer() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		writer.delete_glyph_set(Some("layer 1"))?;
		writer.write_layer_contents(Some(&[Some("public.default"), Some("layer 2")]), None)?;
		// directories
		assert!(t.ufo_path.join("glyphs").exists());
		assert!(!t.ufo_path.join("glyphs.layer 1").exists());
		assert!(t.ufo_path.join("glyphs.layer 2").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value("[['public.default', 'glyphs'], ['layer 2', 'glyphs.layer 2']]")
		);
		Ok(())
	}

	#[test]
	fn test_remove_default_layer() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		writer.delete_glyph_set(Some("public.default"))?;
		writer.write_layer_contents(Some(&[Some("layer 1"), Some("layer 2")]), None)?;
		// directories
		assert!(!t.ufo_path.join("glyphs").exists());
		assert!(t.ufo_path.join("glyphs.layer 1").exists());
		assert!(t.ufo_path.join("glyphs.layer 2").exists());
		// layer contents
		assert_eq!(
			layer_contents(&t.ufo_path),
			py_value("[['layer 1', 'glyphs.layer 1'], ['layer 2', 'glyphs.layer 2']]")
		);
		Ok(())
	}

	#[test]
	fn test_remove_default_layer2() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None, None);
		let mut writer = writer(&t.ufo_path)?;
		assert_ufo_error(writer.delete_glyph_set(Some("does not exist")));
		Ok(())
	}

	#[test]
	fn test_write_ascii_layer_order() -> Result<()> {
		let t = layer_test();
		make_ufo(
			&t.ufo_path,
			None,
			Some(
				"[['public.default', 'glyphs'], ['layer 1', 'glyphs.layer 1'], ['layer 2', 'glyphs.layer 2']]",
			),
		);
		let writer = writer(&t.ufo_path)?;
		writer.write_layer_contents(
			Some(&[Some("public.default"), Some("layer 2"), Some("layer 1")]),
			None,
		)?;
		let result = layer_contents(&t.ufo_path);
		assert_eq!(
			result,
			py_value(
				"[['public.default', 'glyphs'], ['layer 2', 'glyphs.layer 2'], ['layer 1', 'glyphs.layer 1']]"
			)
		);
		let Value::Array(entries) = result else {
			panic!("list");
		};
		for entry in entries {
			assert!(matches!(&entry, Value::Array(pair) if matches!(pair[0], Value::String(_))));
		}
		Ok(())
	}
}

// -----
// /data
// -----

// UFO3ReadDataTestCase

mod read_data {
	use super::*;

	fn font_path() -> PathBuf {
		testdata().join("UFO3-Read Data.ufo")
	}

	#[test]
	fn test_ufo_reader_data_directory_listing() -> Result<()> {
		let reader = UfoReader::new(font_path(), true)?;
		let mut found = reader.get_data_directory_listing()?;
		found.sort();
		assert_eq!(
			found,
			[
				"org.unifiedfontobject.directory/bar/lol.txt",
				"org.unifiedfontobject.directory/foo.txt",
				"org.unifiedfontobject.file.txt",
			]
		);
		Ok(())
	}

	#[test]
	fn test_ufo_reader_bytes_from_path() -> Result<()> {
		let reader = UfoReader::new(font_path(), true)?;
		assert_eq!(
			reader.read_bytes_from_path("data/org.unifiedfontobject.file.txt")?,
			Some(b"file.txt".to_vec())
		);
		assert_eq!(
			reader.read_bytes_from_path("data/org.unifiedfontobject.directory/bar/lol.txt")?,
			Some(b"lol.txt".to_vec())
		);
		assert_eq!(
			reader.read_bytes_from_path("data/org.unifiedfontobject.doesNotExist")?,
			None
		);
		Ok(())
	}

	#[test]
	fn test_ufo_reader_read_file_from_path() -> Result<()> {
		let reader = UfoReader::new(font_path(), true)?;
		let file = reader.get_read_file_for_path("data/org.unifiedfontobject.file.txt")?;
		let mut file = file.expect("file");
		let mut text = String::new();
		std::io::Read::read_to_string(&mut file, &mut text)?;
		assert_eq!(text, "file.txt");
		assert!(
			reader
				.get_read_file_for_path("data/org.unifiedfontobject.doesNotExist")?
				.is_none()
		);
		Ok(())
	}

	#[test]
	fn test_ufo_reader_kern_group_duplicates_removed() -> Result<()> {
		// Non-kerning group duplicates are kept
		// Kerning group duplicates are removed
		let expected = py_dict(
			"{'group1': ['A'], 'group2': ['B', 'C', 'B'], 'public.kern1.A': ['A'], 'public.kern2.B': ['B', 'A', 'C']}",
		);
		let reader = UfoReader::new(font_path(), true)?;
		let groups = fontoxide_ufolib::groups_to_value(&reader.read_groups(None)?);
		assert_eq!(groups, Value::Dict(expected));
		Ok(())
	}
}

// UFO3WriteDataTestCase

mod write_data {
	use super::*;

	#[test]
	fn test_ufo_writer_write_bytes_to_path() -> Result<()> {
		let dir = tempdir("writebytes");
		let dst = dir.join("test.ufo");
		for (path, bytes) in [
			(
				"data/org.unifiedfontobject.writebytesbasicfile.txt",
				&b"test"[..],
			),
			// basic file with unicode text
			(
				"data/org.unifiedfontobject.writebytesbasicunicodefile.txt",
				b"t\xeb\xdft",
			),
			// basic directory
			(
				"data/org.unifiedfontobject.writebytesdirectory/level1/level2/file.txt",
				b"test",
			),
		] {
			let writer = writer_version(&dst, 3)?;
			writer.write_bytes_to_path(path, bytes)?;
			let full = dst.join(path);
			assert!(full.exists());
			assert_eq!(std::fs::read(&full)?, bytes);
			drop(writer);
			clear_ufo(&dst);
		}
		Ok(())
	}

	#[test]
	fn test_ufo_writer_write_file_to_path() -> Result<()> {
		let dir = tempdir("writefile");
		let dst = dir.join("test.ufo");
		let path = "data/org.unifiedfontobject.getwritefile.txt";
		let writer = writer_version(&dst, 3)?;
		let file = writer.get_file_object_for_path(path, "w")?;
		let mut file = file.expect("file");
		let mut buf = Vec::new();
		std::io::Read::read_to_end(&mut file, &mut buf)?;
		file.close()?;
		assert!(dst.join(path).exists());
		Ok(())
	}

	#[test]
	fn test_ufo_writer_remove_file() -> Result<()> {
		let dir = tempdir("removefile");
		let dst = dir.join("test.ufo");
		let path1 = "data/org.unifiedfontobject.removefile/level1/level2/file1.txt";
		let path2 = "data/org.unifiedfontobject.removefile/level1/level2/file2.txt";
		let path3 = "data/org.unifiedfontobject.removefile/level1/file3.txt";
		let writer = writer_version(&dst, 3)?;
		writer.write_bytes_to_path(path1, b"test")?;
		writer.write_bytes_to_path(path2, b"test")?;
		writer.write_bytes_to_path(path3, b"test")?;
		assert!(dst.join(path1).exists());
		assert!(dst.join(path2).exists());
		assert!(dst.join(path3).exists());
		writer.remove_file_for_path(path1, false, true)?;
		assert!(!dst.join(path1).exists());
		assert!(dst.join(path1).parent().expect("parent").exists());
		assert!(dst.join(path2).exists());
		assert!(dst.join(path3).exists());
		writer.remove_file_for_path(path2, false, true)?;
		assert!(!dst.join(path1).parent().expect("parent").exists());
		assert!(!dst.join(path2).exists());
		assert!(dst.join(path3).exists());
		writer.remove_file_for_path(path3, false, true)?;
		assert!(!dst.join(path3).exists());
		assert!(!dst.join(path2).parent().expect("parent").exists());
		assert!(!dst.join("data/org.unifiedfontobject.removefile").exists());
		assert_ufo_error(writer.remove_file_for_path(
			"data/org.unifiedfontobject.doesNotExist.txt",
			false,
			true,
		));
		Ok(())
	}

	#[test]
	fn test_ufo_writer_copy() -> Result<()> {
		let dir = tempdir("copy");
		let dst = dir.join("test.ufo");
		let source_dir = dir.join("test-copy source.ufo");
		let data_path = "data/org.unifiedfontobject.copy/level1/level2/file1.txt";
		let writer = writer_version(&source_dir, 3)?;
		writer.write_bytes_to_path(data_path, b"test")?;
		drop(writer);
		// copy a file
		let reader = UfoReader::new(&source_dir, true)?;
		let writer = writer_version(&dst, 3)?;
		writer.copy_from_reader(&reader, data_path, data_path)?;
		assert!(dst.join(data_path).exists());
		drop(writer);
		clear_ufo(&dst);
		// copy a directory
		let reader = UfoReader::new(&source_dir, true)?;
		let writer = writer_version(&dst, 3)?;
		let p = "data/org.unifiedfontobject.copy";
		writer.copy_from_reader(&reader, p, p)?;
		assert!(dst.join(data_path).exists());
		Ok(())
	}
}

// ---------------
// layerinfo.plist
// ---------------

/// The colors of the layer info color tests, all invalid.
const BAD_COLORS: &[&str] = &[
	"1",
	// not enough commas
	"'1 0, 0, 0'",
	"'1 0 0, 0'",
	"'1 0 0 0'",
	// not enough parts
	"', 0, 0, 0'",
	"'1, , 0, 0'",
	"'1, 0, , 0'",
	"'1, 0, 0, '",
	"', , , '",
	// not a number in all positions
	"'r, 1, 1, 1'",
	"'1, g, 1, 1'",
	"'1, 1, b, 1'",
	"'1, 1, 1, a'",
	// too many parts
	"'1, 0, 0, 0, 0'",
	// < 0 in each position
	"'-1, 0, 0, 0'",
	"'0, -1, 0, 0'",
	"'0, 0, -1, 0'",
	"'0, 0, 0, -1'",
	// > 1 in each position
	"'2, 0, 0, 0'",
	"'0, 2, 0, 0'",
	"'0, 0, 2, 0'",
	"'0, 0, 0, 2'",
];

// UFO3ReadLayerInfoTestCase

mod read_layer_info {
	use super::*;

	/// `makeUFO` of the layer info tests.
	fn make_ufo(ufo_path: &Path, layer_info: Option<&str>) {
		clear_ufo(ufo_path);
		std::fs::create_dir_all(ufo_path).expect("mkdir");
		write_plist(
			&ufo_path.join("metainfo.plist"),
			&py_value("dict(creator='test', formatVersion=3)"),
		);
		write_plist(
			&ufo_path.join("layercontents.plist"),
			&py_value("[('public.default', 'glyphs')]"),
		);
		let glyphs_path = ufo_path.join("glyphs");
		std::fs::create_dir(&glyphs_path).expect("mkdir");
		write_plist(
			&glyphs_path.join("contents.plist"),
			&py_value("dict(a='a.glif')"),
		);
		std::fs::write(glyphs_path.join("a.glif"), " ").expect("write");
		write_plist(
			&glyphs_path.join("layerinfo.plist"),
			&py_value(layer_info.unwrap_or("dict(color='0,0,0,1', lib={'foo': 'bar'})")),
		);
	}

	#[test]
	fn test_valid_layer_info() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None);
		let reader = UfoReader::new(&t.ufo_path, true)?;
		let glyph_set = reader.get_glyph_set(None, None, None)?;
		let info = glyph_set.read_layer_info(None)?;
		assert_eq!(info.get("color"), Some(&Value::from("0,0,0,1")));
		assert_eq!(info.get("lib"), Some(&py_value("{'foo': 'bar'}")));
		Ok(())
	}

	#[test]
	fn test_missing_layer_info() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None);
		std::fs::remove_file(t.ufo_path.join("glyphs").join("layerinfo.plist"))?;
		// read
		let reader = UfoReader::new(&t.ufo_path, true)?;
		let glyph_set = reader.get_glyph_set(None, None, None)?;
		let info = glyph_set.read_layer_info(None)?;
		assert_eq!(info.get("color"), None);
		assert_eq!(info.get("guidelines"), None);
		assert_eq!(info.get("lib"), None);
		Ok(())
	}

	#[test]
	fn test_bogus_layer_info() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None);
		let path = t.ufo_path.join("glyphs").join("layerinfo.plist");
		std::fs::remove_file(&path)?;
		std::fs::write(&path, "test")?;
		// read
		let reader = UfoReader::new(&t.ufo_path, true)?;
		let glyph_set = reader.get_glyph_set(None, None, None)?;
		assert_ufo_error(glyph_set.read_layer_info(None));
		Ok(())
	}

	#[test]
	fn test_invalid_format_layer_info() -> Result<()> {
		let t = layer_test();
		make_ufo(&t.ufo_path, None);
		write_plist(
			&t.ufo_path.join("glyphs").join("layerinfo.plist"),
			&py_value("[('color', '0,0,0,0')]"),
		);
		// read
		let reader = UfoReader::new(&t.ufo_path, true)?;
		let glyph_set = reader.get_glyph_set(None, None, None)?;
		assert_glif_error(glyph_set.read_layer_info(None));
		Ok(())
	}

	#[test]
	fn test_color() -> Result<()> {
		let t = layer_test();
		for color in BAD_COLORS {
			make_ufo(&t.ufo_path, Some(&format!("{{'color': {color}}}")));
			let reader = UfoReader::new(&t.ufo_path, true)?;
			let glyph_set = reader.get_glyph_set(None, None, None)?;
			assert_glif_error(glyph_set.read_layer_info(None));
		}
		Ok(())
	}
}

// UFO3WriteLayerInfoTestCase

mod write_layer_info {
	use super::*;

	fn make_glyph_set(ufo_path: &Path) -> Result<fontoxide_ufolib::GlyphSet> {
		clear_ufo(ufo_path);
		let mut writer = writer(ufo_path)?;
		writer.get_glyph_set(None, true, WriterGlyphSetOptions::default())
	}

	#[test]
	fn test_valid_write() -> Result<()> {
		let t = layer_test();
		let expected = py_dict("dict(color='0,0,0,1', lib={'foo': 'bar'})");
		let glyph_set = make_glyph_set(&t.ufo_path)?;
		glyph_set.write_layer_info(&expected, None)?;
		let result = read_plist(&t.ufo_path.join("glyphs").join("layerinfo.plist"));
		assert_eq!(result, Value::Dict(expected));
		Ok(())
	}

	#[test]
	fn test_color() -> Result<()> {
		let t = layer_test();
		for color in BAD_COLORS {
			let info = py_dict(&format!("{{'color': {color}}}"));
			let glyph_set = make_glyph_set(&t.ufo_path)?;
			assert_glif_error(glyph_set.write_layer_info(&info, None));
		}
		Ok(())
	}
}

// ------------------------------------
// font info reading and writing tables
// ------------------------------------

#[test]
fn test_generic_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("familyName", "123")], true),
			(&[("styleName", "123")], true),
			(&[("styleMapFamilyName", "123")], true),
			(&[("styleMapStyleName", "123")], true),
			(&[("styleMapStyleName", "'REGULAR'")], true),
			(&[("versionMajor", "'1'")], true),
			(&[("versionMinor", "'0'")], true),
			(&[("versionMinor", "-1")], true),
			(&[("copyright", "123")], true),
			(&[("trademark", "123")], true),
			(&[("unitsPerEm", "'abc'")], true),
			(&[("unitsPerEm", "-1")], true),
			(&[("unitsPerEm", "-1.0")], true),
			(&[("descender", "'abc'")], true),
			(&[("xHeight", "'abc'")], true),
			(&[("capHeight", "'abc'")], true),
			(&[("ascender", "'abc'")], true),
			(&[("italicAngle", "'abc'")], true),
		],
	)
}

#[test]
fn test_gasp_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeGaspRangeRecords", "'abc'")], true),
			(&[("openTypeGaspRangeRecords", "[]")], false),
			(&[("openTypeGaspRangeRecords", "['abc']")], true),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 65535, 'notTheRightKey': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'notTheRightKey': 1, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 'abc', 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': 'abc'}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': [-1]}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 10, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 20, 'rangeGaspBehavior': [0]}]",
				)],
				false,
			),
		],
	)
}

#[test]
fn test_head_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeHeadCreated", "123")], true),
			(&[("openTypeHeadCreated", "'2000-Jan-01 00:00:00'")], true),
			(&[("openTypeHeadLowestRecPPEM", "'abc'")], true),
			(&[("openTypeHeadLowestRecPPEM", "-1")], true),
			(&[("openTypeHeadFlags", "[-1]")], true),
		],
	)
}

#[test]
fn test_hhea_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeHheaAscender", "'abc'")], true),
			(&[("openTypeHheaDescender", "'abc'")], true),
			(&[("openTypeHheaLineGap", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeHheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_name_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeNameDesigner", "123")], true),
			(&[("openTypeNameDesignerURL", "123")], true),
			(&[("openTypeNameManufacturer", "123")], true),
			(&[("openTypeNameManufacturerURL", "123")], true),
			(&[("openTypeNameLicense", "123")], true),
			(&[("openTypeNameLicenseURL", "123")], true),
			(&[("openTypeNameVersion", "123")], true),
			(&[("openTypeNameUniqueID", "123")], true),
			(&[("openTypeNameDescription", "123")], true),
			(&[("openTypeNamePreferredFamilyName", "123")], true),
			(&[("openTypeNamePreferredSubfamilyName", "123")], true),
			(&[("openTypeNameCompatibleFullName", "123")], true),
			(&[("openTypeNameSampleText", "123")], true),
			(&[("openTypeNameWWSFamilyName", "123")], true),
			(&[("openTypeNameWWSSubfamilyName", "123")], true),
			(&[("openTypeNameRecords", "'abc'")], true),
			(&[("openTypeNameRecords", "['abc']")], true),
			(&[("openTypeNameRecords", "[{'foo': 'bar'}]")], true),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.', 'foo': 'bar'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': '1', 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': '1', 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': '1', 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': '1', 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}, {'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				false,
			),
		],
	)
}

#[test]
fn test_os2_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeOS2WidthClass", "'abc'")], true),
			(&[("openTypeOS2WidthClass", "15")], true),
			(&[("openTypeOS2WeightClass", "'abc'")], true),
			(
				&[
					("openTypeOS2WeightClass", "'abc'"),
					("openTypeOS2WeightClass", "-50"),
				],
				true,
			),
			(&[("openTypeOS2Selection", "[-1]")], true),
			(&[("openTypeOS2VendorID", "1234")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, '9']")],
				true,
			),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, -9]")],
				true,
			),
			(&[("openTypeOS2Panose", "[0, 1, 2, 3]")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]")],
				true,
			),
			(&[("openTypeOS2FamilyClass", "[1, '1']")], true),
			(&[("openTypeOS2FamilyClass", "[1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 1, 1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 201]")], true),
			(&[("openTypeOS2UnicodeRanges", "['0']")], true),
			(&[("openTypeOS2UnicodeRanges", "[-1]")], true),
			(&[("openTypeOS2CodePageRanges", "['0']")], true),
			(&[("openTypeOS2CodePageRanges", "[-1]")], true),
			(&[("openTypeOS2TypoAscender", "'abc'")], true),
			(&[("openTypeOS2TypoDescender", "'abc'")], true),
			(&[("openTypeOS2TypoLineGap", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "-1")], true),
			(&[("openTypeOS2WinDescent", "'abc'")], true),
			(&[("openTypeOS2WinDescent", "-1")], true),
			(&[("openTypeOS2Type", "['1']")], true),
			(&[("openTypeOS2Type", "[-1]")], true),
			(&[("openTypeOS2SubscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SubscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2StrikeoutSize", "'abc'")], true),
			(&[("openTypeOS2StrikeoutPosition", "'abc'")], true),
		],
	)
}

#[test]
fn test_vhea_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("openTypeVheaVertTypoAscender", "'abc'")], true),
			(&[("openTypeVheaVertTypoDescender", "'abc'")], true),
			(&[("openTypeVheaVertTypoLineGap", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeVheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_fond_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_postscript_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("postscriptFontName", "123")], true),
			(&[("postscriptFullName", "123")], true),
			(&[("postscriptSlantAngle", "'abc'")], true),
			(&[("postscriptUniqueID", "'abc'")], true),
			(&[("postscriptUnderlineThickness", "'abc'")], true),
			(&[("postscriptUnderlinePosition", "'abc'")], true),
			(&[("postscriptIsFixedPitch", "2")], true),
			(&[("postscriptBlueValues", "'abc'")], true),
			(&[("postscriptBlueValues", "[500]")], true),
			(
				&[(
					"postscriptBlueValues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptOtherBlues", "'abc'")], true),
			(&[("postscriptOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyBlues", "'abc'")], true),
			(&[("postscriptFamilyBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyOtherBlues", "'abc'")], true),
			(&[("postscriptFamilyOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapH", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapH",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapV", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapV",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptBlueFuzz", "'abc'")], true),
			(&[("postscriptBlueShift", "'abc'")], true),
			(&[("postscriptBlueScale", "'abc'")], true),
			(&[("postscriptForceBold", "'abc'")], true),
			(&[("postscriptDefaultWidthX", "'abc'")], true),
			(&[("postscriptNominalWidthX", "'abc'")], true),
			(&[("postscriptWeightName", "123")], true),
			(&[("postscriptDefaultCharacter", "123")], true),
			(&[("postscriptWindowsCharacterSet", "-1")], true),
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_woff_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("woffMajorVersion", "1.0")], true),
			(&[("woffMajorVersion", "'abc'")], true),
			(&[("woffMinorVersion", "1.0")], true),
			(&[("woffMinorVersion", "'abc'")], true),
			// `del info["woffMetadataUniqueID"]`
			(&[("woffMetadataUniqueID", "None")], false),
			(&[("woffMetadataUniqueID", "1")], true),
			(
				&[("woffMetadataUniqueID", "{'id': 'foo', 'notTheRightKey': 1}")],
				true,
			),
			(&[("woffMetadataUniqueID", "{}")], true),
			(&[("woffMetadataUniqueID", "{'id': 1}")], true),
			(&[("woffMetadataUniqueID", "{'id': ''}")], false),
			(&[("woffMetadataVendor", "{'url': 'foo'}")], true),
			(&[("woffMetadataVendor", "{'name': 1, 'url': 'foo'}")], true),
			(
				&[("woffMetadataVendor", "{'name': '', 'url': 'foo'}")],
				false,
			),
			(&[("woffMetadataVendor", "{'name': 'foo'}")], false),
			(&[("woffMetadataVendor", "{'name': 'foo', 'url': 1}")], true),
			(
				&[("woffMetadataVendor", "{'name': 'foo', 'url': ''}")],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'ltr'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'rtl'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'utd'}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': 'hello'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': ''}",
				)],
				false,
			),
			(&[("woffMetadataCredits", "{}")], true),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataCredits", "{'credits': 'abc'}")], true),
			(&[("woffMetadataCredits", "{'credits': []}")], true),
			(&[("woffMetadataCredits", "{'credits': ['abc']}")], true),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataCredits", "{'credits': [{'url': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataCredits", "{'credits': [{'name': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'role': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'dir': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'text': 'foo'}]}")],
				false,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo'}], 'url': 1}",
				)],
				true,
			),
			(&[("woffMetadataDescription", "{'url': 'foo'}")], true),
			(&[("woffMetadataDescription", "{'text': 'abc'}")], true),
			(&[("woffMetadataDescription", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataLicense", "{'text': [{'text': 'foo'}]}")],
				false,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo'}], 'url': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo'}], 'id': 1}",
				)],
				true,
			),
			(&[("woffMetadataLicense", "{'url': 'foo'}")], false),
			(&[("woffMetadataLicense", "{'text': 'abc'}")], true),
			(&[("woffMetadataLicense", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataLicense", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(&[("woffMetadataLicense", "{'text': [{'text': 1}]}")], true),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataCopyright", "{}")], true),
			(&[("woffMetadataCopyright", "{'text': 'abc'}")], true),
			(&[("woffMetadataCopyright", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataCopyright", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataCopyright", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataTrademark", "{}")], true),
			(&[("woffMetadataTrademark", "{'text': 'abc'}")], true),
			(&[("woffMetadataTrademark", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataTrademark", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataTrademark", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(&[("woffMetadataLicensee", "{}")], true),
			(
				&[(
					"woffMetadataLicensee",
					"{'name': 'foo', 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataLicensee", "{'name': 1}")], true),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'ltr'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'rtl'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'utd'}")],
				true,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'class': 'hello'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'class': 1}")],
				true,
			),
		],
	)
}

#[test]
fn test_guidelines_read() -> Result<()> {
	check_read(
		3,
		&font_info_version3(),
		&[
			(&[("guidelines", "[{'x': '1'}]")], true),
			(&[("guidelines", "[{'y': '1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'y': 0, 'angle': -1}]")], true),
			(&[("guidelines", "[{'x': 0, 'y': 0, 'angle': 361}]")], true),
			(&[("guidelines", "[{'x': 0, 'name': 1}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': 1}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0 0 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': ', 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, , 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 0, , 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 0, 0, '}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': ', , , '}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': 'r, 1, 1, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, g, 1, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 1, b, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 1, 1, a'}]")], true),
			(
				&[("guidelines", "[{'x': 0, 'color': '1, 0, 0, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '-1, 0, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, -1, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, 0, -1, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, 0, 0, -1'}]")],
				true,
			),
			(&[("guidelines", "[{'x': 0, 'color': '2, 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 2, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 0, 2, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 0, 0, 2'}]")], true),
			(
				&[(
					"guidelines",
					"[{'x': 0, 'identifier': 'guide1'}, {'y': 0, 'identifier': 'guide1'}]",
				)],
				true,
			),
		],
	)
}

#[test]
fn test_generic_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("familyName", "123")], true),
			(&[("styleName", "123")], true),
			(&[("styleMapFamilyName", "123")], true),
			(&[("styleMapStyleName", "123")], true),
			(&[("styleMapStyleName", "'REGULAR'")], true),
			(&[("versionMajor", "'1'")], true),
			(&[("versionMinor", "'0'")], true),
			(&[("copyright", "123")], true),
			(&[("trademark", "123")], true),
			(&[("unitsPerEm", "'abc'")], true),
			(&[("descender", "'abc'")], true),
			(&[("xHeight", "'abc'")], true),
			(&[("capHeight", "'abc'")], true),
			(&[("ascender", "'abc'")], true),
			(&[("italicAngle", "'abc'")], true),
		],
	)
}

#[test]
fn test_gasp_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeGaspRangeRecords", "'abc'")], true),
			(&[("openTypeGaspRangeRecords", "[]")], false),
			(&[("openTypeGaspRangeRecords", "['abc']")], true),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 65535, 'notTheRightKey': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'notTheRightKey': 1, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 'abc', 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': 'abc'}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': [-1]}, {'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 65535, 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 10, 'rangeGaspBehavior': [0]}]",
				)],
				true,
			),
			(
				&[(
					"openTypeGaspRangeRecords",
					"[{'rangeMaxPPEM': 10, 'rangeGaspBehavior': [0]}, {'rangeMaxPPEM': 20, 'rangeGaspBehavior': [0]}]",
				)],
				false,
			),
		],
	)
}

#[test]
fn test_head_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeHeadCreated", "123")], true),
			(&[("openTypeHeadCreated", "'2000-Jan-01 00:00:00'")], true),
			(&[("openTypeHeadLowestRecPPEM", "'abc'")], true),
			(&[("openTypeHeadFlags", "[-1]")], true),
		],
	)
}

#[test]
fn test_hhea_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeHheaAscender", "'abc'")], true),
			(&[("openTypeHheaDescender", "'abc'")], true),
			(&[("openTypeHheaLineGap", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeHheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_name_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeNameDesigner", "123")], true),
			(&[("openTypeNameDesignerURL", "123")], true),
			(&[("openTypeNameManufacturer", "123")], true),
			(&[("openTypeNameManufacturerURL", "123")], true),
			(&[("openTypeNameLicense", "123")], true),
			(&[("openTypeNameLicenseURL", "123")], true),
			(&[("openTypeNameVersion", "123")], true),
			(&[("openTypeNameUniqueID", "123")], true),
			(&[("openTypeNameDescription", "123")], true),
			(&[("openTypeNamePreferredFamilyName", "123")], true),
			(&[("openTypeNamePreferredSubfamilyName", "123")], true),
			(&[("openTypeNameCompatibleFullName", "123")], true),
			(&[("openTypeNameSampleText", "123")], true),
			(&[("openTypeNameWWSFamilyName", "123")], true),
			(&[("openTypeNameWWSSubfamilyName", "123")], true),
			(&[("openTypeNameRecords", "'abc'")], true),
			(&[("openTypeNameRecords", "['abc']")], true),
			(&[("openTypeNameRecords", "[{'foo': 'bar'}]")], true),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.', 'foo': 'bar'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': '1', 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': '1', 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': '1', 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': '1', 'string': 'Name Record.'}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 1}]",
				)],
				true,
			),
			(
				&[(
					"openTypeNameRecords",
					"[{'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}, {'nameID': 1, 'platformID': 1, 'encodingID': 1, 'languageID': 1, 'string': 'Name Record.'}]",
				)],
				false,
			),
		],
	)
}

#[test]
fn test_os2_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeOS2WidthClass", "'abc'")], true),
			(&[("openTypeOS2WidthClass", "15")], true),
			(&[("openTypeOS2WeightClass", "'abc'")], true),
			(&[("openTypeOS2WeightClass", "-50")], true),
			(&[("openTypeOS2Selection", "[-1]")], true),
			(&[("openTypeOS2VendorID", "1234")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, '9']")],
				true,
			),
			(&[("openTypeOS2Panose", "[0, 1, 2, 3]")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]")],
				true,
			),
			(&[("openTypeOS2FamilyClass", "[0, '1']")], true),
			(&[("openTypeOS2FamilyClass", "[1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 1, 1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 20]")], true),
			(&[("openTypeOS2UnicodeRanges", "['0']")], true),
			(&[("openTypeOS2UnicodeRanges", "[-1]")], true),
			(&[("openTypeOS2CodePageRanges", "['0']")], true),
			(&[("openTypeOS2CodePageRanges", "[-1]")], true),
			(&[("openTypeOS2TypoAscender", "'abc'")], true),
			(&[("openTypeOS2TypoDescender", "'abc'")], true),
			(&[("openTypeOS2TypoLineGap", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "-1")], true),
			(&[("openTypeOS2WinDescent", "'abc'")], true),
			(&[("openTypeOS2WinDescent", "-1")], true),
			(&[("openTypeOS2Type", "['1']")], true),
			(&[("openTypeOS2Type", "[-1]")], true),
			(&[("openTypeOS2SubscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SubscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2StrikeoutSize", "'abc'")], true),
			(&[("openTypeOS2StrikeoutPosition", "'abc'")], true),
		],
	)
}

#[test]
fn test_vhea_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("openTypeVheaVertTypoAscender", "'abc'")], true),
			(&[("openTypeVheaVertTypoDescender", "'abc'")], true),
			(&[("openTypeVheaVertTypoLineGap", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeVheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_fond_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_postscript_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("postscriptFontName", "123")], true),
			(&[("postscriptFullName", "123")], true),
			(&[("postscriptSlantAngle", "'abc'")], true),
			(&[("postscriptUniqueID", "'abc'")], true),
			(&[("postscriptUnderlineThickness", "'abc'")], true),
			(&[("postscriptUnderlinePosition", "'abc'")], true),
			(&[("postscriptIsFixedPitch", "2")], true),
			(&[("postscriptBlueValues", "'abc'")], true),
			(&[("postscriptBlueValues", "[500]")], true),
			(
				&[(
					"postscriptBlueValues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptOtherBlues", "'abc'")], true),
			(&[("postscriptOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyBlues", "'abc'")], true),
			(&[("postscriptFamilyBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyOtherBlues", "'abc'")], true),
			(&[("postscriptFamilyOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapH", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapH",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapV", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapV",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptBlueFuzz", "'abc'")], true),
			(&[("postscriptBlueShift", "'abc'")], true),
			(&[("postscriptBlueScale", "'abc'")], true),
			(&[("postscriptForceBold", "'abc'")], true),
			(&[("postscriptDefaultWidthX", "'abc'")], true),
			(&[("postscriptNominalWidthX", "'abc'")], true),
			(&[("postscriptWeightName", "123")], true),
			(&[("postscriptDefaultCharacter", "123")], true),
			(&[("postscriptWindowsCharacterSet", "-1")], true),
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_woff_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("woffMajorVersion", "1.0")], true),
			(&[("woffMajorVersion", "'abc'")], true),
			(&[("woffMinorVersion", "1.0")], true),
			(&[("woffMinorVersion", "'abc'")], true),
			(&[("woffMetadataUniqueID", "None")], false),
			(&[("woffMetadataUniqueID", "1")], true),
			(
				&[("woffMetadataUniqueID", "{'id': 'foo', 'notTheRightKey': 1}")],
				true,
			),
			(&[("woffMetadataUniqueID", "{}")], true),
			(&[("woffMetadataUniqueID", "{'id': 1}")], true),
			(&[("woffMetadataUniqueID", "{'id': ''}")], false),
			(&[("woffMetadataVendor", "{'url': 'foo'}")], true),
			(&[("woffMetadataVendor", "{'name': 1, 'url': 'foo'}")], true),
			(
				&[("woffMetadataVendor", "{'name': '', 'url': 'foo'}")],
				false,
			),
			(&[("woffMetadataVendor", "{'name': 'foo'}")], false),
			(&[("woffMetadataVendor", "{'name': 'foo', 'url': 1}")], true),
			(
				&[("woffMetadataVendor", "{'name': 'foo', 'url': ''}")],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'ltr'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'rtl'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'dir': 'utd'}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': 'hello'}",
				)],
				false,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataVendor",
					"{'name': 'foo', 'url': 'bar', 'class': ''}",
				)],
				false,
			),
			(&[("woffMetadataCredits", "{}")], true),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataCredits", "{'credits': 'abc'}")], true),
			(&[("woffMetadataCredits", "{'credits': []}")], true),
			(&[("woffMetadataCredits", "{'credits': ['abc']}")], true),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataCredits", "{'credits': [{'url': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataCredits", "{'credits': [{'name': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'role': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'dir': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCredits",
					"{'credits': [{'name': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'text': 'foo'}]}")],
				false,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo'}], 'url': 1}",
				)],
				true,
			),
			(&[("woffMetadataDescription", "{'url': 'foo'}")], true),
			(&[("woffMetadataDescription", "{'text': 'abc'}")], true),
			(&[("woffMetadataDescription", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataDescription", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataDescription",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataLicense", "{'text': [{'text': 'foo'}]}")],
				false,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo'}], 'url': 1}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo'}], 'id': 1}",
				)],
				true,
			),
			(&[("woffMetadataLicense", "{'url': 'foo'}")], false),
			(&[("woffMetadataLicense", "{'text': 'abc'}")], true),
			(&[("woffMetadataLicense", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataLicense", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(&[("woffMetadataLicense", "{'text': [{'text': 1}]}")], true),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataLicense",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataCopyright", "{}")], true),
			(&[("woffMetadataCopyright", "{'text': 'abc'}")], true),
			(&[("woffMetadataCopyright", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataCopyright", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataCopyright", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataCopyright",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo'}], 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataTrademark", "{}")], true),
			(&[("woffMetadataTrademark", "{'text': 'abc'}")], true),
			(&[("woffMetadataTrademark", "{'text': ['abc']}")], true),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'notTheRightKey': 1}]}",
				)],
				true,
			),
			(
				&[("woffMetadataTrademark", "{'text': [{'language': 'foo'}]}")],
				true,
			),
			(
				&[("woffMetadataTrademark", "{'text': [{'text': 1}]}")],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'url': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'language': 1}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'dir': 'utd'}]}",
				)],
				true,
			),
			(
				&[(
					"woffMetadataTrademark",
					"{'text': [{'text': 'foo', 'class': 1}]}",
				)],
				true,
			),
			(&[("woffMetadataLicensee", "{}")], true),
			(
				&[(
					"woffMetadataLicensee",
					"{'name': 'foo', 'notTheRightKey': 1}",
				)],
				true,
			),
			(&[("woffMetadataLicensee", "{'name': 1}")], true),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'ltr'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'rtl'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'dir': 'utd'}")],
				true,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'class': 'hello'}")],
				false,
			),
			(
				&[("woffMetadataLicensee", "{'name': 'foo', 'class': 1}")],
				true,
			),
		],
	)
}

#[test]
fn test_guidelines_write() -> Result<()> {
	check_write(
		3,
		&font_info_version3(),
		true,
		&[
			(&[("guidelines", "[{'x': '1'}]")], true),
			(&[("guidelines", "[{'y': '1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'y': 0, 'angle': -1}]")], true),
			(&[("guidelines", "[{'x': 0, 'y': 0, 'angle': 361}]")], true),
			(&[("guidelines", "[{'x': 0, 'name': 1}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': 1}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1 0 0 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': ', 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, , 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 0, , 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 0, 0, '}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': ', , , '}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': 'r, 1, 1, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, g, 1, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 1, b, 1'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '1, 1, 1, a'}]")], true),
			(
				&[("guidelines", "[{'x': 0, 'color': '1, 0, 0, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '-1, 0, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, -1, 0, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, 0, -1, 0'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'color': '0, 0, 0, -1'}]")],
				true,
			),
			(&[("guidelines", "[{'x': 0, 'color': '2, 0, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 2, 0, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 0, 2, 0'}]")], true),
			(&[("guidelines", "[{'x': 0, 'color': '0, 0, 0, 2'}]")], true),
			(
				&[(
					"guidelines",
					"[{'x': 0, 'identifier': 'guide1'}, {'y': 0, 'identifier': 'guide1'}]",
				)],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'identifier': '\\x00x1F'}]")],
				true,
			),
			(
				&[("guidelines", "[{'x': 0, 'identifier': '\\x00x7F'}]")],
				true,
			),
		],
	)
}
