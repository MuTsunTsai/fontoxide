//! The part of Python's `argparse` that the command line uses: options with
//! abbreviable long names, combinable short flags, mutually exclusive groups
//! and one `nargs="+"` positional argument.
//!
//! A copy of the one in `fontoxide-cu2qu`, except that the options other than
//! `-h` are declared long name first, as `fonttools mtiLib` declares them.

#![allow(dead_code)]

use std::collections::BTreeMap;

use fontoxide_misc::py;

/// What an option takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
	/// `store_true` (or `store_false`): no value.
	Flag,
	/// `count`.
	Count,
	/// One value.
	Value,
	/// `nargs="?"`: an optional value.
	OptionalValue,
}

/// An option: its short and long names, what it takes and the index of its
/// mutually exclusive group.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spec {
	pub short: Option<&'static str>,
	pub long: &'static str,
	pub kind: Kind,
	pub group: Option<usize>,
	pub ty: Ty,
}

/// The `type` of an option's value, checked when the value is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
	/// A string.
	Str,
	/// `type=float`.
	Float,
	/// `type=int`.
	Int,
}

impl Spec {
	/// `_get_action_name`: the option strings joined with `/`, in the order
	/// they are declared (`-h/--help`, but `--font/-f`).
	pub fn display(&self) -> String {
		match self.short {
			Some(s) if self.long == "--help" => format!("{s}/{}", self.long),
			Some(s) => format!("{}/{s}", self.long),
			None => self.long.to_owned(),
		}
	}
}

/// The values of an option given on the command line: how often it was
/// given, and the last value (`None` for no value).
#[derive(Debug, Clone, Default)]
pub(crate) struct Seen {
	pub count: usize,
	pub value: Option<String>,
}

/// The parsed command line: options by long name, and the positional
/// arguments.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
	pub options: BTreeMap<&'static str, Seen>,
	pub positional: Vec<String>,
}

impl Parsed {
	pub fn flag(&self, long: &str) -> bool {
		self.options.contains_key(long)
	}

	pub fn count(&self, long: &str) -> usize {
		self.options.get(long).map_or(0, |s| s.count)
	}

	pub fn value(&self, long: &str) -> Option<&Seen> {
		self.options.get(long)
	}
}

/// `argparse`'s `_negative_number_matcher`: `^-\d+$|^-\d*\.\d+$`.
fn is_negative_number(arg: &str) -> bool {
	let Some(s) = arg.strip_prefix('-') else {
		return false;
	};
	let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
	match s.split_once('.') {
		None => !s.is_empty() && digits(s),
		Some((a, b)) => digits(a) && !b.is_empty() && digits(b),
	}
}

/// Whether `argparse` takes an argument as an option string ("O") rather
/// than a value ("A").
fn is_option(arg: &str) -> bool {
	arg.len() > 1 && arg.starts_with('-') && !is_negative_number(arg)
}

