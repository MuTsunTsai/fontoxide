//! Port of fontTools' `Tests/cffLib/specializer_test.py`, the doctest of
//! `Lib/fontTools/cffLib/specializer.py`, and a differential check against vectors made by
//! `tools/reference/specializer_vectors.py`.

#![allow(clippy::type_complexity)]

use std::path::PathBuf;

use fontoxide_misc::transform::Transform;
use fontoxide_pens::{Pen, Point};
use fontoxide_ttlib::cff::charstring::{Context, Interpreter, T2CharString, Token};
use fontoxide_ttlib::cff::font::num_regions;
use fontoxide_ttlib::cff::specializer::{
	Arg, Command, NumRegions, SpecializeOptions, categorize_vector, commands_to_program,
	generalize_commands, generalize_program, program_to_commands, specialize_commands,
	specialize_program,
};
use fontoxide_ttlib::tables::Cff2;
use fontoxide_ttlib::{Error, Font, Result};

/// `maxStackLimit`.
const MAX_STACK: i64 = 513;

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
				// `Token::Op` needs a name that lasts. These are only a few.
				Token::Op(Box::leak(token.to_owned().into_boxed_str()))
			}
		})
		.collect()
}

/// `str(x)` of a float.
fn float_to_string(v: f64) -> String {
	format!("{v:?}")
}

/// `programToString`.
fn program_to_string(program: &[Token]) -> String {
	program
		.iter()
		.map(|token| match token {
			Token::Int(v) => v.to_string(),
			Token::Float(v) => float_to_string(*v),
			Token::Op(v) => (*v).to_owned(),
			Token::Mask(v) => format!("{v:?}"),
		})
		.collect::<Vec<_>>()
		.join(" ")
}

fn charstr_generalize(charstr: &str) -> Result<String> {
	Ok(program_to_string(&generalize_program(
		&string_to_program(charstr),
		None,
		0,
	)?))
}

fn charstr_specialize(charstr: &str, get_num_regions: Option<NumRegions<'_>>) -> Result<String> {
	Ok(program_to_string(&specialize_program(
		&string_to_program(charstr),
		get_num_regions,
		&SpecializeOptions::default(),
	)?))
}

fn int(v: i64) -> Token {
	Token::Int(v)
}

fn op(v: &'static str) -> Token {
	Token::Op(v)
}

fn program_of(items: &[Token]) -> Vec<Token> {
	items.to_vec()
}

fn is_same_token(a: &Token, b: &Token) -> bool {
	let value = |t: &Token| match t {
		Token::Int(v) => Some(*v as f64),
		Token::Float(v) => Some(*v),
		_ => None,
	};
	match (value(a), value(b)) {
		(Some(x), Some(y)) => x == y,
		_ => a == b,
	}
}

/// `program == program`: `1 == 1.0`.
fn assert_same_program(a: &[Token], b: &[Token], what: &str) {
	assert!(
		a.len() == b.len() && a.iter().zip(b).all(|(x, y)| is_same_token(x, y)),
		"{what}: {} != {}",
		program_to_string(a),
		program_to_string(b)
	);
}

/// `program_stack_use`.
fn program_stack_use(program: &[Token], num_regions: i64) -> usize {
	let mut max_stack = 0;
	let mut stack = Vec::new();
	for token in program {
		match token {
			Token::Op("blend") => {
				let Some(Token::Int(num_blends)) = stack.last() else {
					panic!("no number of blends");
				};
				let num_operands_to_pop = (1 + num_blends * num_regions) as usize;
				stack.truncate(stack.len().saturating_sub(num_operands_to_pop));
			}
			Token::Op(_) => stack.clear(),
			token => {
				stack.push(token.clone());
				max_stack = max_stack.max(stack.len());
			}
		}
	}
	max_stack
}

const NONE_ARGUMENTS: &[&str] = &[
	"rmoveto",
	"hmoveto",
	"vmoveto",
	"rlineto",
	"hlineto",
	"vlineto",
	"rrcurveto",
	"hhcurveto",
	"vvcurveto",
	"hvcurveto",
	"vhcurveto",
	"rcurveline",
	"rlinecurve",
];

// `CFFGeneralizeProgramTest`.

#[test]
fn generalize_test_raises() {
	for charstr in NONE_ARGUMENTS {
		assert!(
			matches!(charstr_generalize(charstr), Err(Error::Value(_))),
			"Expected to raise ValueError, but didn't: {charstr}"
		);
	}
}

