//! Subsetting of the `SVG ` table (`fontTools.subset.svg`).

mod xml;

use std::collections::{BTreeSet, HashMap, HashSet};

use fontoxide_misc::py;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::tables::Svg;
use fontoxide_ttlib::tables::svg::SvgDocument;

use self::xml::{Document, XmlError};
use crate::{Error, Result, Subsetter};

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Python's default recursion limit, which `subset_elements` runs into on very
/// deep trees, less the frames of the callers.
const MAX_DEPTH: usize = 990;

fn py_error(message: impl Into<String>) -> Error {
	fontoxide_ttlib::Error::ttlib(message).into()
}

/// The elements with `id` attributes in the SVG namespace (`//svg:*[@id]`).
/// For repeated IDs, the last element is kept in the position of the first, as
/// in a Python dictionary.
fn group_elements_by_id(doc: &Document) -> (Vec<String>, HashMap<String, usize>) {
	let mut order = Vec::new();
	let mut elements = HashMap::new();
	for id in doc.descendants_or_self(doc.root) {
		if doc.namespace(id) == Some(SVG_NS)
			&& let Some(el_id) = doc.attr(id, None, "id")
			&& elements.insert(el_id.to_owned(), id).is_none()
		{
			order.push(el_id.to_owned());
		}
	}
	(order, elements)
}

/// `parse_css_declarations`.
fn parse_css_declarations(style: &str) -> Result<Vec<(&str, &str)>> {
	let mut result = Vec::new();
	for declaration in style.split(';') {
		if declaration.matches(':').count() == 1 {
			if let Some((name, value)) = declaration.split_once(':') {
				result.push((py::strip(name), py::strip(value)));
			}
		} else if !py::strip(declaration).is_empty() {
			return Err(fontoxide_ttlib::Error::value(format!(
				"Invalid CSS declaration syntax: {declaration}"
			))
			.into());
		}
	}
	Ok(result)
}

/// `href_local_target`.
fn href_local_target(doc: &Document, id: usize) -> Option<&str> {
	doc.attr(id, Some(XLINK_NS), "href")?
		.strip_prefix('#')
		.filter(|target| !target.is_empty())
}

/// `iter_referenced_ids`: the IDs referred to from this element tree.
fn referenced_ids(doc: &Document, tree: usize, out: &mut Vec<String>) -> Result<()> {
	let has_reference = |id: usize| {
		doc.namespace(id) == Some(SVG_NS)
			&& (doc
				.attr(id, Some(XLINK_NS), "href")
				.is_some_and(|v| v.starts_with('#'))
				|| doc
					.attr(id, None, "fill")
					.is_some_and(|v| v.starts_with("url(#"))
				|| doc
					.attr(id, None, "clip-path")
					.is_some_and(|v| v.starts_with("url(#"))
				|| doc
					.attr(id, None, "style")
					.is_some_and(|v| v.contains(":url(#")))
	};
	// The tree itself, then `.//svg:*[...]`.
	let mut elements = vec![tree];
	elements.extend(
		doc.descendants_or_self(tree)
			.into_iter()
			.skip(1)
			.filter(|&id| has_reference(id)),
	);
	for id in elements {
		if let Some(target) = href_local_target(doc, id) {
			out.push(target.to_owned());
		}
		let style = match doc.attr(id, None, "style") {
			Some(style) => parse_css_declarations(style)?,
			None => Vec::new(),
		};
		for attr in ["fill", "clip-path"] {
			// The declarations override the attributes; the last one of a name wins.
			let value = style
				.iter()
				.rev()
				.find(|(name, _)| *name == attr)
				.map(|(_, value)| *value)
				.or_else(|| doc.attr(id, None, attr));
			if let Some(target) = value
				.and_then(|v| v.strip_prefix("url(#"))
				.and_then(|v| v.strip_suffix(')'))
			{
				if target.is_empty() {
					return Err(fontoxide_ttlib::Error::Assertion("ref_id".into()).into());
				}
				out.push(target.to_owned());
			}
		}
	}
	Ok(())
}

/// `closure_element_ids`.
fn closure_element_ids(
	doc: &Document,
	elements: &HashMap<String, usize>,
	element_ids: &mut HashSet<String>,
) -> Result<()> {
	let mut unvisited: Vec<String> = element_ids.iter().cloned().collect();
	while !unvisited.is_empty() {
		let mut referenced = Vec::new();
		for el_id in &unvisited {
			// Dangling references are ignored: not our job to validate SVG.
			if let Some(&el) = elements.get(el_id) {
				referenced_ids(doc, el, &mut referenced)?;
			}
		}
		unvisited = referenced
			.into_iter()
			.filter(|id| element_ids.insert(id.clone()))
			.collect();
	}
	Ok(())
}

