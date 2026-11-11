//! `Tests/misc/psCharStrings_test.py`.

use std::collections::BTreeMap;

use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_pens::Pen;
use fontoxide_pens::bounds_pen::BoundsPen;
use fontoxide_pens::recording_pen::{PenOp, RecordingPen};
use fontoxide_ttlib::cff::charstring::{
	Blender, Context, Interpreter, T2_OPERATORS, T2CharString, Token, encode_fixed, read_token,
};
use fontoxide_ttlib::cff::dict::{encode_float, read_real};
use fontoxide_ttlib::cff::font::num_regions;
use fontoxide_ttlib::ot::types::{VarData, VarRegion, VarRegionAxis, VarRegionList, VarStore};
use fontoxide_ttlib::var::VarStoreInstancer;
use fontoxide_ttlib::{Error, Result};

/// The parts of a `PrivateDict` the charstrings use.
#[derive(Default)]
struct Private {
	nominal_width_x: Option<f64>,
	default_width_x: Option<f64>,
	vsindex: Option<i64>,
	vstore: Option<VarStore>,
}

impl Private {
	/// `PrivateDict()`: the widths default to 0.
	fn new() -> Self {
		Private {
			nominal_width_x: Some(0.0),
			default_width_x: Some(0.0),
			..Default::default()
		}
	}

	/// A CFF2 private DICT with a variation store, and no widths.
	fn cff2(vstore: VarStore) -> Self {
		Private {
			vstore: Some(vstore),
			..Default::default()
		}
	}
}

/// Runs a charstring (`draw`, or `decompile` without a pen).
fn run(
	cs: &mut T2CharString,
	private: &Private,
	pen: Option<&mut dyn Pen>,
	blender: Option<Blender<'_>>,
) -> Result<()> {
	let regions = |vsindex: i64| num_regions(private.vstore.as_ref(), vsindex);
	let ctx = Context {
		local_subrs: &mut [],
		global_subrs: &mut [],
		nominal_width_x: private.nominal_width_x,
		default_width_x: private.default_width_x,
		vsindex: private.vsindex,
		num_regions: &regions,
		blender,
	};
	Interpreter::new(ctx, pen).execute(cs)
}

/// `stringToProgram`.
fn string_to_program(string: &str) -> Vec<Token> {
	string
		.split_whitespace()
		.map(|token| {
			if let Ok(v) = token.parse::<i64>() {
				Token::Int(v)
			} else if let Ok(v) = token.parse::<f64>() {
				Token::Float(v)
			} else {
				let op = T2_OPERATORS.iter().find(|(_, n)| *n == token);
				Token::Op(op.map_or("unknown", |(_, n)| n))
			}
		})
		.collect()
}

/// `T2CharString(program=stringToProgram(string), private=PrivateDict())`.
fn string_to_t2_char_string(string: &str) -> T2CharString {
	T2CharString {
		bytecode: None,
		program: string_to_program(string),
		fd_select_index: None,
	}
}

fn calc_bounds(cs: &mut T2CharString) -> Result<Option<[f64; 4]>> {
	let mut pen = BoundsPen::new_bounds(None);
	run(cs, &Private::new(), Some(&mut pen), None)?;
	Ok(pen.bounds())
}

#[test]
fn test_calc_bounds_empty() -> Result<()> {
	let mut cs = string_to_t2_char_string("endchar");
	assert_eq!(calc_bounds(&mut cs)?, None);
	Ok(())
}

#[test]
fn test_calc_bounds_line() -> Result<()> {
	let mut cs = string_to_t2_char_string("100 100 rmoveto 40 10 rlineto -20 50 rlineto endchar");
	assert_eq!(calc_bounds(&mut cs)?, Some([100.0, 100.0, 140.0, 160.0]));
	Ok(())
}