/// Parses `args` as `ArgumentParser.parse_args` does. `Err` holds the
/// message of `parser.error`.
pub(crate) fn parse(
	args: &[&str],
	specs: &[Spec],
	positional_name: &str,
) -> Result<Parsed, String> {
	let mut parsed = Parsed::default();
	let mut runs: Vec<Vec<String>> = Vec::new();
	let mut in_run = false;
	let mut only_positional = false;
	let mut seen_specs: Vec<&Spec> = Vec::new();
	let mut i = 0;
	while i < args.len() {
		let arg = args[i];
		i += 1;
		if only_positional || !is_option(arg) {
			if !in_run {
				runs.push(Vec::new());
				in_run = true;
			}
			if let Some(run) = runs.last_mut() {
				run.push(arg.to_owned());
			}
			continue;
		}
		in_run = false;
		if arg == "--" {
			only_positional = true;
			continue;
		}
		// The option and an explicit value, if any.
		let mut pending: Vec<(&Spec, Option<String>)> = Vec::new();
		if arg.starts_with("--") {
			let (option, inline) = match arg.split_once('=') {
				Some((o, v)) => (o, Some(v.to_owned())),
				None => (arg, None),
			};
			let exact = specs.iter().find(|s| s.long == option);
			let candidates: Vec<&Spec> = match exact {
				Some(e) => vec![e],
				None => specs
					.iter()
					.filter(|s| s.long.starts_with(option))
					.collect(),
			};
			match candidates[..] {
				[spec] => pending.push((spec, inline)),
				[] => return Err(format!("unrecognized arguments: {arg}")),
				_ => {
					let names: Vec<&str> = candidates.iter().map(|s| s.long).collect();
					return Err(format!(
						"ambiguous option: {option} could match {}",
						names.join(", ")
					));
				}
			}
		} else {
			// Short options, possibly combined (`-vv`) or with the value
			// attached (`-e0.002`).
			let mut rest = &arg[1..];
			while let Some(c) = rest.chars().next() {
				let name = format!("-{c}");
				let Some(spec) = specs.iter().find(|s| s.short == Some(name.as_str())) else {
					return Err(if pending.is_empty() {
						format!("unrecognized arguments: {arg}")
					} else {
						format!(
							"argument {}: ignored explicit argument {}",
							pending[pending.len() - 1].0.display(),
							py::str_repr(rest)
						)
					});
				};
				rest = &rest[c.len_utf8()..];
				match spec.kind {
					Kind::Value | Kind::OptionalValue if !rest.is_empty() => {
						pending.push((spec, Some(rest.to_owned())));
						rest = "";
					}
					_ => pending.push((spec, None)),
				}
			}
		}
		for (spec, inline) in pending {
			let value = match spec.kind {
				Kind::Flag | Kind::Count => {
					if inline.is_some() && arg.starts_with("--") {
						return Err(format!(
							"argument {}: ignored explicit argument {}",
							spec.display(),
							py::str_repr(inline.as_deref().unwrap_or_default())
						));
					}
					None
				}
				Kind::Value => match inline {
					Some(v) => Some(v),
					None => match args.get(i) {
						Some(v) if !is_option(v) => {
							i += 1;
							Some((*v).to_owned())
						}
						_ => {
							return Err(format!(
								"argument {}: expected one argument",
								spec.display()
							));
						}
					},
				},
				Kind::OptionalValue => match inline {
					Some(v) => Some(v),
					None => match args.get(i) {
						Some(v) if !is_option(v) && !only_positional => {
							i += 1;
							Some((*v).to_owned())
						}
						_ => None,
					},
				},
			};
			if let Some(v) = &value {
				match spec.ty {
					Ty::Str => {}
					Ty::Float => {
						parse_float(&spec.display(), v)?;
					}
					Ty::Int => {
						parse_int(&spec.display(), v)?;
					}
				}
			}
			if let Some(group) = spec.group
				&& let Some(other) = seen_specs
					.iter()
					.find(|s| s.group == Some(group) && s.long != spec.long)
			{
				return Err(format!(
					"argument {}: not allowed with argument {}",
					spec.display(),
					other.display()
				));
			}
			seen_specs.push(spec);
			let seen = parsed.options.entry(spec.long).or_default();
			seen.count += 1;
			seen.value = value;
			// `-h` and `--version` print and exit right away.
			if spec.long == "--help" || spec.long == "--version" {
				return Ok(parsed);
			}
		}
	}
	let mut runs = runs.into_iter();
	match runs.next() {
		Some(run) => parsed.positional = run,
		None => {
			return Err(format!(
				"the following arguments are required: {positional_name}"
			));
		}
	}
	let extra: Vec<String> = runs.flatten().collect();
	if !extra.is_empty() {
		return Err(format!("unrecognized arguments: {}", extra.join(" ")));
	}
	Ok(parsed)
}

/// `type=float`.
pub(crate) fn parse_float(spec: &str, value: &str) -> Result<f64, String> {
	py::parse_float(value).ok_or_else(|| {
		format!(
			"argument {spec}: invalid float value: {}",
			py::str_repr(value)
		)
	})
}

/// `type=int`.
pub(crate) fn parse_int(spec: &str, value: &str) -> Result<i128, String> {
	py::parse_int(value).ok_or_else(|| {
		format!(
			"argument {spec}: invalid int value: {}",
			py::str_repr(value)
		)
	})
}