/// `subset_elements`: keeps the elements whose ID is in the subset, or that have
/// such descendants.
fn subset_elements(
	doc: &mut Document,
	el: usize,
	retained: &HashSet<String>,
	depth: usize,
) -> Result<bool> {
	if depth >= MAX_DEPTH {
		return Err(py_error("RecursionError: maximum recursion depth exceeded"));
	}
	if doc
		.attr(el, None, "id")
		.is_some_and(|id| retained.contains(id))
	{
		// Don't recurse: keep the whole subtree.
		return Ok(true);
	}
	let mut any = false;
	// No short-circuit, as dropping empty elements is a side effect.
	for child in doc.element_children(el) {
		any |= subset_elements(doc, child, retained, depth + 1)?;
	}
	if any {
		return Ok(true);
	}
	doc.remove(el);
	Ok(false)
}

/// `GID_RE.match`: the glyph index of an element ID. Python's `$` also matches
/// before a final newline.
///
/// LENIENT(L25): `\d` of Python also matches the other decimal digits of Unicode.
fn parse_gid(el_id: &str) -> Option<u64> {
	let digits = el_id.strip_prefix("glyph")?;
	let digits = digits.strip_suffix('\n').unwrap_or(digits);
	if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	// Numbers that don't fit are not glyph indices either.
	Some(digits.parse().unwrap_or(u64::MAX))
}

/// `remap_glyph_ids`: renames the elements with `id="glyph{gid}"`.
fn remap_glyph_ids(
	doc: &mut Document,
	glyph_index_map: &std::collections::BTreeMap<u32, u32>,
) -> HashMap<String, String> {
	let (order, elements) = group_elements_by_id(doc);
	let mut id_map = HashMap::new();
	for el_id in &order {
		let Some(old_index) = parse_gid(el_id) else {
			continue;
		};
		let new_index = u32::try_from(old_index)
			.ok()
			.and_then(|old| glyph_index_map.get(&old));
		let new_id = match new_index {
			Some(&new_index) => {
				if old_index == u64::from(new_index) {
					continue;
				}
				format!("glyph{new_index}")
			}
			None => {
				// The element is of a glyph excluded from the subset. Rename it to
				// avoid clashes with the new IDs or other element IDs.
				let mut new_id = format!(".{el_id}");
				let mut n = 1;
				while elements.contains_key(&new_id) {
					new_id = format!("{new_id}.{n}");
					n += 1;
				}
				new_id
			}
		};
		doc.set_attr(elements[el_id], None, "id", new_id.clone());
		id_map.insert(el_id.clone(), new_id);
	}
	id_map
}

/// `update_glyph_href_links`.
fn update_glyph_href_links(doc: &mut Document, id_map: &HashMap<String, String>) {
	for id in doc.descendants_or_self(doc.root).into_iter().skip(1) {
		if doc.namespace(id) != Some(SVG_NS) {
			continue;
		}
		let Some(href) = doc.attr(id, Some(XLINK_NS), "href") else {
			continue;
		};
		if !href.starts_with("#glyph") {
			continue;
		}
		if let Some(new_id) = href_local_target(doc, id).and_then(|old| id_map.get(old)) {
			let value = format!("#{new_id}");
			doc.set_attr(id, Some(XLINK_NS), "href", value);
		}
	}
}

/// `ranges`: sorted, non-overlapping ranges of consecutive integers.
fn ranges(ints: impl IntoIterator<Item = u32>) -> Vec<(u32, u32)> {
	let sorted: BTreeSet<u32> = ints.into_iter().collect();
	let mut result: Vec<(u32, u32)> = Vec::new();
	for v in sorted {
		match result.last_mut() {
			Some((_, end)) if *end + 1 == v => *end = v,
			_ => result.push((v, v)),
		}
	}
	result
}

fn parse_document(data: &str) -> Result<Document> {
	Document::parse(data).map_err(|e| match e {
		XmlError::Syntax(message) => py_error(format!("XMLSyntaxError: {message}")),
		XmlError::Unsupported(what) => Error::Unsupported(format!("SVG documents with {what}")),
	})
}