#[test]
fn generalize_test_generalize() -> Result<()> {
	let thrice_rlineto = "0 0 rlineto 0 0 rlineto 0 0 rlineto";
	let cases: &[(&str, Option<&str>)] = &[
		// rmoveto
		("0 0 rmoveto", None),
		("100 0 0 rmoveto", None),
		(".55 -.8 rmoveto", Some("0.55 -0.8 rmoveto")),
		("100.5 50 -5.8 rmoveto", None),
		// hmoveto
		("0 hmoveto", Some("0 0 rmoveto")),
		("100 0 hmoveto", Some("100 0 0 rmoveto")),
		(".67 hmoveto", Some("0.67 0 rmoveto")),
		("100 -70 hmoveto", Some("100 -70 0 rmoveto")),
		// vmoveto
		("0 vmoveto", Some("0 0 rmoveto")),
		("100 0 vmoveto", Some("100 0 0 rmoveto")),
		("-.24 vmoveto", Some("0 -0.24 rmoveto")),
		("100 44 vmoveto", Some("100 0 44 rmoveto")),
		// rlineto
		("0 0 rlineto", None),
		("0 0 0 0 0 0 rlineto", Some(thrice_rlineto)),
		(".55 -.8 rlineto", Some("0.55 -0.8 rlineto")),
		(
			".55 -.8 .55 -.8 .55 -.8 rlineto",
			Some("0.55 -0.8 rlineto 0.55 -0.8 rlineto 0.55 -0.8 rlineto"),
		),
		// hlineto
		("0 hlineto", Some("0 0 rlineto")),
		(
			"0 0 0 0 hlineto",
			Some("0 0 rlineto 0 0 rlineto 0 0 rlineto 0 0 rlineto"),
		),
		(".67 hlineto", Some("0.67 0 rlineto")),
		(
			".67 -6.0 .67 hlineto",
			Some("0.67 0 rlineto 0 -6.0 rlineto 0.67 0 rlineto"),
		),
		// vlineto
		("0 vlineto", Some("0 0 rlineto")),
		("0 0 0 vlineto", Some(thrice_rlineto)),
		("-.24 vlineto", Some("0 -0.24 rlineto")),
		(
			"-.24 +50 30 -4 vlineto",
			Some("0 -0.24 rlineto 50 0 rlineto 0 30 rlineto -4 0 rlineto"),
		),
		// rrcurveto
		("-1 56 -2 57 -1 57 rrcurveto", None),
		(
			"-30 8 -36 15 -37 22 44 54 31 61 22 68 rrcurveto",
			Some("-30 8 -36 15 -37 22 rrcurveto 44 54 31 61 22 68 rrcurveto"),
		),
		("1 2 3 4 5 0 rrcurveto", None),
		(
			"0 10 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("0 10 1 2 0 0 rrcurveto 0 0 1 2 0 1 rrcurveto 0 1 3 4 0 0 rrcurveto"),
		),
		(
			"10 0 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("10 0 1 2 0 0 rrcurveto 0 0 1 2 0 1 rrcurveto 0 1 3 4 0 0 rrcurveto"),
		),
		(
			"0 0 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("0 0 1 2 0 0 rrcurveto 0 0 1 2 0 1 rrcurveto 0 1 3 4 0 0 rrcurveto"),
		),
		(
			"10 10 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("10 10 1 2 0 0 rrcurveto 0 0 1 2 0 1 rrcurveto 0 1 3 4 0 0 rrcurveto"),
		),
		(
			"0 10 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("0 10 1 2 0 0 rrcurveto 0 0 1 2 1 0 rrcurveto 1 0 3 4 0 0 rrcurveto"),
		),
		(
			"10 0 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("10 0 1 2 0 0 rrcurveto 0 0 1 2 1 0 rrcurveto 1 0 3 4 0 0 rrcurveto"),
		),
		(
			"0 0 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("0 0 1 2 0 0 rrcurveto 0 0 1 2 1 0 rrcurveto 1 0 3 4 0 0 rrcurveto"),
		),
		(
			"10 10 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("10 10 1 2 0 0 rrcurveto 0 0 1 2 1 0 rrcurveto 1 0 3 4 0 0 rrcurveto"),
		),
		// hhcurveto
		("10 30 0 10 hhcurveto", Some("10 0 30 0 10 0 rrcurveto")),
		(
			"40 -38 -60 41 -91 hhcurveto",
			Some("-38 40 -60 41 -91 0 rrcurveto"),
		),
		(
			"43 23 25 18 29 56 42 -84 hhcurveto",
			Some("43 0 23 25 18 0 rrcurveto 29 0 56 42 -84 0 rrcurveto"),
		),
		(
			"43 23 25 18 29 56 42 -84 79 hhcurveto",
			Some("23 43 25 18 29 0 rrcurveto 56 0 42 -84 79 0 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 hhcurveto",
			Some("1 0 2 3 4 0 rrcurveto 5 0 6 7 8 0 rrcurveto 9 0 10 11 12 0 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 13 hhcurveto",
			Some("2 1 3 4 5 0 rrcurveto 6 0 7 8 9 0 rrcurveto 10 0 11 12 13 0 rrcurveto"),
		),
		// vvcurveto
		("61 6 52 68 vvcurveto", Some("0 61 6 52 0 68 rrcurveto")),
		(
			"61 38 35 56 72 vvcurveto",
			Some("61 38 35 56 0 72 rrcurveto"),
		),
		(
			"-84 -88 -30 -90 -13 19 23 -11 vvcurveto",
			Some("0 -84 -88 -30 0 -90 rrcurveto 0 -13 19 23 0 -11 rrcurveto"),
		),
		(
			"43 12 17 32 65 68 -6 52 61 vvcurveto",
			Some("43 12 17 32 0 65 rrcurveto 0 68 -6 52 0 61 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 vvcurveto",
			Some("0 1 2 3 0 4 rrcurveto 0 5 6 7 0 8 rrcurveto 0 9 10 11 0 12 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 13 vvcurveto",
			Some("1 2 3 4 0 5 rrcurveto 0 6 7 8 0 9 rrcurveto 0 10 11 12 0 13 rrcurveto"),
		),
		// hvcurveto
		("1 2 3 4 hvcurveto", Some("1 0 2 3 0 4 rrcurveto")),
		(
			"57 44 22 40 34 hvcurveto",
			Some("57 0 44 22 34 40 rrcurveto"),
		),
		(
			"65 33 -19 -45 -45 -29 -25 -71 hvcurveto",
			Some("65 0 33 -19 0 -45 rrcurveto 0 -45 -29 -25 -71 0 rrcurveto"),
		),
		(
			"97 69 41 86 58 -36 34 -64 11 hvcurveto",
			Some("97 0 69 41 0 86 rrcurveto 0 58 -36 34 -64 11 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 hvcurveto",
			Some("1 0 2 3 0 4 rrcurveto 0 5 6 7 8 0 rrcurveto 9 0 10 11 0 12 rrcurveto"),
		),
		(
			"-124 -79 104 165 163 82 102 124 56 43 -25 -37 35 hvcurveto",
			Some(
				"-124 0 -79 104 0 165 rrcurveto 0 163 82 102 124 0 rrcurveto 56 0 43 -25 35 -37 rrcurveto",
			),
		),
		(
			"32 25 22 32 31 -25 22 -32 -32 -25 -22 -31 -32 25 -22 32 hvcurveto",
			Some(
				"32 0 25 22 0 32 rrcurveto 0 31 -25 22 -32 0 rrcurveto -32 0 -25 -22 0 -31 rrcurveto 0 -32 25 -22 32 0 rrcurveto",
			),
		),
		(
			"-170 -128 111 195 234 172 151 178 182 95 -118 -161 -130 -71 -77 -63 -55 -19 38 79 20 hvcurveto",
			Some(
				"-170 0 -128 111 0 195 rrcurveto 0 234 172 151 178 0 rrcurveto 182 0 95 -118 0 -161 rrcurveto 0 -130 -71 -77 -63 0 rrcurveto -55 0 -19 38 20 79 rrcurveto",
			),
		),
		// vhcurveto
		(
			"-57 43 -30 53 vhcurveto",
			Some("0 -57 43 -30 53 0 rrcurveto"),
		),
		(
			"41 -27 19 -46 11 vhcurveto",
			Some("0 41 -27 19 -46 11 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 vhcurveto",
			Some("0 1 2 3 4 0 rrcurveto 5 0 6 7 0 8 rrcurveto"),
		),
		(
			"-64 -23 -25 -45 -30 -24 14 33 -19 vhcurveto",
			Some("0 -64 -23 -25 -45 0 rrcurveto -30 0 -24 14 -19 33 rrcurveto"),
		),
		(
			"1 2 3 4 5 6 7 8 9 10 11 12 vhcurveto",
			Some("0 1 2 3 4 0 rrcurveto 5 0 6 7 0 8 rrcurveto 0 9 10 11 12 0 rrcurveto"),
		),
		(
			"108 59 81 98 99 59 -81 -108 -100 -46 -66 -63 -47 vhcurveto",
			Some(
				"0 108 59 81 98 0 rrcurveto 99 0 59 -81 0 -108 rrcurveto 0 -100 -46 -66 -63 -47 rrcurveto",
			),
		),
		(
			"60 -26 37 -43 -33 -28 -22 -36 -37 27 -20 32 3 4 0 1 3 vhcurveto",
			Some(
				"0 60 -26 37 -43 0 rrcurveto -33 0 -28 -22 0 -36 rrcurveto 0 -37 27 -20 32 0 rrcurveto 3 0 4 0 3 1 rrcurveto",
			),
		),
		// rcurveline
		(
			"21 -76 21 -72 24 -73 31 -100 rcurveline",
			Some("21 -76 21 -72 24 -73 rrcurveto 31 -100 rlineto"),
		),
		(
			"-73 80 -80 121 -49 96 60 65 55 41 54 17 -8 78 rcurveline",
			Some("-73 80 -80 121 -49 96 rrcurveto 60 65 55 41 54 17 rrcurveto -8 78 rlineto"),
		),
		(
			"1 64 10 51 29 39 15 21 15 20 15 18 47 -89 63 -98 52 -59 91 8 rcurveline",
			Some(
				"1 64 10 51 29 39 rrcurveto 15 21 15 20 15 18 rrcurveto 47 -89 63 -98 52 -59 rrcurveto 91 8 rlineto",
			),
		),
		(
			"1 64 10 51 29 39 15 21 15 20 15 18 46 -88 63 -97 52 -59 -38 -57 -49 -62 -52 -54 96 -8 rcurveline",
			Some(
				"1 64 10 51 29 39 rrcurveto 15 21 15 20 15 18 rrcurveto 46 -88 63 -97 52 -59 rrcurveto -38 -57 -49 -62 -52 -54 rrcurveto 96 -8 rlineto",
			),
		),
		// rlinecurve
		(
			"21 -76 21 -72 24 -73 31 -100 rlinecurve",
			Some("21 -76 rlineto 21 -72 24 -73 31 -100 rrcurveto"),
		),
		(
			"-73 80 -80 121 -49 96 60 65 55 41 rlinecurve",
			Some("-73 80 rlineto -80 121 rlineto -49 96 60 65 55 41 rrcurveto"),
		),
		(
			"1 64 10 51 29 39 15 21 15 20 15 18 rlinecurve",
			Some("1 64 rlineto 10 51 rlineto 29 39 rlineto 15 21 15 20 15 18 rrcurveto"),
		),
		(
			"1 64 10 51 29 39 15 21 15 20 15 18 46 -88 rlinecurve",
			Some(
				"1 64 rlineto 10 51 rlineto 29 39 rlineto 15 21 rlineto 15 20 15 18 46 -88 rrcurveto",
			),
		),
		// hstem/vstem
		(
			"95 0 58 542 60 hstem 89 65 344 67 vstem 89 45 rmoveto",
			None,
		),
		// hstemhm/vstemhm
		(
			"-16 577 60 24 60 hstemhm 98 55 236 55 vstemhm 343 577 rmoveto",
			None,
		),
		// hintmask/cntrmask
		(
			"52 80 153 61 4 83 -71.5 71.5 hintmask 11011100 94 119 216 119 216 119 cntrmask 1110000 154 -12 rmoveto",
			None,
		),
		// endchar
		("-255 319 rmoveto 266 57 rlineto endchar", None),
		// xtra
		("-255 319 rmoveto 266 57 rlineto xtra 90 34", None),
	];
	for (charstr, expected) in cases {
		let expected = expected.unwrap_or(charstr).trim();
		assert_eq!(charstr_generalize(charstr)?, expected, "{charstr}");
	}
	Ok(())
}

// `CFFSpecializeProgramTest`.

#[test]
fn specialize_test_raises() {
	for charstr in NONE_ARGUMENTS {
		assert!(
			matches!(charstr_specialize(charstr, None), Err(Error::Value(_))),
			"Expected to raise ValueError, but didn't: {charstr}"
		);
	}
}

#[test]
fn specialize_test_specialize() -> Result<()> {
	let thrice_zero_rmoveto = "0 0 rmoveto 0 0 rmoveto 0 0 rmoveto";
	let thrice_zero_rlineto = "0 0 rlineto 0 0 rlineto 0 0 rlineto";
	let cases: &[(&str, Option<&str>)] = &[
		// rmoveto
		("0 0 rmoveto", Some("0 hmoveto")),
		(thrice_zero_rmoveto, Some("0 hmoveto")),
		("100 0 0 rmoveto", Some("100 0 hmoveto")),
		(".55 -.8 rmoveto", Some("0.55 -0.8 rmoveto")),
		(
			"55 -8 rmoveto 55 -8 rmoveto 55 -8 rmoveto",
			Some("165 -24 rmoveto"),
		),
		(
			"0.55 -0.8 rmoveto 10 20 1 blend 30 rmoveto",
			Some("10.55 20 1 blend 29.2 rmoveto"),
		),
		("100.5 50 -5.8 rmoveto", None),
		// rlineto
		("0 0 rlineto", Some("")),
		(thrice_zero_rlineto, Some("")),
		(".55 -.8 rlineto", Some("0.55 -0.8 rlineto")),
		(
			".55 -.8 rlineto .55 -.8 rlineto .55 -.8 rlineto",
			Some("0.55 -0.8 0.55 -0.8 0.55 -0.8 rlineto"),
		),
		(".67 0 rlineto", Some("0.67 hlineto")),
		(
			"62 0 rlineto 62 0 rlineto 62 0 rlineto",
			Some("186 hlineto"),
		),
		(
			".67 0 rlineto 0 -6.0 rlineto .67 0 rlineto",
			Some("0.67 -6.0 0.67 hlineto"),
		),
		("0 -.24 rlineto", Some("-0.24 vlineto")),
		(
			"0 -24 rlineto 0 -24 rlineto 0 -24 rlineto",
			Some("-72 vlineto"),
		),
		(
			"0 -.24 rlineto +50 0 rlineto 0 30 rlineto -4 0 rlineto",
			Some("-0.24 50 30 -4 vlineto"),
		),
		("1 2 0 0 3 4 rlineto", Some("1 2 3 4 rlineto")),
		("1 2 5 0 3 4 rlineto", None),
		("1 2 0 5 3 4 rlineto", None),
		// rrcurveto
		("-1 56 -2 57 -1 57 rrcurveto", None),
		(
			"-30 8 -36 15 -37 22 rrcurveto 44 54 31 61 22 68 rrcurveto",
			Some("-30 8 -36 15 -37 22 44 54 31 61 22 68 rrcurveto"),
		),
		("1 2 3 4 5 0 rrcurveto", Some("2 1 3 4 5 hhcurveto")),
		("10 0 30 0 10 0 rrcurveto", Some("10 30 0 10 hhcurveto")),
		(
			"-38 40 -60 41 -91 0 rrcurveto",
			Some("40 -38 -60 41 -91 hhcurveto"),
		),
		(
			"43 0 23 25 18 0 rrcurveto 29 0 56 42 -84 0 rrcurveto",
			Some("43 23 25 18 29 56 42 -84 hhcurveto"),
		),
		(
			"23 43 25 18 29 0 rrcurveto 56 0 42 -84 79 0 rrcurveto",
			Some("43 23 25 18 29 56 42 -84 79 hhcurveto"),
		),
		(
			"1 0 2 3 4 0 rrcurveto 5 0 6 7 8 0 rrcurveto 9 0 10 11 12 0 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 hhcurveto"),
		),
		(
			"2 1 3 4 5 0 rrcurveto 6 0 7 8 9 0 rrcurveto 10 0 11 12 13 0 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 13 hhcurveto"),
		),
		("0 61 6 52 0 68 rrcurveto", Some("61 6 52 68 vvcurveto")),
		(
			"61 38 35 56 0 72 rrcurveto",
			Some("61 38 35 56 72 vvcurveto"),
		),
		(
			"0 -84 -88 -30 0 -90 rrcurveto 0 -13 19 23 0 -11 rrcurveto",
			Some("-84 -88 -30 -90 -13 19 23 -11 vvcurveto"),
		),
		(
			"43 12 17 32 0 65 rrcurveto 0 68 -6 52 0 61 rrcurveto",
			Some("43 12 17 32 65 68 -6 52 61 vvcurveto"),
		),
		(
			"0 1 2 3 0 4 rrcurveto 0 5 6 7 0 8 rrcurveto 0 9 10 11 0 12 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 vvcurveto"),
		),
		(
			"1 2 3 4 0 5 rrcurveto 0 6 7 8 0 9 rrcurveto 0 10 11 12 0 13 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 13 vvcurveto"),
		),
		("1 0 2 3 0 4 rrcurveto", Some("1 2 3 4 hvcurveto")),
		(
			"57 0 44 22 34 40 rrcurveto",
			Some("57 44 22 40 34 hvcurveto"),
		),
		(
			"65 0 33 -19 0 -45 rrcurveto 0 -45 -29 -25 -71 0 rrcurveto",
			Some("65 33 -19 -45 -45 -29 -25 -71 hvcurveto"),
		),
		(
			"97 0 69 41 0 86 rrcurveto 0 58 -36 34 -64 11 rrcurveto",
			Some("97 69 41 86 58 -36 34 -64 11 hvcurveto"),
		),
		(
			"1 0 2 3 0 4 rrcurveto 0 5 6 7 8 0 rrcurveto 9 0 10 11 0 12 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 hvcurveto"),
		),
		(
			"-124 0 -79 104 0 165 rrcurveto 0 163 82 102 124 0 rrcurveto 56 0 43 -25 35 -37 rrcurveto",
			Some("-124 -79 104 165 163 82 102 124 56 43 -25 -37 35 hvcurveto"),
		),
		(
			"32 0 25 22 0 32 rrcurveto 0 31 -25 22 -32 0 rrcurveto -32 0 -25 -22 0 -31 rrcurveto 0 -32 25 -22 32 0 rrcurveto",
			Some("32 25 22 32 31 -25 22 -32 -32 -25 -22 -31 -32 25 -22 32 hvcurveto"),
		),
		(
			"-170 0 -128 111 0 195 rrcurveto 0 234 172 151 178 0 rrcurveto 182 0 95 -118 0 -161 rrcurveto 0 -130 -71 -77 -63 0 rrcurveto -55 0 -19 38 20 79 rrcurveto",
			Some(
				"-170 -128 111 195 234 172 151 178 182 95 -118 -161 -130 -71 -77 -63 -55 -19 38 79 20 hvcurveto",
			),
		),
		(
			"0 -57 43 -30 53 0 rrcurveto",
			Some("-57 43 -30 53 vhcurveto"),
		),
		(
			"0 41 -27 19 -46 11 rrcurveto",
			Some("41 -27 19 -46 11 vhcurveto"),
		),
		(
			"0 1 2 3 4 0 rrcurveto 5 0 6 7 0 8 rrcurveto",
			Some("1 2 3 4 5 6 7 8 vhcurveto"),
		),
		(
			"0 -64 -23 -25 -45 0 rrcurveto -30 0 -24 14 -19 33 rrcurveto",
			Some("-64 -23 -25 -45 -30 -24 14 33 -19 vhcurveto"),
		),
		(
			"0 1 2 3 4 0 rrcurveto 5 0 6 7 0 8 rrcurveto 0 9 10 11 12 0 rrcurveto",
			Some("1 2 3 4 5 6 7 8 9 10 11 12 vhcurveto"),
		),
		(
			"0 108 59 81 98 0 rrcurveto 99 0 59 -81 0 -108 rrcurveto 0 -100 -46 -66 -63 -47 rrcurveto",
			Some("108 59 81 98 99 59 -81 -108 -100 -46 -66 -63 -47 vhcurveto"),
		),
		(
			"0 60 -26 37 -43 0 rrcurveto -33 0 -28 -22 0 -36 rrcurveto 0 -37 27 -20 32 0 rrcurveto 3 0 4 0 3 1 rrcurveto",
			Some("60 -26 37 -43 -33 -28 -22 -36 -37 27 -20 32 3 4 0 1 3 vhcurveto"),
		),
		(
			"0 10 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("10 1 2 0 0 1 2 1 1 3 4 0 vhcurveto"),
		),
		(
			"10 0 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("10 1 2 0 hhcurveto 0 1 2 1 1 3 4 0 hvcurveto"),
		),
		(
			"0 0 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("1 2 rlineto 0 1 2 1 1 3 4 0 hvcurveto"),
		),
		(
			"10 10 1 2 0 0 0 0 1 2 0 1 0 1 3 4 0 0 rrcurveto",
			Some("10 10 1 2 0 0 1 2 1 1 3 4 0 vvcurveto"),
		),
		(
			"0 10 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("10 1 2 0 vhcurveto 0 1 2 1 1 3 4 0 hhcurveto"),
		),
		(
			"10 0 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("10 1 2 0 0 1 2 1 1 3 4 0 hhcurveto"),
		),
		(
			"0 0 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("1 2 rlineto 0 1 2 1 1 3 4 0 hhcurveto"),
		),
		(
			"10 10 1 2 0 0 0 0 1 2 1 0 1 0 3 4 0 0 rrcurveto",
			Some("10 10 1 2 0 0 1 2 1 1 3 4 0 hhcurveto"),
		),
		("1 2 3 4 5 6 1 2 3 4 5 0 1 2 3 4 5 6 rrcurveto", None),
		("1 2 3 4 5 6 1 2 3 4 0 6 1 2 3 4 5 6 rrcurveto", None),
		("1 2 3 4 5 6 1 0 3 4 5 6 1 2 3 4 5 6 rrcurveto", None),
		("1 2 3 4 5 6 0 2 3 4 5 6 1 2 3 4 5 6 rrcurveto", None),
		(
			"21 -76 21 -72 24 -73 rrcurveto 31 -100 rlineto",
			Some("21 -76 21 -72 24 -73 31 -100 rcurveline"),
		),
		(
			"-73 80 -80 121 -49 96 rrcurveto 60 65 55 41 54 17 rrcurveto -8 78 rlineto",
			Some("-73 80 -80 121 -49 96 60 65 55 41 54 17 -8 78 rcurveline"),
		),
		(
			"1 64 10 51 29 39 rrcurveto 15 21 15 20 15 18 rrcurveto 47 -89 63 -98 52 -59 rrcurveto 91 8 rlineto",
			Some("1 64 10 51 29 39 15 21 15 20 15 18 47 -89 63 -98 52 -59 91 8 rcurveline"),
		),
		(
			"21 -76 rlineto 21 -72 24 -73 31 -100 rrcurveto",
			Some("21 -76 21 -72 24 -73 31 -100 rlinecurve"),
		),
		(
			"-73 80 rlineto -80 121 rlineto -49 96 60 65 55 41 rrcurveto",
			Some("-73 80 -80 121 -49 96 60 65 55 41 rlinecurve"),
		),
		(
			"1 64 rlineto 10 51 rlineto 29 39 rlineto 15 21 15 20 15 18 rrcurveto",
			Some("1 64 10 51 29 39 15 21 15 20 15 18 rlinecurve"),
		),
	];
	let get_num_regions = |_: i64| Ok(2);
	for (charstr, expected) in cases {
		let expected = expected.unwrap_or(charstr).trim();
		assert_eq!(
			charstr_specialize(charstr, Some(&get_num_regions))?,
			expected,
			"{charstr}"
		);
	}
	Ok(())
}

/// maxstack CFF=48, specializer uses up to 47.
#[test]
fn test_maxstack() -> Result<()> {
	let operands = "1 2 3 4 5 6 ";
	let operator = "rrcurveto ";
	let charstr = format!("{operands}{operator}").repeat(9);
	let expected = format!(
		"{}{operator}{}{operator}",
		operands.repeat(2),
		operands.repeat(7)
	);
	assert_eq!(charstr_specialize(&charstr, None)?, expected.trim_end());
	Ok(())
}

fn deep_blends(num_regions: i64, num_ops: usize) -> Vec<Token> {
	let mut blend_one: Vec<Token> = (0..=num_regions).map(Token::Int).collect();
	blend_one.extend([int(1), op("blend")]);
	let mut program = Vec::new();
	for _ in 0..num_ops {
		for _ in 0..6 {
			program.extend_from_slice(&blend_one);
		}
		program.push(op("rrcurveto"));
	}
	program
}

fn specialize_without_generalizing(program: &[Token], regions: i64) -> Result<Vec<Token>> {
	specialize_program(
		program,
		Some(&|_| Ok(regions)),
		&SpecializeOptions {
			max_stack: MAX_STACK,
			generalize_first: false,
			..Default::default()
		},
	)
}

/// maxstack CFF2=513, specializer uses up to 512.
#[test]
fn test_maxstack_blends() -> Result<()> {
	let num_regions = 15;
	let specialized = specialize_without_generalizing(&deep_blends(num_regions, 600), num_regions)?;
	let stack_use = program_stack_use(&specialized, num_regions) as i64;
	assert!(
		MAX_STACK - num_regions < stack_use && stack_use < MAX_STACK,
		"{stack_use}"
	);
	Ok(())
}

/// See if two commands with deep blends are merged into one.
#[test]
fn test_maxstack_commands() -> Result<()> {
	let num_regions = 400;
	let specialized = specialize_without_generalizing(&deep_blends(num_regions, 2), num_regions)?;
	assert_eq!(
		specialized.iter().position(|t| *t == op("rrcurveto")),
		Some(specialized.len() - 1)
	);
	Ok(())
}

// `CFF2VFTestSpecialize`.

fn cff2_font(name: &str) -> Result<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/cffLib/data")
		.join(name);
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path).expect("a test font"))?;
	Ok(font)
}

