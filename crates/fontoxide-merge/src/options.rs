//! Port of `fontTools.merge.options`.

use crate::{Error, Result};

/// An item of a list option: an integer if it starts with a digit, else a
/// string, as `parse_opts` converts them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
	/// An integer (`int(x, 0)`).
	Int(i64),
	/// A string.
	Str(String),
}

impl From<&str> for Item {
	fn from(s: &str) -> Self {
		Item::Str(s.to_owned())
	}
}

/// The merger options (`Options`).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// Output progress information (`verbose`).
	pub verbose: bool,
	/// Output progress timing (`timing`).
	pub timing: bool,
	/// Table tags to skip, case sensitive (`drop_tables`).
	pub drop_tables: Vec<Item>,
	/// A text file listing the files to merge, one per line (`input_file`).
	pub input_file: Option<String>,
	/// The output file name (`output_file`); `merged.ttf` by default.
	pub output_file: String,
	/// A TTX file to import after merging (`import_file`).
	pub import_file: Option<String>,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			verbose: false,
			timing: false,
			drop_tables: Vec::new(),
			input_file: None,
			output_file: "merged.ttf".into(),
			import_file: None,
		}
	}
}

/// A value from the command line: a flag (`--x`, `--no-x`) or a string
/// (`--x=...`).
enum Value {
	Bool(bool),
	Str(String),
}

impl Options {
	/// Whether a table is in [`Options::drop_tables`] (`tag in drop_tables`).
	pub fn drops(&self, tag: &str) -> bool {
		self.drop_tables
			.iter()
			.any(|item| matches!(item, Item::Str(s) if s == tag))
	}

	/// Parses command line options into the options (`parse_opts`) and returns
	/// the other arguments.
	///
	/// Unknown options are an error, unless their name (without the leading
	/// `--`) is in `ignore_unknown`, or `ignore_unknown` contains `*` (fontTools'
	/// `ignore_unknown=True`): those are returned with the other arguments.
	pub fn parse_opts(&mut self, argv: &[&str], ignore_unknown: &[&str]) -> Result<Vec<String>> {
		let mut ret = Vec::new();
		// The values are set at the end, as fontTools does with `set(**opts)`.
		let mut opts: Vec<(String, Value, &str)> = Vec::new();
		for &orig in argv {
			let Some(a) = orig.strip_prefix("--") else {
				ret.push(orig.to_owned());
				continue;
			};
			let mut op = "=";
			let (k, v) = match a.find('=') {
				None => match a.strip_prefix("no-") {
					Some(k) => (k.to_owned(), Value::Bool(false)),
					None => (a.to_owned(), Value::Bool(true)),
				},
				Some(i) => {
					let mut k = &a[..i];
					if let Some(stripped) = k.strip_suffix(['-', '+']) {
						// The operation is `-=` or `+=` now.
						op = if k.ends_with('-') { "-=" } else { "+=" };
						k = stripped;
					}
					(k.to_owned(), Value::Str(a[i + 1..].to_owned()))
				}
			};
			let name = k.replace('-', "_");
			if !matches!(
				name.as_str(),
				"verbose" | "timing" | "drop_tables" | "input_file" | "output_file" | "import_file"
			) {
				if ignore_unknown.contains(&"*") || ignore_unknown.contains(&k.as_str()) {
					ret.push(orig.to_owned());
					continue;
				}
				return Err(Error::UnknownOption(format!("Unknown option '{a}'")));
			}
			opts.push((name, v, op));
		}
		for (name, v, op) in opts {
			// `bool(v)`: a flag, or whether the string is not empty.
			let truth = || match &v {
				Value::Bool(b) => *b,
				Value::Str(s) => !s.is_empty(),
			};
			// Other values are stored as they are; `True` and `False` stand for
			// the flags.
			let string = || match &v {
				Value::Bool(true) => Some("True".to_owned()),
				Value::Bool(false) => None,
				Value::Str(s) => Some(s.clone()),
			};
			match name.as_str() {
				"verbose" => self.verbose = truth(),
				"timing" => self.timing = truth(),
				"input_file" => self.input_file = string(),
				"import_file" => self.import_file = string(),
				"output_file" => self.output_file = string().unwrap_or_else(|| "False".into()),
				_ => {
					let Value::Str(s) = &v else {
						// `v.split(",")` of a bool.
						return Err(Error::Type(
							"AttributeError: 'bool' object has no attribute 'split'".into(),
						));
					};
					let mut vv: Vec<&str> = s.split(',').collect();
					if vv == [""] {
						vv.clear();
					}
					let vv = vv.into_iter().map(parse_item).collect::<Result<Vec<_>>>()?;
					match op {
						"=" => self.drop_tables = vv,
						"+=" => self.drop_tables.extend(vv),
						_ => {
							for x in vv {
								if let Some(i) = self.drop_tables.iter().position(|y| *y == x) {
									self.drop_tables.remove(i);
								}
							}
						}
					}
				}
			}
		}
		Ok(ret)
	}
}

/// `int(x, 0) if len(x) and x[0] in "0123456789" else x`.
fn parse_item(x: &str) -> Result<Item> {
	if !x.starts_with(|c: char| c.is_ascii_digit()) {
		return Ok(Item::Str(x.to_owned()));
	}
	let invalid = || Error::Value(format!("invalid literal for int() with base 0: '{x}'"));
	if x.starts_with('_') || x.ends_with('_') || x.contains("__") {
		return Err(invalid());
	}
	let s = x.replace('_', "");
	let lower = s.to_ascii_lowercase();
	let parsed = if let Some(h) = lower.strip_prefix("0x") {
		i64::from_str_radix(h, 16)
	} else if let Some(o) = lower.strip_prefix("0o") {
		i64::from_str_radix(o, 8)
	} else if let Some(b) = lower.strip_prefix("0b") {
		i64::from_str_radix(b, 2)
	} else if lower.len() > 1 && lower.starts_with('0') && lower.bytes().any(|c| c != b'0') {
		// `int("010", 0)` is an error in Python 3.
		return Err(invalid());
	} else {
		lower.parse()
	};
	parsed.map(Item::Int).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse() -> Result<()> {
		let mut options = Options::default();
		let rest = options.parse_opts(
			&[
				"a.ttf",
				"--drop-tables=DSIG,0x10",
				"--verbose",
				"--no-timing",
				"--output-file=out.ttf",
				"b.ttf",
			],
			&[],
		)?;
		assert_eq!(rest, ["a.ttf", "b.ttf"]);
		assert_eq!(options.drop_tables, [Item::from("DSIG"), Item::Int(16)]);
		assert!(options.verbose);
		assert!(!options.timing);
		assert_eq!(options.output_file, "out.ttf");
		assert!(options.drops("DSIG"));
		assert!(!options.drops("16"));

		options.parse_opts(&["--drop-tables+=kern", "--drop-tables-=DSIG"], &[])?;
		assert_eq!(options.drop_tables, [Item::Int(16), Item::from("kern")]);
		options.parse_opts(&["--drop-tables="], &[])?;
		assert!(options.drop_tables.is_empty());

		assert!(matches!(
			options.parse_opts(&["--foo"], &[]),
			Err(Error::UnknownOption(_))
		));
		assert_eq!(options.parse_opts(&["--foo=1"], &["foo"])?, ["--foo=1"]);
		assert!(options.parse_opts(&["--drop-tables=010"], &[]).is_err());
		Ok(())
	}
}
