//! Port of fontTools' `Tests/subset/svg_test.py`. `test_ranges` is a unit test of
//! the `svg` module.

mod common;

use common::*;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::tables::Svg;
use fontoxide_ttlib::tables::svg::SvgDocument;
use fontoxide_ttlib::{Font, TableXml, Tag, WriteOptions};

const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// The `empty_svg_font` fixture with these documents, saved.
fn svg_font(temp: &mut TempDir, docs: Vec<(String, u16, u16)>) -> TestResult<String> {
	let mut font = fixture("empty_svg")?;
	font.insert(Box::new(Svg {
		doc_list: Some(
			docs.into_iter()
				.map(|(data, start_glyph_id, end_glyph_id)| SvgDocument {
					data,
					start_glyph_id,
					end_glyph_id,
					compressed: false,
				})
				.collect(),
		),
		compressed: false,
	}));
	let path = temp.path(".ttf");
	std::fs::write(&path, font.write(&WriteOptions::default())?)?;
	Ok(path)
}

/// `getXML(font["SVG "].toXML, font)`.
fn svg_xml(font: &mut Font) -> TestResult<Vec<String>> {
	let table = font.require::<Svg>()?.clone();
	let mut writer = XmlWriter::with_options("  ", "\n");
	table.to_xml(&mut writer, font)?;
	let text = writer.into_string();
	let body = text.split_once('\n').map_or("", |(_, body)| body);
	Ok(body.lines().map(str::to_owned).collect())
}

fn lines(s: &str) -> Vec<String> {
	s.lines().map(str::to_owned).collect()
}

fn simple_docs(on_root: bool) -> Vec<(String, u16, u16)> {
	(1..11)
		.map(|i| {
			let doc = if on_root {
				format!("<svg xmlns=\"{SVG_NS}\" id=\"glyph{i}\"><path d=\"M{i},{i}\"/></svg>")
			} else {
				format!("<svg xmlns=\"{SVG_NS}\"><path id=\"glyph{i}\" d=\"M{i},{i}\"/></svg>")
			};
			(doc, i, i)
		})
		.collect()
}

#[test]
fn test_subset_single_glyph_per_svg() -> TestResult {
	for on_root in [false, true] {
		for retain_gids in [false, true] {
			let mut temp = TempDir::new()?;
			let path = svg_font(&mut temp, simple_docs(on_root))?;
			let subset_path = temp.path(".subset.ttf");
			let output = format!("--output-file={subset_path}");
			let retain = if retain_gids {
				"--retain_gids"
			} else {
				"--no-retain_gids"
			};
			subset_main(&[&path, &output, "--gids=2,4-6", retain])?;
			let mut expected = Vec::new();
			for (i, gid) in [2, 4, 5, 6].into_iter().enumerate() {
				let new = if retain_gids { gid } else { i + 1 };
				expected.push(format!(
					"<svgDoc endGlyphID=\"{new}\" startGlyphID=\"{new}\">"
				));
				expected.push(if on_root {
					format!(
						"  <![CDATA[<svg xmlns=\"{SVG_NS}\" id=\"glyph{new}\"><path d=\"M{gid},{gid}\"/></svg>]]>"
					)
				} else {
					format!(
						"  <![CDATA[<svg xmlns=\"{SVG_NS}\"><path id=\"glyph{new}\" d=\"M{gid},{gid}\"/></svg>]]>"
					)
				});
				expected.push("</svgDoc>".to_owned());
			}
			assert_eq!(svg_xml(&mut open(&subset_path)?)?, expected);
		}
	}
	Ok(())
}

/// `COMPLEX_SVG`, taken from the test file of fontTools.
fn complex_svg() -> TestResult<String> {
	let test = std::fs::read_to_string(tests_path("subset/svg_test.py"))?.replace("\r\n", "\n");
	let (_, rest) = test
		.split_once("COMPLEX_SVG = \"\"\"\\\n")
		.ok_or("no COMPLEX_SVG")?;
	let (svg, _) = rest.split_once("\"\"\"").ok_or("no end of COMPLEX_SVG")?;
	Ok(svg.to_owned())
}

const HEAD: &str = "<![CDATA[<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">";

