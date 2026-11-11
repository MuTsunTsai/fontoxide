//! `Tests/t1Lib/t1Lib_test.py`, plus byte-for-byte comparisons with the output
//! of fontTools (`tools/reference/t1lib_fixtures.py`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_misc::ps_operators::Key;
use fontoxide_misc::py::float_repr;
use fontoxide_pens::NullPen;
use fontoxide_pens::recording_pen::{PenOp, RecordingPen};
use fontoxide_t1lib::{Dict, T1CharString, T1Font, Value};
use fontoxide_ttlib::cff::charstring::Token;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data_dir() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests/t1Lib/data")
}

fn fixtures_dir() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

// I used `tx` to convert PFA to LWFN (stored in the data fork)
fn lwfn() -> PathBuf {
	data_dir().join("TestT1-Regular.lwfn")
}
fn pfa() -> PathBuf {
	data_dir().join("TestT1-Regular.pfa")
}
fn pfb() -> PathBuf {
	data_dir().join("TestT1-Regular.pfb")
}
fn weird_zeros() -> PathBuf {
	data_dir().join("TestT1-weird-zeros.pfa")
}
// ellipsis is hinted with 55 131 296 131 537 131 vstem3 0 122 hstem
fn ellipsis_hinted() -> PathBuf {
	data_dir().join("TestT1-ellipsis-hinted.pfa")
}

/// A unique temporary file name: tests run in parallel.
fn temp_path(ext: &str) -> PathBuf {
	static N: AtomicUsize = AtomicUsize::new(0);
	std::env::temp_dir().join(format!(
		"fontoxide-t1lib-{}-{}.{ext}",
		std::process::id(),
		N.fetch_add(1, Ordering::SeqCst)
	))
}

fn key(s: &str) -> Key {
	Key::from(s)
}

#[test]
fn test_find_encrypted_chunks() -> TestResult {
	let data = std::fs::read(pfa())?;
	let chunks = fontoxide_t1lib::find_encrypted_chunks(&data)?;
	assert_eq!(chunks.len(), 3);
	assert!(!chunks[0].0);
	// the second chunk is encrypted
	assert!(chunks[1].0);
	assert!(!chunks[2].0);
	Ok(())
}

#[test]
fn test_find_encrypted_chunks_weird_zeros() -> TestResult {
	let data = std::fs::read(weird_zeros())?;
	// Just assert that this doesn't fail for not finding the end of eexec.
	fontoxide_t1lib::find_encrypted_chunks(&data)?;
	Ok(())
}

#[test]
fn test_decrypt_type1() -> TestResult {
	let data = std::fs::read(pfa())?;
	let decrypted = fontoxide_t1lib::decrypt_type1(&data)?;
	assert_ne!(decrypted, data);
	Ok(())
}

/// `ReadWriteTest.write`: saves the font, reads it back, and returns its data or
/// its parsed font dictionary.
fn write(
	font: &mut T1Font,
	outtype: &str,
	dohex: bool,
	doparse: bool,
) -> Result<Written, Box<dyn std::error::Error>> {
	let temp = temp_path(&outtype.to_lowercase());
	let result = (|| -> Result<Written, Box<dyn std::error::Error>> {
		font.save_as(&temp, outtype, dohex)?;
		let mut newfont = T1Font::open(&temp, "ascii", None)?;
		Ok(if doparse {
			newfont.parse()?;
			Written::Font(newfont.font.take().unwrap_or_default())
		} else {
			Written::Data(newfont.get_data()?.to_vec())
		})
	})();
	let _ = std::fs::remove_file(&temp);
	result
}

enum Written {
	Data(Vec<u8>),
	Font(Dict),
}

impl Written {
	fn data(self) -> Vec<u8> {
		match self {
			Written::Data(d) => d,
			Written::Font(_) => Vec::new(),
		}
	}

	fn font(self) -> Dict {
		match self {
			Written::Font(f) => f,
			Written::Data(_) => Dict::default(),
		}
	}
}

fn compile(cs: &T1CharString) -> Option<Vec<u8>> {
	let mut cs = cs.clone();
	cs.compile().ok()?;
	cs.bytecode
}

/// `same_dicts`: charstrings compare by their compiled bytecode.
fn same_dicts(dict1: &Dict, dict2: &Dict) -> bool {
	let keys1: std::collections::HashSet<&Key> = dict1.keys().collect();
	let keys2: std::collections::HashSet<&Key> = dict2.keys().collect();
	if keys1 != keys2 {
		return false;
	}
	for (key, value) in dict1 {
		let Some(other) = dict2.get(key) else {
			return false;
		};
		let same = match (value, other) {
			(Value::Dict(a), Value::Dict(b)) => same_dicts(a, b),
			(Value::Dict(_), _) => false,
			(Value::List(a), Value::List(b)) => {
				a.len() == b.len()
					&& a.iter().zip(b).all(|(e1, e2)| match (e1, e2) {
						(Value::CharString(c1), Value::CharString(c2)) => {
							compile(c1) == compile(c2)
						}
						_ => e1.py_eq(e2),
					})
			}
			(Value::List(_), _) => false,
			(Value::CharString(a), Value::CharString(b)) => compile(a) == compile(b),
			_ => value.py_eq(other),
		};
		if !same {
			return false;
		}
	}
	true
}

