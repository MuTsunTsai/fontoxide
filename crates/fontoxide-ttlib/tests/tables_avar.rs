//! `Tests/ttLib/tables/_a_v_a_r_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::*;
use fontoxide_pens::statistics_pen::StatisticsPen;
use fontoxide_ttlib::tables::avar::SegmentMap;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, Table, TableType, Tag, WriteOptions};

const TEST_DATA: &str = "00 01 00 00 00 00 00 02 00 04 C0 00 C0 00 00 00 00 00 13 33 33 33 40 00 40 00 00 03 C0 00 C0 00 00 00 00 00 40 00 40 00";

fn tag(s: &str) -> Tag {
	Tag::from_str_exact(s).unwrap()
}

/// `makeFont(axisTags)`: a font with an `fvar` table of the axes.
fn make_font(axis_tags: &[&str]) -> Font {
	let mut fvar = Fvar::default();
	for t in axis_tags {
		fvar.axes.push(Axis {
			axis_tag: tag(t).0,
			..Default::default()
		});
	}
	let mut font = Font::new();
	font.insert(Box::new(fvar));
	font
}

/// `assertAvarAlmostEqual`.
fn assert_avar_almost_equal(
	segments1: &BTreeMap<Tag, SegmentMap>,
	segments2: &[(&str, SegmentMap)],
) {
	let keys: Vec<Tag> = segments1.keys().copied().collect();
	let mut expected_keys: Vec<Tag> = segments2.iter().map(|(t, _)| tag(t)).collect();
	expected_keys.sort();
	assert_eq!(keys, expected_keys);
	for (t, mapping2) in segments2 {
		let mut mapping1 = segments1[&tag(t)].clone();
		let mut mapping2 = mapping2.clone();
		mapping1.sort_by(|a, b| a.partial_cmp(b).unwrap());
		mapping2.sort_by(|a, b| a.partial_cmp(b).unwrap());
		assert_eq!(mapping1.len(), mapping2.len());
		for ((k1, v1), (k2, v2)) in mapping1.iter().zip(&mapping2) {
			assert!((k1 - k2).abs() < 5e-8, "{k1} != {k2}");
			assert!((v1 - v2).abs() < 5e-8, "{v1} != {v2}");
		}
	}
}

#[test]
fn test_compile() -> TestResult {
	let mut avar = Avar::default();
	avar.segments.insert(
		tag("wdth"),
		vec![(-1.0, -1.0), (0.0, 0.0), (0.3, 0.8), (1.0, 1.0)],
	);
	avar.segments
		.insert(tag("wght"), vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]);
	assert_eq!(
		avar.compile(&mut make_font(&["wdth", "wght"]))?,
		hex(TEST_DATA)
	);
	Ok(())
}

#[test]
fn test_decompile() -> TestResult {
	let avar = Avar::decompile(&hex(TEST_DATA), &mut make_font(&["wdth", "wght"]))?;
	assert_avar_almost_equal(
		&avar.segments,
		&[
			(
				"wdth",
				vec![(-1.0, -1.0), (0.0, 0.0), (0.2999878, 0.7999878), (1.0, 1.0)],
			),
			("wght", vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]),
		],
	);
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let mut avar = Avar::default();
	avar.segments.insert(
		tag("opsz"),
		vec![(-1.0, -1.0), (0.0, 0.0), (0.2999878, 0.7999878), (1.0, 1.0)],
	);
	let lines: Vec<String> = get_xml(&avar, &mut make_font(&["opsz"]))?
		.iter()
		.map(|l| l.trim().to_owned())
		.collect();
	assert_eq!(
		lines,
		[
			r#"<version major="1" minor="0"/>"#,
			r#"<segment axis="opsz">"#,
			r#"<mapping from="-1.0" to="-1.0"/>"#,
			r#"<mapping from="0.0" to="0.0"/>"#,
			r#"<mapping from="0.3" to="0.8"/>"#,
			r#"<mapping from="1.0" to="1.0"/>"#,
			"</segment>",
		]
	);
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let mut avar = Avar::default();
	from_xml(
		&mut avar,
		concat!(
			r#"<segment axis="wdth">"#,
			r#"    <mapping from="-1.0" to="-1.0"/>"#,
			r#"    <mapping from="0.0" to="0.0"/>"#,
			r#"    <mapping from="0.7" to="0.2"/>"#,
			r#"    <mapping from="1.0" to="1.0"/>"#,
			"</segment>"
		),
		&mut Font::new(),
	)?;
	assert_avar_almost_equal(
		&avar.segments,
		&[(
			"wdth",
			vec![(-1.0, -1.0), (0.0, 0.0), (0.7000122, 0.2000122), (1.0, 1.0)],
		)],
	);
	Ok(())
}

/// The `avar` of `Avar2Test.test_roundtrip` after saving, as fontTools dumps it.
/// (The upstream test builds the variation store with `varLib`, which this crate
/// does not depend on.)
const AVAR2_XML: &str = r#"<version major="2" minor="0"/>
<segment axis="wght">
</segment>
<segment axis="wdth">
  <mapping from="-1.0" to="-1.0"/>
  <mapping from="0.0" to="0.0"/>
  <mapping from="0.4" to="0.5"/>
  <mapping from="1.0" to="1.0"/>
</segment>
<VarIdxMap Format="0">
  <!-- Omitted values default to 0xFFFF/0xFFFF (no variations) -->
  <Map index="0"/>
  <Map index="1" outer="0" inner="0"/>
