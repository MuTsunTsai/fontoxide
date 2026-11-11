//! Port of fontTools' `Tests/ufoLib/GLIF1_test.py`.
//!
//! `testName_not_a_string` is not ported: the glyph name is a `&str`.

mod common;

use common::{assert_glif_error, glif_to_py, py_to_glif, strip_text};
use fontoxide_ufolib::Result;

const FORMAT: i64 = 1;

#[test]
fn test_top_element() -> Result<()> {
	let glif = r#"
		<notglyph name="a" format="1">
			<outline>
			</outline>
		</notglyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_name_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_name_empty() -> Result<()> {
	let glif = r#"
		<glyph name="" format="1">
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = ""
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_format_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_format_wrong_number() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="-1">
			<outline>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_format_not_an_int() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="A">
			<outline>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_bogus_glyph_structure_unknown_element() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<unknown />
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_bogus_glyph_structure_content() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			Hello World.
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_advance_legal_width_and_height() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance height="200" width="100"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.width = 100
		glyph.height = 200
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_advance_legal_width_and_height_floats() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance height="200.1" width="100.1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.width = 100.1
		glyph.height = 200.1
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_advance_legal_width() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance width="100"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.width = 100
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_advance_legal_height() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance height="200"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.height = 200
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_advance_illegal_width() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance width="a"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.width = "a"
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_advance_illegal_height() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<advance height="a"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.height = "a"
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_unicodes_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<unicode hex="0061"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.unicodes = [97]
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_unicodes_legal_multiple() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<unicode hex="0062"/>
			<unicode hex="0063"/>
			<unicode hex="0061"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.unicodes = [98, 99, 97]
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_unicodes_illegal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<unicode hex="1.1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "zzzzzz"
		glyph.unicodes = ["1.1"]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_unicodes_hex_present() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<unicode />
			<outline>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_note() -> Result<()> {
	let glif = "\n\t\t<glyph name=\"a\" format=\"1\">\n\t\t\t<note>\n\t\t\t\t\u{1f4a9}\n\t\t\t</note>\n\t\t\t<outline>\n\t\t\t</outline>\n\t\t</glyph>\n\t\t";
	let py = "\n\t\tglyph.name = \"a\"\n\t\tglyph.note = \"\u{1f4a9}\"\n\t\t";
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_lib_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
			</outline>
			<lib>
				<dict>
					<key>dict</key>
					<dict>
						<key>hello</key>
						<string>world</string>
					</dict>
					<key>float</key>
					<real>2.5</real>
					<key>int</key>
					<integer>1</integer>
					<key>list</key>
					<array>
						<string>a</string>
						<string>b</string>
						<integer>1</integer>
						<real>2.5</real>
					</array>
					<key>string</key>
					<string>a</string>
				</dict>
			</lib>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.lib = {"dict" : {"hello" : "world"}, "float" : 2.5, "int" : 1, "list" : ["a", "b", 1, 2.5], "string" : "a"}
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_outline_unknown_element() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<unknown/>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_outline_content() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				hello
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_component_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, 3, 6, 5, 1, 4)])
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_component_illegal_no_base() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_component_bogus_transformation() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="a" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", ("a", 3, 6, 5, 1, 4)])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="a" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, "a", 6, 5, 1, 4)])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="2" xyScale="3" yxScale="a" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, 3, "a", 5, 1, 4)])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="2" xyScale="3" yxScale="6" yScale="a" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, 3, 6, "a", 1, 4)])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="a" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, 3, 6, 5, "a", 4)])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<component base="x" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="a"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.addComponent(*["x", (2, 3, 6, 5, 1, "a")])
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_contour_legal_one_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_contour_legal_two_contours() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="2" type="move"/>
					<point x="10" y="20" type="line"/>
				</contour>
				<contour>
					<point x="1" y="2" type="move"/>
					<point x="10" y="20" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(10, 20)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath()
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(10, 20)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_contour_illegal_unkonwn_element() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<unknown/>
				</contour>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_coordinates_legal_int() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="move"/>
					<point x="0" y="0" type="line" name="this is here so that the contour isn't seen as an anchor"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"name" : "this is here so that the contour isn't seen as an anchor", "segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_coordinates_legal_float() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1.1" y="-2.2" type="move"/>
					<point x="0" y="0" type="line" name="this is here so that the contour isn't seen as an anchor"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1.1, -2.2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"name" : "this is here so that the contour isn't seen as an anchor", "segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_coordinates_illegal_x() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="a" y="2" type="move"/>
					<point x="0" y="0" type="line" name="this is here so that the contour isn't seen as an anchor"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[("a", 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"name" : "this is here so that the contour isn't seen as an anchor", "segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_coordinates_illegal_y() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="a" type="move"/>
					<point x="0" y="0" type="line" name="this is here so that the contour isn't seen as an anchor"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, "a")], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"name" : "this is here so that the contour isn't seen as an anchor", "segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_type_move_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="move"/>
					<point x="3" y="-4" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_move_legal_smooth() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="move" smooth="yes"/>
					<point x="3" y="-4" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : True})
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_move_illegal_not_at_start() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="3" y="-4" type="line"/>
					<point x="1" y="-2" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_type_line_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="move"/>
					<point x="3" y="-4" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_line_legal_start_of_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="line"/>
					<point x="3" y="-4" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_line_legal_smooth() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="-2" type="move"/>
					<point x="3" y="-4" type="line" smooth="yes"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(3, -4)], **{"segmentType" : "line", "smooth" : True})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_legal_start_of_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="100" y="200" type="curve"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_legal_smooth() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="curve" smooth="yes"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : True})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_legal_no_off_curves() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_legal_1_off_curve() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="50" y="100"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(50, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_curve_illegal_3_off_curves() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="100"/>
					<point x="35" y="125"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(35, 125)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_qcurve_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="qcurve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_qcurve_legal_start_of_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="100" y="200" type="qcurve"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_qcurve_legal_smooth() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="qcurve" smooth="yes"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : True})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_qcurve_legal_no_off_curves() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="100" y="200" type="qcurve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_qcurve_legal_one_off_curve() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="50" y="100"/>
					<point x="100" y="200" type="qcurve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(50, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_qcurve_legal_3_off_curves() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="100"/>
					<point x="35" y="125"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="qcurve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(35, 125)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_special_case_qcurve() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0"/>
					<point x="0" y="100"/>
					<point x="100" y="100"/>
					<point x="100" y="0"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"smooth" : False})
		pointPen.addPoint(*[(0, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 100)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 0)], **{"smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_off_curve_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="0" type="move"/>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_off_curve_legal_start_of_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="65"/>
					<point x="65" y="200"/>
					<point x="100" y="200" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(65, 200)], **{"smooth" : False})
		pointPen.addPoint(*[(100, 200)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_type_off_curve_illegal_before_move() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="65"/>
					<point x="0" y="0" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_type_off_curve_illegal_before_line() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="65"/>
					<point x="0" y="0" type="line"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 65)], **{"smooth" : False})
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "line", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_type_off_curve_illegal_smooth() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="0" y="65" smooth="yes"/>
					<point x="0" y="0" type="curve"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(0, 65)], **{"smooth" : True})
		pointPen.addPoint(*[(0, 0)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_single_point_legal_without_name() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="2" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_anchor_legal_with_name() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="2" type="move" name="test"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"name" : "test", "x" : 1, "y" : 2}]
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_open_contour_loose_off_curves_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="1">
			<outline>
				<contour>
					<point x="1" y="2" type="move"/>
					<point x="1" y="2"/>
					<point x="1" y="2"/>
					<point x="1" y="2" type="curve"/>
					<point x="1" y="2"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let expected_py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(&result_py), strip_text(expected_py));
	Ok(())
}

#[test]
fn test_open_contour_loose_off_curves_illegal() -> Result<()> {
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, 2)], **{"smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	Ok(())
}
