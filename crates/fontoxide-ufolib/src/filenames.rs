//! Port of `fontTools.ufoLib.filenames`: converting user-provided names
//! (glyph and layer names) to file names that are safe on all file systems.

use std::collections::{BTreeSet, HashSet};

use indexmap::IndexSet;

use crate::errors::{Result, UfoLibError};

/// `illegalCharacters`: control characters and characters that (mostly)
/// Windows and POSIX file systems don't allow, plus `(` and `)`.
pub fn is_illegal_character(c: char) -> bool {
	matches!(
		c,
		'\x00'
			..='\x1f'
				| '"' | '*' | '+'
				| '/' | ':' | '<'
				| '>' | '?' | '['
				| '\\' | ']' | '('
				| ')' | '|' | '\x7f'
	)
}

/// `reservedFileNames`.
pub const RESERVED_FILE_NAMES: [&str; 23] = [
	"aux", "clock$", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "con",
	"lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "nul", "prn",
];

/// `maxFileNameLength`.
pub const MAX_FILE_NAME_LENGTH: i64 = 255;

/// A collection of existing (lowercase) file names, as the `existing`
/// arguments take: anything that supports `in`.
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

impl ExistingNames for IndexSet<String> {
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
pub(crate) fn lower(s: &str) -> String {
	s.chars().flat_map(char::to_lowercase).collect()
}

/// `s[:n]` on code points, with Python's negative indices.
fn slice_to(s: &str, n: i64) -> String {
	let len = s.chars().count() as i64;
	let end = if n < 0 { (len + n).max(0) } else { n.min(len) };
	s.chars().take(end as usize).collect()
}

/// `userNameToFileName`: converts a user name to a file name, avoiding
/// illegal characters, reserved file names, ambiguity between upper- and
/// lower-case characters, and clashes with the (lowercase) `existing` names.
///
/// ```
/// use fontoxide_ufolib::filenames::user_name_to_file_name;
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
			None => return Err(UfoLibError::Value("string index out of range".into())),
			Some('.') => user_name = format!("_{}", &user_name[1..]),
			Some(_) => {}
		}
	}
	// filter the user name
	let mut filtered = String::with_capacity(user_name.len());
	for c in user_name.chars() {
		if is_illegal_character(c) {
			filtered.push('_');
		} else {
			filtered.push(c);
			let lowered: String = c.to_lowercase().collect();
			if lowered.chars().ne(std::iter::once(c)) {
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

/// `handleClash1`: resolves a clash by appending a 15-digit counter to the
/// user name.
pub fn handle_clash1<E: ExistingNames + ?Sized>(
	user_name: &str,
	existing: &E,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	let prefix_length = prefix.chars().count() as i64;
	let suffix_length = suffix.chars().count() as i64;
	let name_length = user_name.chars().count() as i64;
	let mut user_name = user_name.to_owned();
	if prefix_length + name_length + suffix_length + 15 > MAX_FILE_NAME_LENGTH {
		let l = prefix_length + name_length + suffix_length + 15;
		let slice_length = MAX_FILE_NAME_LENGTH - l;
		user_name = slice_to(&user_name, slice_length);
	}
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
	handle_clash2(existing, prefix, suffix)
}

/// `handleClash2`: the fallback of [`handle_clash1`], a plain counter between
/// the prefix and the suffix.
pub fn handle_clash2<E: ExistingNames + ?Sized>(
	existing: &E,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	let max_length =
		MAX_FILE_NAME_LENGTH - prefix.chars().count() as i64 - suffix.chars().count() as i64;
	// `int("9" * maxLength)`
	if max_length <= 0 {
		return Err(UfoLibError::Value(
			"invalid literal for int() with base 10: ''".into(),
		));
	}
	let max_value: Option<u128> = if max_length < 39 {
		Some(10u128.pow(max_length as u32) - 1)
	} else {
		None
	};
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
	Err(UfoLibError::NameTranslation(
		"No unique name could be found.".into(),
	))
}
