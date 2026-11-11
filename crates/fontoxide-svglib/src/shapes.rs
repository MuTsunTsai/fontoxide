//! Converting SVG basic shapes to path data (`fontTools.svgLib.path.shapes`).

use fontoxide_misc::py::{is_space, parse_float, str_repr};

use crate::{Element, Error, Result};

/// `_prefer_non_zero`: the first value that is not zero.
fn prefer_non_zero(args: &[f64]) -> f64 {
	args.iter().copied().find(|&a| a != 0.0).unwrap_or(0.0)
}

/// `_ntos`: `"%.3f"` without trailing zeros.
pub fn ntos(n: f64) -> String {
	let s = if n.is_nan() {
		"nan".to_owned()
	} else if n.is_infinite() {
		if n > 0.0 { "inf" } else { "-inf" }.to_owned()
	} else {
		format!("{n:.3}")
	};
	s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `float(text)`.
fn py_float(text: &str) -> Result<f64> {
	parse_float(text).ok_or_else(|| {
		Error::Value(format!(
			"could not convert string to float: {}",
			str_repr(text)
		))
	})
}

/// `_transform`: the six values of a `matrix(...)` transform.
pub fn parse_transform(raw_value: &str) -> Result<[f64; 6]> {
	// `re.match(r"matrix\((.*)\)", raw_value)`: `.` doesn't match newlines,
	// and `.*` is greedy.
	let Some(rest) = raw_value.strip_prefix("matrix(") else {
		return Err(Error::NotImplemented(raw_value.to_owned()));
	};
	let line = rest.split('\n').next().unwrap_or_default();
	let Some(end) = line.rfind(')') else {
		return Err(Error::NotImplemented(raw_value.to_owned()));
	};
	// `re.split(r"\s+|,", ...)`
	let mut parts: Vec<String> = vec![String::new()];
	let mut chars = line[..end].chars().peekable();
	while let Some(c) = chars.next() {
		if is_space(c) {
			while chars.peek().is_some_and(|&c| is_space(c)) {
				chars.next();
			}
			parts.push(String::new());
		} else if c == ',' {
			parts.push(String::new());
		} else if let Some(last) = parts.last_mut() {
			last.push(c);
		}
	}
	let matrix = parts
		.iter()
		.map(|p| py_float(p))
		.collect::<Result<Vec<f64>>>()?;
	<[f64; 6]>::try_from(matrix)
		.map_err(|_| Error::Value(format!("wrong # of terms in {raw_value}")))
}

/// Collects the path data of SVG elements (`PathBuilder`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathBuilder {
	/// The path data of each shape (`paths`).
	pub paths: Vec<String>,
	/// The `matrix(...)` transform of each shape (`transforms`).
	pub transforms: Vec<Option<[f64; 6]>>,
}

impl PathBuilder {
	/// `PathBuilder()`.
	pub fn new() -> Self {
		Self::default()
	}

	fn start_path(&mut self, initial_path: String) {
		self.paths.push(initial_path);
		self.transforms.push(None);
	}

	fn end_path(&mut self) {
		self.add("z");
	}

	fn add(&mut self, path_snippet: &str) {
		if let Some(path) = self.paths.last_mut() {
			if !path.is_empty() {
				path.push(' ');
			}
			path.push_str(path_snippet);
		}
	}

	fn move_to(&mut self, c: char, x: f64, y: f64) {
		self.add(&format!("{c}{},{}", ntos(x), ntos(y)));
	}

	fn arc(&mut self, c: char, rx: f64, ry: f64, x: f64, y: f64, large_arc: i32) {
		self.add(&format!(
			"{c}{},{} 0 {large_arc} 1 {},{}",
			ntos(rx),
			ntos(ry),
			ntos(x),
			ntos(y)
		));
	}

	fn vhline(&mut self, c: char, x: f64) {
		self.add(&format!("{c}{}", ntos(x)));
	}

	fn line(&mut self, c: char, x: f64, y: f64) {
		self.add(&format!("{c}{},{}", ntos(x), ntos(y)));
	}

	/// `float(el.attrib.get(name, default))`; a missing attribute without a
	/// default is `float(None)`.
	fn attr(el: &Element, name: &str, default: Option<f64>) -> Result<f64> {
		match (el.get(name), default) {
			(Some(v), _) => py_float(v),
			(None, Some(d)) => Ok(d),
			(None, None) => Err(Error::Value(
				"float() argument must be a string or a real number, not 'NoneType'".into(),
			)),
		}
	}

