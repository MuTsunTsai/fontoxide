//! Port of `fontTools.misc.filenames`: converting a "user name" (something a
//! user can choose arbitrarily inside a font editor, such as a glyph name) to a
//! file name that is safe on a wide range of operating systems and file
//! systems.
//!
//! This is the older copy of the algorithm that `fontTools.ttLib` uses (for
//! example, for the glyph files of `ttx -s`). `fontTools.ufoLib.filenames`
//! (`fontoxide_ufolib::filenames`) has the same algorithm with different
//! tables of illegal characters and reserved names.

use std::collections::{BTreeSet, HashSet};

/// The errors of the module.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilenamesError {
	/// `IndexError`: an empty user name without a prefix.
	#[error("{0}")]
	Index(String),
	/// `ValueError`: the prefix and suffix leave no room for a counter.
	#[error("{0}")]
	Value(String),
	/// `NameTranslationError`: no unique name could be found.
	#[error("{0}")]
	NameTranslation(String),
}

/// The result type of the module.
pub type Result<T> = std::result::Result<T, FilenamesError>;

/// `illegalCharacters`: `* + / : < > ? [ \ ] |`, U+0001 to U+001F and U+007F.
///
/// UPSTREAM-BUG(B66): the list is split from the raw string
/// `r"\" * + / : < > ? [ \ ] | \0"`, so its first and last items are the
/// two-character strings `\"` and `\0`, which never match a character: the
/// double quote and NUL are not replaced.
pub fn is_illegal_character(c: char) -> bool {
	matches!(
		c,
		'*' | '+' | '/' | ':' | '<' | '>' | '?' | '[' | '\\' | ']' | '|' | '\x01'..='\x1f' | '\x7f'
	)
}

/// `reservedFileNames`. (`a:-z:` can never match, since `:` is replaced first.)
pub const RESERVED_FILE_NAMES: [&str; 13] = [
	"con", "prn", "aux", "clock$", "nul", "a:-z:", "com1", "lpt1", "lpt2", "lpt3", "com2", "com3",
	"com4",
];

/// `maxFileNameLength`.
pub const MAX_FILE_NAME_LENGTH: i64 = 255;

/// The existing (lowercase) file names, as the `existing` arguments take:
/// anything that supports `in`.
pub trait ExistingNames {
	/// `name in existing`.
	fn contains_name(&self, name: &str) -> bool;
}

impl ExistingNames for HashSet<String> {
	fn contains_name(&self, name: &str) -> bool {
		self.contains(name)
	}
}

impl ExistingNames for BTreeSet<String> {
	fn contains_name(&self, name: &str) -> bool {
		self.contains(name)
	}
}

impl ExistingNames for [String] {
	fn contains_name(&self, name: &str) -> bool {
		self.iter().any(|n| n == name)
	}
}

impl ExistingNames for Vec<String> {
	fn contains_name(&self, name: &str) -> bool {
		self.iter().any(|n| n == name)
	}
}

impl ExistingNames for [&str] {
	fn contains_name(&self, name: &str) -> bool {
		self.contains(&name)
	}
}

impl<const N: usize> ExistingNames for [&str; N] {
	fn contains_name(&self, name: &str) -> bool {
		self.contains(&name)
	}
}

/// `str.lower()`.
fn lower(s: &str) -> String {
	s.chars().flat_map(char::to_lowercase).collect()
}

/// `s[:n]` on code points, with Python's negative indices.
fn slice_to(s: &str, n: i64) -> String {
	let len = s.chars().count() as i64;
	let end = if n < 0 { (len + n).max(0) } else { n.min(len) };
	s.chars().take(end as usize).collect()
}

