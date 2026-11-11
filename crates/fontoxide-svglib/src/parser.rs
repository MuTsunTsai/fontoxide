//! SVG path data parser (`fontTools.svgLib.path.parser`), an adaptation of
//! `svg.path` by Lennart Regebro that draws with a pen.

use fontoxide_misc::complex::{Complex, J};
use fontoxide_misc::py::{is_space, parse_float, str_repr};
use fontoxide_pens::{Pen, Point};

use crate::arc::EllipticalArc;
use crate::{ArcArgumentError, ArcPen, Error, Result, Target};

/// `COMMANDS`.
fn is_command(c: char) -> bool {
	"MmZzLlHhVvCcSsQqTtAa".contains(c)
}

/// A token of the path data: a command or a number.
#[derive(Debug, Clone, PartialEq)]
enum Token {
	Command(char),
	Number(String),
}

/// `FLOAT_RE` (https://www.w3.org/TR/css-syntax-3/#number-token-diagram)
/// matched at `start`: the end of the match.
fn match_float(s: &[u8], start: usize) -> Option<usize> {
	let digits = |mut i: usize| {
		let from = i;
		while s.get(i).is_some_and(u8::is_ascii_digit) {
			i += 1;
		}
		(i > from).then_some(i)
	};
	// `(?:[eE][-+]?[0-9]+)?`
	let exponent = |i: usize| {
		if matches!(s.get(i), Some(b'e' | b'E')) {
			let j = if matches!(s.get(i + 1), Some(b'-' | b'+')) {
				i + 2
			} else {
				i + 1
			};
			if let Some(end) = digits(j) {
				return end;
			}
		}
		i
	};
	let unsigned = |i: usize| -> Option<usize> {
		match s.get(i) {
			// `(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][-+]?[0-9]+)?`
			Some(b'0'..=b'9') => {
				let mut end = if s[i] == b'0' { i + 1 } else { digits(i)? };
				if s.get(end) == Some(&b'.')
					&& let Some(e) = digits(end + 1)
				{
					end = e;
				}
				Some(exponent(end))
			}
			// `\.[0-9]+(?:[eE][-+]?[0-9]+)?`
			Some(b'.') => Some(exponent(digits(i + 1)?)),
			_ => None,
		}
	};
	if matches!(s.get(start), Some(b'-' | b'+'))
		&& let Some(end) = unsigned(start + 1)
	{
		return Some(end);
	}
	unsigned(start)
}

/// `FLOAT_RE.findall(s)`.
fn find_floats(s: &str) -> Vec<String> {
	let b = s.as_bytes();
	let mut out = Vec::new();
	let mut i = 0;
	while i < b.len() {
		match match_float(b, i) {
			Some(end) => {
				out.push(s[i..end].to_owned());
				i = end;
			}
			None => i += 1,
		}
	}
	out
}

/// `FLOAT_RE.search(s)`: the span of the first match.
fn search_float(s: &str) -> Option<(usize, usize)> {
	let b = s.as_bytes();
	(0..b.len()).find_map(|i| match_float(b, i).map(|end| (i, end)))
}

/// The names of the arguments of an arc command (`ARC_ARGUMENT_TYPES`); the
/// flags match `^[01]`, the others `FLOAT_RE`.
const ARC_ARGUMENT_NAMES: [&str; 7] = [
	"rx",
	"ry",
	"x-axis-rotation",
	"large-arc-flag",
	"sweep-flag",
	"x",
	"y",
];

/// `_tokenize_arc_arguments`: the arguments of arc commands, where flags
/// need no separators.
fn tokenize_arc_arguments(arcdef: &str) -> std::result::Result<Vec<String>, ArcArgumentError> {
	// `SEPARATOR_RE.split(arcdef)`, without the empty strings
	let mut raw_args: Vec<String> = arcdef
		.split([',', ' ', '\t'])
		.filter(|s| !s.is_empty())
		.map(str::to_owned)
		.collect();
	if raw_args.is_empty() {
		return Err(ArcArgumentError(format!(
			"Not enough arguments: '{arcdef}'"
		)));
	}
	raw_args.reverse();
	let mut out = Vec::new();
	let mut i = 0;
	while let Some(arg) = raw_args.pop() {
		let name = ARC_ARGUMENT_NAMES[i];
		let span = if i == 3 || i == 4 {
			// `^[01]`
			matches!(arg.as_bytes().first(), Some(b'0' | b'1')).then_some((0, 1))
		} else {
			search_float(&arg)
		};
		let Some((j, k)) = span else {
			return Err(ArcArgumentError(format!(
				"Invalid argument for '{name}' parameter: {}",
				str_repr(&arg)
			)));
		};
		out.push(arg[j..k].to_owned());
		let rest = &arg[k..];
		if !rest.is_empty() {
			raw_args.push(rest.to_owned());
		}
		// wrap around every 7 consecutive arguments
		i = if i == 6 { 0 } else { i + 1 };
	}
	if i != 0 {
		return Err(ArcArgumentError(format!(
			"Not enough arguments: '{arcdef}'"
		)));
	}
	Ok(out)
}