#[test]
fn test_blend_round_trip() -> Result<()> {
	let mut font = cff2_font("TestSparseCFF2VF.ttx")?;
	let order = font.glyph_order()?;
	let cff = &mut font.require::<Cff2>()?.cff;
	cff.decompile_all_char_strings()?;
	let top = &cff.top_dicts[0];
	let char_strings = top.char_strings.as_ref().expect("CharStrings");
	let store = top.var_store.as_deref();
	let get_num_regions = |vs: i64| num_regions(store, vs);
	let names = order.names().to_vec();
	assert!(!names.is_empty());
	for name in &names {
		let index = char_strings.index(name).expect("a glyph");
		let program = &char_strings.items[index].program;
		let cmds = program_to_commands(program, Some(&get_num_regions), 0)?;
		let cmds_g = generalize_commands(&cmds, false, None, 0)?;
		let cmds = specialize_commands(
			&cmds_g,
			&SpecializeOptions {
				generalize_first: false,
				..Default::default()
			},
		)?;
		let new_program = commands_to_program(&cmds)?;
		assert_same_program(&new_program, program, name);
		let new_program = specialize_program(
			&new_program,
			Some(&get_num_regions),
			&SpecializeOptions::default(),
		)?;
		assert_same_program(&new_program, program, name);
		let program_g = generalize_program(&new_program, Some(&get_num_regions), 0)?;
		let new_program = commands_to_program(&cmds_g)?;
		assert_same_program(&new_program, &program_g, name);
	}
	Ok(())
}