#[test]
fn test_read_pfa_write_pfb() -> TestResult {
	let mut font = T1Font::open(pfa(), "ascii", None)?;
	let data = write(&mut font, "PFB", false, false)?.data();
	assert_eq!(font.get_data()?, data);
	Ok(())
}

#[test]
fn test_read_and_parse_pfa_write_pfb() -> TestResult {
	let mut font = T1Font::open(pfa(), "ascii", None)?;
	font.parse()?;
	let saved_font = write(&mut font, "PFB", false, true)?.font();
	assert!(same_dicts(
		font.font.as_ref().ok_or("not parsed")?,
		&saved_font
	));
	Ok(())
}

#[test]
fn test_read_pfb_write_pfa() -> TestResult {
	let mut font = T1Font::open(pfb(), "ascii", None)?;
	// 'OTHER' == 'PFA'
	let data = write(&mut font, "OTHER", true, false)?.data();
	assert_eq!(font.get_data()?, data);
	Ok(())
}

#[test]
fn test_read_and_parse_pfb_write_pfa() -> TestResult {
	let mut font = T1Font::open(pfb(), "ascii", None)?;
	font.parse()?;
	// 'OTHER' == 'PFA'
	let saved_font = write(&mut font, "OTHER", true, true)?.font();
	assert!(same_dicts(
		font.font.as_ref().ok_or("not parsed")?,
		&saved_font
	));
	Ok(())
}

#[test]
fn test_read_with_path() -> TestResult {
	T1Font::open(Path::new(&pfb()), "ascii", None)?;
	Ok(())
}

fn assert_parsed(font: &mut T1Font) -> TestResult {
	font.parse()?;
	assert_eq!(font.get("FontName")?.as_str(), Some("TestT1-Regular"));
	let private = font
		.get("Private")?
		.as_dict()
		.ok_or("Private is not a dict")?;
	assert!(private.contains_key(&key("Subrs")));
	Ok(())
}

#[test]
fn test_parse_lwfn() -> TestResult {
	// the extended attrs are lost on git so we can't auto-detect 'LWFN'
	assert_parsed(&mut T1Font::open(lwfn(), "ascii", Some("LWFN"))?)
}

#[test]
fn test_parse_pfa() -> TestResult {
	assert_parsed(&mut T1Font::open(pfa(), "ascii", None)?)
}

#[test]
fn test_parse_pfb() -> TestResult {
	assert_parsed(&mut T1Font::open(pfb(), "ascii", None)?)
}

/// `test_getGlyphSet`, for every glyph rather than a random one.
#[test]
fn test_get_glyph_set() -> TestResult {
	let mut font = T1Font::open(pfa(), "ascii", None)?;
	let mut glyphs = font.glyph_set()?;
	for name in glyphs.keys() {
		assert!(glyphs.get(&name).ok_or("no glyph")?.width.is_none());
		glyphs.draw(&name, &mut NullPen)?;
		assert!(glyphs.get(&name).ok_or("no glyph")?.width.is_some());
	}
	Ok(())
}

#[test]
fn test_edit_pfa() -> TestResult {
	let mut font = T1Font::open(pfa(), "ascii", None)?;
	{
		let mut glyphs = font.glyph_set()?;
		let ellipsis = glyphs.get_mut("ellipsis").ok_or("no ellipsis")?;
		ellipsis.decompile()?;
		let mut program = Vec::new();
		for v in &ellipsis.program {
			program.push(v.clone());
			if *v == Token::Op("hsbw") {
				program.extend([
					Token::Int(55),
					Token::Int(131),
					Token::Int(296),
					Token::Int(131),
					Token::Int(537),
					Token::Int(131),
					Token::Op("vstem3"),
					Token::Int(0),
					Token::Int(122),
					Token::Op("hstem"),
				]);
			}
		}
		ellipsis.program = program;
	}
	// 'OTHER' == 'PFA'
	let saved_font = write(&mut font, "OTHER", true, true)?.font();
	let mut hinted_font = T1Font::open(ellipsis_hinted(), "ascii", None)?;
	hinted_font.parse()?;
	assert!(same_dicts(
		hinted_font.font.as_ref().ok_or("not parsed")?,
		&saved_font
	));
	Ok(())
}

// --- Byte-for-byte comparisons with fontTools

const FILES: [&str; 5] = [
	"TestT1-Regular.pfa",
	"TestT1-Regular.pfb",
	"TestT1-Regular.lwfn",
	"TestT1-weird-zeros.pfa",
	"TestT1-ellipsis-hinted.pfa",
];