/// `_tokenize_path`.
fn tokenize_path(pathdef: &str) -> Result<Vec<Token>> {
	let mut tokens = Vec::new();
	let mut arc_cmd: Option<char> = None;
	// `COMMAND_RE.split(pathdef)`: the text between commands, and the commands.
	let mut pieces: Vec<&str> = Vec::new();
	let mut start = 0;
	for (i, c) in pathdef.char_indices() {
		if is_command(c) {
			pieces.push(&pathdef[start..i]);
			pieces.push(&pathdef[i..i + 1]);
			start = i + 1;
		}
	}
	pieces.push(&pathdef[start..]);
	for x in pieces {
		let mut chars = x.chars();
		if let (Some(c), None) = (chars.next(), chars.next())
			&& is_command(c)
		{
			arc_cmd = matches!(c, 'A' | 'a').then_some(c);
			tokens.push(Token::Command(c));
			continue;
		}
		match arc_cmd {
			Some(cmd) => match tokenize_arc_arguments(x) {
				Ok(args) => tokens.extend(args.into_iter().map(Token::Number)),
				Err(cause) => {
					return Err(Error::InvalidArc {
						command: format!("{cmd}{x}"),
						cause,
					});
				}
			},
			None => tokens.extend(find_floats(x).into_iter().map(Token::Number)),
		}
	}
	Ok(tokens)
}

/// `float(token)`.
fn to_float(token: &str) -> Result<f64> {
	parse_float(token).ok_or_else(|| {
		Error::Value(format!(
			"could not convert string to float: {}",
			str_repr(token)
		))
	})
}

/// `cmath.isclose(a, b, rel_tol=rel_tol, abs_tol=abs_tol)`.
fn cmath_isclose(a: Complex, b: Complex, rel_tol: f64, abs_tol: f64) -> bool {
	if a == b {
		return true;
	}
	if a.re.is_infinite() || a.im.is_infinite() || b.re.is_infinite() || b.im.is_infinite() {
		return false;
	}
	let diff = (a - b).abs();
	diff <= rel_tol * b.abs() || diff <= rel_tol * a.abs() || diff <= abs_tol
}

/// `float(x) + float(y) * 1j`.
fn complex_of(x: f64, y: f64) -> Complex {
	Complex::real(x) + y * J
}

fn pt(z: Complex) -> Point {
	(z.re, z.im)
}

/// Parses SVG path data (the `d` attribute of `<path>` elements) and draws it
/// with the pen (`parse_path`). Elliptical arcs are approximated by cubic
/// Bézier curves, one every 90 degrees; see [`parse_path_arcs`] for pens that
/// take arcs.
///
/// The initial moveto is relative to `current_pos` if it is relative (`m`);
/// in the SVG specification, it is absolute, which `(0.0, 0.0)` gives.
///
/// ```
/// use fontoxide_pens::RecordingPen;
/// use fontoxide_pens::recording_pen::PenOp;
/// let mut pen = RecordingPen::new();
/// fontoxide_svglib::parse_path("M 0 0 L 10 0 z", &mut pen, (0.0, 0.0)).unwrap();
/// assert_eq!(pen.value.len(), 4);
/// assert_eq!(pen.value[2], PenOp::LineTo((0.0, 0.0)));
/// ```
pub fn parse_path(pathdef: &str, pen: &mut dyn Pen, current_pos: Point) -> Result<()> {
	parse(pathdef, Target::Plain(pen), current_pos)
}