#[test]
fn test_blend_program_to_commands() -> Result<()> {
	let mut font = cff2_font("TestCFF2Widths.ttx")?;
	let order = font.glyph_order()?;
	let cff = &mut font.require::<Cff2>()?.cff;
	cff.decompile_all_char_strings()?;
	let top = &cff.top_dicts[0];
	let char_strings = top.char_strings.as_ref().expect("CharStrings");
	let store = top.var_store.as_deref();
	let get_num_regions = |vs: i64| num_regions(store, vs);
	let names = order.names().to_vec();
	assert!(!names.is_empty());
	for name in &names {
		let index = char_strings.index(name).expect("a glyph");
		let program = &char_strings.items[index].program;
		let cmds = program_to_commands(program, Some(&get_num_regions), 0)?;
		assert_same_program(&commands_to_program(&cmds)?, program, name);
	}
	Ok(())
}

// The module level tests.

fn zero_region(_: i64) -> Result<i64> {
	Ok(0)
}

fn both_conversions(
	program: &[Token],
	get_num_regions: NumRegions<'_>,
	vs_index: i64,
) -> Result<[Vec<Token>; 2]> {
	Ok([
		generalize_program(program, Some(get_num_regions), vs_index)?,
		specialize_program(
			program,
			Some(get_num_regions),
			&SpecializeOptions {
				vs_index,
				..Default::default()
			},
		)?,
	])
}