#[test]
fn test_calc_bounds_curve() -> Result<()> {
	let mut cs =
		string_to_t2_char_string("100 100 rmoveto -50 -150 200 0 -50 150 rrcurveto endchar");
	assert_eq!(
		calc_bounds(&mut cs)?,
		Some([91.90524980688875, -12.5, 208.09475019311125, 100.0])
	);
	Ok(())
}

/// Python's `==` between program items: numbers by value.
fn assert_program(program: &[Token], expected: &[Token]) {
	assert_eq!(program.len(), expected.len(), "{program:?}");
	for (a, b) in program.iter().zip(expected) {
		let num = |t: &Token| match t {
			Token::Int(i) => Some(*i as f64),
			Token::Float(f) => Some(*f),
			_ => None,
		};
		match (num(a), num(b)) {
			(Some(x), Some(y)) => assert_eq!(x, y),
			_ => assert_eq!(a, b),
		}
	}
}

#[test]
fn test_charstring_bytecode_optimization() -> Result<()> {
	use Token::{Float, Int, Op};
	let mut cs = string_to_t2_char_string(
		"100.0 100 rmoveto -50.0 -150 200.5 0.0 -50 150 rrcurveto endchar",
	);
	cs.compile(false)?;
	run(&mut cs, &Private::new(), None, None)?;
	assert_program(
		&cs.program,
		&[
			Int(100),
			Int(100),
			Op("rmoveto"),
			Int(-50),
			Int(-150),
			Float(200.5),
			Int(0),
			Int(-50),
			Int(150),
			Op("rrcurveto"),
			Op("endchar"),
		],
	);
	// The compiled numbers are ints where they can be.
	assert_eq!(cs.program[0], Int(100));
	assert_eq!(cs.program[6], Int(0));

	let mut cs2 = string_to_t2_char_string("100.0 rmoveto -50.0 -150 200.5 0.0 -50 150 rrcurveto");
	cs2.compile(true)?;
	let private = Private {
		nominal_width_x: None,
		default_width_x: None,
		..Default::default()
	};
	run(&mut cs2, &private, None, None)?;
	assert_program(
		&cs2.program,
		&[
			Int(100),
			Op("rmoveto"),
			Int(-50),
			Int(-150),
			Float(200.5),
			Int(0),
			Int(-50),
			Int(150),
			Op("rrcurveto"),
		],
	);
	Ok(())
}

fn hexenc(s: &[u8]) -> String {
	s.iter()
		.map(|b| format!("{b:02x}"))
		.collect::<Vec<_>>()
		.join(" ")
}

// The literals of the upstream test, some beyond f64 precision.
#[allow(clippy::excessive_precision)]
#[test]
fn test_encode_float() -> Result<()> {
	let test_nums: [(f64, &str); 24] = [
		// value                expected result
		(-9.399999999999999, "1e e9 a4 ff"),    // -9.4
		(9.399999999999999999, "1e 9a 4f"),     // 9.4
		(456.8, "1e 45 6a 8f"),                 // 456.8
		(0.0, "1e 0f"),                         // 0
		(-0.0, "1e 0f"),                        // 0
		(1.0, "1e 1f"),                         // 1
		(-1.0, "1e e1 ff"),                     // -1
		(98765.37e2, "1e 98 76 53 7f"),         // 9876537
		(1234567890.0, "1e 12 34 56 79 b2 ff"), // 12345679E2
		(9.876537e-4, "1e 98 76 53 7c 10 ff"),  // 9876537E-10
		(9.876537e4, "1e 98 76 5a 37 ff"),      // 98765.37
		(1000.0, "1e 1b 3f"),                   // 1E3
		(-1000.0, "1e e1 b3 ff"),               // -1E3
		(1e8, "1e 1b 8f"),                      // 1E8
		(1e-5, "1e 1c 5f"),                     // 1E-5
		(1.2e8, "1e 12 b7 ff"),                 // 12E7
		(1.2345e-5, "1e 12 34 5c 9f"),          // 12345E-9
		(9.0987654e8, "1e 90 98 76 54 0f"),     // 909876540
		(0.1, "1e a1 ff"),                      // .1
		(-0.1, "1e ea 1f"),                     // -.1
		(0.01, "1e 1c 2f"),                     // 1e-2
		(-0.01, "1e e1 c2 ff"),                 // -1e-2
		(0.0123, "1e 12 3c 4f"),                // 123e-4
		(-0.0123, "1e e1 23 c4 ff"),            // -123e-4
	];
	for (value, expected) in test_nums {
		let mut encoded = Vec::new();
		encode_float(value, &mut encoded);
		// check to see if we got the expected bytes
		assert_eq!(hexenc(&encoded), expected);
		// check to see if we get the same value by decoding the data; 8 digits
		// of precision, as in encodeFloat
		let (decoded, _) = read_real(&encoded, 1)?;
		let expected: f64 = fontoxide_misc::py::format_g_lower(value, 8)
			.parse()
			.map_err(|_| Error::value("bad float"))?;
		assert_eq!(decoded, expected);
	}
	Ok(())
}