	fn parse_line(&mut self, line: &Element) -> Result<()> {
		let x1 = Self::attr(line, "x1", Some(0.0))?;
		let y1 = Self::attr(line, "y1", Some(0.0))?;
		let x2 = Self::attr(line, "x2", Some(0.0))?;
		let y2 = Self::attr(line, "y2", Some(0.0))?;
		self.start_path(String::new());
		self.move_to('M', x1, y1);
		self.line('L', x2, y2);
		Ok(())
	}

	fn parse_rect(&mut self, rect: &Element) -> Result<()> {
		let x = Self::attr(rect, "x", Some(0.0))?;
		let y = Self::attr(rect, "y", Some(0.0))?;
		let w = Self::attr(rect, "width", None)?;
		let h = Self::attr(rect, "height", None)?;
		let rx = Self::attr(rect, "rx", Some(0.0))?;
		let ry = Self::attr(rect, "ry", Some(0.0))?;

		let rx = prefer_non_zero(&[rx, ry]);
		let ry = prefer_non_zero(&[ry, rx]);
		// TODO there are more rules for adjusting rx, ry

		self.start_path(String::new());
		self.move_to('M', x + rx, y);
		self.vhline('H', x + w - rx);
		if rx > 0.0 {
			self.arc('A', rx, ry, x + w, y + ry, 0);
		}
		self.vhline('V', y + h - ry);
		if rx > 0.0 {
			self.arc('A', rx, ry, x + w - rx, y + h, 0);
		}
		self.vhline('H', x + rx);
		if rx > 0.0 {
			self.arc('A', rx, ry, x, y + h - ry, 0);
		}
		self.vhline('V', y + ry);
		if rx > 0.0 {
			self.arc('A', rx, ry, x + rx, y, 0);
		}
		self.end_path();
		Ok(())
	}

	fn parse_path(&mut self, path: &Element) {
		if let Some(d) = path.get("d") {
			self.start_path(d.to_owned());
		}
	}

	fn parse_polygon(&mut self, poly: &Element) {
		if let Some(points) = poly.get("points") {
			self.start_path(format!("M{points}"));
			self.end_path();
		}
	}

	fn parse_polyline(&mut self, poly: &Element) {
		if let Some(points) = poly.get("points") {
			self.start_path(format!("M{points}"));
		}
	}

	fn parse_circle(&mut self, circle: &Element) -> Result<()> {
		let cx = Self::attr(circle, "cx", Some(0.0))?;
		let cy = Self::attr(circle, "cy", Some(0.0))?;
		let r = Self::attr(circle, "r", None)?;
		// arc doesn't seem to like being a complete shape, draw two halves
		self.start_path(String::new());
		self.move_to('M', cx - r, cy);
		self.arc('A', r, r, cx + r, cy, 1);
		self.arc('A', r, r, cx - r, cy, 1);
		Ok(())
	}

	fn parse_ellipse(&mut self, ellipse: &Element) -> Result<()> {
		let cx = Self::attr(ellipse, "cx", Some(0.0))?;
		let cy = Self::attr(ellipse, "cy", Some(0.0))?;
		let rx = Self::attr(ellipse, "rx", None)?;
		let ry = Self::attr(ellipse, "ry", None)?;
		// arc doesn't seem to like being a complete shape, draw two halves
		self.start_path(String::new());
		self.move_to('M', cx - rx, cy);
		self.arc('A', rx, ry, cx + rx, cy, 1);
		self.arc('A', rx, ry, cx - rx, cy, 1);
		Ok(())
	}

	/// Adds the path of a shape element (`add_path_from_element`): `path`,
	/// `line`, `rect`, `polygon`, `polyline`, `circle` or `ellipse`, ignoring
	/// the namespace and the case of the tag. Returns whether the element is
	/// one of those.
	///
	/// As in fontTools, the `transform` of a shape element is set on the last
	/// path even if the element added none (a `path` without `d`).
	pub fn add_path_from_element(&mut self, el: &Element) -> Result<bool> {
		match el.tag.to_lowercase().as_str() {
			"line" => self.parse_line(el)?,
			"rect" => self.parse_rect(el)?,
			"path" => self.parse_path(el),
			"polygon" => self.parse_polygon(el),
			"polyline" => self.parse_polyline(el),
			"circle" => self.parse_circle(el)?,
			"ellipse" => self.parse_ellipse(el)?,
			_ => return Ok(false),
		}
		if let Some(transform) = el.get("transform") {
			let transform = parse_transform(transform)?;
			let last = self
				.transforms
				.last_mut()
				.ok_or_else(|| Error::Value("list assignment index out of range".into()))?;
			*last = Some(transform);
		}
		Ok(true)
	}
}
