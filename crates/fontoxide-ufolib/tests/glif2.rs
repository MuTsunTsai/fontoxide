//! Port of fontTools' `Tests/ufoLib/GLIF2_test.py`.
//!
//! `testName_not_a_string` is not ported: the glyph name is a `&str`.

mod common;

use common::{assert_glif_error, glif_to_py, py_to_glif, strip_text};
use fontoxide_ufolib::Result;

const FORMAT: i64 = 2;

#[test]
fn test_top_element() -> Result<()> {
	let glif = r#"
		<notglyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="" format="2">
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
		<glyph name="a" format="2">
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
fn test_format_illegal_wrong_number() -> Result<()> {
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
fn test_format_illegal_not_int() -> Result<()> {
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
		<glyph name="a" format="2">
			<unknown />
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_bogus_glyph_structure_content() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			Hello World.
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_advance_legal_widht_and_height() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
	let glif = "\n\t\t<glyph name=\"a\" format=\"2\">\n\t\t\t<note>\n\t\t\t\th\u{eb}ll\u{f6}\n\t\t\t</note>\n\t\t\t<outline>\n\t\t\t</outline>\n\t\t</glyph>\n\t\t";
	let py = "\n\t\tglyph.name = \"a\"\n\t\tglyph.note = \"h\u{eb}ll\u{f6}\"\n\t\t";
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_lib() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
fn test_guidelines_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1"/>
			<guideline y="1"/>
			<guideline x="1" y="1" angle="0"/>
			<guideline x="1" y="1" angle="360"/>
			<guideline x="1.1" y="1.1" angle="45.5"/>
			<guideline x="1" name="a"/>
			<guideline x="1" color="1,1,1,1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"x" : 1}, {"y" : 1}, {"angle" : 0, "x" : 1, "y" : 1}, {"angle" : 360, "x" : 1, "y" : 1}, {"angle" : 45.5, "x" : 1.1, "y" : 1.1}, {"name" : "a", "x" : 1}, {"color" : "1,1,1,1", "x" : 1}]
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_guidelines_illegal_x() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="a" y="1" angle="45"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : 45, "x" : "a", "y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_guidelines_illegal_y() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" y="y" angle="45"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : 45, "x" : 1, "y" : "a"}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_guidelines_illegal_angle() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" y="1" angle="a"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : "a", "x" : 1, "y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_guidelines_x_missing() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline y="1" angle="45"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : 45, "y" : 1}]
		"#;
	let result = py_to_glif(py, FORMAT)?;
	let expected = r#"<glyph name="a" format="2">
  <guideline y="1" angle="45"/>
  <outline>
  </outline>
</glyph>"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	let result = glif_to_py(glif)?;
	let expected = r#"glyph.name = "a"
glyph.guidelines = [{"angle" : 45, "y" : 1}]"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	Ok(())
}

#[test]
fn test_guidelines_y_missing() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" angle="45"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : 45, "x" : 1}]
		"#;
	let result = py_to_glif(py, FORMAT)?;
	let expected = r#"<glyph name="a" format="2">
  <guideline x="1" angle="45"/>
  <outline>
  </outline>
</glyph>"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	let result = glif_to_py(glif)?;
	let expected = r#"glyph.name = "a"
glyph.guidelines = [{"angle" : 45, "x" : 1}]"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	Ok(())
}

#[test]
fn test_guidelines_angle_missing() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" y="1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"x" : 1, "y" : 1}]
		"#;
	let result = py_to_glif(py, FORMAT)?;
	let expected = r#"<glyph name="a" format="2">
  <guideline x="1" y="1"/>
  <outline>
  </outline>
</glyph>"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	let result = glif_to_py(glif)?;
	let expected = r#"glyph.name = "a"
glyph.guidelines = [{"x" : 1, "y" : 1}]"#;
	assert_eq!(strip_text(&result), strip_text(expected));
	Ok(())
}

