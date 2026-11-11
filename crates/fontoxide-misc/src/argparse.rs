//! The part of Python's `argparse` (CPython 3.13) that the fontTools command
//! lines use: parsing the arguments as `ArgumentParser.parse_args` does, with
//! its error messages.
//!
//! The arguments are declared as with `add_argument` ([`Argument`]): options
//! with abbreviable long names and combinable short names, positional
//! arguments with `nargs`, `type=float`/`type=int` values, mutually exclusive
//! groups ([`Argument::group`]). [`parse`] follows `_parse_known_args`: the
//! positional arguments are matched against the pattern of the arguments
//! between the options, as `argparse` does with regular expressions.
//!
//! The help and usage texts are not generated: [`Command`] holds them as
//! fontTools prints them, and prints them as `argparse` does
//! (`print_help`, `error`), through [`program_text`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use crate::cli_tools::program_text;
use crate::py;

/// What an argument does when it is given (`action=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
	/// `store`: keeps the value.
	Store,
	/// `append`: collects the values.
	Append,
	/// `store_true` (or `store_false`, `store_const`): no value.
	StoreTrue,
	/// `count`.
	Count,
	/// `help`: prints the help and exits.
	Help,
	/// `version`: prints the version and exits.
	Version,
}

/// How many values an argument takes (`nargs=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nargs {
	/// No value (the flags).
	Zero,
	/// The default: one value.
	One,
	/// `"?"`.
	Optional,
	/// `"*"`.
	ZeroOrMore,
	/// `"+"`.
	OneOrMore,
}

/// The `type` of an argument's values, checked when they are taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
	/// A string.
	Str,
	/// `type=float`.
	Float,
	/// `type=int`.
	Int,
}

/// An argument (`add_argument`).
#[derive(Debug, Clone, Copy)]
pub struct Argument {
	/// The option strings, in the order they are declared; empty for a
	/// positional argument.
	pub strings: &'static [&'static str],
	/// The name of a positional argument in the messages (its `metavar`, or
	/// `dest`).
	pub name: &'static str,
	/// What it does.
	pub action: Action,
	/// How many values it takes.
	pub nargs: Nargs,
	/// The type of its values.
	pub ty: Ty,
	/// Its mutually exclusive group, if any.
	pub group: Option<usize>,
	/// Whether it is required.
	pub required: bool,
}

impl Argument {
	const fn new(strings: &'static [&'static str], action: Action, nargs: Nargs) -> Self {
		Argument {
			strings,
			name: "",
			action,
			nargs,
			ty: Ty::Str,
			group: None,
			required: false,
		}
	}

	/// `-h/--help`.
	pub const fn help() -> Self {
		Self::new(&["-h", "--help"], Action::Help, Nargs::Zero)
	}

	/// `action="version"`.
	pub const fn version(strings: &'static [&'static str]) -> Self {
		Self::new(strings, Action::Version, Nargs::Zero)
	}

	/// `action="store_true"` (or `store_false`).
	pub const fn flag(strings: &'static [&'static str]) -> Self {
		Self::new(strings, Action::StoreTrue, Nargs::Zero)
	}

	/// `action="count"`.
	pub const fn count(strings: &'static [&'static str]) -> Self {
		Self::new(strings, Action::Count, Nargs::Zero)
	}

	/// An option with one value.
	pub const fn option(strings: &'static [&'static str]) -> Self {
		Self::new(strings, Action::Store, Nargs::One)
	}

	/// `action="append"`: an option with one value, which may be given more
	/// than once.
	pub const fn append(strings: &'static [&'static str]) -> Self {
		Self::new(strings, Action::Append, Nargs::One)
	}

	/// A positional argument with one value, named `name` in the messages.
	pub const fn positional(name: &'static str) -> Self {
		let mut argument = Self::new(&[], Action::Store, Nargs::One);
		argument.name = name;
		argument.required = true;
		argument
	}

	/// `nargs=`; a positional argument is required unless it takes `"?"` or
	/// `"*"`.
	pub const fn nargs(mut self, nargs: Nargs) -> Self {
		self.nargs = nargs;
		if self.strings.is_empty() {
			self.required = !matches!(nargs, Nargs::Optional | Nargs::ZeroOrMore | Nargs::Zero);
		}
		self
	}

	/// `type=`.
	pub const fn ty(mut self, ty: Ty) -> Self {
		self.ty = ty;
		self
	}

	/// The mutually exclusive group `group`.
	pub const fn group(mut self, group: usize) -> Self {
		self.group = Some(group);
		self
	}

	/// `required=True`.
	pub const fn required(mut self) -> Self {
		self.required = true;
		self
	}

	/// The key of the argument in [`Parsed`]: its first long option string,
	/// or its first option string, or the name of a positional argument.
	pub fn key(&self) -> &'static str {
		self.strings
			.iter()
			.find(|s| s.starts_with("--"))
			.or(self.strings.first())
			.copied()
			.unwrap_or(self.name)
	}

	/// `_get_action_name`: the option strings joined with `/`, or the name of
	/// a positional argument.
	pub fn display(&self) -> String {
		if self.strings.is_empty() {
			self.name.to_owned()
		} else {
			self.strings.join("/")
		}
	}
}