#[test]
fn test_subset_svg_with_references() -> TestResult {
	let lg1 = "    <linearGradient id=\"lg1\" x1=\"50\" x2=\"50\" y1=\"80\" y2=\"80\" gradientUnits=\"userSpaceOnUse\">
      <stop stop-color=\"#A47B62\" offset=\"0\"/>
      <stop stop-color=\"#AD8264\" offset=\"1.0\"/>
    </linearGradient>";
	let cases: Vec<(&str, u16, u16, String)> = vec![
		// Only gid 2, with 'glyph2' defined inside 'glyph1': 'glyph2' becomes 'glyph1',
		// and the old 'glyph1' is kept but renamed '.glyph1' to avoid a clash.
		(
			"2",
			1,
			1,
			"  <g id=\".glyph1\">
    <g id=\"glyph1\">
      <path d=\"M0,0\"/>
    </g>
  </g>"
				.to_owned(),
		),
		// Both gids 1 and 2: the IDs stay; a gradient is referred to by glyph1.
		(
			"1,2",
			1,
			2,
			format!(
				"  <defs>
{lg1}
  </defs>
  <g id=\"glyph1\">
    <g id=\"glyph2\">
      <path d=\"M0,0\"/>
    </g>
    <g>
      <path d=\"M1,1\" fill=\"url(#lg1)\"/>
      <path d=\"M2,2\"/>
    </g>
  </g>"
			),
		),
		// Both gids 3 and 6 refer to path 'p1', which is kept in <defs>.
		(
			"3,6",
			1,
			2,
			"  <defs>
    <path id=\"p1\" d=\"M3,3\"/>
  </defs>
  <g id=\"glyph1\">
    <use xlink:href=\"#p1\"/>
  </g>
  <g id=\"glyph2\">
    <use xlink:href=\"#p1\" transform=\"scale(2, 1)\"/>
  </g>"
				.to_owned(),
		),
		// 'glyph4' uses the whole 'glyph1' element, which is kept renamed.
		(
			"3-4",
			1,
			2,
			format!(
				"  <defs>
{lg1}
    <path id=\"p1\" d=\"M3,3\"/>
  </defs>
  <g id=\".glyph1\">
    <g id=\".glyph2\">
      <path d=\"M0,0\"/>
    </g>
    <g>
      <path d=\"M1,1\" fill=\"url(#lg1)\"/>
      <path d=\"M2,2\"/>
    </g>
  </g>
  <g id=\"glyph1\">
    <use xlink:href=\"#p1\"/>
  </g>
  <use id=\"glyph2\" xlink:href=\"#.glyph1\" x=\"10\"/>"
			),
		),
		// 'glyph9' uses a path inside 'glyph7', which is renamed '.glyph7'; an
		// unrelated element with that ID doesn't clash because it was dropped. Since
		// id=".glyph8" is used in the document, a suffix is added for 'glyph8'.
		(
			"9,10",
			1,
			2,
			"  <g id=\"group1\">
    <g id=\".glyph7\">
      <path id=\"p2\" d=\"M4,4\"/>
    </g>
    <g id=\".glyph8.1\">
      <g id=\".glyph8\">
        <path id=\"p3\" d=\"M5,5\"/>
      </g>
    </g>
    <g id=\"glyph1\">
      <use xlink:href=\"#p2\"/>
    </g>
    <g id=\"glyph2\">
      <use xlink:href=\"#p3\"/>
    </g>
  </g>"
				.to_owned(),
		),
		// 'glyph11' uses gradient 'rg4', which inherits from 'rg3', from 'rg2'.
		(
			"11",
			1,
			1,
			"  <defs>
    <radialGradient id=\"rg2\" cx=\"50\" cy=\"50\" r=\"10\" gradientUnits=\"userSpaceOnUse\">
      <stop stop-color=\"#A47B62\" offset=\"0\"/>
      <stop stop-color=\"#AD8264\" offset=\"1.0\"/>
    </radialGradient>
    <radialGradient id=\"rg3\" xlink:href=\"#rg2\" r=\"20\"/>
    <radialGradient id=\"rg4\" xlink:href=\"#rg3\" cy=\"100\"/>
  </defs>
  <g id=\"glyph1\">
    <path d=\"M7,7\" fill=\"url(#rg4)\"/>
  </g>"
				.to_owned(),
		),
		// 'glyph12' has a style attribute with references to a gradient and a clipPath.
		(
			"12",
			1,
			1,
			format!(
				"  <defs>
{lg1}
    <clipPath id=\"c1\">
      <circle cx=\"10\" cy=\"10\" r=\"1\"/>
    </clipPath>
  </defs>
  <g id=\"glyph1\">
    <path d=\"M7,7\" style=\"fill:url(#lg1);stroke:red;clip-path:url(#c1)\"/>
  </g>"
			),
		),
	];
	let complex = complex_svg()?;
	for (gids, start, end, body) in cases {
		let mut temp = TempDir::new()?;
		let path = svg_font(&mut temp, vec![(complex.clone(), 1, 12)])?;
		let subset_path = temp.path(".subset.ttf");
		let output = format!("--output-file={subset_path}");
		let gids_option = format!("--gids={gids}");
		subset_main(&[&path, &output, &gids_option, "--pretty-svg"])?;
		let expected = lines(&format!(
			"<svgDoc endGlyphID=\"{end}\" startGlyphID=\"{start}\">\n  {HEAD}\n{body}\n</svg>\n]]>\n</svgDoc>"
		));
		assert_eq!(svg_xml(&mut open(&subset_path)?)?, expected, "gids {gids}");
	}
	Ok(())
}

fn rect_doc() -> String {
	format!("<svg xmlns=\"{SVG_NS}\"><rect id=\"glyph1\" x=\"1\" y=\"2\"/></svg>")
}

#[test]
fn test_subset_svg_empty_table() -> TestResult {
	let mut temp = TempDir::new()?;
	let path = svg_font(&mut temp, vec![(rect_doc(), 1, 1)])?;
	let subset_path = temp.path(".subset.ttf");
	let output = format!("--output-file={subset_path}");
	// There's no gid 2 in the table: the empty table is dropped.
	subset_main(&[&path, &output, "--gids=2"])?;
	assert!(!open(&subset_path)?.contains(Tag::new(b"SVG ")));
	Ok(())
}

#[test]
fn test_subset_svg_missing_glyph() -> TestResult {
	let mut temp = TempDir::new()?;
	// The range declares two glyphs, but there is no "glyph2" element.
	let path = svg_font(&mut temp, vec![(rect_doc(), 1, 2)])?;
	let subset_path = temp.path(".subset.ttf");
	let output = format!("--output-file={subset_path}");
	subset_main(&[&path, &output, "--gids=1"])?;
	assert_eq!(
		svg_xml(&mut open(&subset_path)?)?,
		[
			"<svgDoc endGlyphID=\"1\" startGlyphID=\"1\">".to_owned(),
			format!("  <![CDATA[{}]]>", rect_doc()),
			"</svgDoc>".to_owned(),
		]
	);
	// The missing glyph is ignored even if in the subset: the empty document is
	// dropped, along with the empty table.
	subset_main(&[&path, &output, "--gids=2"])?;
	assert!(!open(&subset_path)?.contains(Tag::new(b"SVG ")));
	Ok(())
}