</VarIdxMap>
<VarStore Format="1">
  <Format value="1"/>
  <VarRegionList>
    <!-- RegionAxisCount=2 -->
    <!-- RegionCount=1 -->
    <Region index="0">
      <VarRegionAxis index="0">
        <StartCoord value="0.0"/>
        <PeakCoord value="1.0"/>
        <EndCoord value="1.0"/>
      </VarRegionAxis>
      <VarRegionAxis index="1">
        <StartCoord value="-1.0"/>
        <PeakCoord value="-1.0"/>
        <EndCoord value="0.0"/>
      </VarRegionAxis>
    </Region>
  </VarRegionList>
  <!-- VarDataCount=1 -->
  <VarData index="0">
    <!-- ItemCount=1 -->
    <NumShorts value="1"/>
    <!-- VarRegionCount=1 -->
    <VarRegionIndex index="0" value="0"/>
    <Item index="0" value="[-13107]"/>
  </VarData>
</VarStore>"#;

/// `font["avar"].compile(font)` of the same table in fontTools.
const AVAR2_DATA: &str = "000200000000000200000004c000c00000000000199a2000400040000000002400000030003f0002ffffffff0000000000010000000c00010000001c00020001000040004000c000c00000000001000100010000cccd";

#[test]
fn test_roundtrip() -> TestResult {
	let mut font = make_font(&["wght", "wdth"]);
	let mut avar = Avar::default();
	from_xml(&mut avar, AVAR2_XML, &mut font)?;
	assert_eq!(avar.major_version, 2);
	font.insert(Box::new(avar));

	let data = font.write(&WriteOptions::default())?;
	let mut font2 = Font::read(data, ReadOptions::default())?;
	assert_eq!(font2.table_data(Avar::TAG)?, hex(AVAR2_DATA));
	let avar2 = font2.require::<Avar>()?.clone();
	let xml2 = get_xml(&avar2, &mut font2)?;
	assert_eq!(xml2, AVAR2_XML.lines().collect::<Vec<_>>());
	assert!(xml2.contains(&"    <!-- RegionAxisCount=2 -->".to_owned()));
	assert!(xml2.contains(&"    <!-- RegionCount=1 -->".to_owned()));

	let mut avar3 = Avar::default();
	from_xml(&mut avar3, &xml2.concat(), &mut Font::new())?;
	let store = avar3.table.var_store.as_deref().unwrap();
	let regions = store.var_region_list.as_deref().unwrap();
	assert_eq!(regions.region.len(), 1);
	assert_eq!(regions.region[0].var_region_axis.len(), 2);
	Ok(())
}

#[test]
fn test_tt_glyph_set() -> TestResult {
	let data = std::fs::read(tables_data("Amstelvar-avar2.subset.ttf"))?;
	let mut font = Font::read(data, ReadOptions::default())?;
	let mut stats = |location: &[(&str, f64)]| -> Result<Vec<_>, Box<dyn std::error::Error>> {
		let location: BTreeMap<String, f64> = location
			.iter()
			.map(|(t, v)| ((*t).to_owned(), *v))
			.collect();
		let glyph_set = GlyphSet::with_user_location(&mut font, &location)?;
		let mut pen = StatisticsPen::new_statistics(None);
		for glyph in ["T", "e", "s", "t"] {
			glyph_set.draw(glyph, &mut pen)?;
		}
		Ok(vec![pen.statistics()])
	};
	let regular = stats(&[])?.remove(0);
	let black = stats(&[("wght", 900.0)])?.remove(0);
	let expanded = stats(&[("wdth", 125.0)])?.remove(0);
	assert!(regular.area.abs() < black.area.abs());
	assert!(expanded.area.abs() < black.area.abs());
	assert!(regular.mean_x < expanded.mean_x);
	Ok(())
}

fn limits(entries: &[(&str, &[f64])]) -> BTreeMap<Tag, Vec<f64>> {
	entries.iter().map(|(t, v)| (tag(t), v.to_vec())).collect()
}

#[test]
fn test_maps_coordinates_not_distances() -> TestResult {
	// The trailing distances are user-space distances, not coordinates.
	let mut avar = Avar::default();
	avar.segments.insert(
		tag("ELSH"),
		vec![
			(-1.0, -1.0),
			(-0.5, -0.8),
			(0.0, 0.0),
			(0.5, 0.8),
			(1.0, 1.0),
		],
	);
	let out = avar.renormalize_axis_limits(&limits(&[("ELSH", &[-0.5, 0.0, 0.5, 0.25, 0.75])]))?;
	let values = &out[&tag("ELSH")];
	assert!((values[0] + 0.8).abs() <= 1.0 / 16384.0);
	assert_eq!(values[1], 0.0);
	assert!((values[2] - 0.8).abs() <= 1.0 / 16384.0);
	assert_eq!(values[3..], [0.25, 0.75]);
	Ok(())
}

#[test]
fn test_quantizes_to_f2dot14_after_map() -> TestResult {
	let mut avar = Avar::default();
	avar.segments.insert(
		tag("wght"),
		vec![(-1.0, -1.0), (0.0, 0.0), (0.7, 0.2), (1.0, 1.0)],
	);
	let out = avar.renormalize_axis_limits(&limits(&[("wght", &[0.0, 0.5, 0.5])]))?;
	// 0.5 maps to 1/7, which must be quantized.
	assert_eq!(out[&tag("wght")], [0.0, 0.14288330078125, 0.14288330078125]);
	Ok(())
}

#[test]
fn test_axis_without_segment_map_passes_through() -> TestResult {
	let avar = Avar::default();
	let out = avar.renormalize_axis_limits(&limits(&[("wght", &[-0.25, 0.0, 0.75])]))?;
	assert_eq!(out[&tag("wght")], [-0.25, 0.0, 0.75]);
	Ok(())
}