/// The values of an argument given on the command line.
#[derive(Debug, Clone, Default)]
pub struct Seen {
	/// How often it was given.
	pub count: usize,
	/// The last value of an argument taking one value (`None` for an omitted
	/// `nargs="?"` value).
	pub value: Option<String>,
	/// The last values of an argument taking `nargs="*"` or `"+"`, or all the
	/// values of an `append` argument.
	pub values: Vec<String>,
}

/// The parsed command line, by [`Argument::key`].
#[derive(Debug, Clone, Default)]
pub struct Parsed {
	/// The arguments given.
	pub seen: BTreeMap<&'static str, Seen>,
}

impl Parsed {
	/// Whether the argument was given.
	pub fn flag(&self, key: &str) -> bool {
		self.seen.contains_key(key)
	}

	/// How often the argument was given.
	pub fn count(&self, key: &str) -> usize {
		self.seen.get(key).map_or(0, |s| s.count)
	}

	/// The values of the argument, if it was given.
	pub fn get(&self, key: &str) -> Option<&Seen> {
		self.seen.get(key)
	}

	/// The value of an argument taking one value, if it was given.
	pub fn str(&self, key: &str) -> Option<&str> {
		self.seen.get(key).and_then(|s| s.value.as_deref())
	}

	/// The values of an argument taking `nargs="*"` or `"+"` (empty if it was
	/// not given).
	pub fn list(&self, key: &str) -> &[String] {
		self.seen.get(key).map_or(&[], |s| s.values.as_slice())
	}
}

/// `type=float`: the value, or the message of the error for the argument
/// `display` (see [`Argument::display`]).
pub fn parse_float(display: &str, value: &str) -> Result<f64, String> {
	py::parse_float(value).ok_or_else(|| {
		format!(
			"argument {display}: invalid float value: {}",
			py::str_repr(value)
		)
	})
}

