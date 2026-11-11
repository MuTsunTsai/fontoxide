//! `Tests/tfmLib/tfmLib_test.py`, the doctest of `fontTools.tfmLib`, and
//! comparisons of everything read with fontTools
//! (`tools/reference/tfmlib_fixtures.py`).

use std::path::PathBuf;

use fontoxide_misc::py::{float_repr, str_repr};
use fontoxide_misc::tfm_lib::{
	Face, HeaderString, Tfm, repr_fontdimens, repr_kerning, repr_ligatures,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data_dir() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests/tfmLib/data")
}

fn open(name: &str) -> fontoxide_misc::tfm_lib::Result<Tfm> {
	Tfm::open(data_dir().join(name))
}

#[test]
fn test_read() -> TestResult {
	for name in ["cmex10.tfm", "cmr10.tfm", "cmsy10.tfm"] {
		let tfm = open(name)?;
		assert_eq!(tfm.designsize, 10.0);
		assert!(!tfm.fontdimens.is_empty());
		assert!(tfm.fontdimens.len() >= 7);
		assert!(tfm.extraheader.is_empty());
		assert_eq!(tfm.right_boundary_char, None);
		assert_eq!(tfm.left_boundary_char, None);
		assert_eq!(tfm.chars.len(), 128);
	}
	Ok(())
}

#[test]
fn test_read_boundary_char() -> TestResult {
	let tfm = open("dummy-space.tfm")?;
	assert_eq!(tfm.right_boundary_char, Some(1));
	assert_eq!(tfm.left_boundary_char, Some(256));
	Ok(())
}

fn assert_fontdimens(tfm: &Tfm, expected: &[(&str, f64)]) {
	let mut ours: Vec<(&str, f64)> = tfm
		.fontdimens
		.iter()
		.map(|(k, v)| (k.as_str(), *v))
		.collect();
	let mut expected = expected.to_vec();
	ours.sort_by(|a, b| a.0.cmp(b.0));
	expected.sort_by(|a, b| a.0.cmp(b.0));
	assert_eq!(ours, expected);
}

#[test]
fn test_read_fontdimens_vanilla() -> TestResult {
	let tfm = open("cmr10.tfm")?;
	assert_fontdimens(
		&tfm,
		&[
			("SLANT", 0.0),
			("SPACE", 0.33333396911621094),
			("STRETCH", 0.16666698455810547),
			("SHRINK", 0.11111164093017578),
			("XHEIGHT", 0.4305553436279297),
			("QUAD", 1.0000028610229492),
			("EXTRASPACE", 0.11111164093017578),
		],
	);
	Ok(())
}

#[test]
fn test_read_fontdimens_mathex() -> TestResult {
	let tfm = open("cmex10.tfm")?;
	assert_fontdimens(
		&tfm,
		&[
			("SLANT", 0.0),
			("SPACE", 0.0),
			("STRETCH", 0.0),
			("SHRINK", 0.0),
			("XHEIGHT", 0.4305553436279297),
			("QUAD", 1.0000028610229492),
			("EXTRASPACE", 0.0),
			("DEFAULTRULETHICKNESS", 0.03999900817871094),
			("BIGOPSPACING1", 0.11111164093017578),
			("BIGOPSPACING2", 0.16666698455810547),
			("BIGOPSPACING3", 0.19999980926513672),
			("BIGOPSPACING4", 0.6000003814697266),
			("BIGOPSPACING5", 0.10000038146972656),
		],
	);
	Ok(())
}

#[test]
fn test_read_fontdimens_mathsy() -> TestResult {
	let tfm = open("cmsy10.tfm")?;
	assert_fontdimens(
		&tfm,
		&[
			("SLANT", 0.25),
			("SPACE", 0.0),
			("STRETCH", 0.0),
			("SHRINK", 0.0),
			("XHEIGHT", 0.4305553436279297),
			("QUAD", 1.0000028610229492),
			("EXTRASPACE", 0.0),
			("NUM1", 0.6765079498291016),
			("NUM2", 0.39373207092285156),
			("NUM3", 0.44373130798339844),
			("DENOM1", 0.6859512329101562),
			("DENOM2", 0.34484100341796875),
			("SUP1", 0.41289234161376953),
			("SUP2", 0.36289215087890625),
			("SUP3", 0.28888893127441406),
			("SUB1", 0.14999961853027344),
			("SUB2", 0.24721717834472656),
			("SUBDROP", 0.05000019073486328),
			("SUPDROP", 0.3861083984375),
			("DELIM1", 2.3899993896484375),
			("DELIM2", 1.010000228881836),
			("AXISHEIGHT", 0.25),
		],
	);
	Ok(())
}