#[allow(clippy::excessive_precision)]
#[test]
fn test_encode_decode_fixed() -> Result<()> {
	let test_nums = [
		// value                expected hex      expected float
		(-9.399999999999999, "ff ff f6 99 9a", -9.3999939),
		(-9.4, "ff ff f6 99 9a", -9.3999939),
		(9.399999999999999999, "ff 00 09 66 66", 9.3999939),
		(9.4, "ff 00 09 66 66", 9.3999939),
		(456.8, "ff 01 c8 cc cd", 456.8000031),
		(-456.8, "ff fe 37 33 33", -456.8000031),
	];
	for (value, expected_hex, expected_float) in test_nums {
		let mut encoded = Vec::new();
		encode_fixed(value, &mut encoded);
		assert_eq!(hexenc(&encoded), expected_hex);
		let Some((Token::Float(decoded), _)) = read_token(&encoded, 0)? else {
			panic!("not a 16.16 number");
		};
		assert!(
			(decoded - expected_float).abs() < 5e-8,
			"{decoded} != {expected_float}"
		);
	}
	Ok(())
}

#[test]
fn test_to_xml() -> Result<()> {
	let program = [
		"107 53.4004 166.199 hstem",
		"174.6 163.801 vstem",
		"338.4 142.8 rmoveto",
		"28 0 21.9 9 15.8 18 15.8 18 7.9 20.79959 0 23.6 rrcurveto",
		"endchar",
	];
	let cs = string_to_t2_char_string(&program.join(" "));
	let mut writer = XmlWriter::new();
	cs.to_xml(&mut writer)?;
	// `getXML` erases the XML declaration.
	let xml = writer.into_string();
	let lines: Vec<&str> = xml.lines().skip(1).collect();
	assert_eq!(lines, program);
	Ok(())
}

#[test]
fn test_from_xml() -> Result<()> {
	use Token::Op;
	let e = fontoxide_misc::xml_reader::parse(concat!(
		"<CharString name=\"period\">  338.4 142.8 rmoveto",
		"  28 0 21.9 9 15.8 18 15.8 18 7.9 20.79959 0 23.6 rrcurveto",
		"  endchar</CharString>",
	))
	.map_err(|e| Error::value(e.to_string()))?;
	let cs = T2CharString::from_xml(&e)?;
	let n = |v: f64| Token::Float(v);
	let expected = [
		n(338.3999939),
		n(142.8000031),
		Op("rmoveto"),
		Token::Int(28),
		Token::Int(0),
		n(21.8999939),
		Token::Int(9),
		n(15.8000031),
		Token::Int(18),
		n(15.8000031),
		Token::Int(18),
		n(7.8999939),
		n(20.7995911),
		Token::Int(0),
		n(23.6000061),
		Op("rrcurveto"),
		Op("endchar"),
	];
	assert_eq!(cs.program.len(), expected.len());
	for (arg, expected) in cs.program.iter().zip(&expected) {
		match (arg, expected) {
			(Token::Op(a), Token::Op(b)) => assert_eq!(a, b),
			(Token::Int(a), Token::Int(b)) => assert_eq!(a, b),
			(Token::Float(a), Token::Float(b)) => {
				// assertAlmostEqual: 7 decimal places
				assert!((a - b).abs() < 5e-8, "{a} != {b}");
			}
			_ => panic!("{arg:?} != {expected:?}"),
		}
	}
	Ok(())
}

