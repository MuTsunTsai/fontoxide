//! `fontTools.svgLib.path.SVGPath`: drawing the shapes of an SVG document.

use std::path::Path;

use fontoxide_misc::transform::Transform;
use fontoxide_pens::{Pen, TransformPen};

use crate::parser::parse;
use crate::shapes::PathBuilder;
use crate::{ArcPen, Error, Result, Target};

/// An element of a parsed SVG document (the parts of an lxml element that
/// svgLib uses).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
	/// The tag without its namespace.
	pub tag: String,
	/// The attributes in document order; names in a namespace are in Clark
	/// notation (`{namespace}name`), as in lxml.
	pub attrib: Vec<(String, String)>,
	/// The text before the first child element (`text`).
	pub text: Option<String>,
	/// The child elements; comments and processing instructions are left out.
	pub children: Vec<Element>,
}

impl Element {
	/// `etree.fromstring(data)`: parses an XML document and returns its root.
	/// External entities are never loaded.
	pub fn fromstring(data: &str) -> Result<Element> {
		let data = data.strip_prefix('\u{feff}').unwrap_or(data);
		let options = roxmltree::ParsingOptions {
			allow_dtd: true,
			..Default::default()
		};
		let doc = roxmltree::Document::parse_with_options(data, options)
			.map_err(|e| Error::Xml(e.to_string()))?;
		Ok(convert(doc.root_element()))
	}

	/// `el.attrib.get(name)`.
	pub fn get(&self, name: &str) -> Option<&str> {
		self.attrib
			.iter()
			.find(|(k, _)| k == name)
			.map(|(_, v)| v.as_str())
	}

	/// `el.iter()`: the element and its descendants, in document order.
	pub fn iter(&self) -> impl Iterator<Item = &Element> {
		let mut stack = vec![self];
		std::iter::from_fn(move || {
			let el = stack.pop()?;
			stack.extend(el.children.iter().rev());
			Some(el)
		})
	}
}

fn convert(node: roxmltree::Node<'_, '_>) -> Element {
	let attrib = node
		.attributes()
		.map(|a| {
			let name = match a.namespace() {
				Some(ns) => format!("{{{ns}}}{}", a.name()),
				None => a.name().to_owned(),
			};
			(name, a.value().to_owned())
		})
		.collect();
	let mut text: Option<String> = None;
	let mut children = Vec::new();
	for child in node.children() {
		if child.is_element() {
			children.push(convert(child));
		} else if child.is_text() && children.is_empty() {
			text.get_or_insert_with(String::new)
				.push_str(child.text().unwrap_or_default());
		}
	}
	Element {
		tag: node.tag_name().name().to_owned(),
		attrib,
		text,
		children,
	}
}

/// The shapes of an SVG document, to draw with a pen (`SVGPath`).
///
/// ```
/// use fontoxide_pens::RecordingPen;
/// use fontoxide_svglib::SvgPath;
/// let svg = SvgPath::fromstring(
///     "<svg xmlns='http://www.w3.org/2000/svg'><rect width='1' height='1'/></svg>",
///     None,
/// )
/// .unwrap();
/// let mut pen = RecordingPen::new();
/// svg.draw(&mut pen).unwrap();
/// assert_eq!(pen.value.len(), 6);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SvgPath {
	/// The root element (`root`), `None` for an empty tree.
	pub root: Option<Element>,
	/// The transformation applied to the drawing (`transform`).
	pub transform: Option<Transform>,
}

impl SvgPath {
	/// `SVGPath(filename, transform)`: reads an SVG file. The file must be in
	/// UTF-8.
	pub fn new(filename: impl AsRef<Path>, transform: Option<Transform>) -> Result<SvgPath> {
		let data = std::fs::read(filename)?;
		Self::from_bytes(&data, transform)
	}

	/// `SVGPath.fromstring(data, transform)` for SVG data in UTF-8.
	pub fn from_bytes(data: &[u8], transform: Option<Transform>) -> Result<SvgPath> {
		// LENIENT(L48): lxml decodes the document in the encoding it declares; only
		// UTF-8 is supported here.
		let text = std::str::from_utf8(data).map_err(|e| Error::Xml(e.to_string()))?;
		Self::fromstring(text, transform)
	}

	/// `SVGPath.fromstring(data, transform)`.
	pub fn fromstring(data: &str, transform: Option<Transform>) -> Result<SvgPath> {
		Ok(SvgPath {
			root: Some(Element::fromstring(data)?),
			transform,
		})
	}

	/// Draws the shapes with the pen (`draw`), approximating elliptical arcs
	/// by cubic curves.
	pub fn draw(&self, pen: &mut dyn Pen) -> Result<()> {
		self.draw_target(Target::Plain(pen))
	}

	/// [`SvgPath::draw`] for a pen that takes elliptical arcs: they are passed
	/// on unless they are transformed.
	pub fn draw_arcs(&self, pen: &mut dyn ArcPen) -> Result<()> {
		self.draw_target(Target::Arcs(pen))
	}

	fn draw_target(&self, pen: Target<'_>) -> Result<()> {
		let mut pb = PathBuilder::new();
		if let Some(root) = &self.root {
			for el in root.iter() {
				pb.add_path_from_element(el)?;
			}
		}
		match (self.transform, pen) {
			(Some(t), mut pen) => {
				let mut tpen = TransformPen::new(pen.pen(), t);
				draw_paths(&pb, Target::Plain(&mut tpen))
			}
			(None, pen) => draw_paths(&pb, pen),
		}
	}
}

fn draw_paths(pb: &PathBuilder, mut original_pen: Target<'_>) -> Result<()> {
	for (path, transform) in pb.paths.iter().zip(&pb.transforms) {
		match transform {
			Some([xx, xy, yx, yy, dx, dy]) => {
				let t = Transform::new(*xx, *xy, *yx, *yy, *dx, *dy);
				let mut tpen = TransformPen::new(original_pen.pen(), t);
				parse(path, Target::Plain(&mut tpen), (0.0, 0.0))?;
			}
			None => {
				let pen = match &mut original_pen {
					Target::Plain(p) => Target::Plain(&mut **p),
					Target::Arcs(p) => Target::Arcs(&mut **p),
				};
				parse(path, pen, (0.0, 0.0))?;
			}
		}
	}
	Ok(())
}