#[test]
fn test_guidelines_illegal_angle_out_of_range() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" y="1" angle="-1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : -1, "x" : "1", "y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="1" y="1" angle="361"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"angle" : 361, "x" : "1", "y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_anchors_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<anchor x="1" y="2" name="test" color="1,0,0,1"/>
			<anchor x="1" y="2"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"color" : "1,0,0,1", "name" : "test", "x" : 1, "y" : 2}, {"x" : 1, "y" : 2}]
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_anchors_illegal_x() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<anchor x="a" y="1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"x" : "a", "y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_anchors_illegal_y() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<anchor x="1" y="a"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"x" : 1, "y" : "a"}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_anchors_illegal_x_missing() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<anchor y="1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"y" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_anchors_illegal_y_missing() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<anchor x="1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.anchors = [{"x" : 1}]
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_image_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4" color="1,1,1,1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"color" : "1,1,1,1", "fileName" : "test.png", "xOffset" : 1, "xScale" : 2, "xyScale" : 3, "yOffset" : 4, "yScale" : 5, "yxScale" : 6}
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_image_legal_no_color_or_transformation() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 0, "xScale" : 1, "xyScale" : 0, "yOffset" : 0, "yScale" : 1, "yxScale" : 0}
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_image_illegal_no_file_name() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<image xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4" color="1,1,1,1"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"color" : "1,1,1,1", "xOffset" : 1, "xScale" : 2, "xyScale" : 3, "yOffset" : 4, "yScale" : 5, "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_image_bogus_transformation() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="a" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 1, "xScale" : "a", "xyScale" : 3, "yOffset" : 4, "yScale" : 5, "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="a" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 1, "xScale" : 2, "xyScale" : "a", "yOffset" : 4, "yScale" : 5, "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="3" yxScale="a" yScale="5" xOffset="1" yOffset="4"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 1, "xScale" : 2, "xyScale" : 3, "yOffset" : 4, "yScale" : 5, "yxScale" : "a"}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="3" yxScale="6" yScale="a" xOffset="1" yOffset="4"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 1, "xScale" : 2, "xyScale" : 3, "yOffset" : 4, "yScale" : "a", "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="a" yOffset="4"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : "a", "xScale" : 2, "xyScale" : 3, "yOffset" : 4, "yScale" : 5, "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="a"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"fileName" : "test.png", "xOffset" : 1, "xScale" : 2, "xyScale" : 3, "yOffset" : "a", "yScale" : 5, "yxScale" : 6}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_image_bogus_color() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<image fileName="test.png" color="1,1,1,x"/>
			<outline>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.image = {"color" : "1,1,1,x"}
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_outline_unknown_element() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
			<outline>
				<component xScale="2" xyScale="3" yxScale="6" yScale="5" xOffset="1" yOffset="4"/>
			</outline>
		</glyph>
		"#;
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_component_illegal_bogus_transformation() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="1" y="2" type="move"/>
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
		<glyph name="a" format="2">
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
fn test_contour_identifier() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<outline>
				<contour identifier="foo">
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath(**{"identifier" : "foo"})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_point_coordinates_legal_int() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="1" y="-2" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"segmentType" : "move", "smooth" : False})
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
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="1.1" y="-2.2" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1.1, -2.2)], **{"segmentType" : "move", "smooth" : False})
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
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="a" y="2" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[("a", 2)], **{"segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_coordinates_illegal_y() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="1" y="a" type="move"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, "a")], **{"segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_point_type_move_legal() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
fn test_special_case_qcurve_legal_no_on_curve() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
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
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="0" y="65" smooth="yess"/>
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
fn test_open_contour_loose_off_curves() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
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
	assert_glif_error(glif_to_py(glif));
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

#[test]
fn test_point_identifier() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<outline>
				<contour>
					<point x="1" y="-2" type="move" identifier="1"/>
					<point x="1" y="-2" type="line" identifier="2"/>
					<point x="1" y="-2" type="curve" identifier="3"/>
					<point x="1" y="-2" type="qcurve" identifier="4"/>
				</contour>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		pointPen.beginPath()
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_identifier_conflict_legal_no_conflict() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	let result_glif = py_to_glif(py, FORMAT)?;
	let result_py = glif_to_py(glif)?;
	assert_eq!(strip_text(glif), strip_text(&result_glif));
	assert_eq!(strip_text(py), strip_text(&result_py));
	Ok(())
}

#[test]
fn test_identifier_conflict_point_point() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point1"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_point_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="contour1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "contour1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_point_component() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="component1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "component1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_point_guideline() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="guideline1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "guideline1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_point_anchor() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="anchor1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "anchor1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_contour_contour() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_contour_component() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="contour1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "contour1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_contour_guideline() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="contour1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "contour1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_contour_anchor() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="anchor1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "anchor1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_component_component() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_component_guideline() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="component1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "component1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_component_anchor() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="anchor1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "anchor1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_guideline_guideline() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline1"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline1", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_guideline_anchor() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="anchor1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor2"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "anchor1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor2", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}

#[test]
fn test_identifier_conflict_anchor_anchor() -> Result<()> {
	let glif = r#"
		<glyph name="a" format="2">
			<guideline x="0" identifier="guideline1"/>
			<guideline x="0" identifier="guideline2"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<anchor x="0" y="0" identifier="anchor1"/>
			<outline>
				<contour identifier="contour1">
					<point x="1" y="-2" type="move" identifier="point1"/>
					<point x="1" y="-2" type="line" identifier="point2"/>
					<point x="1" y="-2" type="curve" identifier="point3"/>
					<point x="1" y="-2" type="qcurve" identifier="point4"/>
				</contour>
				<contour identifier="contour2">
					<point x="1" y="-2" type="move" identifier="point5"/>
				</contour>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component1"/>
				<component base="x" xyScale="1" yxScale="1" xOffset="1" yOffset="1" identifier="component2"/>
			</outline>
		</glyph>
		"#;
	let py = r#"
		glyph.name = "a"
		glyph.guidelines = [{"identifier" : "guideline1", "x" : 0}, {"identifier" : "guideline2", "x" : 0}]
		glyph.anchors = [{"identifier" : "anchor1", "x" : 0, "y" : 0}, {"identifier" : "anchor1", "x" : 0, "y" : 0}]
		pointPen.beginPath(**{"identifier" : "contour1"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point1", "segmentType" : "move", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point2", "segmentType" : "line", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point3", "segmentType" : "curve", "smooth" : False})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point4", "segmentType" : "qcurve", "smooth" : False})
		pointPen.endPath()
		pointPen.beginPath(**{"identifier" : "contour2"})
		pointPen.addPoint(*[(1, -2)], **{"identifier" : "point5", "segmentType" : "move", "smooth" : False})
		pointPen.endPath()
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component1"})
		pointPen.addComponent(*["x", (1, 1, 1, 1, 1, 1)], **{"identifier" : "component2"})
		"#;
	assert_glif_error(py_to_glif(py, FORMAT));
	assert_glif_error(glif_to_py(glif));
	Ok(())
}
