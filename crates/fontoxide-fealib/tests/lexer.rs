//! `Tests/feaLib/lexer_test.py` and `Tests/feaLib/error_test.py`.

use std::path::PathBuf;

use fontoxide_fealib::lexer::{FeatureSource, IncludingLexer, Lexer, Token, TokenType};
use fontoxide_fealib::{Error, FeatureLibLocation, Result};

use TokenType::*;

fn lex(s: &str) -> Result<Vec<(TokenType, Token)>> {
	Ok(Lexer::new(s, Some("test.fea"))
		.tokens()?
		.into_iter()
		.map(|(t, tok, _)| (t, tok))
		.collect())
}

fn s(t: TokenType, v: &str) -> (TokenType, Token) {
	(t, Token::Str(v.to_owned()))
}

fn i(t: TokenType, v: i64) -> (TokenType, Token) {
	(t, Token::Int(v))
}

fn err_contains(result: Result<impl std::fmt::Debug>, text: &str) {
	match result {
		Ok(v) => panic!("expected an error containing {text:?}, got {v:?}"),
		Err(e) => assert!(e.to_string().contains(text), "{e}"),
	}
}

#[test]
fn test_empty() -> Result<()> {
	assert_eq!(lex("")?, vec![]);
	assert_eq!(lex(" \t ")?, vec![]);
	Ok(())
}

#[test]
fn test_name() -> Result<()> {
	assert_eq!(lex("a17")?, vec![s(Name, "a17")]);
	assert_eq!(lex(".notdef")?, vec![s(Name, ".notdef")]);
	assert_eq!(lex("two.oldstyle")?, vec![s(Name, "two.oldstyle")]);
	assert_eq!(lex("_")?, vec![s(Name, "_")]);
	assert_eq!(lex("\\table")?, vec![s(Name, "\\table")]);
	assert_eq!(lex("a+*:^~!")?, vec![s(Name, "a+*:^~!")]);
	assert_eq!(lex("with-dash")?, vec![s(Name, "with-dash")]);
	Ok(())
}

#[test]
fn test_cid() -> Result<()> {
	assert_eq!(lex("\\0 \\987")?, vec![i(Cid, 0), i(Cid, 987)]);
	Ok(())
}

#[test]
fn test_glyphclass() -> Result<()> {
	assert_eq!(lex("@Vowel.sc")?, vec![s(Glyphclass, "Vowel.sc")]);
	assert_eq!(lex("@Vowel-sc")?, vec![s(Glyphclass, "Vowel-sc")]);
	err_contains(lex("@(a)"), "Expected glyph class");
	err_contains(lex("@ A"), "Expected glyph class");
	let long = "A".repeat(600);
	assert_eq!(lex(&format!("@{long}"))?, vec![s(Glyphclass, &long)]);
	err_contains(lex("@Ab:c"), "Glyph class names must consist of");
	Ok(())
}

#[test]
fn test_include() -> Result<()> {
	assert_eq!(
		lex("include (~/foo/bar baz.fea);")?,
		vec![
			s(Name, "include"),
			s(Filename, "~/foo/bar baz.fea"),
			s(Symbol, ";"),
		]
	);
	assert_eq!(
		lex("include # Comment\n    (foo) \n;")?,
		vec![
			s(Name, "include"),
			s(Comment, "# Comment"),
			s(Filename, "foo"),
			s(Symbol, ";"),
		]
	);
	assert!(matches!(lex("include blah"), Err(Error::FeatureLib { .. })));
	assert!(matches!(
		lex("include (blah"),
		Err(Error::FeatureLib { .. })
	));
	Ok(())
}

#[test]
fn test_number() -> Result<()> {
	assert_eq!(lex("123 -456")?, vec![i(Number, 123), i(Number, -456)]);
	assert_eq!(lex("0xCAFED00D")?, vec![i(Hexadecimal, 0xCAFED00D)]);
	assert_eq!(lex("0xcafed00d")?, vec![i(Hexadecimal, 0xCAFED00D)]);
	assert_eq!(lex("010")?, vec![i(Octal, 0o10)]);
	Ok(())
}

#[test]
fn test_float() -> Result<()> {
	assert_eq!(
		lex("1.23 -4.5")?,
		vec![(Float, Token::Float(1.23)), (Float, Token::Float(-4.5))]
	);
	Ok(())
}

#[test]
fn test_symbol() -> Result<()> {
	assert_eq!(lex("a'")?, vec![s(Name, "a"), s(Symbol, "'")]);
	assert_eq!(lex("-A-B")?, vec![s(Symbol, "-"), s(Name, "A-B")]);
	assert_eq!(
		lex("foo - -2")?,
		vec![s(Name, "foo"), s(Symbol, "-"), i(Number, -2)]
	);
	Ok(())
}

#[test]
fn test_comment() -> Result<()> {
	assert_eq!(
		lex("# Comment\n#")?,
		vec![s(Comment, "# Comment"), s(Comment, "#")]
	);
	Ok(())
}