/// `type=int`: the value, or the message of the error for the argument
/// `display` (see [`Argument::display`]).
pub fn parse_int(display: &str, value: &str) -> Result<i128, String> {
	py::parse_int(value).ok_or_else(|| {
		format!(
			"argument {display}: invalid int value: {}",
			py::str_repr(value)
		)
	})
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

/// A possible interpretation of an option string (`option_tuple`): the
/// argument (`None` for an unknown option), the option string, whether it was
/// separated from an explicit value by `=`, and the explicit value.
#[derive(Debug, Clone)]
struct OptionTuple {
	action: Option<usize>,
	option_string: String,
	sep: bool,
	explicit: Option<String>,
}

/// The characters of a pattern of argument strings: `A` for an argument, `O`
/// for an option, `-` for `--`.
const A: u8 = 1;
const O: u8 = 2;
const DASH: u8 = 4;

/// A quantified character class of a `nargs` pattern.
#[derive(Debug, Clone, Copy)]
struct Atom {
	set: u8,
	min: usize,
	max: usize,
}

const fn atom(set: u8, min: usize, max: usize) -> Atom {
	Atom { set, min, max }
}

/// `_get_nargs_pattern`, as the atoms of the regular expression.
fn nargs_pattern(argument: &Argument) -> Vec<Atom> {
	let dashes = atom(DASH, 0, usize::MAX);
	if argument.strings.is_empty() {
		match argument.nargs {
			Nargs::Zero => vec![dashes],
			Nargs::One => vec![dashes, atom(A, 1, 1), dashes],
			Nargs::Optional => vec![dashes, atom(A, 0, 1), dashes],
			Nargs::ZeroOrMore => vec![dashes, atom(A | DASH, 0, usize::MAX)],
			Nargs::OneOrMore => vec![dashes, atom(A, 1, 1), atom(A | DASH, 0, usize::MAX)],
		}
	} else {
		match argument.nargs {
			Nargs::Zero => vec![],
			Nargs::One => vec![atom(A, 1, 1)],
			Nargs::Optional => vec![atom(A, 0, 1)],
			Nargs::ZeroOrMore => vec![atom(A, 0, usize::MAX)],
			Nargs::OneOrMore => vec![atom(A, 1, usize::MAX)],
		}
	}
}

/// Matches the groups of atoms at the start of `pattern` as Python's `re.match`
/// does (greedy, backtracking); returns the length of each group.
fn match_groups(groups: &[Vec<Atom>], pattern: &[u8]) -> Option<Vec<usize>> {
	let atoms: Vec<(usize, Atom)> = groups
		.iter()
		.enumerate()
		.flat_map(|(g, atoms)| atoms.iter().map(move |a| (g, *a)))
		.collect();
	let mut lengths = vec![0; atoms.len()];
	if !match_atoms(&atoms, 0, pattern, 0, &mut lengths) {
		return None;
	}
	let mut result = vec![0; groups.len()];
	for ((g, _), len) in atoms.iter().zip(&lengths) {
		result[*g] += len;
	}
	Some(result)
}

fn match_atoms(
	atoms: &[(usize, Atom)],
	index: usize,
	pattern: &[u8],
	pos: usize,
	lengths: &mut [usize],
) -> bool {
	let Some((_, atom)) = atoms.get(index) else {
		return true;
	};
	let available = pattern[pos..]
		.iter()
		.take_while(|&&c| c & atom.set != 0)
		.count()
		.min(atom.max);
	if available < atom.min {
		return false;
	}
	for len in (atom.min..=available).rev() {
		lengths[index] = len;
		if match_atoms(atoms, index + 1, pattern, pos + len, lengths) {
			return true;
		}
	}
	false
}

/// The parser state of [`parse`].
struct State<'a> {
	arguments: &'a [Argument],
	parsed: Parsed,
	seen_actions: BTreeSet<usize>,
	seen_non_default: BTreeSet<usize>,
}

/// What [`State::take_action`] asks the parser to do next.
enum Next {
	Continue,
	/// `-h` or `--version` was given: stop parsing.
	Exit,
}

impl State<'_> {
	/// `_get_values`, then the conflict check and the action of `take_action`.
	fn take_action(&mut self, index: usize, strings: &[String]) -> Result<Next, String> {
		let argument = &self.arguments[index];
		self.seen_actions.insert(index);
		// `_get_values`: the type conversions.
		let single = strings.len() == 1 && matches!(argument.nargs, Nargs::One | Nargs::Optional);
		if argument.nargs != Nargs::Zero {
			for s in strings {
				match argument.ty {
					Ty::Str => {}
					Ty::Float => {
						parse_float(&argument.display(), s)?;
					}
					Ty::Int => {
						parse_int(&argument.display(), s)?;
					}
				}
			}
		}
		if !argument.strings.is_empty() || !strings.is_empty() {
			self.seen_non_default.insert(index);
			if let Some(group) = argument.group
				&& let Some(conflict) = self
					.arguments
					.iter()
					.enumerate()
					.find(|(i, a)| {
						*i != index && a.group == Some(group) && self.seen_non_default.contains(i)
					})
					.map(|(_, a)| a)
			{
				return Err(format!(
					"argument {}: not allowed with argument {}",
					argument.display(),
					conflict.display()
				));
			}
		}
		let seen = self.parsed.seen.entry(argument.key()).or_default();
		seen.count += 1;
		match argument.action {
			Action::Help | Action::Version => return Ok(Next::Exit),
			Action::StoreTrue | Action::Count => {}
			Action::Append => seen.values.extend(strings.iter().cloned()),
			Action::Store => {
				if single {
					seen.value = strings.first().cloned();
				} else if matches!(argument.nargs, Nargs::One | Nargs::Optional) {
					seen.value = None;
				} else {
					seen.values = strings.to_vec();
				}
			}
		}
		Ok(Next::Continue)
	}
}

