//! `Tests/otlLib/builder_test.py`.

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_otllib::builder::*;
use fontoxide_otllib::{Error, GlyphMap, Result};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::table::{OtTable, xml_struct};
use fontoxide_ttlib::ot::types::{self as ot, Anchor, LookupSubTable};
use fontoxide_ttlib::tables::Math;
use fontoxide_ttlib::tables::name::Name;
use fontoxide_ttlib::{Font, ReadOptions, TableKey, TtxOptions, WriteOptions};
use indexmap::IndexMap;

const GLYPHS: &str =
	".notdef space zero one two three four five six A B C a b c grave acute cedilla f_f_i f_i c_t";

fn names() -> Vec<String> {
	GLYPHS.split(' ').map(str::to_owned).collect()
}

fn glyph_map() -> GlyphMap {
	GlyphMap::from_names(&names())
}

/// `ExtendedGlyphMap(GLYPHMAP)`.
fn extended() -> GlyphMap {
	glyph_map().with_len(0x10001)
}

fn font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order(names());
	font
}

fn s(v: &str) -> String {
	v.to_owned()
}

fn strings(v: &[&str]) -> Vec<String> {
	v.iter().map(|x| s(x)).collect()
}

/// `getXML(table.toXML)`, with the class name as element name.
fn get_xml<T: OtTable>(table: &T) -> Result<Vec<String>> {
	get_xml_in(table, &mut font())
}

fn get_xml_in<T: OtTable>(table: &T, font: &mut Font) -> Result<Vec<String>> {
	let mut writer = XmlWriter::with_options("  ", "\n");
	xml_struct(&mut writer, font, Some(table), None, &[])?;
	Ok(lines(writer.into_string()))
}

fn lines(text: String) -> Vec<String> {
	let body = text.split_once('\n').map_or(text.as_str(), |(_, b)| b);
	body.lines().map(str::to_owned).collect()
}

fn anchor(x: i16, y: i16) -> Anchor {
	build_anchor(x, y, None, None, None).expect("anchor")
}

fn anchor1() -> Anchor {
	anchor(11, -11)
}

fn anchor2() -> Anchor {
	anchor(22, -22)
}

fn anchor3() -> Anchor {
	anchor(33, -33)
}

fn value(fields: &[(&str, i16)]) -> ValueRecord {
	build_value(fields).expect("value")
}

fn device(deltas: &[(i64, i64)]) -> Result<Option<ot::Device>> {
	build_device(&deltas.iter().copied().collect())
}

#[test]
fn test_build_anchor_format1() -> Result<()> {
	let anchor = build_anchor(23, 42, None, None, None)?;
	assert_eq!(
		get_xml(&anchor)?,
		&[
			"<Anchor Format=\"1\">",
			"  <XCoordinate value=\"23\"/>",
			"  <YCoordinate value=\"42\"/>",
			"</Anchor>",
		]
	);
	Ok(())
}

#[test]
fn test_build_anchor_format2() -> Result<()> {
	let anchor = build_anchor(23, 42, Some(17), None, None)?;
	assert_eq!(
		get_xml(&anchor)?,
		&[
			"<Anchor Format=\"2\">",
			"  <XCoordinate value=\"23\"/>",
			"  <YCoordinate value=\"42\"/>",
			"  <AnchorPoint value=\"17\"/>",
			"</Anchor>",
		]
	);
	Ok(())
}

#[test]
fn test_build_anchor_format3() -> Result<()> {
	let anchor = build_anchor(23, 42, None, device(&[(1, 1), (0, 0)])?, device(&[(7, 7)])?)?;
	assert_eq!(
		get_xml(&anchor)?,
		&[
			"<Anchor Format=\"3\">",
			"  <XCoordinate value=\"23\"/>",
			"  <YCoordinate value=\"42\"/>",
			"  <XDeviceTable>",
			"    <StartSize value=\"0\"/>",
			"    <EndSize value=\"1\"/>",
			"    <DeltaFormat value=\"1\"/>",
			"    <DeltaValue value=\"[0, 1]\"/>",
			"  </XDeviceTable>",
			"  <YDeviceTable>",
			"    <StartSize value=\"7\"/>",
			"    <EndSize value=\"7\"/>",
			"    <DeltaFormat value=\"2\"/>",
			"    <DeltaValue value=\"[7]\"/>",
			"  </YDeviceTable>",
			"</Anchor>",
		]
	);
	Ok(())
}

#[test]
fn test_build_attach_list() -> Result<()> {
	let points: IndexMap<String, Vec<u16>> = [(s("zero"), vec![23, 7]), (s("one"), vec![1])]
		.into_iter()
		.collect();
	let attach_list = build_attach_list(&points, &glyph_map())?.expect("attach list");
	assert_eq!(
		get_xml(&attach_list)?,
		&[
			"<AttachList>",
			"  <Coverage>",
			"    <Glyph value=\"zero\"/>",
			"    <Glyph value=\"one\"/>",
			"  </Coverage>",
			"  <!-- GlyphCount=2 -->",
			"  <AttachPoint index=\"0\">",
			"    <!-- PointCount=2 -->",
			"    <PointIndex index=\"0\" value=\"7\"/>",
			"    <PointIndex index=\"1\" value=\"23\"/>",
			"  </AttachPoint>",
			"  <AttachPoint index=\"1\">",
			"    <!-- PointCount=1 -->",
			"    <PointIndex index=\"0\" value=\"1\"/>",
			"  </AttachPoint>",
			"</AttachList>",
		]
	);
	Ok(())
}

#[test]
fn test_build_attach_list_empty() -> Result<()> {
	assert!(build_attach_list(&IndexMap::new(), &glyph_map())?.is_none());
	Ok(())
}

#[test]
fn test_build_attach_point() -> Result<()> {
	let point = build_attach_point(&[7, 3]).expect("attach point");
	assert_eq!(
		get_xml(&point)?,
		&[
			"<AttachPoint>",
			"  <!-- PointCount=2 -->",
			"  <PointIndex index=\"0\" value=\"3\"/>",
			"  <PointIndex index=\"1\" value=\"7\"/>",
			"</AttachPoint>",
		]
	);
	Ok(())
}

#[test]
fn test_build_attach_point_empty() {
	assert!(build_attach_point(&[]).is_none());
}

#[test]
fn test_build_attach_point_duplicate() -> Result<()> {
	let point = build_attach_point(&[7, 3, 7]).expect("attach point");
	assert_eq!(
		get_xml(&point)?,
		&[
			"<AttachPoint>",
			"  <!-- PointCount=2 -->",
			"  <PointIndex index=\"0\" value=\"3\"/>",
			"  <PointIndex index=\"1\" value=\"7\"/>",
			"</AttachPoint>",
		]
	);
	Ok(())
}

#[test]
fn test_build_base_array() -> Result<()> {
	let bases: IndexMap<String, BTreeMap<u16, Anchor>> = [
		(s("a"), [(2, anchor(300, 80))].into_iter().collect()),
		(
			s("c"),
			[(1, anchor(300, 80)), (2, anchor(300, -20))]
				.into_iter()
				.collect(),
		),
	]
	.into_iter()
	.collect();
	let base_array = build_base_array(&bases, 4, &glyph_map())?
		.normal()
		.expect("normal");
	assert_eq!(
		get_xml(&base_array)?,
		&[
			"<BaseArray>",
			"  <!-- BaseCount=2 -->",
			"  <BaseRecord index=\"0\">",
			"    <BaseAnchor index=\"0\" empty=\"1\"/>",
			"    <BaseAnchor index=\"1\" empty=\"1\"/>",
			"    <BaseAnchor index=\"2\" Format=\"1\">",
			"      <XCoordinate value=\"300\"/>",
			"      <YCoordinate value=\"80\"/>",
			"    </BaseAnchor>",
			"    <BaseAnchor index=\"3\" empty=\"1\"/>",
			"  </BaseRecord>",
			"  <BaseRecord index=\"1\">",
			"    <BaseAnchor index=\"0\" empty=\"1\"/>",
			"    <BaseAnchor index=\"1\" Format=\"1\">",
			"      <XCoordinate value=\"300\"/>",
			"      <YCoordinate value=\"80\"/>",
			"    </BaseAnchor>",
			"    <BaseAnchor index=\"2\" Format=\"1\">",
			"      <XCoordinate value=\"300\"/>",
			"      <YCoordinate value=\"-20\"/>",
			"    </BaseAnchor>",
			"    <BaseAnchor index=\"3\" empty=\"1\"/>",
			"  </BaseRecord>",
			"</BaseArray>",
		]
	);
	Ok(())
}

#[test]
fn test_build_base_record() -> Result<()> {
	let rec = build_base_record(
		vec![Some(anchor(500, -20)), None, Some(anchor(300, -15))],
		false,
	)
	.normal()
	.expect("normal");
	assert_eq!(
		get_xml(&rec)?,
		&[
			"<BaseRecord>",
			"  <BaseAnchor index=\"0\" Format=\"1\">",
			"    <XCoordinate value=\"500\"/>",
			"    <YCoordinate value=\"-20\"/>",
			"  </BaseAnchor>",
			"  <BaseAnchor index=\"1\" empty=\"1\"/>",
			"  <BaseAnchor index=\"2\" Format=\"1\">",
			"    <XCoordinate value=\"300\"/>",
			"    <YCoordinate value=\"-15\"/>",
			"  </BaseAnchor>",
			"</BaseRecord>",
		]
	);
	Ok(())
}

#[test]
fn test_build_caret_value_for_coord() -> Result<()> {
	let caret = build_caret_value_for_coord(&CaretCoord::Coord(500));
	assert_eq!(
		get_xml(&caret)?,
		&[
			"<CaretValue Format=\"1\">",
			"  <Coordinate value=\"500\"/>",
			"</CaretValue>",
		]
	);
	Ok(())
}

#[test]
fn test_build_caret_value_for_point() -> Result<()> {
	let caret = build_caret_value_for_point(23);
	assert_eq!(
		get_xml(&caret)?,
		&[
			"<CaretValue Format=\"2\">",
			"  <CaretValuePoint value=\"23\"/>",
			"</CaretValue>",
		]
	);
	Ok(())
}

#[test]
fn test_build_component_record() -> Result<()> {
	let rec = build_component_record(
		Some(vec![Some(anchor(500, -20)), None, Some(anchor(300, -15))]),
		false,
	)
	.and_then(MaybeExtended::normal)
	.expect("record");
	assert_eq!(
		get_xml(&rec)?,
		&[
			"<ComponentRecord>",
			"  <LigatureAnchor index=\"0\" Format=\"1\">",
			"    <XCoordinate value=\"500\"/>",
			"    <YCoordinate value=\"-20\"/>",
			"  </LigatureAnchor>",
			"  <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"  <LigatureAnchor index=\"2\" Format=\"1\">",
			"    <XCoordinate value=\"300\"/>",
			"    <YCoordinate value=\"-15\"/>",
			"  </LigatureAnchor>",
			"</ComponentRecord>",
		]
	);
	Ok(())
}

#[test]
fn test_build_component_record_empty() {
	assert!(build_component_record(Some(Vec::new()), false).is_none());
}

#[test]
fn test_build_component_record_none() {
	assert!(build_component_record(None, false).is_none());
}

/// A font where glyphs 2 and 4 are "two" and "four", for the glyph map of the
/// coverage test.
fn coverage_font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order(strings(&["g0", "g1", "two", "g3", "four"]));
	font
}

#[test]
fn test_build_coverage() -> Result<()> {
	let map = GlyphMap::new([(s("two"), 2), (s("four"), 4)].into_iter().collect());
	let cov = build_coverage(["two", "four", "two"], &map)?;
	assert_eq!(
		get_xml_in(&cov, &mut coverage_font())?,
		&[
			"<Coverage>",
			"  <Glyph value=\"two\"/>",
			"  <Glyph value=\"four\"/>",
			"</Coverage>",
		]
	);
	Ok(())
}

#[test]
fn test_build_coverage_empty() -> Result<()> {
	let map = GlyphMap::new([(s("two"), 2), (s("four"), 4)].into_iter().collect());
	let cov = build_coverage([], &map)?;
	assert!(cov.glyphs.is_empty());
	assert_eq!(get_xml(&cov)?, &["<Coverage>", "</Coverage>",]);
	Ok(())
}

fn attachments(
	items: &[(&str, Anchor, Anchor)],
) -> IndexMap<String, (Option<Anchor>, Option<Anchor>)> {
	items
		.iter()
		.map(|(g, a, b)| (s(g), (Some(a.clone()), Some(b.clone()))))
		.collect()
}