fn open(file: &str) -> fontoxide_t1lib::Result<T1Font> {
	let kind = file.ends_with(".lwfn").then_some("LWFN");
	T1Font::open(data_dir().join(file), "ascii", kind)
}

/// Compares with a fixture, or checks that both failed.
fn check<T: AsRef<[u8]>>(name: &str, ours: Result<T, Box<dyn std::error::Error>>) {
	let expected = std::fs::read(fixtures_dir().join(name));
	match (expected, ours) {
		(Ok(expected), Ok(ours)) => assert!(expected == ours.as_ref(), "{name} differs"),
		(Err(_), Err(_)) => assert!(
			fixtures_dir().join(format!("{name}.err")).exists(),
			"{name}: no fixture"
		),
		(Ok(_), Err(e)) => panic!("{name}: fontTools succeeded, we failed: {e}"),
		(Err(_), Ok(_)) => panic!("{name}: fontTools failed, we succeeded"),
	}
}

fn written(data: &[u8], kind: &str, dohex: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
	let temp = temp_path("out");
	let result = fontoxide_t1lib::write(&temp, data, kind, dohex);
	let out = std::fs::read(&temp);
	let _ = std::fs::remove_file(&temp);
	result?;
	Ok(out?)
}

fn num(v: f64) -> String {
	float_repr(v)
}

fn glyphs(file: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
	let mut font = open(file)?;
	font.parse()?;
	let mut glyphs = font.glyph_set()?;
	let mut out = String::new();
	for name in glyphs.keys() {
		let mut pen = RecordingPen::new();
		let width = glyphs.draw(&name, &mut pen)?;
		out.push_str(&format!("glyph {name} {}\n", num(width)));
		for op in &pen.value {
			let line = match op {
				PenOp::MoveTo(p) => format!("moveTo {} {}", num(p.0), num(p.1)),
				PenOp::LineTo(p) => format!("lineTo {} {}", num(p.0), num(p.1)),
				PenOp::CurveTo(pts) => {
					let mut s = "curveTo".to_owned();
					for p in pts {
						s.push_str(&format!(" {} {}", num(p.0), num(p.1)));
					}
					s
				}
				PenOp::ClosePath => "closePath".into(),
				PenOp::EndPath => "endPath".into(),
				PenOp::AddComponent(base, t) => format!(
					"addComponent {base} {} {} {} {} {} {}",
					num(t.xx),
					num(t.xy),
					num(t.yx),
					num(t.yy),
					num(t.dx),
					num(t.dy)
				),
				other => format!("{other:?}"),
			};
			out.push_str(&line);
			out.push('\n');
		}
	}
	Ok(out.into_bytes())
}

#[test]
fn fonttools_output() -> TestResult {
	for file in FILES {
		let raw = open(file)?.get_data()?.to_vec();
		check(&format!("{file}.raw"), Ok(&raw));
		check(
			&format!("{file}.decrypted"),
			fontoxide_t1lib::decrypt_type1(&raw).map_err(Into::into),
		);
		check(&format!("{file}.pfb"), written(&raw, "PFB", false));
		check(&format!("{file}.pfa"), written(&raw, "OTHER", false));
		check(&format!("{file}.hex.pfa"), written(&raw, "OTHER", true));
		check(
			&format!("{file}.created"),
			(|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
				let mut font = open(file)?;
				font.parse()?;
				Ok(font.create_data()?)
			})(),
		);
		check(&format!("{file}.glyphs"), glyphs(file));
	}
	Ok(())
}

#[test]
fn fonttools_output_edited() -> TestResult {
	let mut font = open("TestT1-Regular.pfa")?;
	{
		let mut glyphs = font.glyph_set()?;
		let ellipsis = glyphs.get_mut("ellipsis").ok_or("no ellipsis")?;
		ellipsis.decompile()?;
		let mut program = Vec::new();
		for v in &ellipsis.program {
			program.push(v.clone());
			if *v == Token::Op("hsbw") {
				program.extend([55, 131, 296, 131, 537, 131].map(Token::Int));
				program.push(Token::Op("vstem3"));
				program.extend([0, 122].map(Token::Int));
				program.push(Token::Op("hstem"));
			}
		}
		ellipsis.program = program;
	}
	check(
		"edited-ellipsis.created",
		font.get_data().map(<[u8]>::to_vec).map_err(Into::into),
	);
	Ok(())
}

/// Quirks of fontTools that are reproduced (checked against fontTools 4.68.0).
#[test]
fn upstream_bugs() {
	// UPSTREAM-BUG(B63): Type 1 charstrings have no encoder for floats.
	let mut cs = T1CharString::from_program(vec![Token::Float(1.5), Token::Op("hsbw")]);
	let err = cs.compile().err().map(|e| e.to_string());
	assert_eq!(
		err.as_deref(),
		Some("TypeError: 'NoneType' object is not callable")
	);
}