/// Parses `args` as `ArgumentParser.parse_args` does, for the declared
/// `arguments`; `required_groups` are the mutually exclusive groups with
/// `required=True`. `Err` holds the message of `parser.error`.
///
/// Parsing stops at `-h` or a `version` action: the result then has the
/// argument (see [`Parsed::flag`]), as `argparse` prints and exits there.
pub fn parse(
	args: &[&str],
	arguments: &[Argument],
	required_groups: &[usize],
) -> Result<Parsed, String> {
	// `_option_string_actions`, in the order of the declarations.
	let table: Vec<(&str, usize)> = arguments
		.iter()
		.enumerate()
		.flat_map(|(i, a)| a.strings.iter().map(move |s| (*s, i)))
		.collect();
	let lookup = |s: &str| table.iter().find(|(o, _)| *o == s).map(|(_, i)| *i);
	let has_negative_number_optionals = table.iter().any(|(o, _)| is_negative_number(o));

	// `_get_option_tuples`.
	let option_tuples = |arg: &str| -> Vec<OptionTuple> {
		let mut result = Vec::new();
		let second = arg.chars().nth(1);
		if second == Some('-') {
			let (prefix, sep, explicit) = match arg.split_once('=') {
				Some((p, e)) => (p, true, Some(e.to_owned())),
				None => (arg, false, None),
			};
			for (o, i) in &table {
				if o.starts_with(prefix) {
					result.push(OptionTuple {
						action: Some(*i),
						option_string: (*o).to_owned(),
						sep,
						explicit: explicit.clone(),
					});
				}
			}
		} else {
			let (prefix, sep, explicit) = match arg.split_once('=') {
				Some((p, e)) => (p, true, Some(e.to_owned())),
				None => (arg, false, None),
			};
			let cut = arg.char_indices().nth(2).map_or(arg.len(), |(i, _)| i);
			let (short_prefix, short_explicit) = arg.split_at(cut);
			for (o, i) in &table {
				if *o == short_prefix {
					result.push(OptionTuple {
						action: Some(*i),
						option_string: (*o).to_owned(),
						sep: false,
						explicit: Some(short_explicit.to_owned()),
					});
				} else if o.starts_with(prefix) {
					result.push(OptionTuple {
						action: Some(*i),
						option_string: (*o).to_owned(),
						sep,
						explicit: explicit.clone(),
					});
				}
			}
		}
		result
	};

	// `_parse_optional`.
	let parse_optional = |arg: &str| -> Option<Vec<OptionTuple>> {
		if !arg.starts_with('-') {
			return None;
		}
		if let Some(i) = lookup(arg) {
			return Some(vec![OptionTuple {
				action: Some(i),
				option_string: arg.to_owned(),
				sep: false,
				explicit: None,
			}]);
		}
		if arg.chars().count() == 1 {
			return None;
		}
		if let Some((option, explicit)) = arg.split_once('=')
			&& let Some(i) = lookup(option)
		{
			return Some(vec![OptionTuple {
				action: Some(i),
				option_string: option.to_owned(),
				sep: true,
				explicit: Some(explicit.to_owned()),
			}]);
		}
		let tuples = option_tuples(arg);
		if !tuples.is_empty() {
			return Some(tuples);
		}
		if is_negative_number(arg) && !has_negative_number_optionals {
			return None;
		}
		if arg.contains(' ') {
			return None;
		}
		Some(vec![OptionTuple {
			action: None,
			option_string: arg.to_owned(),
			sep: false,
			explicit: None,
		}])
	};

	// The pattern of the argument strings, and the options at their indices.
	let mut pattern: Vec<u8> = Vec::with_capacity(args.len());
	let mut options: BTreeMap<usize, Vec<OptionTuple>> = BTreeMap::new();
	let mut iter = args.iter().enumerate();
	while let Some((i, arg)) = iter.next() {
		if *arg == "--" {
			pattern.push(DASH);
			pattern.extend(iter.by_ref().map(|_| A));
		} else if let Some(tuples) = parse_optional(arg) {
			options.insert(i, tuples);
			pattern.push(O);
		} else {
			pattern.push(A);
		}
	}

	let mut state = State {
		arguments,
		parsed: Parsed::default(),
		seen_actions: BTreeSet::new(),
		seen_non_default: BTreeSet::new(),
	};
	let mut extras: Vec<String> = Vec::new();
	let mut positionals: Vec<usize> = arguments
		.iter()
		.enumerate()
		.filter(|(_, a)| a.strings.is_empty())
		.map(|(i, _)| i)
		.collect();

	// `_match_argument`.
	let match_argument = |index: usize, pattern: &[u8]| -> Result<usize, String> {
		let argument = &arguments[index];
		match match_groups(&[nargs_pattern(argument)], pattern) {
			Some(lengths) => Ok(lengths[0]),
			None => {
				let message = match argument.nargs {
					Nargs::Optional => "expected at most one argument",
					Nargs::OneOrMore => "expected at least one argument",
					_ => "expected one argument",
				};
				Err(format!("argument {}: {message}", argument.display()))
			}
		}
	};

	// `consume_positionals`; `None` when parsing stops.
	let consume_positionals = |state: &mut State<'_>,
	                           positionals: &mut Vec<usize>,
	                           start: usize|
	 -> Result<Option<usize>, String> {
		let selected = &pattern[start..];
		// `_match_arguments_partial`.
		let mut counts: Vec<usize> = Vec::new();
		for n in (1..=positionals.len()).rev() {
			let groups: Vec<Vec<Atom>> = positionals[..n]
				.iter()
				.map(|&i| nargs_pattern(&arguments[i]))
				.collect();
			if let Some(mut result) = match_groups(&groups, selected) {
				let end: usize = result.iter().sum();
				if end < selected.len() && selected[end] == O {
					while result.last() == Some(&0) {
						result.pop();
					}
				}
				counts = result;
				break;
			}
		}
		let mut start = start;
		for (&index, &count) in positionals.iter().zip(&counts) {
			let mut strings: Vec<String> = args[start..start + count]
				.iter()
				.map(|s| (*s).to_owned())
				.collect();
			if pattern[start..start + count].contains(&DASH)
				&& let Some(at) = strings.iter().position(|s| s == "--")
			{
				strings.remove(at);
			}
			start += count;
			if let Next::Exit = state.take_action(index, &strings)? {
				return Ok(None);
			}
		}
		positionals.drain(..counts.len());
		Ok(Some(start))
	};

	let max_option_index = options.keys().next_back().copied();
	let mut start = 0;
	while let Some(max) = max_option_index
		&& start <= max
	{
		// Consume any positionals preceding the next option.
		let next_option = options.range(start..).next().map_or(max + 1, |(i, _)| *i);
		if start != next_option {
			let Some(end) = consume_positionals(&mut state, &mut positionals, start)? else {
				return Ok(state.parsed);
			};
			if end > start {
				start = end;
				continue;
			}
			start = end;
		}
		if !options.contains_key(&start) {
			extras.extend(args[start..next_option].iter().map(|s| (*s).to_owned()));
			start = next_option;
		}

		// `consume_optional`.
		let tuples = &options[&start];
		if tuples.len() > 1 {
			let matches: Vec<&str> = tuples.iter().map(|t| t.option_string.as_str()).collect();
			return Err(format!(
				"ambiguous option: {} could match {}",
				args[start],
				matches.join(", ")
			));
		}
		let OptionTuple {
			mut action,
			mut option_string,
			mut sep,
			mut explicit,
		} = tuples[0].clone();
		let mut action_tuples: Vec<(usize, Vec<String>)> = Vec::new();
		loop {
			let Some(index) = action else {
				extras.push(args[start].to_owned());
				break;
			};
			match explicit.take() {
				Some(explicit_arg) => {
					let count = match_argument(index, &[A])?;
					let mut chars = option_string.chars();
					chars.next();
					let single_dash = chars.next() != Some('-');
					if count == 0 && single_dash && !explicit_arg.is_empty() {
						if sep || explicit_arg.starts_with('-') {
							return Err(format!(
								"argument {}: ignored explicit argument {}",
								arguments[index].display(),
								py::str_repr(&explicit_arg)
							));
						}
						action_tuples.push((index, Vec::new()));
						let mut rest = explicit_arg.chars();
						let c = rest.next().unwrap_or_default();
						option_string = format!("-{c}");
						match lookup(&option_string) {
							Some(i) => {
								action = Some(i);
								let rest = rest.as_str();
								if rest.is_empty() {
									sep = false;
									explicit = None;
								} else if let Some(r) = rest.strip_prefix('=') {
									sep = true;
									explicit = Some(r.to_owned());
								} else {
									sep = false;
									explicit = Some(rest.to_owned());
								}
							}
							None => {
								extras.push(format!("-{explicit_arg}"));
								break;
							}
						}
					} else if count == 1 {
						action_tuples.push((index, vec![explicit_arg]));
						break;
					} else {
						return Err(format!(
							"argument {}: ignored explicit argument {}",
							arguments[index].display(),
							py::str_repr(&explicit_arg)
						));
					}
				}
				None => {
					let count = match_argument(index, &pattern[start + 1..])?;
					let strings = args[start + 1..start + 1 + count]
						.iter()
						.map(|s| (*s).to_owned())
						.collect();
					action_tuples.push((index, strings));
					start += count;
					break;
				}
			}
		}
		let stop = start + 1;
		for (index, strings) in action_tuples {
			if let Next::Exit = state.take_action(index, &strings)? {
				return Ok(state.parsed);
			}
		}
		start = stop;
	}

	// Consume any positionals following the last option.
	let Some(stop) = consume_positionals(&mut state, &mut positionals, start)? else {
		return Ok(state.parsed);
	};
	extras.extend(args[stop..].iter().map(|s| (*s).to_owned()));

	let required: Vec<String> = arguments
		.iter()
		.enumerate()
		.filter(|(i, a)| a.required && !state.seen_actions.contains(i))
		.map(|(_, a)| a.display())
		.collect();
	if !required.is_empty() {
		return Err(format!(
			"the following arguments are required: {}",
			required.join(", ")
		));
	}
	for &group in required_groups {
		let members: Vec<(usize, &Argument)> = arguments
			.iter()
			.enumerate()
			.filter(|(_, a)| a.group == Some(group))
			.collect();
		if !members
			.iter()
			.any(|(i, _)| state.seen_non_default.contains(i))
		{
			let names: Vec<String> = members.iter().map(|(_, a)| a.display()).collect();
			return Err(format!(
				"one of the arguments {} is required",
				names.join(" ")
			));
		}
	}
	if !extras.is_empty() {
		return Err(format!("unrecognized arguments: {}", extras.join(" ")));
	}
	Ok(state.parsed)
}