#[test]
fn test_string() -> Result<()> {
	assert_eq!(
		lex("\"foo\" \"bar\"")?,
		vec![s(String, "foo"), s(String, "bar")]
	);
	assert_eq!(
		lex("\"foo \nbar\r baz \r\nqux\n\n \"")?,
		vec![s(String, "foo bar baz qux ")]
	);
	// The lexer should preserve escape sequences because they have different
	// interpretations depending on context.
	assert_eq!(
		lex(r#""M\00fcller-Lanc\00e9""#)?,
		vec![s(String, r"M\00fcller-Lanc\00e9")]
	);
	assert_eq!(
		lex(r#""M\9fller-Lanc\8e""#)?,
		vec![s(String, r"M\9fller-Lanc\8e")]
	);
	assert!(matches!(lex("\"foo\n bar"), Err(Error::FeatureLib { .. })));
	Ok(())
}

#[test]
fn test_bad_character() {
	assert!(matches!(lex("123 \u{0001}"), Err(Error::FeatureLib { .. })));
}

#[test]
fn test_newline() -> Result<()> {
	let lines = |s: &str| -> Result<Vec<usize>> {
		Ok(Lexer::new(s, Some("test.fea"))
			.tokens()?
			.into_iter()
			.map(|(_, _, loc)| loc.line)
			.collect())
	};
	assert_eq!(lines("FOO\n\nBAR\nBAZ")?, vec![1, 3, 4]); // Unix
	assert_eq!(lines("FOO\r\rBAR\rBAZ")?, vec![1, 3, 4]); // Macintosh
	assert_eq!(lines("FOO\r\n\r\n BAR\r\nBAZ")?, vec![1, 3, 4]); // Windows
	assert_eq!(lines("FOO\n\rBAR\r\nBAZ")?, vec![1, 3, 4]); // mixed
	Ok(())
}

#[test]
fn test_location() -> Result<()> {
	let locs: Vec<std::string::String> = Lexer::new("a b # Comment\n12 @x", Some("test.fea"))
		.tokens()?
		.into_iter()
		.map(|(_, _, loc)| loc.to_string())
		.collect();
	assert_eq!(
		locs,
		vec![
			"test.fea:1:1",
			"test.fea:1:3",
			"test.fea:1:5",
			"test.fea:2:1",
			"test.fea:2:4",
		]
	);
	Ok(())
}

#[test]
fn test_scan_over_() {
	let mut lexer = Lexer::new("abbacabba12", Some("test.fea"));
	assert_eq!(lexer.pos(), 0);
	lexer.scan_over_("xyz");
	assert_eq!(lexer.pos(), 0);
	lexer.scan_over_("abc");
	assert_eq!(lexer.pos(), 9);
	lexer.scan_over_("abc");
	assert_eq!(lexer.pos(), 9);
	lexer.scan_over_("0123456789");
	assert_eq!(lexer.pos(), 11);
}

#[test]
fn test_scan_until_() {
	let mut lexer = Lexer::new("foo'bar", Some("test.fea"));
	assert_eq!(lexer.pos(), 0);
	lexer.scan_until_("'");
	assert_eq!(lexer.pos(), 3);
	lexer.scan_until_("'");
	assert_eq!(lexer.pos(), 3);
}

/// Inputs that end in the middle of a check make fontTools raise `TypeError`
/// or `ValueError`; the lexer reports those as errors too.
#[test]
fn test_python_errors_at_end_of_text() {
	for text in ["0", "-", "\\"] {
		match lex(text) {
			Err(Error::Value(m)) => assert!(m.contains("requires string as left operand"), "{m}"),
			other => panic!("{text:?}: {other:?}"),
		}
	}
	for (text, message) in [
		("08", "invalid literal for int() with base 8: '08'"),
		("0x", "invalid literal for int() with base 16: '0x'"),
		("1..2", "could not convert string to float: '1..2'"),
	] {
		match lex(text) {
			Err(Error::Value(m)) => assert_eq!(m, message),
			other => panic!("{text:?}: {other:?}"),
		}
	}
}

mod including_lexer_test {
	use super::*;

	fn getpath(filename: &str) -> PathBuf {
		PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../upstream/fonttools/Tests/feaLib/data")
			.join(filename)
	}

	fn files(lexer: &mut IncludingLexer) -> Result<Vec<std::string::String>> {
		Ok(lexer
			.tokens()?
			.into_iter()
			.map(|(_, _, loc)| loc.file.to_string())
			.collect())
	}

	#[test]
	fn test_include() -> Result<()> {
		let mut lexer = IncludingLexer::new(getpath("include/include4.fea"), None)?;
		let result: Vec<std::string::String> = lexer
			.tokens()?
			.into_iter()
			.map(|(_, token, loc)| {
				let file = PathBuf::from(&*loc.file);
				format!(
					"{} {}:{}",
					token,
					file.file_name()
						.map(|f| f.to_string_lossy().into_owned())
						.unwrap_or_default(),
					loc.line
				)
			})
			.collect();
		assert_eq!(
			result,
			vec![
				"I4a include4.fea:1",
				"I3a include3.fea:1",
				"I2a include2.fea:1",
				"I1a include1.fea:1",
				"I0 include0.fea:1",
				"I1b include1.fea:3",
				"; include2.fea:2",
				"I2b include2.fea:3",
				"; include3.fea:2",
				"I3b include3.fea:3",
				"; include4.fea:2",
				"I4b include4.fea:3",
			]
		);
		Ok(())
	}

	#[test]
	fn test_include_limit() -> Result<()> {
		let mut lexer = IncludingLexer::new(getpath("include/include6.fea"), None)?;
		assert!(matches!(lexer.tokens(), Err(Error::FeatureLib { .. })));
		Ok(())
	}

	#[test]
	fn test_include_self() -> Result<()> {
		let mut lexer = IncludingLexer::new(getpath("include/includeself.fea"), None)?;
		assert!(matches!(lexer.tokens(), Err(Error::FeatureLib { .. })));
		Ok(())
	}

	#[test]
	fn test_include_missing_file() -> Result<()> {
		let mut lexer = IncludingLexer::new(getpath("include/includemissingfile.fea"), None)?;
		match lexer.tokens() {
			Err(e @ Error::IncludedFeaNotFound { .. }) => assert!(
				e.to_string().contains(
					"includemissingfile.fea:1:8: The following feature file \
					 should be included but cannot be found: missingfile.fea"
				),
				"{e}"
			),
			other => panic!("{other:?}"),
		}
		Ok(())
	}

	#[test]
	fn test_featurefilepath_none() -> Result<()> {
		let mut lexer = IncludingLexer::new(FeatureSource::text("# foobar"), None)?;
		assert!(lexer.featurefilepath.is_none());
		assert!(files(&mut lexer)?.contains(&"<features>".to_owned()));
		Ok(())
	}

	#[test]
	fn test_include_absolute_path() -> Result<()> {
		let dir = std::env::temp_dir().join(format!("fontoxide-fealib-abs-{}", std::process::id()));
		std::fs::create_dir_all(&dir).map_err(|e| Error::Value(e.to_string()))?;
		let included = dir.join("included.fea");
		std::fs::write(
			&included,
			"\n                feature kern {\n                    pos A B -40;\n                } kern;\n                ",
		)
		.map_err(|e| Error::Value(e.to_string()))?;
		let including = FeatureSource::text(format!("include({});", included.display()));
		let result = IncludingLexer::new(including, None).and_then(|mut l| files(&mut l));
		let _ = std::fs::remove_dir_all(&dir);
		assert!(result?.contains(&included.to_string_lossy().into_owned()));
		Ok(())
	}

	#[test]
	fn test_include_relative_to_cwd() -> Result<()> {
		// save current working directory, to be restored later
		let cwd = std::env::current_dir().map_err(|e| Error::Value(e.to_string()))?;
		let tmpdir =
			std::env::temp_dir().join(format!("fontoxide-fealib-cwd-{}", std::process::id()));
		std::fs::create_dir_all(&tmpdir).map_err(|e| Error::Value(e.to_string()))?;
		let run = || -> Result<Vec<PathBuf>> {
			// create new feature file in a temporary directory
			std::fs::write(
				tmpdir.join("included.fea"),
				"\n    feature kern {\n        pos A B -40;\n    } kern;\n    ",
			)
			.map_err(|e| Error::Value(e.to_string()))?;
			// change current folder to the temporary dir
			std::env::set_current_dir(&tmpdir).map_err(|e| Error::Value(e.to_string()))?;
			// The lexer has no path, because it was initialized from an
			// in-memory stream, so it uses the current working directory to
			// resolve relative include statements.
			let mut lexer =
				IncludingLexer::new(FeatureSource::text("include(included.fea);"), None)?;
			Ok(files(&mut lexer)?
				.into_iter()
				.map(|f| std::fs::canonicalize(&f).unwrap_or_else(|_| PathBuf::from(f)))
				.collect())
		};
		let result = run();
		// restore the previous working directory and remove the temporary folder
		let _ = std::env::set_current_dir(&cwd);
		let expected = std::fs::canonicalize(tmpdir.join("included.fea"));
		let _ = std::fs::remove_dir_all(&tmpdir);
		let expected = expected.map_err(|e| Error::Value(e.to_string()))?;
		assert!(result?.contains(&expected));
		Ok(())
	}
}

mod error_test {
	use super::*;

	#[test]
	fn test_str() {
		let err = Error::feature_lib("Squeak!", Some(FeatureLibLocation::new("foo.fea", 23, 42)));
		assert_eq!(err.to_string(), "foo.fea:23:42: Squeak!");
	}

	#[test]
	fn test_str_nolocation() {
		let err = Error::feature_lib("Squeak!", None);
		assert_eq!(err.to_string(), "Squeak!");
	}
}