#[test]
fn test_zero_region_blend() -> Result<()> {
	let program = program_of(&[
		int(100),
		int(200),
		int(1),
		op("blend"),
		op("rmoveto"),
		int(100),
		int(0),
		op("rlineto"),
	]);
	for result in both_conversions(&program, &zero_region, 0)? {
		assert!(!result.contains(&op("blend")));
		assert_eq!(
			generalize_program(&result, None, 0)?,
			[
				int(100),
				int(200),
				op("rmoveto"),
				int(100),
				int(0),
				op("rlineto")
			]
		);
	}
	Ok(())
}

#[test]
fn test_nested_zero_region_blend() -> Result<()> {
	let program = program_of(&[
		int(100),
		int(200),
		int(1),
		op("blend"),
		int(2),
		op("blend"),
		op("rmoveto"),
	]);
	for result in both_conversions(&program, &zero_region, 0)? {
		assert_eq!(
			generalize_program(&result, None, 0)?,
			[int(100), int(200), op("rmoveto")]
		);
	}
	Ok(())
}

fn ints(values: &[i64]) -> Vec<Arg> {
	values.iter().map(|v| Arg::Int(*v)).collect()
}

fn blend(values: &[i64]) -> Arg {
	Arg::Blend(ints(values))
}

fn command(op: &'static str, args: Vec<Arg>) -> Command {
	Command::new(op, args)
}