/// `table_S_V_G_.subset_glyphs`.
pub(crate) fn subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let num_glyphs = s.orig_glyph_order.len() as u32;
	let svg = font.require::<Svg>()?;
	let docs = svg.doc_list.as_ref().ok_or_else(|| {
		py_error("AttributeError: 'table_S_V_G_' object has no attribute 'docList'")
	})?;
	let mut new_docs = Vec::new();
	for doc in docs {
		let (start, end) = (u32::from(doc.start_glyph_id), u32::from(doc.end_glyph_id));
		if start <= end && end >= num_glyphs {
			return Err(py_error("IndexError: list index out of range"));
		}
		let gids: Vec<u32> = (start..=end).filter(|g| s.glyphs.contains(g)).collect();
		if gids.is_empty() {
			// No intersection: the whole record can be dropped.
			continue;
		}
		let mut tree = parse_document(&doc.data)?;
		let (_, elements) = group_elements_by_id(&tree);
		let mut element_ids: HashSet<String> = gids.iter().map(|g| format!("glyph{g}")).collect();
		closure_element_ids(&tree, &elements, &mut element_ids)?;
		let root = tree.root;
		if !subset_elements(&mut tree, root, &element_ids, 0)? {
			continue;
		}
		if !s.options.retain_gids {
			let id_map = remap_glyph_ids(&mut tree, &s.glyph_index_map);
			update_glyph_href_links(&mut tree, &id_map);
		}
		let new_doc = tree.to_string(s.options.pretty_svg);
		let new_gids = gids
			.iter()
			.map(|g| {
				s.glyph_index_map
					.get(g)
					.copied()
					.ok_or_else(|| py_error(format!("KeyError: {g}")))
			})
			.collect::<Result<Vec<u32>>>()?;
		for (start, end) in ranges(new_gids) {
			new_docs.push(SvgDocument {
				data: new_doc.clone(),
				start_glyph_id: fit(start, "startGlyphID")?,
				end_glyph_id: fit(end, "endGlyphID")?,
				compressed: doc.compressed,
			});
		}
	}
	let keep = !new_docs.is_empty();
	svg.doc_list = Some(new_docs);
	Ok(keep)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// `test_ranges`.
	#[test]
	fn test_ranges() {
		type Case = (&'static [u32], &'static [(u32, u32)]);
		let cases: &[Case] = &[
			(&[], &[]),
			(&[0], &[(0, 0)]),
			(&[0, 1], &[(0, 1)]),
			(&[1, 1, 1, 1], &[(1, 1)]),
			(&[1, 3], &[(1, 1), (3, 3)]),
			(&[4, 2, 1, 3], &[(1, 4)]),
			(
				&[1, 2, 4, 5, 6, 9, 13, 14, 15],
				&[(1, 2), (4, 6), (9, 9), (13, 15)],
			),
		];
		for (ints, expected) in cases {
			assert_eq!(ranges(ints.iter().copied()), *expected);
		}
	}

	#[test]
	fn css_declarations() -> Result<()> {
		assert_eq!(
			parse_css_declarations(" fill : url(#a) ;stroke:red; ")?,
			[("fill", "url(#a)"), ("stroke", "red")]
		);
		assert!(parse_css_declarations("fill:url(http://x)").is_err());
		assert!(parse_css_declarations("fill").is_err());
		Ok(())
	}

	#[test]
	fn glyph_ids() {
		assert_eq!(parse_gid("glyph12"), Some(12));
		assert_eq!(parse_gid("glyph007\n"), Some(7));
		assert_eq!(parse_gid("glyph"), None);
		assert_eq!(parse_gid(".glyph1"), None);
		assert_eq!(parse_gid("glyph1a"), None);
	}

	#[test]
	fn tail_goes_with_the_removed_element() -> Result<()> {
		let mut doc = parse_document("<a>x<b/>y<c id=\"k\"/>z</a>")?;
		let retained = HashSet::from(["k".to_owned()]);
		assert!(subset_elements(&mut doc, 0, &retained, 0)?);
		assert_eq!(doc.to_string(false), "<a>x<c id=\"k\"/>z</a>");
		let mut doc = parse_document("<a>x<!--c-->y<c id=\"k\"/></a>")?;
		assert!(subset_elements(&mut doc, 0, &retained, 0)?);
		assert_eq!(doc.to_string(false), "<a>x<c id=\"k\"/></a>");
		Ok(())
	}

	#[test]
	fn entity_references_are_dropped_like_comments() -> Result<()> {
		let mut doc = parse_document("<!DOCTYPE a [<!ENTITY e \"v\">]><a>&e;<c id=\"k\"/></a>")?;
		let retained = HashSet::from(["k".to_owned()]);
		assert!(subset_elements(&mut doc, 0, &retained, 0)?);
		assert_eq!(doc.to_string(false), "<a><c id=\"k\"/></a>");
		Ok(())
	}
}