/// [`parse_path`] for a pen that takes elliptical arcs: it is called with the
/// original values of the arc commands (`pen.arcTo(rx, ry, rotation,
/// arc_large, arc_sweep, (x, y))`).
pub fn parse_path_arcs(pathdef: &str, pen: &mut dyn ArcPen, current_pos: Point) -> Result<()> {
	parse(pathdef, Target::Arcs(pen), current_pos)
}

pub(crate) fn parse(pathdef: &str, mut pen: Target<'_>, current_pos: Point) -> Result<()> {
	// In the SVG specs, initial movetos are absolute, even if specified as 'm'.
	// But if you pass in a current_pos variable, the initial moveto will be
	// relative to that current_pos.
	let mut current_pos = Complex::from_point(current_pos);

	let mut elements = tokenize_path(pathdef)?;
	// Reverse for easy use of .pop()
	elements.reverse();

	// A path data segment (if there is one) must begin with a moveto command
	// (https://www.w3.org/TR/SVG11/paths.html#PathDataMovetoCommands); raise
	// before any pen call instead of inventing a start point.
	if let Some(last) = elements.last()
		&& !matches!(last, Token::Command('M' | 'm'))
	{
		return Err(Error::Value(format!(
			"Path must start with a moveto command: {}",
			str_repr(pathdef)
		)));
	}

	// start_pos is the initial point of the current subpath; it is retained
	// after a closepath so that a drawto command following Z can start a new
	// subpath at that same point, while subpath_open tracks whether there is a
	// subpath in progress that still needs closePath/endPath.
	let mut start_pos: Option<Complex> = None;
	let mut subpath_open = false;
	let mut command: Option<char> = None;
	let mut last_control = Complex::default();
	let mut absolute = false;

	// `pop_coord`: a drawto command consumes a fixed number of coordinate
	// tokens; a malformed path can run the token list dry.
	fn pop_coord(elements: &mut Vec<Token>, command: Option<char>, pathdef: &str) -> Result<f64> {
		match elements.pop() {
			Some(Token::Number(n)) => to_float(&n),
			Some(Token::Command(c)) => Err(Error::Value(format!(
				"could not convert string to float: {}",
				str_repr(&c.to_string())
			))),
			None => Err(Error::Value(format!(
				"Not enough coordinates for command {} in path: {}",
				command.map_or("None".to_owned(), |c| str_repr(&c.to_string())),
				str_repr(pathdef)
			))),
		}
	}

	while let Some(last) = elements.last() {
		let last_command;
		if let Token::Command(c) = *last {
			// New command.
			last_command = command; // Used by S and T
			elements.pop();
			absolute = c.is_ascii_uppercase();
			command = Some(c.to_ascii_uppercase());
		} else {
			// If this element starts with numbers, it is an implicit command and
			// we don't change the command. Check that it's allowed:
			if command.is_none() {
				let words = pathdef.split(is_space).filter(|w| !w.is_empty()).count();
				return Err(Error::Value(format!(
					"Unallowed implicit command in {pathdef}, position {}",
					words as i64 - elements.len() as i64
				)));
			}
			last_command = command; // Used by S and T
		}
		let Some(cmd) = command else {
			continue;
		};

		if cmd != 'M'
			&& cmd != 'Z'
			&& !subpath_open
			&& let Some(start) = start_pos
		{
			// If a closepath is followed immediately by any other command, the
			// next subpath starts at the same initial point as the current
			// subpath:
			// https://www.w3.org/TR/SVG11/paths.html#PathDataClosePathCommand
			pen.pen().move_to(pt(start))?;
			subpath_open = true;
		}

		let current_command = command;
		let mut pop = || pop_coord(&mut elements, current_command, pathdef);
		match cmd {
			'M' => {
				// Moveto command.
				let x = pop()?;
				let y = pop()?;
				let pos = complex_of(x, y);
				if absolute {
					current_pos = pos;
				} else {
					current_pos += pos;
				}
				// M is not preceded by Z; it's an open subpath
				if subpath_open {
					pen.pen().end_path()?;
				}
				pen.pen().move_to(pt(current_pos))?;
				// when M is called, reset start_pos
				start_pos = Some(current_pos);
				subpath_open = true;
				// Implicit moveto commands are treated as lineto commands.
				command = Some('L');
			}
			'Z' => {
				// Close path. A redundant Z with no open subpath (e.g.
				// "M0,0 Z Z") is valid and has no effect.
				if subpath_open && let Some(start) = start_pos {
					if !cmath_isclose(current_pos, start, 1e-15, 1e-15) {
						pen.pen().line_to(pt(start))?;
					}
					pen.pen().close_path()?;
					current_pos = start;
					subpath_open = false;
				}
				// You can't have implicit commands after closing.
				command = None;
			}
			'L' => {
				let x = pop()?;
				let y = pop()?;
				let mut pos = complex_of(x, y);
				if !absolute {
					pos += current_pos;
				}
				pen.pen().line_to(pt(pos))?;
				current_pos = pos;
			}
			'H' => {
				let x = pop()?;
				let mut pos = Complex::real(x) + current_pos.im * J;
				if !absolute {
					pos += Complex::real(current_pos.re);
				}
				pen.pen().line_to(pt(pos))?;
				current_pos = pos;
			}
			'V' => {
				let y = pop()?;
				let mut pos = Complex::real(current_pos.re) + y * J;
				if !absolute {
					pos += current_pos.im * J;
				}
				pen.pen().line_to(pt(pos))?;
				current_pos = pos;
			}
			'C' => {
				let mut control1 = complex_of(pop()?, pop()?);
				let mut control2 = complex_of(pop()?, pop()?);
				let mut end = complex_of(pop()?, pop()?);
				if !absolute {
					control1 += current_pos;
					control2 += current_pos;
					end += current_pos;
				}
				pen.pen().curve_to(&[pt(control1), pt(control2), pt(end)])?;
				current_pos = end;
				last_control = control2;
			}
			'S' => {
				// Smooth curve. First control point is the "reflection" of the
				// second control point in the previous path; last_command is None
				// right after a closepath (Z is not C/S).
				let control1 = if matches!(last_command, Some('C' | 'S')) {
					current_pos + current_pos - last_control
				} else {
					current_pos
				};
				let mut control2 = complex_of(pop()?, pop()?);
				let mut end = complex_of(pop()?, pop()?);
				if !absolute {
					control2 += current_pos;
					end += current_pos;
				}
				pen.pen().curve_to(&[pt(control1), pt(control2), pt(end)])?;
				current_pos = end;
				last_control = control2;
			}
			'Q' => {
				let mut control = complex_of(pop()?, pop()?);
				let mut end = complex_of(pop()?, pop()?);
				if !absolute {
					control += current_pos;
					end += current_pos;
				}
				pen.pen().q_curve_to(&[pt(control)], Some(pt(end)))?;
				current_pos = end;
				last_control = control;
			}
			'T' => {
				// Smooth curve. Control point is the "reflection" of the second
				// control point in the previous path; last_command is None right
				// after a closepath (Z is not Q/T).
				let control = if matches!(last_command, Some('Q' | 'T')) {
					current_pos + current_pos - last_control
				} else {
					current_pos
				};
				let mut end = complex_of(pop()?, pop()?);
				if !absolute {
					end += current_pos;
				}
				pen.pen().q_curve_to(&[pt(control)], Some(pt(end)))?;
				current_pos = end;
				last_control = control;
			}
			'A' => {
				let rx = pop()?.abs();
				let ry = pop()?.abs();
				let rotation = pop()?;
				// `bool(int(token))`: the flags are "0" or "1".
				let arc_large = pop()? != 0.0;
				let arc_sweep = pop()? != 0.0;
				let mut end = complex_of(pop()?, pop()?);
				if !absolute {
					end += current_pos;
				}
				// if the pen supports arcs, pass the values unchanged, otherwise
				// approximate the arc with a series of cubic bezier curves
				match &mut pen {
					Target::Arcs(p) => p.arc_to(rx, ry, rotation, arc_large, arc_sweep, pt(end))?,
					Target::Plain(p) => {
						let mut arc = EllipticalArc::new(
							current_pos,
							rx,
							ry,
							rotation,
							arc_large,
							arc_sweep,
							end,
						);
						arc.draw(&mut **p)?;
					}
				}
				current_pos = end;
			}
			_ => {}
		}
	}

	// no final Z command, it's an open path
	if subpath_open {
		pen.pen().end_path()?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn float_re() {
		assert_eq!(
			find_floats("1-2.5e3.5,-.5e+1 007 1.e5 +-3 6.e-5"),
			[
				"1", "-2.5e3", ".5", "-.5e+1", "0", "0", "7", "1", "5", "-3", "6", "-5"
			]
		);
	}
}
