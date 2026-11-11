//! Port of fontTools' `Tests/ufoLib/UFO1_test.py`.

mod common;

use common::{
	TempDir, font_info_version1, font_info_version2, py_value, read_plist, tempdir, write_plist,
};
use fontoxide_ufolib::{Dict, Result, UfoReader, UfoWriter, UfoWriterOptions, Value};

const FONT_STYLE_1_TO_2: [(i64, &str); 4] = [
	(64, "regular"),
	(1, "italic"),
	(32, "bold"),
	(33, "bold italic"),
];

const WIDTH_NAME_1_TO_2: [(&str, i64); 9] = [
	("Ultra-condensed", 1),
	("Extra-condensed", 2),
	("Condensed", 3),
	("Semi-condensed", 4),
	("Medium (normal)", 5),
	("Semi-expanded", 6),
	("Expanded", 7),
	("Extra-expanded", 8),
	("Ultra-expanded", 9),
];

// ReadFontInfoVersion1TestCase

mod read {
	use super::*;

	fn setup() -> TempDir {
		let dir = tempdir("ufo1read");
		write_plist(
			&dir.join("metainfo.plist"),
			&py_value("{'creator': 'test', 'formatVersion': 1}"),
		);
		dir
	}

	fn read_info(dir: &TempDir, info: Dict) -> Result<Dict> {
		write_plist(&dir.join("fontinfo.plist"), &Value::Dict(info));
		UfoReader::new(dir.path(), true)?.read_info(None)
	}

	#[test]
	fn test_read() -> Result<()> {
		let dir = setup();
		let info = read_info(&dir, font_info_version1())?;
		let expected = font_info_version2();
		for (attr, value) in &info {
			if let Some(original) = expected.get(attr) {
				assert_eq!(original, value, "{attr}");
			}
		}
		Ok(())
	}

	#[test]
	fn test_font_style_conversion() -> Result<()> {
		let dir = setup();
		for (old, new) in FONT_STYLE_1_TO_2 {
			let mut info = font_info_version1();
			info.insert("fontStyle".into(), Value::from(old));
			let read = read_info(&dir, info)?;
			assert_eq!(read.get("styleMapStyleName"), Some(&Value::from(new)));
		}
		Ok(())
	}

	#[test]
	fn test_width_name_conversion() -> Result<()> {
		let dir = setup();
		for (old, new) in WIDTH_NAME_1_TO_2 {
			let mut info = font_info_version1();
			info.insert("widthName".into(), Value::from(old));
			let read = read_info(&dir, info)?;
			assert_eq!(read.get("openTypeOS2WidthClass"), Some(&Value::from(new)));
		}
		Ok(())
	}
}

// WriteFontInfoVersion1TestCase

mod write {
	use super::*;

	fn write_info(dir: &TempDir, info: &Dict) -> Result<Dict> {
		let ufo = dir.join("test.ufo");
		let writer = UfoWriter::new(
			&ufo,
			UfoWriterOptions {
				format_version: Some(1.into()),
				..Default::default()
			},
		)?;
		writer.write_info(info, None)?;
		match read_plist(&ufo.join("fontinfo.plist")) {
			Value::Dict(d) => Ok(d),
			other => panic!("{other:?}"),
		}
	}

	#[test]
	fn test_write() -> Result<()> {
		let dir = tempdir("ufo1write");
		let written = write_info(&dir, &font_info_version2())?;
		for (attr, original) in font_info_version1() {
			assert_eq!(written.get(&attr), Some(&original), "{attr}");
		}
		Ok(())
	}

	#[test]
	fn test_font_style_conversion() -> Result<()> {
		let dir = tempdir("ufo1write");
		for (old, new) in FONT_STYLE_1_TO_2 {
			let mut info = font_info_version2();
			info.insert("styleMapStyleName".into(), Value::from(new));
			let written = write_info(&dir, &info)?;
			assert_eq!(written.get("fontStyle"), Some(&Value::from(old)));
		}
		Ok(())
	}

	#[test]
	fn test_width_name_conversion() -> Result<()> {
		let dir = tempdir("ufo1write");
		for (old, new) in WIDTH_NAME_1_TO_2 {
			let mut info = font_info_version2();
			info.insert("openTypeOS2WidthClass".into(), Value::from(new));
			let written = write_info(&dir, &info)?;
			assert_eq!(written.get("widthName"), Some(&Value::from(old)));
		}
		Ok(())
	}
}