/// A command line declared with `argparse`: its `prog`, its module (for
/// [`program_text`]), its help text as `argparse` prints it, and its arguments.
#[derive(Debug, Clone, Copy)]
pub struct Command {
	/// The `prog` of the parser, e.g. `"fonttools ttLib.scaleUpem"`.
	pub prog: &'static str,
	/// The module, without `fontTools.`, e.g. `"ttLib.scaleUpem"`.
	pub module: &'static str,
	/// `parser.format_help()`; its usage is the text before the first empty
	/// line.
	pub help: &'static str,
	/// The arguments.
	pub arguments: &'static [Argument],
	/// The mutually exclusive groups with `required=True`.
	pub required_groups: &'static [usize],
}

impl Command {
	/// `parser.format_usage()`.
	pub fn usage(&self) -> &'static str {
		match self.help.find("\n\n") {
			Some(at) => &self.help[..at + 1],
			None => self.help,
		}
	}

	/// `parser.print_help()`: writes the help to `out`.
	pub fn print_help(&self, out: &mut dyn Write) -> std::io::Result<()> {
		out.write_all(program_text(self.help, self.prog, self.module).as_bytes())
	}

	/// `parser.error(message)`: writes the usage and the message to `err`;
	/// returns the exit code, 2.
	pub fn error(&self, err: &mut dyn Write, message: &str) -> std::io::Result<i32> {
		let text = format!("{}{}: error: {message}\n", self.usage(), self.prog);
		err.write_all(program_text(&text, self.prog, self.module).as_bytes())?;
		Ok(2)
	}

	/// `parser.parse_args(args)`: the parsed arguments, or the exit code when
	/// the help was printed to `out` (0) or an error to `err` (2).
	pub fn parse_args(
		&self,
		args: &[&str],
		out: &mut dyn Write,
		err: &mut dyn Write,
	) -> std::io::Result<Result<Parsed, i32>> {
		match parse(args, self.arguments, self.required_groups) {
			Ok(parsed) if parsed.flag("--help") => {
				self.print_help(out)?;
				Ok(Err(0))
			}
			Ok(parsed) => Ok(Ok(parsed)),
			Err(message) => Ok(Err(self.error(err, &message)?)),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const ARGS: &[Argument] = &[
		Argument::help(),
		Argument::count(&["-v", "--verbose"]),
		Argument::option(&["-e", "--conversion-error"]).ty(Ty::Float),
		Argument::flag(&["-m", "--mixed"]),
		Argument::option(&["-o", "--output-file"]).group(0),
		Argument::option(&["-d", "--output-dir"]).group(0),
		Argument::positional("INPUT").nargs(Nargs::OneOrMore),
	];

	fn error(args: &[&str]) -> String {
		parse(args, ARGS, &[]).err().unwrap_or_default()
	}

	#[test]
	fn messages() {
		assert_eq!(error(&[]), "the following arguments are required: INPUT");
		assert_eq!(error(&["-vx", "a"]), "unrecognized arguments: -x");
		assert_eq!(
			error(&["-ex", "a"]),
			"argument -e/--conversion-error: invalid float value: 'x'"
		);
		assert_eq!(
			error(&["--mixed=3", "a"]),
			"argument -m/--mixed: ignored explicit argument '3'"
		);
		assert_eq!(
			error(&["--o=x", "a"]),
			"ambiguous option: --o=x could match --output-file, --output-dir"
		);
		assert_eq!(
			error(&["a", "-o"]),
			"argument -o/--output-file: expected one argument"
		);
		assert_eq!(
			error(&["-o", "x", "-d", "y", "a"]),
			"argument -d/--output-dir: not allowed with argument -o/--output-file"
		);
		assert_eq!(error(&["a", "-m", "b"]), "unrecognized arguments: b");
	}

	#[test]
	fn values() -> Result<(), String> {
		let parsed = parse(&["-vv", "-e0.5", "a", "b", "--", "-c"], ARGS, &[])?;
		assert_eq!(parsed.count("--verbose"), 2);
		assert_eq!(parsed.str("--conversion-error"), Some("0.5"));
		assert_eq!(parsed.list("INPUT"), ["a", "b", "-c"]);
		let parsed = parse(&["--output-f", "x", "-h"], ARGS, &[])?;
		assert_eq!(parsed.str("--output-file"), Some("x"));
		assert!(parsed.flag("--help"));
		Ok(())
	}

	#[test]
	fn optional_positionals() -> Result<(), String> {
		const ARGS: &[Argument] = &[
			Argument::help(),
			Argument::flag(&["--keep"]),
			Argument::positional("INPUT"),
			Argument::positional("OUTPUT").nargs(Nargs::Optional),
			Argument::positional("GLYPHS").nargs(Nargs::ZeroOrMore),
		];
		let parsed = parse(&["a", "--keep", "b", "c", "d"], ARGS, &[])?;
		assert_eq!(parsed.str("INPUT"), Some("a"));
		assert_eq!(parsed.str("OUTPUT"), Some("b"));
		assert_eq!(parsed.list("GLYPHS"), ["c", "d"]);
		let parsed = parse(&["a"], ARGS, &[])?;
		assert_eq!(parsed.str("OUTPUT"), None);
		assert!(parsed.list("GLYPHS").is_empty());
		// Python 3.13 leaves `nargs="*"` for after an option, rather than
		// consuming nothing for it before.
		const STAR: &[Argument] = &[
			Argument::help(),
			Argument::flag(&["-c"]),
			Argument::positional("font"),
			Argument::positional("glyphs").nargs(Nargs::ZeroOrMore),
		];
		let parsed = parse(&["a", "-c", "g1", "g2"], STAR, &[])?;
		assert_eq!(parsed.list("glyphs"), ["g1", "g2"]);
		Ok(())
	}

	#[test]
	fn required_groups() {
		const ARGS: &[Argument] = &[
			Argument::help(),
			Argument::option(&["-d", "--designspace"]).group(0),
			Argument::option(&["-l", "--locations"])
				.nargs(Nargs::OneOrMore)
				.group(0),
		];
		assert_eq!(
			parse(&[], ARGS, &[0]).err().unwrap_or_default(),
			"one of the arguments -d/--designspace -l/--locations is required"
		);
		assert_eq!(
			parse(&["-l"], ARGS, &[0]).err().unwrap_or_default(),
			"argument -l/--locations: expected at least one argument"
		);
		assert!(
			parse(&["-l", "0,0", "1,0"], ARGS, &[0])
				.is_ok_and(|p| p.list("--locations").len() == 2)
		);
	}
}