/// `numRegions = {0: 1, 1: 2}.get`.
fn regions_by_vs_index(vs: i64) -> Result<i64> {
	match vs {
		0 => Ok(1),
		1 => Ok(2),
		_ => Err(Error::ttlib("TypeError: None")),
	}
}

#[test]
fn test_program_to_commands_vs_index() -> Result<()> {
	// VarData 1 has two regions; the Private DICT selects it.
	let program = string_to_program("100 200 1 2 3 4 2 blend rmoveto");
	let commands = program_to_commands(&program, Some(&regions_by_vs_index), 1)?;
	assert_eq!(
		commands,
		[command("rmoveto", vec![blend(&[100, 200, 1, 2, 3, 4, 2])])]
	);
	Ok(())
}

#[test]
fn test_convert_program_vs_index() -> Result<()> {
	let program = string_to_program("100 200 1 2 3 4 2 blend rmoveto");
	let [generalized, specialized] = both_conversions(&program, &regions_by_vs_index, 1)?;
	assert_eq!(
		generalized,
		string_to_program("100 1 2 1 blend 200 3 4 1 blend rmoveto")
	);
	assert_eq!(
		specialized,
		string_to_program("100 200 1 2 3 4 2 blend rmoveto")
	);
	Ok(())
}

#[test]
fn test_initial_separate_blends_are_not_width() -> Result<()> {
	// Two separate blends before the first stem op must not make the operand count odd and
	// turn the first stem edge into a width.
	let program = string_to_program("101.25 3 2 -1 1 blend -21 60.5 -2 1 3 1 blend 31.75 hstemhm");
	let commands = program_to_commands(&program, Some(&|_| Ok(3)), 0)?;
	assert_eq!(
		commands,
		[command(
			"hstemhm",
			vec![
				Arg::Blend(vec![
					Arg::Float(101.25),
					Arg::Int(3),
					Arg::Int(2),
					Arg::Int(-1),
					Arg::Int(1)
				]),
				Arg::Int(-21),
				Arg::Blend(vec![
					Arg::Float(60.5),
					Arg::Int(-2),
					Arg::Int(1),
					Arg::Int(3),
					Arg::Int(1)
				]),
				Arg::Float(31.75),
			]
		)]
	);
	assert_eq!(commands_to_program(&commands)?, program);
	Ok(())
}

