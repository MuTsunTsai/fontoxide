//! `Tests/voltLib/lexer_test.py`.

use fontoxide_voltlib::Result;
use fontoxide_voltlib::lexer::{Lexer, Token};

fn lex(s: &str) -> Result<Vec<Token>> {
	Lexer::new(s, Some("test.vtp"))
		.map(|t| t.map(|(token, _)| token))
		.collect()
}

#[test]
fn test_empty() -> Result<()> {
	assert_eq!(lex("")?, []);
	assert_eq!(lex("\t")?, []);
	Ok(())
}

#[test]
fn test_string() -> Result<()> {
	assert_eq!(
		lex("\"foo\" \"bar\"")?,
		[Token::String("foo".into()), Token::String("bar".into())]
	);
	assert!(lex("\"foo\n bar\"").is_err());
	Ok(())
}

#[test]
fn test_name() -> Result<()> {
	assert_eq!(
		lex("DEF_FOO bar.alt1")?,
		[
			Token::Name("DEF_FOO".into()),
			Token::Name("bar.alt1".into())
		]
	);
	Ok(())
}

#[test]
fn test_number() -> Result<()> {
	assert_eq!(lex("123 -456")?, [Token::Number(123), Token::Number(-456)]);
	Ok(())
}
