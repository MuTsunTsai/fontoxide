//! `Tests/misc/psLib_test.py`.

use std::time::Instant;

use fontoxide_misc::ps_lib::{PsError, PsTokenizer, suckfont};
use fontoxide_misc::ps_operators::Key;

fn tokenize(buf: &[u8]) -> Result<Vec<(&'static str, String)>, PsError> {
	let mut tokenizer = PsTokenizer::new(buf, "ascii");
	let mut tokens = Vec::new();
	while let Some(token) = tokenizer.get_next_token()? {
		tokens.push(token);
	}
	tokenizer.close();
	Ok(tokens)
}

#[test]
fn test_plain_string() -> Result<(), PsError> {
	assert_eq!(tokenize(b"(hello)")?, [("do_string", "(hello)".to_owned())]);
	Ok(())
}

#[test]
fn test_nested_parens() -> Result<(), PsError> {
	assert_eq!(tokenize(b"(a(b)c)")?, [("do_string", "(a(b)c)".to_owned())]);
	Ok(())
}

#[test]
fn test_escaped_parens() -> Result<(), PsError> {
	for buf in [&br"(a\(b)"[..], br"(a\)b)", br"(Copyright \(c\) 2000)"] {
		// An escaped "(" or ")" is part of the string, it does not open or
		// close it.
		assert_eq!(
			tokenize(buf)?,
			[("do_string", String::from_utf8_lossy(buf).into_owned())]
		);
	}
	Ok(())
}

#[test]
fn test_even_backslash_run_before_open_paren() -> Result<(), PsError> {
	// "\\" is an escaped backslash, so the "(" after it is a real paren that
	// opens a nested pair; the whole literal is one string token.
	assert_eq!(
		tokenize(br"(a\\(b)c)")?,
		[("do_string", r"(a\\(b)c)".to_owned())]
	);
	Ok(())
}

#[test]
fn test_even_backslash_run_before_close_paren() -> Result<(), PsError> {
	// The ")" after "\\" is the real closing paren.
	let mut tokenizer = PsTokenizer::new(br"(a\\) rest)", "ascii");
	assert_eq!(
		tokenizer.get_next_token()?,
		Some(("do_string", r"(a\\)".to_owned()))
	);
	assert_eq!(tokenizer.get_next_token()?, Some(("", "rest".to_owned())));
	Ok(())
}

#[test]
fn test_unterminated_string_is_rejected_quickly() {
	let mut buf = b"(".to_vec();
	buf.extend(b"[]".repeat(25));
	buf.extend(b" never closed");
	let start = Instant::now();
	assert!(tokenize(&buf).is_err());
	assert!(start.elapsed().as_secs_f64() < 2.0);
}

#[test]
fn test_escaped_paren_in_font() -> Result<(), PsError> {
	// An unbalanced escaped paren is a valid PostScript string; suckfont
	// consumes it.
	let font = suckfont(b"%!FontType1\n/Foo (a\\(b) def\n", "ascii")?;
	// The fallback font, Helvetica, is a dict.
	assert!(font.contains_key(&Key::from("Encoding")));
	Ok(())
}

/// Quirks of fontTools that are reproduced (checked against fontTools 4.68.0).
#[test]
fn upstream_bugs() -> Result<(), PsError> {
	use fontoxide_misc::ps_lib::{PsInterpreter, PsValue, unpack_item};
	// UPSTREAM-BUG(B62): `store` also defines the name in the current dictionary.
	let mut i = PsInterpreter::new("ascii");
	i.interpret(b"/x 1 def 5 dict begin /x 2 store")?;
	let user = i.dictstack[1].clone();
	assert_eq!(
		unpack_item(&user.borrow()[&Key::from("x")])?,
		PsValue::Int(2)
	);
	let top = i
		.dictstack
		.last()
		.cloned()
		.ok_or(PsError::Ps("no dict".into()))?;
	assert!(top.borrow().contains_key(&Key::from("x")));
	Ok(())
}