/// A pen that records what is drawn (`RecordingPen`).
#[derive(Default)]
struct RecordingPen {
	value: Vec<(&'static str, Vec<Point>)>,
}

impl Pen for RecordingPen {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(("moveTo", vec![pt]));
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(("lineTo", vec![pt]));
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		self.value.push(("curveTo", points.to_vec()));
		Ok(())
	}

	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		let mut points = off_curves.to_vec();
		points.extend(on_curve);
		self.value.push(("qCurveTo", points));
		Ok(())
	}

	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(("closePath", Vec::new()));
		Ok(())
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(("endPath", Vec::new()));
		Ok(())
	}

	fn add_component(
		&mut self,
		_glyph_name: &str,
		_transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		self.value.push(("addComponent", Vec::new()));
		Ok(())
	}
}

/// `drawProgram`.
fn draw_program(program: &[Token]) -> Result<Vec<(&'static str, Vec<Point>)>> {
	let mut cs = T2CharString {
		bytecode: None,
		program: program.to_vec(),
		fd_select_index: None,
	};
	cs.program.push(op("endchar"));
	let mut pen = RecordingPen::default();
	let regions = |_: i64| Ok(0);
	Interpreter::new(
		Context {
			local_subrs: &mut [],
			global_subrs: &mut [],
			nominal_width_x: Some(0.0),
			default_width_x: Some(0.0),
			vsindex: None,
			num_regions: &regions,
			blender: None,
		},
		Some(&mut pen),
	)
	.execute(&mut cs)?;
	Ok(pen.value)
}

#[test]
fn test_generalize_and_specialize_flex() -> Result<()> {
	for flex in [
		"10 20 30 0 40 -20 50 20 60 0 70 -20 50 flex",
		"10 20 15 30 40 50 60 hflex",
		"10 5 20 15 30 40 50 -12 60 hflex1",
		"10 5 20 15 30 2 40 -3 50 -12 60 flex1",
		"5 10 15 20 2 30 -3 40 -12 50 60 flex1",
	] {
		let program = string_to_program(&format!("100 100 rmoveto {flex}"));
		let commands = program_to_commands(&program, None, 0)?;
		let op = commands[1].op;

		let generalized = generalize_commands(&commands, false, None, 0)?;
		assert_eq!(generalized[1].op, "flex", "{flex}");
		assert_eq!(generalized[1].args.len(), 13, "{flex}");
		assert_eq!(
			draw_program(&commands_to_program(&generalized)?)?,
			draw_program(&program)?,
			"{flex}"
		);

		// The flex test case can be spelled as a flex1.
		let expected_op = if op == "flex" { "flex1" } else { op };
		let specialized = specialize_commands(
			&generalized,
			&SpecializeOptions {
				generalize_first: false,
				..Default::default()
			},
		)?;
		assert_eq!(specialized[1].op, expected_op, "{flex}");
		assert_eq!(
			draw_program(&commands_to_program(&specialized)?)?,
			draw_program(&program)?,
			"{flex}"
		);
	}
	Ok(())
}

#[test]
fn test_malformed_flex1() -> Result<()> {
	for num_args in [0, 1, 10, 12] {
		let args = vec![Arg::Int(10); num_args];
		let commands = [command("flex1", args.clone())];
		let expected = [
			command("", args.clone()),
			command("", vec![Arg::Op("flex1")]),
		];

		assert!(matches!(
			generalize_commands(&commands, false, None, 0),
			Err(Error::Value(_))
		));
		assert_eq!(generalize_commands(&commands, true, None, 0)?, expected);

		assert!(matches!(
			specialize_commands(&commands, &SpecializeOptions::default()),
			Err(Error::Value(_))
		));
		assert_eq!(
			specialize_commands(
				&commands,
				&SpecializeOptions {
					ignore_errors: true,
					..Default::default()
				}
			)?,
			expected
		);
	}
	Ok(())
}

fn flex1_commands(dx: &Arg, dy: &Arg, regions: i64) -> Result<Vec<Command>> {
	let mut args = vec![dx.clone(), dy.clone()];
	args.extend(ints(&[0; 9]));
	let program = commands_to_program(&[command("flex1", args)])?;
	program_to_commands(&program, Some(&|_| Ok(regions)), 0)
}

fn negate(arg: &Arg) -> Arg {
	match arg {
		Arg::Int(v) => Arg::Int(-v),
		Arg::Blend(values) => {
			let (last, values) = values.split_last().expect("values");
			let mut out: Vec<Arg> = values.iter().map(negate).collect();
			out.push(last.clone());
			Arg::Blend(out)
		}
		arg => panic!("{arg:?}"),
	}
}