#[test]
fn test_build_cursive_pos() -> Result<()> {
	let pos = build_cursive_pos_subtable(
		&attachments(&[
			("two", anchor1(), anchor2()),
			("four", anchor3(), anchor1()),
		]),
		&glyph_map(),
	)?
	.expect("subtable");
	assert_eq!(
		get_xml(&pos)?,
		&[
			"<CursivePos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"two\"/>",
			"    <Glyph value=\"four\"/>",
			"  </Coverage>",
			"  <!-- EntryExitCount=2 -->",
			"  <EntryExitRecord index=\"0\">",
			"    <EntryAnchor Format=\"1\">",
			"      <XCoordinate value=\"11\"/>",
			"      <YCoordinate value=\"-11\"/>",
			"    </EntryAnchor>",
			"    <ExitAnchor Format=\"1\">",
			"      <XCoordinate value=\"22\"/>",
			"      <YCoordinate value=\"-22\"/>",
			"    </ExitAnchor>",
			"  </EntryExitRecord>",
			"  <EntryExitRecord index=\"1\">",
			"    <EntryAnchor Format=\"1\">",
			"      <XCoordinate value=\"33\"/>",
			"      <YCoordinate value=\"-33\"/>",
			"    </EntryAnchor>",
			"    <ExitAnchor Format=\"1\">",
			"      <XCoordinate value=\"11\"/>",
			"      <YCoordinate value=\"-11\"/>",
			"    </ExitAnchor>",
			"  </EntryExitRecord>",
			"</CursivePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_cursive_pos_format2() -> Result<()> {
	let pos =
		build_cursive_pos_subtable(&attachments(&[("two", anchor1(), anchor2())]), &extended())?;
	assert!(matches!(pos, Some(ot::CursivePos::Format2(p)) if p.entry_exit_record.len() == 1));
	Ok(())
}

#[test]
fn test_build_device_format1() -> Result<()> {
	let d = device(&[(1, 1), (0, 0)])?.expect("device");
	assert_eq!(
		get_xml(&d)?,
		&[
			"<Device>",
			"  <StartSize value=\"0\"/>",
			"  <EndSize value=\"1\"/>",
			"  <DeltaFormat value=\"1\"/>",
			"  <DeltaValue value=\"[0, 1]\"/>",
			"</Device>",
		]
	);
	Ok(())
}

#[test]
fn test_build_device_format2() -> Result<()> {
	let d = device(&[(2, 2), (0, 1), (1, 0)])?.expect("device");
	assert_eq!(
		get_xml(&d)?,
		&[
			"<Device>",
			"  <StartSize value=\"0\"/>",
			"  <EndSize value=\"2\"/>",
			"  <DeltaFormat value=\"2\"/>",
			"  <DeltaValue value=\"[1, 0, 2]\"/>",
			"</Device>",
		]
	);
	Ok(())
}

#[test]
fn test_build_device_format3() -> Result<()> {
	let d = device(&[(5, 3), (1, 77)])?.expect("device");
	assert_eq!(
		get_xml(&d)?,
		&[
			"<Device>",
			"  <StartSize value=\"1\"/>",
			"  <EndSize value=\"5\"/>",
			"  <DeltaFormat value=\"3\"/>",
			"  <DeltaValue value=\"[77, 0, 0, 0, 3]\"/>",
			"</Device>",
		]
	);
	Ok(())
}

fn classes(items: &[(u16, Anchor)]) -> BTreeMap<u16, Anchor> {
	items.iter().cloned().collect()
}

#[test]
fn test_build_ligature_array() -> Result<()> {
	let ligs: IndexMap<String, Vec<BTreeMap<u16, Anchor>>> = [
		(
			s("f_i"),
			vec![classes(&[(2, anchor(300, -20))]), classes(&[])],
		),
		(
			s("c_t"),
			vec![
				classes(&[]),
				classes(&[(1, anchor(500, 350)), (2, anchor(1300, -20))]),
			],
		),
	]
	.into_iter()
	.collect();
	let array = build_ligature_array(&ligs, 4, &glyph_map())?
		.normal()
		.expect("normal");
	assert_eq!(
		get_xml(&array)?,
		&[
			"<LigatureArray>",
			"  <!-- LigatureCount=2 -->",
			"  <LigatureAttach index=\"0\">",
			"    <!-- ComponentCount=2 -->",
			"    <ComponentRecord index=\"0\">",
			"      <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"2\" Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"-20\"/>",
			"      </LigatureAnchor>",
			"      <LigatureAnchor index=\"3\" empty=\"1\"/>",
			"    </ComponentRecord>",
			"    <ComponentRecord index=\"1\">",
			"      <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"2\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"3\" empty=\"1\"/>",
			"    </ComponentRecord>",
			"  </LigatureAttach>",
			"  <LigatureAttach index=\"1\">",
			"    <!-- ComponentCount=2 -->",
			"    <ComponentRecord index=\"0\">",
			"      <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"2\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"3\" empty=\"1\"/>",
			"    </ComponentRecord>",
			"    <ComponentRecord index=\"1\">",
			"      <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"      <LigatureAnchor index=\"1\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"350\"/>",
			"      </LigatureAnchor>",
			"      <LigatureAnchor index=\"2\" Format=\"1\">",
			"        <XCoordinate value=\"1300\"/>",
			"        <YCoordinate value=\"-20\"/>",
			"      </LigatureAnchor>",
			"      <LigatureAnchor index=\"3\" empty=\"1\"/>",
			"    </ComponentRecord>",
			"  </LigatureAttach>",
			"</LigatureArray>",
		]
	);
	Ok(())
}

#[test]
fn test_build_ligature_attach() -> Result<()> {
	let attach = build_ligature_attach(
		vec![
			Some(vec![Some(anchor(500, -10)), None]),
			Some(vec![None, Some(anchor(300, -20)), None]),
		],
		false,
	)
	.normal()
	.expect("normal");
	assert_eq!(
		get_xml(&attach)?,
		&[
			"<LigatureAttach>",
			"  <!-- ComponentCount=2 -->",
			"  <ComponentRecord index=\"0\">",
			"    <LigatureAnchor index=\"0\" Format=\"1\">",
			"      <XCoordinate value=\"500\"/>",
			"      <YCoordinate value=\"-10\"/>",
			"    </LigatureAnchor>",
			"    <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"  </ComponentRecord>",
			"  <ComponentRecord index=\"1\">",
			"    <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"    <LigatureAnchor index=\"1\" Format=\"1\">",
			"      <XCoordinate value=\"300\"/>",
			"      <YCoordinate value=\"-20\"/>",
			"    </LigatureAnchor>",
			"    <LigatureAnchor index=\"2\" empty=\"1\"/>",
			"  </ComponentRecord>",
			"</LigatureAttach>",
		]
	);
	Ok(())
}

#[test]
fn test_build_ligature_attach_no_components() -> Result<()> {
	let attach = build_ligature_attach(Vec::new(), false)
		.normal()
		.expect("normal");
	assert_eq!(
		get_xml(&attach)?,
		&[
			"<LigatureAttach>",
			"  <!-- ComponentCount=0 -->",
			"</LigatureAttach>",
		]
	);
	Ok(())
}

fn coords(items: &[(&str, &[i16])]) -> IndexMap<String, Vec<CaretCoord>> {
	items
		.iter()
		.map(|(g, c)| (s(g), c.iter().map(|&c| CaretCoord::Coord(c)).collect()))
		.collect()
}

fn points(items: &[(&str, &[u16])]) -> IndexMap<String, Vec<u16>> {
	items.iter().map(|(g, p)| (s(g), p.to_vec())).collect()
}

#[test]
fn test_build_lig_caret_list() -> Result<()> {
	let carets = build_lig_caret_list(
		&coords(&[("f_f_i", &[300, 600])]),
		&points(&[("c_t", &[42])]),
		&glyph_map(),
	)?
	.and_then(MaybeExtended::normal)
	.expect("carets");
	assert_eq!(
		get_xml(&carets)?,
		&[
			"<LigCaretList>",
			"  <Coverage>",
			"    <Glyph value=\"f_f_i\"/>",
			"    <Glyph value=\"c_t\"/>",
			"  </Coverage>",
			"  <!-- LigGlyphCount=2 -->",
			"  <LigGlyph index=\"0\">",
			"    <!-- CaretCount=2 -->",
			"    <CaretValue index=\"0\" Format=\"1\">",
			"      <Coordinate value=\"300\"/>",
			"    </CaretValue>",
			"    <CaretValue index=\"1\" Format=\"1\">",
			"      <Coordinate value=\"600\"/>",
			"    </CaretValue>",
			"  </LigGlyph>",
			"  <LigGlyph index=\"1\">",
			"    <!-- CaretCount=1 -->",
			"    <CaretValue index=\"0\" Format=\"2\">",
			"      <CaretValuePoint value=\"42\"/>",
			"    </CaretValue>",
			"  </LigGlyph>",
			"</LigCaretList>",
		]
	);
	Ok(())
}

#[test]
fn test_build_lig_caret_list_both_coords_and_points_for_same_glyph() -> Result<()> {
	let carets = build_lig_caret_list(
		&coords(&[("f_f_i", &[300])]),
		&points(&[("f_f_i", &[7])]),
		&glyph_map(),
	)?
	.and_then(MaybeExtended::normal)
	.expect("carets");
	assert_eq!(
		get_xml(&carets)?,
		&[
			"<LigCaretList>",
			"  <Coverage>",
			"    <Glyph value=\"f_f_i\"/>",
			"  </Coverage>",
			"  <!-- LigGlyphCount=1 -->",
			"  <LigGlyph index=\"0\">",
			"    <!-- CaretCount=2 -->",
			"    <CaretValue index=\"0\" Format=\"1\">",
			"      <Coordinate value=\"300\"/>",
			"    </CaretValue>",
			"    <CaretValue index=\"1\" Format=\"2\">",
			"      <CaretValuePoint value=\"7\"/>",
			"    </CaretValue>",
			"  </LigGlyph>",
			"</LigCaretList>",
		]
	);
	Ok(())
}

#[test]
fn test_build_lig_caret_list_extended() -> Result<()> {
	let carets =
		build_lig_caret_list(&coords(&[("f_f_i", &[300])]), &IndexMap::new(), &extended())?;
	assert!(matches!(carets, Some(MaybeExtended::Extended(_))));
	Ok(())
}

#[test]
fn test_build_lig_caret_list_empty() -> Result<()> {
	assert!(build_lig_caret_list(&IndexMap::new(), &IndexMap::new(), &glyph_map())?.is_none());
	Ok(())
}

#[test]
fn test_build_lig_glyph_coords() -> Result<()> {
	let lig = build_lig_glyph(
		Some(&[CaretCoord::Coord(500), CaretCoord::Coord(800)]),
		None,
	)
	.expect("lig");
	assert_eq!(
		get_xml(&lig)?,
		&[
			"<LigGlyph>",
			"  <!-- CaretCount=2 -->",
			"  <CaretValue index=\"0\" Format=\"1\">",
			"    <Coordinate value=\"500\"/>",
			"  </CaretValue>",
			"  <CaretValue index=\"1\" Format=\"1\">",
			"    <Coordinate value=\"800\"/>",
			"  </CaretValue>",
			"</LigGlyph>",
		]
	);
	Ok(())
}

#[test]
fn test_build_lig_glyph_empty() {
	assert!(build_lig_glyph(Some(&[]), Some(&[])).is_none());
}

#[test]
fn test_build_lig_glyph_none() {
	assert!(build_lig_glyph(None, None).is_none());
}

#[test]
fn test_build_lig_glyph_points() -> Result<()> {
	let lig = build_lig_glyph(None, Some(&[2])).expect("lig");
	assert_eq!(
		get_xml(&lig)?,
		&[
			"<LigGlyph>",
			"  <!-- CaretCount=1 -->",
			"  <CaretValue index=\"0\" Format=\"2\">",
			"    <CaretValuePoint value=\"2\"/>",
			"  </CaretValue>",
			"</LigGlyph>",
		]
	);
	Ok(())
}

fn single_subst(pairs: &[(&str, &str)]) -> Result<Option<LookupSubTable>> {
	let mapping: IndexMap<String, String> = pairs.iter().map(|(a, b)| (s(a), s(b))).collect();
	Ok(build_single_subst_subtable(&mapping, &glyph_map())?.map(LookupSubTable::SingleSubst))
}

#[test]
fn test_build_lookup() -> Result<()> {
	let s1 = single_subst(&[("one", "two")])?;
	let s2 = single_subst(&[("three", "four")])?;
	let lookup = build_lookup(vec![s1, s2], 7, None, None, false)?.expect("lookup");
	assert_eq!(
		get_xml(&lookup)?,
		&[
			"<Lookup>",
			"  <LookupType value=\"1\"/>",
			"  <LookupFlag value=\"7\"/><!-- rightToLeft ignoreBaseGlyphs ignoreLigatures -->",
			"  <!-- SubTableCount=2 -->",
			"  <SingleSubst index=\"0\">",
			"    <Substitution in=\"one\" out=\"two\"/>",
			"  </SingleSubst>",
			"  <SingleSubst index=\"1\">",
			"    <Substitution in=\"three\" out=\"four\"/>",
			"  </SingleSubst>",
			"</Lookup>",
		]
	);
	Ok(())
}

#[test]
fn test_build_lookup_bad_flags() -> Result<()> {
	let st = single_subst(&[("one", "two")])?;
	let err = build_lookup(
		vec![st],
		LOOKUP_FLAG_USE_MARK_FILTERING_SET,
		None,
		None,
		false,
	)
	.expect_err("bad flags");
	assert!(matches!(&err, Error::Assertion(_)));
	assert!(err.to_string().contains(
		"if markFilterSet is None, flags must not set LOOKUP_FLAG_USE_MARK_FILTERING_SET; flags=0x0010"
	));
	Ok(())
}

#[test]
fn test_build_lookup_conflicting_subtable_types() -> Result<()> {
	let s1 = single_subst(&[("one", "two")])?;
	let alternates: IndexMap<String, Vec<String>> = [(s("one"), strings(&["two", "three"]))]
		.into_iter()
		.collect();
	let s2 = build_alternate_subst_subtable(&alternates, &glyph_map())?
		.map(LookupSubTable::AlternateSubst);
	let err = build_lookup(vec![s1, s2], 0, None, None, false).expect_err("conflict");
	assert!(
		err.to_string()
			.contains("all subtables must have the same LookupType")
	);
	Ok(())
}

#[test]
fn test_build_lookup_no_subtables() -> Result<()> {
	assert!(build_lookup(vec![], 0, None, None, false)?.is_none());
	assert!(build_lookup(vec![None], 0, None, None, false)?.is_none());
	assert!(build_lookup(vec![None, None], 0, None, None, false)?.is_none());
	Ok(())
}

#[test]
fn test_build_lookup_mark_filter_set() -> Result<()> {
	let st = single_subst(&[("one", "two")])?;
	let flags = LOOKUP_FLAG_RIGHT_TO_LEFT | LOOKUP_FLAG_USE_MARK_FILTERING_SET;
	let lookup = build_lookup(vec![st], flags, Some(999), None, false)?.expect("lookup");
	assert_eq!(
		get_xml(&lookup)?,
		&[
			"<Lookup>",
			"  <LookupType value=\"1\"/>",
			"  <LookupFlag value=\"17\"/><!-- rightToLeft useMarkFilteringSet -->",
			"  <!-- SubTableCount=1 -->",
			"  <SingleSubst index=\"0\">",
			"    <Substitution in=\"one\" out=\"two\"/>",
			"  </SingleSubst>",
			"  <MarkFilteringSet value=\"999\"/>",
			"</Lookup>",
		]
	);
	Ok(())
}

fn marks(items: &[(&str, u16, Anchor)]) -> IndexMap<String, (u16, Anchor)> {
	items
		.iter()
		.map(|(g, c, a)| (s(g), (*c, a.clone())))
		.collect()
}

#[test]
fn test_build_mark_array() -> Result<()> {
	let array = build_mark_array(
		&marks(&[("acute", 7, anchor(300, 800)), ("grave", 2, anchor(10, 80))]),
		&glyph_map(),
	)?
	.normal()
	.expect("normal");
	assert!(glyph_map().get("grave") < glyph_map().get("acute"));
	assert_eq!(
		get_xml(&array)?,
		&[
			"<MarkArray>",
			"  <!-- MarkCount=2 -->",
			"  <MarkRecord index=\"0\">",
			"    <Class value=\"2\"/>",
			"    <MarkAnchor Format=\"1\">",
			"      <XCoordinate value=\"10\"/>",
			"      <YCoordinate value=\"80\"/>",
			"    </MarkAnchor>",
			"  </MarkRecord>",
			"  <MarkRecord index=\"1\">",
			"    <Class value=\"7\"/>",
			"    <MarkAnchor Format=\"1\">",
			"      <XCoordinate value=\"300\"/>",
			"      <YCoordinate value=\"800\"/>",
			"    </MarkAnchor>",
			"  </MarkRecord>",
			"</MarkArray>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark_base_pos_subtable() -> Result<()> {
	let marks = marks(&[
		("acute", 0, anchor(300, 700)),
		("cedilla", 1, anchor(300, -100)),
		("grave", 0, anchor(300, 700)),
	]);
	let bases: IndexMap<String, BTreeMap<u16, Anchor>> = [
		// Make sure we can handle missing entries.
		(s("A"), classes(&[])),
		(s("B"), classes(&[(0, anchor(500, 900))])),
		(s("C"), classes(&[(1, anchor(500, -10))])),
		(
			s("a"),
			classes(&[(0, anchor(500, 400)), (1, anchor(500, -20))]),
		),
		(
			s("b"),
			classes(&[(0, anchor(500, 800)), (1, anchor(500, -20))]),
		),
	]
	.into_iter()
	.collect();
	let table = build_mark_base_pos_subtable(&marks, &bases, &glyph_map())?;
	assert_eq!(
		get_xml(&table)?,
		&[
			"<MarkBasePos Format=\"1\">",
			"  <MarkCoverage>",
			"    <Glyph value=\"grave\"/>",
			"    <Glyph value=\"acute\"/>",
			"    <Glyph value=\"cedilla\"/>",
			"  </MarkCoverage>",
			"  <BaseCoverage>",
			"    <Glyph value=\"A\"/>",
			"    <Glyph value=\"B\"/>",
			"    <Glyph value=\"C\"/>",
			"    <Glyph value=\"a\"/>",
			"    <Glyph value=\"b\"/>",
			"  </BaseCoverage>",
			"  <!-- ClassCount=2 -->",
			"  <MarkArray>",
			"    <!-- MarkCount=3 -->",
			"    <MarkRecord index=\"0\">",
			"      <Class value=\"0\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"700\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"    <MarkRecord index=\"1\">",
			"      <Class value=\"0\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"700\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"    <MarkRecord index=\"2\">",
			"      <Class value=\"1\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"-100\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"  </MarkArray>",
			"  <BaseArray>",
			"    <!-- BaseCount=5 -->",
			"    <BaseRecord index=\"0\">",
			"      <BaseAnchor index=\"0\" empty=\"1\"/>",
			"      <BaseAnchor index=\"1\" empty=\"1\"/>",
			"    </BaseRecord>",
			"    <BaseRecord index=\"1\">",
			"      <BaseAnchor index=\"0\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"900\"/>",
			"      </BaseAnchor>",
			"      <BaseAnchor index=\"1\" empty=\"1\"/>",
			"    </BaseRecord>",
			"    <BaseRecord index=\"2\">",
			"      <BaseAnchor index=\"0\" empty=\"1\"/>",
			"      <BaseAnchor index=\"1\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"-10\"/>",
			"      </BaseAnchor>",
			"    </BaseRecord>",
			"    <BaseRecord index=\"3\">",
			"      <BaseAnchor index=\"0\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"400\"/>",
			"      </BaseAnchor>",
			"      <BaseAnchor index=\"1\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"-20\"/>",
			"      </BaseAnchor>",
			"    </BaseRecord>",
			"    <BaseRecord index=\"4\">",
			"      <BaseAnchor index=\"0\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"800\"/>",
			"      </BaseAnchor>",
			"      <BaseAnchor index=\"1\" Format=\"1\">",
			"        <XCoordinate value=\"500\"/>",
			"        <YCoordinate value=\"-20\"/>",
			"      </BaseAnchor>",
			"    </BaseRecord>",
			"  </BaseArray>",
			"</MarkBasePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark_base_pos_subtable_format2() -> Result<()> {
	let bases: IndexMap<String, BTreeMap<u16, Anchor>> =
		[(s("A"), classes(&[(0, anchor2())]))].into_iter().collect();
	let table =
		build_mark_base_pos_subtable(&marks(&[("acute", 0, anchor1())]), &bases, &extended())?;
	assert!(matches!(
		table,
		ot::MarkBasePos::Format2(t) if t.mark_array.is_some() && t.base_array.is_some()
	));
	Ok(())
}

fn sets(items: &[&[&str]]) -> Vec<Vec<String>> {
	items.iter().map(|s| strings(s)).collect()
}

#[test]
fn test_build_mark_glyph_sets_def() -> Result<()> {
	let marksets = build_mark_glyph_sets_def(
		&sets(&[&["acute", "grave"], &["cedilla", "grave"]]),
		&glyph_map(),
	)?
	.expect("sets");
	assert_eq!(
		get_xml(&marksets)?,
		&[
			"<MarkGlyphSetsDef>",
			"  <MarkSetTableFormat value=\"1\"/>",
			"  <!-- MarkSetCount=2 -->",
			"  <Coverage index=\"0\">",
			"    <Glyph value=\"grave\"/>",
			"    <Glyph value=\"acute\"/>",
			"  </Coverage>",
			"  <Coverage index=\"1\">",
			"    <Glyph value=\"grave\"/>",
			"    <Glyph value=\"cedilla\"/>",
			"  </Coverage>",
			"</MarkGlyphSetsDef>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark_glyph_sets_def_empty() -> Result<()> {
	assert!(build_mark_glyph_sets_def(&[], &glyph_map())?.is_none());
	Ok(())
}

#[test]
fn test_build_mark_glyph_sets_def_empty_mark_set() -> Result<()> {
	let marksets =
		build_mark_glyph_sets_def(&sets(&[&["acute", "grave"], &[]]), &glyph_map())?.expect("sets");
	assert_eq!(
		get_xml(&marksets)?,
		&[
			"<MarkGlyphSetsDef>",
			"  <MarkSetTableFormat value=\"1\"/>",
			"  <!-- MarkSetCount=2 -->",
			"  <Coverage index=\"0\">",
			"    <Glyph value=\"grave\"/>",
			"    <Glyph value=\"acute\"/>",
			"  </Coverage>",
			"  <Coverage index=\"1\">",
			"  </Coverage>",
			"</MarkGlyphSetsDef>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark_lig_pos_subtable() -> Result<()> {
	let marks = marks(&[
		("acute", 0, anchor(300, 700)),
		("cedilla", 1, anchor(300, -100)),
		("grave", 0, anchor(300, 700)),
	]);
	let bases: IndexMap<String, Vec<BTreeMap<u16, Anchor>>> = [
		// Nothing on f; only 1 on i.
		(
			s("f_i"),
			vec![classes(&[]), classes(&[(0, anchor(200, 400))])],
		),
		(
			s("c_t"),
			vec![
				classes(&[(0, anchor(500, 600)), (1, anchor(500, -20))]),
				classes(&[(0, anchor(1300, 800)), (1, anchor(1300, -20))]),
			],
		),
	]
	.into_iter()
	.collect();
	let table = build_mark_lig_pos_subtable(&marks, &bases, &glyph_map())?;
	assert_eq!(
		get_xml(&table)?,
		&[
			"<MarkLigPos Format=\"1\">",
			"  <MarkCoverage>",
			"    <Glyph value=\"grave\"/>",
			"    <Glyph value=\"acute\"/>",
			"    <Glyph value=\"cedilla\"/>",
			"  </MarkCoverage>",
			"  <LigatureCoverage>",
			"    <Glyph value=\"f_i\"/>",
			"    <Glyph value=\"c_t\"/>",
			"  </LigatureCoverage>",
			"  <!-- ClassCount=2 -->",
			"  <MarkArray>",
			"    <!-- MarkCount=3 -->",
			"    <MarkRecord index=\"0\">",
			"      <Class value=\"0\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"700\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"    <MarkRecord index=\"1\">",
			"      <Class value=\"0\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"700\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"    <MarkRecord index=\"2\">",
			"      <Class value=\"1\"/>",
			"      <MarkAnchor Format=\"1\">",
			"        <XCoordinate value=\"300\"/>",
			"        <YCoordinate value=\"-100\"/>",
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"  </MarkArray>",
			"  <LigatureArray>",
			"    <!-- LigatureCount=2 -->",
			"    <LigatureAttach index=\"0\">",
			"      <!-- ComponentCount=2 -->",
			"      <ComponentRecord index=\"0\">",
			"        <LigatureAnchor index=\"0\" empty=\"1\"/>",
			"        <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"      </ComponentRecord>",
			"      <ComponentRecord index=\"1\">",
			"        <LigatureAnchor index=\"0\" Format=\"1\">",
			"          <XCoordinate value=\"200\"/>",
			"          <YCoordinate value=\"400\"/>",
			"        </LigatureAnchor>",
			"        <LigatureAnchor index=\"1\" empty=\"1\"/>",
			"      </ComponentRecord>",
			"    </LigatureAttach>",
			"    <LigatureAttach index=\"1\">",
			"      <!-- ComponentCount=2 -->",
			"      <ComponentRecord index=\"0\">",
			"        <LigatureAnchor index=\"0\" Format=\"1\">",
			"          <XCoordinate value=\"500\"/>",
			"          <YCoordinate value=\"600\"/>",
			"        </LigatureAnchor>",
			"        <LigatureAnchor index=\"1\" Format=\"1\">",
			"          <XCoordinate value=\"500\"/>",
			"          <YCoordinate value=\"-20\"/>",
			"        </LigatureAnchor>",
			"      </ComponentRecord>",
			"      <ComponentRecord index=\"1\">",
			"        <LigatureAnchor index=\"0\" Format=\"1\">",
			"          <XCoordinate value=\"1300\"/>",
			"          <YCoordinate value=\"800\"/>",
			"        </LigatureAnchor>",
			"        <LigatureAnchor index=\"1\" Format=\"1\">",
			"          <XCoordinate value=\"1300\"/>",
			"          <YCoordinate value=\"-20\"/>",
			"        </LigatureAnchor>",
			"      </ComponentRecord>",
			"    </LigatureAttach>",
			"  </LigatureArray>",
			"</MarkLigPos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark_lig_pos_subtable_format2() -> Result<()> {
	let ligs: IndexMap<String, Vec<BTreeMap<u16, Anchor>>> =
		[(s("f_i"), vec![classes(&[(0, anchor2())])])]
			.into_iter()
			.collect();
	let table =
		build_mark_lig_pos_subtable(&marks(&[("acute", 0, anchor1())]), &ligs, &extended())?;
	assert!(matches!(
		table,
		ot::MarkLigPos::Format2(t) if t.mark_array.is_some() && t.ligature_array.is_some()
	));
	Ok(())
}

#[test]
fn test_mark_mark_pos_builder_format2() -> Result<()> {
	let mut builder = LookupBuilder::mark_mark_pos(extended(), None);
	if let LookupBuilder::MarkMarkPos(b) = &mut builder {
		b.marks = [(s("acute"), (s("top"), anchor1()))].into_iter().collect();
		b.base_marks = [(s("grave"), [(s("top"), anchor2())].into_iter().collect())]
			.into_iter()
			.collect();
	}
	let lookup = builder.build(&mut font())?.expect("lookup");
	let [Some(subtable)] = lookup.sub_table.as_slice() else {
		panic!("one subtable expected");
	};
	assert!(matches!(
		&**subtable,
		LookupSubTable::MarkMarkPos(ot::MarkMarkPos::Format2(t))
			if t.mark1_array.is_some() && t.mark2_array.is_some()
	));
	Ok(())
}

#[test]
fn test_build_mark_record() -> Result<()> {
	let rec = build_mark_record(17, anchor(500, -20), false)
		.normal()
		.expect("normal");
	assert_eq!(
		get_xml(&rec)?,
		&[
			"<MarkRecord>",
			"  <Class value=\"17\"/>",
			"  <MarkAnchor Format=\"1\">",
			"    <XCoordinate value=\"500\"/>",
			"    <YCoordinate value=\"-20\"/>",
			"  </MarkAnchor>",
			"</MarkRecord>",
		]
	);
	Ok(())
}

#[test]
fn test_build_mark2_record() -> Result<()> {
	let rec = build_mark2_record(
		vec![Some(anchor(500, -20)), None, Some(anchor(300, -15))],
		false,
	)
	.normal()
	.expect("normal");
	assert_eq!(
		get_xml(&rec)?,
		&[
			"<Mark2Record>",
			"  <Mark2Anchor index=\"0\" Format=\"1\">",
			"    <XCoordinate value=\"500\"/>",
			"    <YCoordinate value=\"-20\"/>",
			"  </Mark2Anchor>",
			"  <Mark2Anchor index=\"1\" empty=\"1\"/>",
			"  <Mark2Anchor index=\"2\" Format=\"1\">",
			"    <XCoordinate value=\"300\"/>",
			"    <YCoordinate value=\"-15\"/>",
			"  </Mark2Anchor>",
			"</Mark2Record>",
		]
	);
	Ok(())
}

fn class(v: &[&str]) -> Vec<String> {
	strings(v)
}

#[test]
fn test_build_pair_pos_classes_subtable() -> Result<()> {
	let d20 = value(&[("XPlacement", -20)]);
	let d50 = value(&[("XPlacement", -50)]);
	let d0 = value(&[]);
	let d8020 = value(&[("XPlacement", -80), ("YPlacement", -20)]);
	let pairs: IndexMap<(Vec<String>, Vec<String>), ValuePair> = [
		(
			(class(&["A"]), class(&["zero"])),
			(Some(d0), Some(d50.clone())),
		),
		((class(&["A"]), class(&["one", "two"])), (None, Some(d20))),
		(
			(class(&["B", "C"]), class(&["zero"])),
			(Some(d8020), Some(d50)),
		),
	]
	.into_iter()
	.collect();
	let subtable = build_pair_pos_classes_subtable(&pairs, &glyph_map(), None, None)?;
	assert_eq!(
		get_xml(&subtable)?,
		&[
			"<PairPos Format=\"2\">",
			"  <Coverage>",
			"    <Glyph value=\"A\"/>",
			"    <Glyph value=\"B\"/>",
			"    <Glyph value=\"C\"/>",
			"  </Coverage>",
			"  <ValueFormat1 value=\"3\"/>",
			"  <ValueFormat2 value=\"1\"/>",
			"  <ClassDef1>",
			"    <ClassDef glyph=\"A\" class=\"1\"/>",
			"  </ClassDef1>",
			"  <ClassDef2>",
			"    <ClassDef glyph=\"one\" class=\"1\"/>",
			"    <ClassDef glyph=\"two\" class=\"1\"/>",
			"    <ClassDef glyph=\"zero\" class=\"2\"/>",
			"  </ClassDef2>",
			"  <!-- Class1Count=2 -->",
			"  <!-- Class2Count=3 -->",
			"  <Class1Record index=\"0\">",
			"    <Class2Record index=\"0\">",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"0\"/>",
			"    </Class2Record>",
			"    <Class2Record index=\"1\">",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"0\"/>",
			"    </Class2Record>",
			"    <Class2Record index=\"2\">",
			"      <Value1 XPlacement=\"-80\" YPlacement=\"-20\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </Class2Record>",
			"  </Class1Record>",
			"  <Class1Record index=\"1\">",
			"    <Class2Record index=\"0\">",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"0\"/>",
			"    </Class2Record>",
			"    <Class2Record index=\"1\">",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"-20\"/>",
			"    </Class2Record>",
			"    <Class2Record index=\"2\">",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </Class2Record>",
			"  </Class1Record>",
			"</PairPos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_pair_pos_classes_subtable_format4() -> Result<()> {
	let pairs: IndexMap<(Vec<String>, Vec<String>), ValuePair> =
		[((class(&["one"]), class(&["two"])), (Some(value(&[])), None))]
			.into_iter()
			.collect();
	let subtable = build_pair_pos_classes_subtable(&pairs, &extended(), None, None)?;
	assert!(matches!(subtable, ot::PairPos::Format4(_)));
	Ok(())
}

#[test]
fn test_build_pair_pos_glyphs() -> Result<()> {
	let d50 = value(&[("XPlacement", -50)]);
	let d8020 = value(&[("XPlacement", -80), ("YPlacement", -20)]);
	let pairs: IndexMap<(String, String), ValuePair> = [
		((s("A"), s("zero")), (None, Some(d50.clone()))),
		((s("A"), s("one")), (Some(d8020), Some(d50))),
	]
	.into_iter()
	.collect();
	let subtables = build_pair_pos_glyphs(&pairs, &glyph_map())?;
	let mut xml = Vec::new();
	for t in &subtables {
		xml.extend(get_xml(t)?);
	}
	assert_eq!(
		xml,
		&[
			"<PairPos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"A\"/>",
			"  </Coverage>",
			"  <ValueFormat1 value=\"0\"/>",
			"  <ValueFormat2 value=\"1\"/>",
			"  <!-- PairSetCount=1 -->",
			"  <PairSet index=\"0\">",
			"    <!-- PairValueCount=1 -->",
			"    <PairValueRecord index=\"0\">",
			"      <SecondGlyph value=\"zero\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </PairValueRecord>",
			"  </PairSet>",
			"</PairPos>",
			"<PairPos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"A\"/>",
			"  </Coverage>",
			"  <ValueFormat1 value=\"3\"/>",
			"  <ValueFormat2 value=\"1\"/>",
			"  <!-- PairSetCount=1 -->",
			"  <PairSet index=\"0\">",
			"    <!-- PairValueCount=1 -->",
			"    <PairValueRecord index=\"0\">",
			"      <SecondGlyph value=\"one\"/>",
			"      <Value1 XPlacement=\"-80\" YPlacement=\"-20\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </PairValueRecord>",
			"  </PairSet>",
			"</PairPos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_pair_pos_glyphs_subtable() -> Result<()> {
	let d20 = value(&[("XPlacement", -20)]);
	let d50 = value(&[("XPlacement", -50)]);
	let d0 = value(&[]);
	let d8020 = value(&[("XPlacement", -80), ("YPlacement", -20)]);
	let pairs: IndexMap<(String, String), ValuePair> = [
		((s("A"), s("zero")), (Some(d0), Some(d50.clone()))),
		((s("A"), s("one")), (None, Some(d20))),
		((s("B"), s("five")), (Some(d8020), Some(d50))),
	]
	.into_iter()
	.collect();
	let subtable = build_pair_pos_glyphs_subtable(&pairs, &glyph_map(), None, None)?;
	assert_eq!(
		get_xml(&subtable)?,
		&[
			"<PairPos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"A\"/>",
			"    <Glyph value=\"B\"/>",
			"  </Coverage>",
			"  <ValueFormat1 value=\"3\"/>",
			"  <ValueFormat2 value=\"1\"/>",
			"  <!-- PairSetCount=2 -->",
			"  <PairSet index=\"0\">",
			"    <!-- PairValueCount=2 -->",
			"    <PairValueRecord index=\"0\">",
			"      <SecondGlyph value=\"zero\"/>",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </PairValueRecord>",
			"    <PairValueRecord index=\"1\">",
			"      <SecondGlyph value=\"one\"/>",
			"      <Value1 XPlacement=\"0\" YPlacement=\"0\"/>",
			"      <Value2 XPlacement=\"-20\"/>",
			"    </PairValueRecord>",
			"  </PairSet>",
			"  <PairSet index=\"1\">",
			"    <!-- PairValueCount=1 -->",
			"    <PairValueRecord index=\"0\">",
			"      <SecondGlyph value=\"five\"/>",
			"      <Value1 XPlacement=\"-80\" YPlacement=\"-20\"/>",
			"      <Value2 XPlacement=\"-50\"/>",
			"    </PairValueRecord>",
			"  </PairSet>",
			"</PairPos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_pair_pos_glyphs_subtable_format3() -> Result<()> {
	let pairs: IndexMap<(String, String), ValuePair> =
		[((s("one"), s("two")), (Some(value(&[])), None))]
			.into_iter()
			.collect();
	let subtable = build_pair_pos_glyphs_subtable(&pairs, &extended(), None, None)?;
	assert!(matches!(subtable, ot::PairPos::Format3(t) if t.pair_set.len() == 1));
	Ok(())
}

fn single_pos_mapping(items: &[(&str, ValueRecord)]) -> IndexMap<String, ValueRecord> {
	items.iter().map(|(g, v)| (s(g), v.clone())).collect()
}

#[test]
fn test_build_single_pos() -> Result<()> {
	let x = |v| value(&[("XPlacement", v)]);
	let subtables = build_single_pos(
		&single_pos_mapping(&[
			("one", x(500)),
			("two", x(500)),
			("three", x(200)),
			("four", x(400)),
			("five", x(500)),
			("six", value(&[("YPlacement", -6)])),
		]),
		&glyph_map(),
	)?;
	let mut xml = Vec::new();
	for t in &subtables {
		xml.extend(get_xml(t)?);
	}
	assert_eq!(
		xml,
		&[
			"<SinglePos Format=\"2\">",
			"  <Coverage>",
			"    <Glyph value=\"one\"/>",
			"    <Glyph value=\"two\"/>",
			"    <Glyph value=\"three\"/>",
			"    <Glyph value=\"four\"/>",
			"    <Glyph value=\"five\"/>",
			"  </Coverage>",
			"  <ValueFormat value=\"1\"/>",
			"  <!-- ValueCount=5 -->",
			"  <Value index=\"0\" XPlacement=\"500\"/>",
			"  <Value index=\"1\" XPlacement=\"500\"/>",
			"  <Value index=\"2\" XPlacement=\"200\"/>",
			"  <Value index=\"3\" XPlacement=\"400\"/>",
			"  <Value index=\"4\" XPlacement=\"500\"/>",
			"</SinglePos>",
			"<SinglePos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"six\"/>",
			"  </Coverage>",
			"  <ValueFormat value=\"2\"/>",
			"  <Value YPlacement=\"-6\"/>",
			"</SinglePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_single_pos_value_format0() -> Result<()> {
	let subtables = build_single_pos(&single_pos_mapping(&[("zero", value(&[]))]), &glyph_map())?;
	let mut xml = Vec::new();
	for t in &subtables {
		xml.extend(get_xml(t)?);
	}
	assert_eq!(
		xml,
		&[
			"<SinglePos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"zero\"/>",
			"  </Coverage>",
			"  <ValueFormat value=\"0\"/>",
			"</SinglePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_single_pos_subtable_format1() -> Result<()> {
	let x = value(&[("XPlacement", 777)]);
	let subtable = build_single_pos_subtable(
		&single_pos_mapping(&[("one", x.clone()), ("two", x)]),
		&glyph_map(),
	)?;
	assert_eq!(
		get_xml(&subtable)?,
		&[
			"<SinglePos Format=\"1\">",
			"  <Coverage>",
			"    <Glyph value=\"one\"/>",
			"    <Glyph value=\"two\"/>",
			"  </Coverage>",
			"  <ValueFormat value=\"1\"/>",
			"  <Value XPlacement=\"777\"/>",
			"</SinglePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_single_pos_subtable_format2() -> Result<()> {
	let subtable = build_single_pos_subtable(
		&single_pos_mapping(&[
			("one", value(&[("XPlacement", 777)])),
			("two", value(&[("YPlacement", -888)])),
		]),
		&glyph_map(),
	)?;
	assert_eq!(
		get_xml(&subtable)?,
		&[
			"<SinglePos Format=\"2\">",
			"  <Coverage>",
			"    <Glyph value=\"one\"/>",
			"    <Glyph value=\"two\"/>",
			"  </Coverage>",
			"  <ValueFormat value=\"3\"/>",
			"  <!-- ValueCount=2 -->",
			"  <Value index=\"0\" XPlacement=\"777\" YPlacement=\"0\"/>",
			"  <Value index=\"1\" XPlacement=\"0\" YPlacement=\"-888\"/>",
			"</SinglePos>",
		]
	);
	Ok(())
}

#[test]
fn test_build_single_pos_subtable_format3() -> Result<()> {
	let subtable = build_single_pos_subtable(
		&single_pos_mapping(&[("one", value(&[("XPlacement", 777)]))]),
		&extended(),
	)?;
	assert!(matches!(subtable, ot::SinglePos::Format3(_)));
	Ok(())
}

#[test]
fn test_build_single_pos_subtable_format4() -> Result<()> {
	let subtable = build_single_pos_subtable(
		&single_pos_mapping(&[
			("one", value(&[("XPlacement", 777)])),
			("two", value(&[("YPlacement", -888)])),
		]),
		&extended(),
	)?;
	assert!(matches!(subtable, ot::SinglePos::Format4(_)));
	Ok(())
}

#[test]
fn test_build_value() -> Result<()> {
	let v = value(&[("XPlacement", 7), ("YPlacement", 23)]);
	let mut writer = XmlWriter::with_options("  ", "\n");
	v.to_xml(&mut writer, &mut font(), "Val", &[])?;
	assert_eq!(
		lines(writer.into_string()),
		&["<Val XPlacement=\"7\" YPlacement=\"23\"/>",]
	);
	Ok(())
}

#[test]
fn test_get_ligature_sort_key() {
	let components = |s: &str| -> Vec<Vec<String>> {
		s.split(' ')
			.map(|w| w.chars().map(|c| c.to_string()).collect())
			.collect()
	};
	let mut c = components("fi fl ff ffi fff");
	c.sort_by_key(|x| ligature_sort_key(x));
	assert_eq!(c, components("ffi fff fi fl ff"));
}

#[test]
fn test_get_single_pos_value_key() -> Result<()> {
	use std::hash::{BuildHasher, RandomState};
	let device = device(&[(10, 1), (11, 3)])?;
	let mut a1 = value(&[("XPlacement", 500)]);
	a1.x_pla_device = Some(device.map(Box::new));
	let a2 = a1.clone();
	let b = value(&[("XPlacement", 500)]);
	let key_a1 = get_single_pos_value_key(&a1);
	let key_a2 = get_single_pos_value_key(&a2);
	let key_b = get_single_pos_value_key(&b);
	let hasher = RandomState::new();
	assert_eq!(key_a1, key_a2);
	assert_eq!(hasher.hash_one(&key_a1), hasher.hash_one(&key_a2));
	assert_ne!(key_a1, key_b);
	assert_ne!(hasher.hash_one(&key_a1), hasher.hash_one(&key_b));
	Ok(())
}

fn sorted_set(v: &[&str]) -> Vec<String> {
	let mut v = strings(v);
	v.sort();
	v
}

#[test]
fn test_build_using_class0() -> Result<()> {
	let mut b = ClassDefBuilder::new(true);
	b.add(&sorted_set(&["aa", "bb"]))?;
	b.add(&sorted_set(&["a", "b"]))?;
	b.add(&sorted_set(&["c"]))?;
	b.add(&sorted_set(&["e", "f", "g", "h"]))?;
	let defs = b.class_defs();
	let expected: IndexMap<String, u32> = [
		(s("a"), 1),
		(s("b"), 1),
		(s("c"), 3),
		(s("aa"), 2),
		(s("bb"), 2),
	]
	.into_iter()
	.collect();
	assert_eq!(defs.len(), expected.len());
	for (k, v) in &expected {
		assert_eq!(defs.get(k), Some(v), "{k}");
	}
	Ok(())
}

#[test]
fn test_build_not_using_class0() -> Result<()> {
	let mut b = ClassDefBuilder::new(false);
	b.add(&sorted_set(&["a", "b"]))?;
	b.add(&sorted_set(&["c"]))?;
	b.add(&sorted_set(&["e", "f", "g", "h"]))?;
	let defs = b.class_defs();
	let expected: IndexMap<String, u32> = [
		(s("a"), 2),
		(s("b"), 2),
		(s("c"), 3),
		(s("e"), 1),
		(s("f"), 1),
		(s("g"), 1),
		(s("h"), 1),
	]
	.into_iter()
	.collect();
	assert_eq!(defs.len(), expected.len());
	for (k, v) in &expected {
		assert_eq!(defs.get(k), Some(v), "{k}");
	}
	Ok(())
}

#[test]
fn test_can_add() -> Result<()> {
	let mut b = ClassDefBuilder::new(true);
	b.add(&sorted_set(&["a", "b", "c", "d"]))?;
	b.add(&sorted_set(&["e", "f"]))?;
	assert!(b.can_add(&sorted_set(&["a", "b", "c", "d"])));
	assert!(b.can_add(&sorted_set(&["e", "f"])));
	assert!(b.can_add(&sorted_set(&["g", "h", "i"])));
	assert!(!b.can_add(&sorted_set(&["b", "c", "d"])));
	assert!(!b.can_add(&sorted_set(&["a", "b", "c", "d", "e", "f"])));
	assert!(!b.can_add(&sorted_set(&["d", "e", "f"])));
	assert!(!b.can_add(&sorted_set(&["f"])));
	Ok(())
}

#[test]
fn test_add_exception() -> Result<()> {
	let mut b = ClassDefBuilder::new(true);
	b.add(&sorted_set(&["a", "b", "c"]))?;
	assert!(matches!(
		b.add(&sorted_set(&["a", "d"])),
		Err(Error::OpenTypeLib { .. })
	));
	Ok(())
}

fn stat_font() -> Result<Font> {
	let mut font = Font::new();
	let mut name = Name::default();
	// fonttools#1985: a name ID below 256 that matches an axis name of a test,
	// which must not be reused: axis name IDs must be above 255.
	name.add_multilingual_name(&[("en", "ABCDTest")], Some(6), true, true, 0)?;
	font.insert(Box::new(name));
	Ok(font)
}

fn stat_ttx(font: &mut Font) -> Result<Vec<String>> {
	let ttx = font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"STAT"))],
		..Default::default()
	})?;
	let lines: Vec<String> = ttx.lines().map(str::to_owned).collect();
	Ok(lines[3..lines.len() - 2].to_vec())
}

fn value_on(name: &str, value: f64, flags: u16) -> StatAxisValue {
	StatAxisValue {
		name: StatName::from(name),
		flags,
		kind: StatAxisValueKind::Value {
			value,
			linked_value: None,
		},
	}
}

fn multilingual(items: &[(&str, &str)]) -> StatName {
	StatName::Multilingual(items.iter().map(|(l, s)| (s_(l), s_(s))).collect())
}

fn s_(v: &str) -> String {
	v.to_owned()
}

fn check_stat(
	axes: &[StatAxis],
	locations: Option<&[StatLocation]>,
	elided: StatName,
	expected: &[&str],
) -> Result<()> {
	let mut font = stat_font()?;
	build_stat_table(&mut font, axes, locations, &elided, true, true)?;
	assert_eq!(stat_ttx(&mut font)?, expected);
	// Compile and round-trip.
	let data = font.write(&WriteOptions::default())?;
	let mut font = Font::read(data, ReadOptions::default())?;
	assert_eq!(stat_ttx(&mut font)?, expected);
	Ok(())
}

fn tag(t: &str) -> Tag {
	Tag::from_str_exact(t).expect("tag")
}

#[test]
fn test_build_stat_table_0() -> Result<()> {
	let axes = [StatAxis {
		tag: tag("wght"),
		name: StatName::from("Weight"),
		ordering: None,
		values: vec![
			value_on("Thin", 100.0, 0),
			value_on("Regular", 400.0, 0x2),
			value_on("Black", 900.0, 0),
		],
	}];
	check_stat(
		&axes,
		None,
		StatName::from("Regular"),
		&[
			"  <STAT>",
			"    <Version value=\"0x00010001\"/>",
			"    <DesignAxisRecordSize value=\"8\"/>",
			"    <!-- DesignAxisCount=1 -->",
			"    <DesignAxisRecord>",
			"      <Axis index=\"0\">",
			"        <AxisTag value=\"wght\"/>",
			"        <AxisNameID value=\"257\"/>  <!-- Weight -->",
			"        <AxisOrdering value=\"0\"/>",
			"      </Axis>",
			"    </DesignAxisRecord>",
			"    <!-- AxisValueCount=3 -->",
			"    <AxisValueArray>",
			"      <AxisValue index=\"0\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"258\"/>  <!-- Thin -->",
			"        <Value value=\"100.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"1\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"256\"/>  <!-- Regular -->",
			"        <Value value=\"400.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"2\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"259\"/>  <!-- Black -->",
			"        <Value value=\"900.0\"/>",
			"      </AxisValue>",
			"    </AxisValueArray>",
			"    <ElidedFallbackNameID value=\"256\"/>  <!-- Regular -->",
			"  </STAT>",
		],
	)
}

#[test]
fn test_build_stat_table_1() -> Result<()> {
	let axes = [
		StatAxis {
			tag: tag("wght"),
			name: multilingual(&[("en", "Weight"), ("nl", "Gewicht")]),
			ordering: None,
			values: vec![
				StatAxisValue {
					name: multilingual(&[("en", "Thin"), ("nl", "Dun")]),
					flags: 0,
					kind: StatAxisValueKind::Value {
						value: 100.0,
						linked_value: None,
					},
				},
				value_on("Regular", 400.0, 0x2),
				value_on("Black", 900.0, 0),
			],
		},
		StatAxis {
			tag: tag("wdth"),
			name: StatName::from("Width"),
			ordering: None,
			values: vec![
				value_on("Condensed", 50.0, 0),
				value_on("Regular", 100.0, 0x2),
				value_on("Extended", 200.0, 0),
			],
		},
	];
	check_stat(
		&axes,
		None,
		StatName::Id(2),
		&[
			"  <STAT>",
			"    <Version value=\"0x00010001\"/>",
			"    <DesignAxisRecordSize value=\"8\"/>",
			"    <!-- DesignAxisCount=2 -->",
			"    <DesignAxisRecord>",
			"      <Axis index=\"0\">",
			"        <AxisTag value=\"wght\"/>",
			"        <AxisNameID value=\"256\"/>  <!-- Weight -->",
			"        <AxisOrdering value=\"0\"/>",
			"      </Axis>",
			"      <Axis index=\"1\">",
			"        <AxisTag value=\"wdth\"/>",
			"        <AxisNameID value=\"260\"/>  <!-- Width -->",
			"        <AxisOrdering value=\"1\"/>",
			"      </Axis>",
			"    </DesignAxisRecord>",
			"    <!-- AxisValueCount=6 -->",
			"    <AxisValueArray>",
			"      <AxisValue index=\"0\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"257\"/>  <!-- Thin -->",
			"        <Value value=\"100.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"1\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"258\"/>  <!-- Regular -->",
			"        <Value value=\"400.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"2\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"259\"/>  <!-- Black -->",
			"        <Value value=\"900.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"3\" Format=\"1\">",
			"        <AxisIndex value=\"1\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"261\"/>  <!-- Condensed -->",
			"        <Value value=\"50.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"4\" Format=\"1\">",
			"        <AxisIndex value=\"1\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"258\"/>  <!-- Regular -->",
			"        <Value value=\"100.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"5\" Format=\"1\">",
			"        <AxisIndex value=\"1\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"262\"/>  <!-- Extended -->",
			"        <Value value=\"200.0\"/>",
			"      </AxisValue>",
			"    </AxisValueArray>",
			"    <ElidedFallbackNameID value=\"2\"/>  <!-- missing from name table -->",
			"  </STAT>",
		],
	)
}

#[test]
fn test_build_stat_table_2() -> Result<()> {
	let axes = [StatAxis {
		tag: tag("wght"),
		name: StatName::from("Weight"),
		ordering: None,
		values: vec![
			value_on("Regular", 400.0, 0x2),
			StatAxisValue {
				name: StatName::from("Bold"),
				flags: 0,
				kind: StatAxisValueKind::Value {
					value: 600.0,
					linked_value: Some(650.0),
				},
			},
		],
	}];
	check_stat(
		&axes,
		None,
		StatName::Id(18),
		&[
			"  <STAT>",
			"    <Version value=\"0x00010001\"/>",
			"    <DesignAxisRecordSize value=\"8\"/>",
			"    <!-- DesignAxisCount=1 -->",
			"    <DesignAxisRecord>",
			"      <Axis index=\"0\">",
			"        <AxisTag value=\"wght\"/>",
			"        <AxisNameID value=\"256\"/>  <!-- Weight -->",
			"        <AxisOrdering value=\"0\"/>",
			"      </Axis>",
			"    </DesignAxisRecord>",
			"    <!-- AxisValueCount=2 -->",
			"    <AxisValueArray>",
			"      <AxisValue index=\"0\" Format=\"1\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"257\"/>  <!-- Regular -->",
			"        <Value value=\"400.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"1\" Format=\"3\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"258\"/>  <!-- Bold -->",
			"        <Value value=\"600.0\"/>",
			"        <LinkedValue value=\"650.0\"/>",
			"      </AxisValue>",
			"    </AxisValueArray>",
			"    <ElidedFallbackNameID value=\"18\"/>  <!-- missing from name table -->",
			"  </STAT>",
		],
	)
}

#[test]
fn test_build_stat_table_3() -> Result<()> {
	let range =
		|name: &str, min: Option<f64>, nominal: f64, max: Option<f64>, flags: u16| StatAxisValue {
			name: StatName::from(name),
			flags,
			kind: StatAxisValueKind::Range {
				nominal_value: nominal,
				range_min_value: min,
				range_max_value: max,
			},
		};
	let axes = [StatAxis {
		tag: tag("opsz"),
		name: StatName::from("Optical Size"),
		ordering: None,
		values: vec![
			range("Small", None, 6.0, Some(10.0), 0),
			range("Text", Some(10.0), 14.0, Some(24.0), 0x2),
			range("Display", Some(24.0), 600.0, None, 0),
		],
	}];
	check_stat(
		&axes,
		None,
		StatName::Id(2),
		&[
			"  <STAT>",
			"    <Version value=\"0x00010001\"/>",
			"    <DesignAxisRecordSize value=\"8\"/>",
			"    <!-- DesignAxisCount=1 -->",
			"    <DesignAxisRecord>",
			"      <Axis index=\"0\">",
			"        <AxisTag value=\"opsz\"/>",
			"        <AxisNameID value=\"256\"/>  <!-- Optical Size -->",
			"        <AxisOrdering value=\"0\"/>",
			"      </Axis>",
			"    </DesignAxisRecord>",
			"    <!-- AxisValueCount=3 -->",
			"    <AxisValueArray>",
			"      <AxisValue index=\"0\" Format=\"2\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"257\"/>  <!-- Small -->",
			"        <NominalValue value=\"6.0\"/>",
			"        <RangeMinValue value=\"-32768.0\"/>",
			"        <RangeMaxValue value=\"10.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"1\" Format=\"2\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"258\"/>  <!-- Text -->",
			"        <NominalValue value=\"14.0\"/>",
			"        <RangeMinValue value=\"10.0\"/>",
			"        <RangeMaxValue value=\"24.0\"/>",
			"      </AxisValue>",
			"      <AxisValue index=\"2\" Format=\"2\">",
			"        <AxisIndex value=\"0\"/>",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"259\"/>  <!-- Display -->",
			"        <NominalValue value=\"600.0\"/>",
			"        <RangeMinValue value=\"24.0\"/>",
			"        <RangeMaxValue value=\"32767.99998\"/>",
			"      </AxisValue>",
			"    </AxisValueArray>",
			"    <ElidedFallbackNameID value=\"2\"/>  <!-- missing from name table -->",
			"  </STAT>",
		],
	)
}

#[test]
fn test_build_stat_table_4() -> Result<()> {
	let axes = [
		StatAxis {
			tag: tag("wght"),
			name: StatName::from("Weight"),
			ordering: Some(1),
			values: Vec::new(),
		},
		StatAxis {
			tag: tag("ABCD"),
			name: StatName::from("ABCDTest"),
			ordering: Some(0),
			values: vec![value_on("Regular", 100.0, 0x2)],
		},
	];
	let locations = [StatLocation {
		name: StatName::from("Regular ABCD"),
		flags: 0,
		location: vec![(tag("wght"), 300.0), (tag("ABCD"), 100.0)],
	}];
	check_stat(
		&axes,
		Some(&locations),
		StatName::Id(18),
		&[
			"  <STAT>",
			"    <Version value=\"0x00010002\"/>",
			"    <DesignAxisRecordSize value=\"8\"/>",
			"    <!-- DesignAxisCount=2 -->",
			"    <DesignAxisRecord>",
			"      <Axis index=\"0\">",
			"        <AxisTag value=\"wght\"/>",
			"        <AxisNameID value=\"256\"/>  <!-- Weight -->",
			"        <AxisOrdering value=\"1\"/>",
			"      </Axis>",
			"      <Axis index=\"1\">",
			"        <AxisTag value=\"ABCD\"/>",
			"        <AxisNameID value=\"257\"/>  <!-- ABCDTest -->",
			"        <AxisOrdering value=\"0\"/>",
			"      </Axis>",
			"    </DesignAxisRecord>",
			"    <!-- AxisValueCount=2 -->",
			"    <AxisValueArray>",
			"      <AxisValue index=\"0\" Format=\"4\">",
			"        <!-- AxisCount=2 -->",
			"        <Flags value=\"0\"/>",
			"        <ValueNameID value=\"259\"/>  <!-- Regular ABCD -->",
			"        <AxisValueRecord index=\"0\">",
			"          <AxisIndex value=\"0\"/>",
			"          <Value value=\"300.0\"/>",
			"        </AxisValueRecord>",
			"        <AxisValueRecord index=\"1\">",
			"          <AxisIndex value=\"1\"/>",
			"          <Value value=\"100.0\"/>",
			"        </AxisValueRecord>",
			"      </AxisValue>",
			"      <AxisValue index=\"1\" Format=\"1\">",
			"        <AxisIndex value=\"1\"/>",
			"        <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
			"        <ValueNameID value=\"258\"/>  <!-- Regular -->",
			"        <Value value=\"100.0\"/>",
			"      </AxisValue>",
			"    </AxisValueArray>",
			"    <ElidedFallbackNameID value=\"18\"/>  <!-- missing from name table -->",
			"  </STAT>",
		],
	)
}

fn name_strings(font: &mut Font) -> Result<Vec<String>> {
	Ok(font
		.require::<Name>()?
		.names
		.iter()
		.map(|r| r.to_str_lossy())
		.collect())
}

#[test]
fn test_build_stat_table_platform_specific_names() -> Result<()> {
	// fonttools#2528: windowsNames and macNames choose the platforms of the new
	// name records.
	let mut font = Font::new();
	font.insert(Box::new(Name::default()));
	let range = |name: &str, nominal: f64, min: f64, max: f64, flags: u16| StatAxisValue {
		name: StatName::from(name),
		flags,
		kind: StatAxisValueKind::Range {
			nominal_value: nominal,
			range_min_value: Some(min),
			range_max_value: Some(max),
		},
	};
	let axes = [StatAxis {
		tag: tag("wght"),
		name: StatName::from("Weight"),
		ordering: Some(1),
		values: vec![
			range("ExtraLight", 200.0, 200.0, 250.0, 0),
			range("Light", 300.0, 250.0, 350.0, 0),
			range("Regular", 400.0, 350.0, 450.0, 0x2),
			range("Medium", 500.0, 450.0, 650.0, 0),
			range("Bold", 700.0, 650.0, 750.0, 0),
			range("ExtraBold", 800.0, 750.0, 850.0, 0),
			range("Black", 900.0, 850.0, 900.0, 0),
		],
	}];
	{
		let name = font.require::<Name>()?;
		for (i, n) in [
			"ExtraLight",
			"Light",
			"Regular",
			"Medium",
			"Bold",
			"ExtraBold",
			"Black",
		]
		.iter()
		.enumerate()
		{
			name.set_name(*n, 260 + i as u16, 3, 1, 0x409);
		}
		name.set_name("Weight", 270, 3, 1, 0x409);
	}
	let expected = name_strings(&mut font)?;
	build_stat_table(&mut font, &axes, None, &StatName::Id(2), true, false)?;
	// No new name records: Windows-only names with the same strings exist.
	assert_eq!(name_strings(&mut font)?, expected);

	font.require::<Name>()?
		.remove_names(Some(270), None, None, None);
	let mut expected = name_strings(&mut font)?;
	expected.push(s("Weight"));
	build_stat_table(&mut font, &axes, None, &StatName::Id(2), true, false)?;
	// One new name record, 'Weight'.
	assert_eq!(name_strings(&mut font)?, expected);

	build_stat_table(&mut font, &axes, None, &StatName::Id(2), true, true)?;
	let mut actual = name_strings(&mut font)?;
	let mut expected: Vec<String> = [
		"Weight",
		"ExtraLight",
		"Light",
		"Regular",
		"Medium",
		"Bold",
		"ExtraBold",
		"Black",
	]
	.iter()
	.flat_map(|n| [s(n), s(n), s(n)])
	.collect();
	// The names are inconsistent, so new name IDs are added for each platform.
	actual.sort();
	expected.sort();
	assert_eq!(actual, expected);
	Ok(())
}

#[test]
fn test_stat_infinities() {
	use fontoxide_misc::fixed_tools::float_to_fixed;
	let neg_inf = float_to_fixed(AXIS_VALUE_NEGATIVE_INFINITY, 16);
	assert_eq!((neg_inf as i32).to_be_bytes(), [0x80, 0, 0, 0]);
	let pos_inf = float_to_fixed(AXIS_VALUE_POSITIVE_INFINITY, 16);
	assert_eq!((pos_inf as i32).to_be_bytes(), [0x7F, 0xFF, 0xFF, 0xFF]);
}

fn math_font(glyphs: &[&str]) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(strings(glyphs));
	font
}

fn math(font: &mut Font) -> Result<ot::MATH> {
	Ok(font.require::<Math>()?.table.clone())
}

fn coverage_names(font: &mut Font, cov: &Option<Box<ot::Coverage>>) -> Result<Vec<String>> {
	let order = font.glyph_order()?;
	Ok(cov
		.as_ref()
		.map(|c| {
			c.glyphs
				.iter()
				.map(|&g| order.name(g).into_owned())
				.collect()
		})
		.unwrap_or_default())
}

#[test]
fn test_build_math_table_empty() -> Result<()> {
	let mut font = math_font(&[]);
	build_math_table(&mut font, &MathTableArgs::default())?;
	let m = math(&mut font)?;
	assert_eq!(m.version, 0x0001_0000);
	assert!(m.math_constants.is_none());
	assert!(m.math_glyph_info.is_none());
	assert!(m.math_variants.is_none());
	Ok(())
}

const MATH_CONSTANTS: &[(&str, f64)] = &[
	("AccentBaseHeight", 516.0),
	("AxisHeight", 262.0),
	("DelimitedSubFormulaMinHeight", 1500.0),
	("DisplayOperatorMinHeight", 2339.0),
	("FlattenedAccentBaseHeight", 698.0),
	("FractionDenomDisplayStyleGapMin", 198.0),
	("FractionDenominatorDisplayStyleShiftDown", 698.0),
	("FractionDenominatorGapMin", 66.0),
	("FractionDenominatorShiftDown", 465.0),
	("FractionNumDisplayStyleGapMin", 198.0),
	("FractionNumeratorDisplayStyleShiftUp", 774.0),
	("FractionNumeratorGapMin", 66.0),
	("FractionNumeratorShiftUp", 516.0),
	("FractionRuleThickness", 66.0),
	("LowerLimitBaselineDropMin", 585.0),
	("LowerLimitGapMin", 132.0),
	("MathLeading", 300.0),
	("OverbarExtraAscender", 66.0),
	("OverbarRuleThickness", 66.0),
	("OverbarVerticalGap", 198.0),
	("RadicalDegreeBottomRaisePercent", 75.0),
	("RadicalDisplayStyleVerticalGap", 195.0),
	("RadicalExtraAscender", 66.0),
	("RadicalKernAfterDegree", -556.0),
	("RadicalKernBeforeDegree", 278.0),
	("RadicalRuleThickness", 66.0),
	("RadicalVerticalGap", 82.0),
	("ScriptPercentScaleDown", 70.0),
	("ScriptScriptPercentScaleDown", 55.0),
	("SkewedFractionHorizontalGap", 66.0),
	("SkewedFractionVerticalGap", 77.0),
	("SpaceAfterScript", 42.0),
	("StackBottomDisplayStyleShiftDown", 698.0),
	("StackBottomShiftDown", 465.0),
	("StackDisplayStyleGapMin", 462.0),
	("StackGapMin", 198.0),
	("StackTopDisplayStyleShiftUp", 774.0),
	("StackTopShiftUp", 516.0),
	("StretchStackBottomShiftDown", 585.0),
	("StretchStackGapAboveMin", 132.0),
	("StretchStackGapBelowMin", 132.0),
	("StretchStackTopShiftUp", 165.0),
	("SubSuperscriptGapMin", 264.0),
	("SubscriptBaselineDropMin", 105.0),
	("SubscriptShiftDown", 140.0),
	("SubscriptTopMax", 413.0),
	("SuperscriptBaselineDropMax", 221.0),
	("SuperscriptBottomMaxWithSubscript", 413.0),
	("SuperscriptBottomMin", 129.0),
	("SuperscriptShiftUp", 477.0),
	("SuperscriptShiftUpCramped", 358.0),
	("UnderbarExtraDescender", 66.0),
	("UnderbarRuleThickness", 66.0),
	("UnderbarVerticalGap", 198.0),
	("UpperLimitBaselineRiseMin", 165.0),
	("UpperLimitGapMin", 132.0),
];

#[test]
fn test_build_math_table_constants() -> Result<()> {
	let mut font = math_font(&[]);
	let args = MathTableArgs {
		constants: Some(MATH_CONSTANTS.iter().map(|(k, v)| (s(k), *v)).collect()),
		..Default::default()
	};
	build_math_table(&mut font, &args)?;
	let m = math(&mut font)?;
	assert!(m.math_glyph_info.is_none());
	assert!(m.math_variants.is_none());
	let constants = m.math_constants.expect("constants");
	// The value of each constant, as TTX shows it: `<Name value="v"/>` for
	// numbers, `<Name>` then `<Value value="v"/>` for value records.
	let xml = get_xml(&*constants)?;
	let mut values: IndexMap<String, String> = IndexMap::new();
	let mut pending: Option<String> = None;
	for line in &xml[1..xml.len() - 1] {
		let line = line.trim();
		if let Some(rest) = line.strip_prefix("<Value value=\"") {
			let name = pending.take().expect("record name");
			values.insert(name, rest.trim_end_matches("\"/>").to_owned());
		} else if let Some((name, rest)) = line.trim_start_matches('<').split_once(" value=\"") {
			values.insert(name.to_owned(), rest.trim_end_matches("\"/>").to_owned());
		} else if !line.starts_with("</") {
			pending = Some(
				line.trim_start_matches('<')
					.trim_end_matches('>')
					.to_owned(),
			);
		}
	}
	assert_eq!(values.len(), MATH_CONSTANTS.len());
	for (k, v) in MATH_CONSTANTS {
		assert_eq!(values.get(*k), Some(&(*v as i64).to_string()), "{k}");
	}
	Ok(())
}

const LETTERS: &[&str] = &["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

fn glyph_values(items: &[(&str, f64)]) -> IndexMap<String, f64> {
	items.iter().map(|(g, v)| (s(g), *v)).collect()
}

#[test]
fn test_build_math_table_italics_correction() -> Result<()> {
	let mut font = math_font(LETTERS);
	let corrections = glyph_values(&[("A", 100.0), ("C", 300.0), ("D", 400.0), ("E", 500.0)]);
	build_math_table(
		&mut font,
		&MathTableArgs {
			italics_corrections: Some(corrections.clone()),
			..Default::default()
		},
	)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_variants.is_none());
	let info = m
		.math_glyph_info
		.expect("info")
		.math_italics_correction_info
		.expect("italics");
	let glyphs = coverage_names(&mut font, &info.coverage)?;
	let mut sorted = glyphs.clone();
	sorted.sort();
	assert_eq!(sorted, strings(&["A", "C", "D", "E"]));
	for (glyph, correction) in glyphs.iter().zip(&info.italics_correction) {
		assert_eq!(correction.value as f64, corrections[glyph]);
	}
	Ok(())
}

#[test]
fn test_build_math_table_top_accent_attachment() -> Result<()> {
	let mut font = math_font(LETTERS);
	let attachments = glyph_values(&[("A", 10.0), ("B", 20.0), ("C", 30.0), ("E", 50.0)]);
	build_math_table(
		&mut font,
		&MathTableArgs {
			top_accent_attachments: Some(attachments.clone()),
			..Default::default()
		},
	)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_variants.is_none());
	let info = m
		.math_glyph_info
		.expect("info")
		.math_top_accent_attachment
		.expect("attachment");
	let glyphs = coverage_names(&mut font, &info.top_accent_coverage)?;
	let mut sorted = glyphs.clone();
	sorted.sort();
	assert_eq!(sorted, strings(&["A", "B", "C", "E"]));
	for (glyph, attachment) in glyphs.iter().zip(&info.top_accent_attachment) {
		assert_eq!(attachment.value as f64, attachments[glyph]);
	}
	Ok(())
}

#[test]
fn test_build_math_table_extended_shape() -> Result<()> {
	let mut font = math_font(LETTERS);
	build_math_table(
		&mut font,
		&MathTableArgs {
			extended_shapes: Some(strings(&["A", "C", "E", "F"])),
			..Default::default()
		},
	)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_variants.is_none());
	let info = m.math_glyph_info.expect("info");
	let mut glyphs = coverage_names(&mut font, &info.extended_shape_coverage)?;
	glyphs.sort();
	assert_eq!(glyphs, strings(&["A", "C", "E", "F"]));
	Ok(())
}

#[test]
fn test_build_math_table_math_kern() -> Result<()> {
	let mut font = math_font(&["A", "B"]);
	let sides: IndexMap<MathKernSide, (Vec<f64>, Vec<f64>)> = [
		(
			MathKernSide::TopRight,
			(vec![10.0, 20.0], vec![10.0, 20.0, 30.0]),
		),
		(MathKernSide::BottomRight, (vec![], vec![10.0])),
		(MathKernSide::TopLeft, (vec![10.0], vec![0.0, 20.0])),
		(
			MathKernSide::BottomLeft,
			(vec![-10.0, 0.0], vec![0.0, 10.0, 20.0]),
		),
	]
	.into_iter()
	.collect();
	build_math_table(
		&mut font,
		&MathTableArgs {
			math_kerns: Some([(s("A"), sides.clone())].into_iter().collect()),
			..Default::default()
		},
	)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_variants.is_none());
	let info = m
		.math_glyph_info
		.expect("info")
		.math_kern_info
		.expect("kerns");
	assert_eq!(
		coverage_names(&mut font, &info.math_kern_coverage)?,
		strings(&["A"])
	);
	let record = &info.math_kern_info_records[0];
	let values = |k: &Option<Box<ot::MathKern>>| {
		let k = k.as_ref().expect("kern");
		(
			k.correction_height
				.iter()
				.map(|v| v.value as f64)
				.collect::<Vec<_>>(),
			k.kern_value
				.iter()
				.map(|v| v.value as f64)
				.collect::<Vec<_>>(),
		)
	};
	assert_eq!(
		values(&record.top_right_math_kern),
		sides[&MathKernSide::TopRight]
	);
	assert_eq!(
		values(&record.bottom_right_math_kern),
		sides[&MathKernSide::BottomRight]
	);
	assert_eq!(
		values(&record.top_left_math_kern),
		sides[&MathKernSide::TopLeft]
	);
	assert_eq!(
		values(&record.bottom_left_math_kern),
		sides[&MathKernSide::BottomLeft]
	);
	Ok(())
}

fn check_variants(vertical: bool) -> Result<()> {
	let mut font = math_font(&["A", "A.size1", "A.size2"]);
	let variants: IndexMap<String, Vec<(String, f64)>> =
		[(s("A"), vec![(s("A.size1"), 100.0), (s("A.size2"), 200.0)])]
			.into_iter()
			.collect();
	let mut args = MathTableArgs::default();
	if vertical {
		args.vert_glyph_variants = Some(variants.clone());
	} else {
		args.horiz_glyph_variants = Some(variants.clone());
	}
	build_math_table(&mut font, &args)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_glyph_info.is_none());
	let v = m.math_variants.expect("variants");
	let (coverage, constructions) = if vertical {
		(&v.vert_glyph_coverage, &v.vert_glyph_construction)
	} else {
		(&v.horiz_glyph_coverage, &v.horiz_glyph_construction)
	};
	let glyphs = coverage_names(&mut font, coverage)?;
	assert_eq!(glyphs, strings(&["A"]));
	let order = font.glyph_order()?;
	for (glyph, construction) in glyphs.iter().zip(constructions) {
		let records: Vec<(String, f64)> = construction
			.as_ref()
			.expect("construction")
			.math_glyph_variant_record
			.iter()
			.map(|r| {
				(
					order.name(r.variant_glyph).into_owned(),
					r.advance_measurement as f64,
				)
			})
			.collect();
		assert_eq!(records, variants[glyph]);
	}
	Ok(())
}

#[test]
fn test_build_math_table_vert_variants() -> Result<()> {
	check_variants(true)
}

#[test]
fn test_build_math_table_horiz_variants() -> Result<()> {
	check_variants(false)
}

fn check_assembly(vertical: bool) -> Result<()> {
	let mut font = math_font(&["A", "A.top", "A.middle", "A.bottom", "A.extender"]);
	let parts: Vec<GlyphPart> = vec![
		(s("A.bottom"), 0, 0.0, 100.0, 200.0),
		(s("A.extender"), 1, 50.0, 50.0, 100.0),
		(s("A.middle"), 0, 100.0, 100.0, 200.0),
		(s("A.extender"), 1, 50.0, 50.0, 100.0),
		(s("A.top"), 0, 100.0, 0.0, 200.0),
	];
	let assembly: IndexMap<String, (Vec<GlyphPart>, f64)> =
		[(s("A"), (parts, 10.0))].into_iter().collect();
	let mut args = MathTableArgs::default();
	if vertical {
		args.vert_glyph_assembly = Some(assembly.clone());
	} else {
		args.horiz_glyph_assembly = Some(assembly.clone());
	}
	build_math_table(&mut font, &args)?;
	let m = math(&mut font)?;
	assert!(m.math_constants.is_none());
	assert!(m.math_glyph_info.is_none());
	let v = m.math_variants.expect("variants");
	let (coverage, constructions) = if vertical {
		(&v.vert_glyph_coverage, &v.vert_glyph_construction)
	} else {
		(&v.horiz_glyph_coverage, &v.horiz_glyph_construction)
	};
	let glyphs = coverage_names(&mut font, coverage)?;
	assert_eq!(glyphs, strings(&["A"]));
	let order = font.glyph_order()?;
	for (glyph, construction) in glyphs.iter().zip(constructions) {
		let a = construction
			.as_ref()
			.expect("construction")
			.glyph_assembly
			.as_ref()
			.expect("assembly");
		let records: Vec<GlyphPart> = a
			.part_records
			.iter()
			.map(|r| {
				(
					order.name(r.glyph).into_owned(),
					r.part_flags,
					r.start_connector_length as f64,
					r.end_connector_length as f64,
					r.full_advance as f64,
				)
			})
			.collect();
		assert_eq!(
			(records, a.italics_correction.value as f64),
			assembly[glyph]
		);
	}
	Ok(())
}

#[test]
fn test_build_math_table_vert_assembly() -> Result<()> {
	check_assembly(true)
}

#[test]
fn test_build_math_table_horiz_assembly() -> Result<()> {
	check_assembly(false)
}

fn rule(
	prefix: &[&[&str]],
	glyphs: &[&[&str]],
	suffix: &[&[&str]],
	lookups: Vec<Option<RuleLookup>>,
) -> ChainContextualRule {
	ChainContextualRule::new(sets(prefix), sets(glyphs), sets(suffix), lookups)
}

#[test]
fn test_build_extended_formats() -> Result<()> {
	for pos in [false, true] {
		for chaining in [false, true] {
			let mut font = Font::new();
			font.set_glyph_order(strings(&[".notdef", "a", "b", "c", "d"]));
			let gm = GlyphMap::from_font(&mut font)?.with_len(0x10001);
			let builder = if pos {
				LookupBuilder::chain_context_pos(gm.clone(), None)
			} else {
				LookupBuilder::chain_context_subst(gm.clone(), None)
			};
			let common = builder.common().clone();
			let lookup = LookupBuilder::single_subst(gm, None).into_ref();
			lookup.borrow_mut().common_mut().lookup_index = Some(3);
			let prefix: &[&[&str]] = if chaining { &[&["a"]] } else { &[] };
			let suffix: &[&[&str]] = if chaining { &[&["d"]] } else { &[] };
			let r = rule(
				prefix,
				&[&["b"], &["c"]],
				suffix,
				vec![Some(RuleLookup::Many(vec![lookup])), None],
			);
			let mut ruleset = ChainContextualRuleset::new();
			ruleset.add_rule(r.clone());

			let format1 = build_format1_subtable(&common, &ruleset, chaining)?;
			check_extended_format1(&format1, chaining);

			let classdefs = ruleset.format2_class_defs().expect("class defs");
			let format2 = build_format2_subtable(&common, &ruleset, &classdefs, chaining)?;
			check_extended_format2(&format2, chaining);

			if !chaining {
				let format3 = build_format3_subtable(&common, &r, chaining)?;
				let (count, index) = match &format3 {
					LookupSubTable::ContextSubst(ot::ContextSubst::Format6(t)) => {
						(t.seq_lookup_count, t.seq_lookup_record[0].lookup_list_index)
					}
					LookupSubTable::ContextPos(ot::ContextPos::Format6(t)) => {
						(t.seq_lookup_count, t.seq_lookup_record[0].lookup_list_index)
					}
					other => panic!("format 6 expected: {other:?}"),
				};
				assert_eq!((count, index), (Some(1), 3));
			}
		}
	}
	Ok(())
}

fn check_extended_format1(st: &LookupSubTable, chaining: bool) {
	// glyph IDs: a = 1, c = 3, d = 4
	match st {
		LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format4(t)) => {
			check_chained_seq_rule(&t.chained_seq_rule_set)
		}
		LookupSubTable::ChainContextPos(ot::ChainContextPos::Format4(t)) => {
			check_chained_seq_rule(&t.chained_seq_rule_set)
		}
		LookupSubTable::ContextSubst(ot::ContextSubst::Format4(t)) => {
			check_seq_rule(&t.seq_rule_set)
		}
		LookupSubTable::ContextPos(ot::ContextPos::Format4(t)) => check_seq_rule(&t.seq_rule_set),
		other => panic!("format 4 expected (chaining {chaining}): {other:?}"),
	}
}

fn check_chained_seq_rule(sets: &[Option<Box<ot::ChainedSeqRuleSet2>>]) {
	let rule = sets[0].as_ref().expect("set").chained_seq_rule[0]
		.as_ref()
		.expect("rule");
	assert_eq!(rule.input_sequence, [3]);
	assert_eq!(rule.seq_lookup_count, Some(1));
	assert_eq!(rule.seq_lookup_record[0].lookup_list_index, 3);
	assert_eq!(rule.backtrack_sequence, [1]);
	assert_eq!(rule.look_ahead_sequence, [4]);
}

fn check_seq_rule(sets: &[Option<Box<ot::SeqRuleSet2>>]) {
	let rule = sets[0].as_ref().expect("set").seq_rule[0]
		.as_ref()
		.expect("rule");
	assert_eq!(rule.input_sequence, [3]);
	assert_eq!(rule.seq_lookup_count, Some(1));
	assert_eq!(rule.seq_lookup_record[0].lookup_list_index, 3);
}

fn check_extended_format2(st: &LookupSubTable, chaining: bool) {
	let (input_len, count) = match st {
		LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format5(t)) => {
			let r = first_class_rule(&t.chained_class_seq_rule_set, |s| &s.chained_class_seq_rule);
			(r.input_sequence.len(), r.seq_lookup_count)
		}
		LookupSubTable::ChainContextPos(ot::ChainContextPos::Format5(t)) => {
			let r = first_class_rule(&t.chained_class_seq_rule_set, |s| &s.chained_class_seq_rule);
			(r.input_sequence.len(), r.seq_lookup_count)
		}
		LookupSubTable::ContextSubst(ot::ContextSubst::Format5(t)) => {
			let r = first_class_rule(&t.class_seq_rule_set, |s| &s.class_seq_rule);
			(r.input_sequence.len(), r.seq_lookup_count)
		}
		LookupSubTable::ContextPos(ot::ContextPos::Format5(t)) => {
			let r = first_class_rule(&t.class_seq_rule_set, |s| &s.class_seq_rule);
			(r.input_sequence.len(), r.seq_lookup_count)
		}
		other => panic!("format 5 expected (chaining {chaining}): {other:?}"),
	};
	assert_eq!((input_len, count), (1, Some(1)));
}

fn first_class_rule<'a, S, R>(
	sets: &'a [Option<Box<S>>],
	rules: impl Fn(&'a S) -> &'a Vec<Option<Box<R>>>,
) -> &'a R {
	sets.iter()
		.flatten()
		.map(|s| rules(s)[0].as_deref().expect("rule"))
		.next()
		.expect("a class set")
}

#[test]
fn test_format2_overflow_falls_back_to_format3() -> Result<()> {
	let glyphs: Vec<String> = (0..44000).map(|i| format!("g{i}")).collect();
	let input: Vec<String> = glyphs.iter().step_by(2).cloned().collect();
	for pos in [true, false] {
		for extension in [false, true] {
			let mut font = Font::new();
			font.set_glyph_order(glyphs.clone());
			let gm = GlyphMap::from_font(&mut font)?;
			let mut builder = if pos {
				LookupBuilder::chain_context_pos(gm, None)
			} else {
				LookupBuilder::chain_context_subst(gm, None)
			};
			builder.common_mut().extension = extension;
			if let LookupBuilder::ChainContextPos(b) | LookupBuilder::ChainContextSubst(b) =
				&mut builder
			{
				// Alternating glyph IDs make the format 2 ClassDef too large, while
				// the format 3 Coverage fits in one subtable.
				b.rules.push(ChainContextualRule::new(
					vec![],
					vec![input.clone()],
					vec![],
					vec![None],
				));
			}
			let lookup = builder.build(&mut font)?.expect("lookup");
			assert_eq!(lookup.sub_table.len(), 1);
			let mut st = lookup.sub_table[0].as_deref().expect("subtable");
			if extension {
				st = match st {
					LookupSubTable::ExtensionPos(ot::ExtensionPos::Format1(e)) => {
						e.ext_sub_table.as_deref()
					}
					LookupSubTable::ExtensionSubst(ot::ExtensionSubst::Format1(e)) => {
						e.ext_sub_table.as_deref()
					}
					_ => None,
				}
				.expect("extension");
			}
			assert!(
				matches!(
					st,
					LookupSubTable::ChainContextPos(ot::ChainContextPos::Format3(_))
						| LookupSubTable::ContextSubst(ot::ContextSubst::Format3(_))
				),
				"{pos} {extension}"
			);
		}
	}
	Ok(())
}

fn class_set(b: &ClassDefBuilder) -> Vec<Vec<String>> {
	let mut c = b.classes()[1..].to_vec();
	c.sort();
	c
}

#[test]
fn test_make_rulesets() -> Result<()> {
	let mut font = Font::new();
	font.set_glyph_order(strings(&["a", "b", "c", "d", "A", "B", "C", "D", "E"]));
	let mut builder = LookupBuilder::chain_context_subst(GlyphMap::from_font(&mut font)?, None);
	let LookupBuilder::ChainContextSubst(sb) = &mut builder else {
		unreachable!()
	};
	sb.rules
		.push(rule(&[&["a"], &["b"]], &[&["c"]], &[], vec![None]));
	sb.rules
		.push(rule(&[&["a"], &["d"]], &[&["c"]], &[], vec![None]));
	sb.rules.push(ChainContextualRule::SubtableBreak);
	// The second subtable has some glyph classes.
	sb.rules.push(rule(&[&["A"]], &[&["E"]], &[], vec![None]));
	sb.rules
		.push(rule(&[&["A"]], &[&["C", "D"]], &[], vec![None]));
	sb.rules
		.push(rule(&[&["A", "B"]], &[&["E"]], &[], vec![None]));
	sb.rules.push(ChainContextualRule::SubtableBreak);
	// The third subtable has no context.
	sb.rules.push(rule(&[], &[&["E"]], &[], vec![None]));
	sb.rules.push(rule(&[], &[&["C", "D"]], &[], vec![None]));

	let rulesets = sb.rulesets();
	assert_eq!(rulesets.len(), 3);
	assert!(rulesets[0].has_prefix_or_suffix());
	assert!(!rulesets[0].has_any_glyph_classes());
	let cd = rulesets[0].format2_class_defs().expect("class defs");
	assert_eq!(class_set(&cd[0]), sets(&[&["a"], &["b"], &["d"]]));
	assert_eq!(class_set(&cd[1]), sets(&[&["c"]]));
	assert!(class_set(&cd[2]).is_empty());

	assert!(rulesets[1].has_prefix_or_suffix());
	assert!(rulesets[1].has_any_glyph_classes());
	assert!(rulesets[1].format2_class_defs().is_none());

	assert!(!rulesets[2].has_prefix_or_suffix());
	assert!(rulesets[2].has_any_glyph_classes());
	let cd = rulesets[2].format2_class_defs().expect("class defs");
	assert!(class_set(&cd[0]).is_empty());
	assert_eq!(class_set(&cd[1]), sets(&[&["C", "D"], &["E"]]));
	assert!(class_set(&cd[2]).is_empty());
	Ok(())
}

#[test]
fn test_build_extended_reverse_chain_single_subst() -> Result<()> {
	let mut font = Font::new();
	font.set_glyph_order(strings(&[".notdef", "a", "b", "c", "d"]));
	let gm = GlyphMap::from_font(&mut font)?.with_len(0x10001);
	let mut builder = LookupBuilder::reverse_chain_single_subst(gm, None);
	if let LookupBuilder::ReverseChainSingleSubst(b) = &mut builder {
		b.rules.push((
			sets(&[&["a"]]),
			sets(&[&["d"]]),
			[(s("c"), s("b"))].into_iter().collect(),
		));
	}
	let lookup = builder.build(&mut font)?.expect("lookup");
	let Some(LookupSubTable::ReverseChainSingleSubst(ot::ReverseChainSingleSubst::Format2(st))) =
		lookup.sub_table[0].as_deref()
	else {
		panic!("format 2 expected");
	};
	let cov = |c: &Option<Box<ot::Coverage>>| c.as_ref().expect("coverage").glyphs.clone();
	assert_eq!(cov(&st.backtrack_coverage[0]), [1]);
	assert_eq!(cov(&st.look_ahead_coverage[0]), [4]);
	assert_eq!(cov(&st.coverage), [3]);
	assert_eq!(st.substitute, [2]);
	Ok(())
}