#[test]
fn test_pen_close_path() -> Result<()> {
	// Test CFF2/T2 charstring: it does NOT end in "endchar"
	// https://github.com/fonttools/fonttools/issues/2455
	let mut cs = string_to_t2_char_string("100 100 rmoveto -50 -150 200 0 -50 150 rrcurveto");
	let mut pen = RecordingPen::new();
	run(&mut cs, &Private::new(), Some(&mut pen), None)?;
	assert_eq!(pen.value.last(), Some(&PenOp::ClosePath));
	Ok(())
}

/// `buildVarStore(buildVarRegionList(regions, ["TEST"]), [buildVarData(...)])`
/// with `optimize=False` and no items.
fn build_var_store(regions: &[(f64, f64, f64)], var_data: &[Vec<u16>]) -> VarStore {
	VarStore {
		format: 1,
		var_region_list: Some(Box::new(VarRegionList {
			region_axis_count: Some(1),
			region_count: Some(regions.len() as u32),
			region: regions
				.iter()
				.map(|&(start, peak, end)| VarRegion {
					var_region_axis: vec![VarRegionAxis {
						start_coord: start,
						peak_coord: peak,
						end_coord: end,
					}],
				})
				.collect(),
		})),
		var_data_count: Some(var_data.len() as u32),
		var_data: var_data
			.iter()
			.map(|indices| {
				Some(Box::new(VarData {
					item_count: Some(0),
					num_shorts: 0,
					var_region_count: Some(indices.len() as u32),
					var_region_index: indices.clone(),
					item: Vec::new(),
				}))
			})
			.collect(),
	}
}

fn draw_cff2(
	program: Vec<Token>,
	private: &Private,
	bytecode: bool,
	blender: Option<Blender<'_>>,
) -> Result<Vec<PenOp>> {
	let mut charstring = T2CharString {
		bytecode: None,
		program,
		fd_select_index: None,
	};
	if bytecode {
		charstring.compile(true)?;
		charstring = T2CharString::from_bytecode(charstring.bytecode.unwrap_or_default());
	}
	let mut pen = RecordingPen::new();
	run(&mut charstring, private, Some(&mut pen), blender)?;
	Ok(pen.value)
}