#[test]
fn test_flex1_direction() -> Result<()> {
	let cases = [
		(Arg::Int(50), Arg::Int(0), Some(true)),
		(Arg::Int(0), Arg::Int(50), Some(false)),
		(blend(&[50, 10, 1]), blend(&[0, 20, 1]), Some(true)),
		(blend(&[50, 10, -20, 1]), blend(&[0, 20, 20, 1]), None),
		// Horizontal at both ends of the region, vertical half way.
		(blend(&[10, -20, 1]), blend(&[5, -5, 1]), None),
	];
	for (dx, dy, expected) in cases {
		let regions = [&dx, &dy]
			.iter()
			.filter_map(|arg| match arg {
				Arg::Blend(v) => Some(v.len() as i64 - 2),
				_ => None,
			})
			.max()
			.unwrap_or(0);
		let commands = flex1_commands(&dx, &dy, regions)?;
		let generalized = generalize_commands(&commands, false, None, 0)?;
		let [Command { op, args }] = generalized.as_slice() else {
			panic!("{generalized:?}");
		};
		match expected {
			None => assert_eq!(*op, "flex1"),
			Some(horizontal) => {
				assert_eq!(*op, "flex");
				let expected = if horizontal {
					[Arg::Int(0), negate(&dy)]
				} else {
					[negate(&dx), Arg::Int(0)]
				};
				assert!(
					args[10].py_eq(&expected[0]) && args[11].py_eq(&expected[1]),
					"{args:?} {expected:?}"
				);
			}
		}
	}
	Ok(())
}

#[test]
fn test_flex1_direction_with_scalar_ranges() -> Result<()> {
	let cases: [(&[(f64, f64)], Option<bool>); 4] = [
		(&[(0.0, 1.0)], None),
		// A pinned region: the direction at its scalar.
		(&[(0.5, 0.5)], Some(false)),
		(&[(1.0, 1.0)], Some(true)),
		// A region pinned on some axes only: scalar 0.2 at most.
		(&[(0.0, 0.2)], Some(true)),
	];
	let (dx, dy) = (blend(&[10, -20, 1]), blend(&[5, -5, 1]));
	for (scalar_ranges, expected) in cases {
		let commands = flex1_commands(&dx, &dy, 1)?;
		let generalized =
			generalize_commands(&commands, false, Some(&|_| Ok(scalar_ranges.to_vec())), 0)?;
		assert_eq!(generalized.len(), 1);
		assert_eq!(
			generalized[0].op == "flex",
			expected.is_some(),
			"{scalar_ranges:?}"
		);
	}
	Ok(())
}

#[test]
fn test_generalize_flex_uses_active_vsindex() -> Result<()> {
	let (dx, dy) = (blend(&[10, -20, 1]), blend(&[5, -5, 1]));
	let cases: [(i64, Option<i64>, &str); 4] = [
		(0, None, "flex1"),
		(1, None, "flex"),
		(0, Some(1), "flex"),
		(1, Some(0), "flex1"),
	];
	let scalar_ranges = |vs: i64| -> Result<Vec<(f64, f64)>> {
		Ok(match vs {
			0 => vec![(0.0, 1.0)],
			_ => vec![(1.0, 1.0)],
		})
	};
	for (vs_index, vsindex_command, expected_op) in cases {
		let mut commands: Vec<Command> = vsindex_command
			.iter()
			.map(|v| command("vsindex", vec![Arg::Int(*v)]))
			.collect();
		let vsindex_commands = commands.clone();
		let mut args = vec![dx.clone(), dy.clone()];
		args.extend(ints(&[0; 9]));
		commands.push(command("flex1", args));

		let result = generalize_commands(&commands, false, Some(&scalar_ranges), vs_index)?;

		assert_eq!(result[..result.len() - 1], vsindex_commands);
		assert_eq!(result[result.len() - 1].op, expected_op, "{vs_index}");
	}
	Ok(())
}

/// The doctest of `_categorizeVector`.
#[test]
fn doctest_categorize_vector() -> Result<()> {
	assert_eq!(categorize_vector(&ints(&[0, 0]))?, (b'0', ints(&[0])));
	assert_eq!(categorize_vector(&ints(&[1, 0]))?, (b'h', ints(&[1])));
	assert_eq!(categorize_vector(&ints(&[0, 2]))?, (b'v', ints(&[2])));
	assert_eq!(categorize_vector(&ints(&[1, 2]))?, (b'r', ints(&[1, 2])));
	Ok(())
}

// A differential check with fontTools.

/// One vector, or a description of how it failed.
fn check_vector(line: &str) -> std::result::Result<(), String> {
	let fields: Vec<&str> = line.split('\t').collect();
	let [kind, regions, vs_index, flags, program, expected] = fields[..] else {
		return Err(format!("not a vector: {line}"));
	};
	let regions: Option<Vec<i64>> = (regions != "-").then(|| {
		regions
			.split(',')
			.map(|n| n.parse().expect("a number of regions"))
			.collect()
	});
	let get_num_regions = |vs: i64| -> Result<i64> {
		usize::try_from(vs)
			.ok()
			.and_then(|vs| regions.as_ref()?.get(vs).copied())
			.ok_or_else(|| Error::ttlib("TypeError: None"))
	};
	let get_num_regions_opt: Option<NumRegions<'_>> = regions
		.is_some()
		.then_some(&get_num_regions as NumRegions<'_>);
	let vs_index: i64 = vs_index.parse().expect("a vsIndex");
	let tokens = string_to_program(program);
	let result = if kind == "G" {
		generalize_program(&tokens, get_num_regions_opt, vs_index)
	} else {
		let mut options = SpecializeOptions {
			vs_index,
			..Default::default()
		};
		match flags {
			"t" => options.preserve_topology = true,
			"n" => options.generalize_first = false,
			"m" => options.max_stack = MAX_STACK,
			_ => {}
		}
		specialize_program(&tokens, get_num_regions_opt, &options)
	};
	let actual = match result {
		Ok(program) => program_to_string(&program),
		Err(Error::Value(_)) => "!V".to_owned(),
		Err(_) => "!E".to_owned(),
	};
	if actual == expected {
		Ok(())
	} else {
		Err(format!("{line}\n  actual: {actual}"))
	}
}

fn check_vectors(vectors: &str) {
	let failures: Vec<String> = vectors
		.lines()
		.filter_map(|line| check_vector(line).err())
		.collect();
	assert!(
		failures.is_empty(),
		"{} of {} vectors failed:\n{}",
		failures.len(),
		vectors.lines().count(),
		failures[..failures.len().min(30)].join("\n")
	);
}

#[test]
fn differential_vectors() {
	check_vectors(include_str!("data/specializer_vectors.txt"));
	// More of them, generated elsewhere with a larger count.
	if let Ok(path) = std::env::var("FONTOXIDE_SPECIALIZER_VECTORS") {
		check_vectors(&std::fs::read_to_string(path).expect("the vectors"));
	}
}