/// The doctest of the module.
#[test]
fn tfm_lib_doctest() -> TestResult {
	let tfm = open("cmr10.tfm")?;
	// Accessing an attribute gets you metadata.
	assert_eq!(tfm.checksum, 1274110073);
	assert_eq!(float_repr(tfm.designsize), "10.0");
	assert_eq!(tfm.codingscheme, "TeX text");
	assert_eq!(tfm.family, HeaderString::Str("CMR".into()));
	assert!(!tfm.seven_bit_safe_flag);
	assert_eq!(tfm.face, Face::Int(234));
	assert!(tfm.extraheader.is_empty());
	assert_eq!(
		repr_fontdimens(&tfm.fontdimens),
		"{'SLANT': 0.0, 'SPACE': 0.33333396911621094, 'STRETCH': 0.16666698455810547, 'SHRINK': 0.11111164093017578, 'XHEIGHT': 0.4305553436279297, 'QUAD': 1.0000028610229492, 'EXTRASPACE': 0.11111164093017578}"
	);
	// Accessing a character gets you its metrics.
	assert_eq!(
		tfm.chars[&(b'g' as u16)].repr(),
		"{'width': 0.5000019073486328, 'height': 0.4305553436279297, 'depth': 0.1944446563720703, 'italic': 0.013888359069824219}"
	);
	// Kerning and ligature can be accessed as well.
	assert_eq!(
		repr_kerning(&tfm.kerning[&(b'c' as u16)]),
		"{104: -0.02777862548828125, 107: -0.02777862548828125}"
	);
	assert_eq!(
		repr_ligatures(&tfm.ligatures[&(b'f' as u16)]),
		"{105: ('LIG', 12), 102: ('LIG', 11), 108: ('LIG', 13)}"
	);
	Ok(())
}

/// The lines of `tools/reference/tfmlib_fixtures.py`.
fn dump(tfm: &Tfm) -> String {
	let opt = |v: Option<String>| v.unwrap_or_else(|| "None".into());
	let header = |s: &HeaderString| match s {
		HeaderString::Str(s) => str_repr(s),
		HeaderString::Bytes(b) => fontoxide_misc::py::bytes_repr(b),
	};
	let dict = |items: Vec<String>| format!("{{{}}}", items.join(", "));
	let lines = [
		format!("checksum={}", tfm.checksum),
		format!("designsize={}", float_repr(tfm.designsize)),
		format!("codingscheme={}", str_repr(&tfm.codingscheme)),
		format!("fonttype={}", tfm.fonttype),
		format!("family={}", header(&tfm.family)),
		format!(
			"seven_bit_safe_flag={}",
			if tfm.seven_bit_safe_flag {
				"True"
			} else {
				"False"
			}
		),
		format!(
			"face={}",
			match &tfm.face {
				Face::Int(i) => i.to_string(),
				Face::Str(s) => str_repr(s),
			}
		),
		format!(
			"extraheader={}",
			dict(
				tfm.extraheader
					.iter()
					.map(|(k, v)| format!("{}: {v}", str_repr(k)))
					.collect()
			)
		),
		format!("fontdimens={}", repr_fontdimens(&tfm.fontdimens)),
		format!(
			"right_boundary_char={}",
			opt(tfm.right_boundary_char.map(|c| c.to_string()))
		),
		format!(
			"left_boundary_char={}",
			opt(tfm.left_boundary_char.map(|c| c.to_string()))
		),
		format!(
			"kerning={}",
			dict(
				tfm.kerning
					.iter()
					.map(|(k, v)| format!("{k}: {}", repr_kerning(v)))
					.collect()
			)
		),
		format!(
			"ligatures={}",
			dict(
				tfm.ligatures
					.iter()
					.map(|(k, v)| format!("{k}: {}", repr_ligatures(v)))
					.collect()
			)
		),
		format!(
			"chars={}",
			dict(
				tfm.chars
					.iter()
					.map(|(k, v)| format!("{k}: {}", v.repr()))
					.collect()
			)
		),
		tfm.repr(),
	];
	lines.join("\n") + "\n"
}

#[test]
fn fonttools_output() -> TestResult {
	let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/tfm_lib");
	for name in ["cmex10.tfm", "cmr10.tfm", "cmsy10.tfm", "dummy-space.tfm"] {
		let expected = std::fs::read_to_string(fixtures.join(format!("{name}.txt")))?;
		assert_eq!(dump(&open(name)?), expected, "{name}");
	}
	Ok(())
}

/// Quirks of fontTools that are reproduced (checked against fontTools 4.68.0).
#[test]
fn upstream_bugs() {
	let file = |sizes: [i16; 12]| {
		let mut d: Vec<u8> = sizes.iter().flat_map(|s| s.to_be_bytes()).collect();
		d.extend([0; 200]);
		d
	};
	// UPSTREAM-BUG(B65): the message names an undefined variable.
	let err = Tfm::from_bytes(&file([30, 2, 0, 0, 1, 1, 1, 1, 0, 0, 300, 0]));
	assert_eq!(
		err.err().map(|e| e.to_string()).as_deref(),
		Some("NameError: name 'ne' is not defined")
	);
	// UPSTREAM-BUG(B64): the message is not an f-string.
	let err = Tfm::from_bytes(&file([30, 2, 0, 0, 1, 1, 1, 1, 0, -1, 0, 0]));
	assert_eq!(
		err.err().map(|e| e.to_string()).as_deref(),
		Some("The subfile size: '{name}' is negative!")
	);
}