/// Converts a user name to a file name, avoiding illegal characters, reserved
/// file names, ambiguity between upper- and lower-case characters, and clashes
/// with the (lowercase) `existing` names (`userNameToFileName`).
///
/// ```
/// use fontoxide_misc::filenames::user_name_to_file_name;
/// let none: [&str; 0] = [];
/// assert_eq!(user_name_to_file_name("A.alt", &none, "", "").unwrap(), "A_.alt");
/// assert_eq!(user_name_to_file_name(".notdef", &none, "", "").unwrap(), "_notdef");
/// assert_eq!(user_name_to_file_name("con.alt", &none, "", "").unwrap(), "_con.alt");
/// ```
pub fn user_name_to_file_name<E: ExistingNames + ?Sized>(
	user_name: &str,
	existing: &E,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	let prefix_length = prefix.chars().count() as i64;
	let suffix_length = suffix.chars().count() as i64;
	let mut user_name = user_name.to_owned();
	// replace an initial period with an _ if no prefix is to be added
	if prefix.is_empty() {
		match user_name.chars().next() {
			// `userName[0]` of an empty name.
			None => return Err(FilenamesError::Index("string index out of range".into())),
			Some('.') => user_name = format!("_{}", &user_name[1..]),
			Some(_) => {}
		}
	}
	// filter the user name
	let mut filtered = String::with_capacity(user_name.len());
	for c in user_name.chars() {
		if is_illegal_character(c) {
			// replace illegal characters with _
			filtered.push('_');
		} else {
			// add _ to all non-lower characters
			filtered.push(c);
			if c.to_lowercase().ne(std::iter::once(c)) {
				filtered.push('_');
			}
		}
	}
	// clip to 255
	let slice_length = MAX_FILE_NAME_LENGTH - prefix_length - suffix_length;
	let user_name = slice_to(&filtered, slice_length);
	// test for illegal files names
	let user_name = user_name
		.split('.')
		.map(|part| {
			if RESERVED_FILE_NAMES.contains(&lower(part).as_str()) {
				format!("_{part}")
			} else {
				part.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join(".");
	// test for clash
	let full_name = format!("{prefix}{user_name}{suffix}");
	if existing.contains_name(&lower(&full_name)) {
		return handle_clash1(&user_name, existing, prefix, suffix);
	}
	Ok(full_name)
}

/// Resolves a clash by appending a 15-digit counter to the user name
/// (`handleClash1`); `existing` holds lowercase names.
pub fn handle_clash1<E: ExistingNames + ?Sized>(
	user_name: &str,
	existing: &E,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	// if the prefix length + user name length + suffix length + 15 is at or
	// past the maximum length, slice 15 characters off of the user name
	let prefix_length = prefix.chars().count() as i64;
	let suffix_length = suffix.chars().count() as i64;
	let name_length = user_name.chars().count() as i64;
	let mut user_name = user_name.to_owned();
	if prefix_length + name_length + suffix_length + 15 > MAX_FILE_NAME_LENGTH {
		let l = prefix_length + name_length + suffix_length + 15;
		let slice_length = MAX_FILE_NAME_LENGTH - l;
		user_name = slice_to(&user_name, slice_length);
	}
	// try to add numbers to create a unique name
	let mut counter: u64 = 1;
	loop {
		let full_name = format!("{prefix}{user_name}{counter:015}{suffix}");
		if !existing.contains_name(&lower(&full_name)) {
			return Ok(full_name);
		}
		counter += 1;
		if counter >= 999_999_999_999_999 {
			break;
		}
	}
	// if there is a clash, go to the next fallback
	handle_clash2(existing, prefix, suffix)
}

/// The fallback of [`handle_clash1`]: a plain counter between the prefix and
/// the suffix (`handleClash2`).
pub fn handle_clash2<E: ExistingNames + ?Sized>(
	existing: &E,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	// calculate the longest possible string
	let max_length =
		MAX_FILE_NAME_LENGTH - prefix.chars().count() as i64 - suffix.chars().count() as i64;
	// `int("9" * maxLength)`
	if max_length <= 0 {
		return Err(FilenamesError::Value(
			"invalid literal for int() with base 10: ''".into(),
		));
	}
	let max_value: Option<u128> = (max_length < 39).then(|| 10u128.pow(max_length as u32) - 1);
	// try to find a number
	let mut counter: u128 = 1;
	loop {
		let full_name = format!("{prefix}{counter}{suffix}");
		if !existing.contains_name(&lower(&full_name)) {
			return Ok(full_name);
		}
		counter += 1;
		if max_value.is_some_and(|m| counter >= m) {
			break;
		}
	}
	// raise an error if nothing has been found
	Err(FilenamesError::NameTranslation(
		"No unique name could be found.".into(),
	))
}