#[test]
fn test_cff2_blend_stack() -> Result<()> {
	let sum = |_: i64, deltas: &[f64]| -> Result<f64> { Ok(deltas.iter().sum()) };
	for num_regions in [0usize, 1, 2] {
		for num_blends in [1i64, 2] {
			for use_blender in [false, true] {
				for bytecode in [false, true] {
					let store = build_var_store(
						&vec![(0.0, 1.0, 1.0); num_regions],
						&[(0..num_regions as u16).collect()],
					);
					let private = Private::cff2(store);
					let mut program: Vec<Token> = Vec::new();
					if num_blends == 1 {
						program.push(Token::Int(100));
					}
					if num_blends == 2 {
						program.extend([Token::Int(100), Token::Int(200)]);
					} else {
						program.push(Token::Int(200));
					}
					for j in 0..num_blends {
						for i in 0..num_regions as i64 {
							program.push(Token::Int(10 * (i + 1) * (j + 1)));
						}
					}
					program.extend([
						Token::Int(num_blends),
						Token::Op("blend"),
						Token::Op("rmoveto"),
						Token::Int(100),
						Token::Int(0),
						Token::Op("rlineto"),
						Token::Int(-100),
						Token::Int(100),
						Token::Op("rlineto"),
					]);
					let blender: Option<Blender<'_>> = use_blender.then_some(&sum);
					let value = draw_cff2(program, &private, bytecode, blender)?;
					let delta = if use_blender {
						10.0 * (1..=num_regions).sum::<usize>() as f64
					} else {
						0.0
					};
					let x = 100.0 + if num_blends == 2 { delta } else { 0.0 };
					let y = 200.0 + delta * num_blends as f64;
					assert_eq!(
						value,
						[
							PenOp::MoveTo((x, y)),
							PenOp::LineTo((x + 100.0, y)),
							PenOp::LineTo((x, y + 100.0)),
							PenOp::ClosePath,
						],
						"{num_regions} {num_blends} {use_blender} {bytecode}"
					);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_cff2_vsindex_zero_regions() -> Result<()> {
	let sum = |_: i64, deltas: &[f64]| -> Result<f64> { Ok(deltas.iter().sum()) };
	for use_blender in [false, true] {
		for bytecode in [false, true] {
			let store = build_var_store(&[(0.0, 1.0, 1.0)], &[vec![0], vec![]]);
			let private = Private::cff2(store);
			let program = vec![
				Token::Int(1),
				Token::Op("vsindex"),
				Token::Int(100),
				Token::Int(200),
				Token::Int(1),
				Token::Op("blend"),
				Token::Op("rmoveto"),
				Token::Int(100),
				Token::Int(0),
				Token::Op("rlineto"),
			];
			let blender: Option<Blender<'_>> = use_blender.then_some(&sum);
			assert_eq!(
				draw_cff2(program, &private, bytecode, blender)?,
				[
					PenOp::MoveTo((100.0, 200.0)),
					PenOp::LineTo((200.0, 200.0)),
					PenOp::ClosePath,
				]
			);
		}
	}
	Ok(())
}

#[test]
fn test_cff2_inherited_vsindex() -> Result<()> {
	let axes = vec!["TEST".to_owned()];
	for private_vsindex in [None, Some(0), Some(1)] {
		for charstring_vsindex in [None, Some(0i64), Some(1)] {
			for location in [0.0, 0.5, 1.0] {
				for bytecode in [false, true] {
					let store =
						build_var_store(&[(-1.0, -1.0, 0.0), (0.0, 1.0, 1.0)], &[vec![0], vec![1]]);
					let mut private = Private::cff2(store.clone());
					private.vsindex = private_vsindex;
					let mut program = vec![
						Token::Int(100),
						Token::Int(100),
						Token::Op("rmoveto"),
						Token::Int(100),
						Token::Int(80),
						Token::Int(1),
						Token::Op("blend"),
						Token::Op("hlineto"),
					];
					if let Some(v) = charstring_vsindex {
						program.splice(0..0, [Token::Int(v), Token::Op("vsindex")]);
					}
					let loc: BTreeMap<String, f64> = [("TEST".to_owned(), location)].into();
					let instancer = VarStoreInstancer::new(Some(&store), &axes, loc);
					let blender =
						|i: i64, deltas: &[f64]| instancer.interpolate_from_deltas(i, deltas);
					let value = draw_cff2(program, &private, bytecode, Some(&blender))?;
					let mut vsindex = private_vsindex.unwrap_or(0);
					if let Some(v) = charstring_vsindex {
						vsindex = v;
					}
					let delta = if vsindex == 1 { 80.0 * location } else { 0.0 };
					assert_eq!(
						value,
						[
							PenOp::MoveTo((100.0, 100.0)),
							PenOp::LineTo((200.0 + delta, 100.0)),
							PenOp::ClosePath,
						]
					);
				}
			}
		}
	}
	Ok(())
}
